// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use futures_util::{Stream, StreamExt};
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::BodyExt;
use httpant_core::http::{self, HeaderMap, Method, Uri};
use httpant_core::{Body, BoxFuture, BoxService, Bytes, Capabilities, Request, Result, Support};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tower::ServiceExt;
use warp::Filter;
use warp::Reply;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// warp 0.4 serving the one boxed service behind a filter that matches every request: warp's own routing, extractors and rejections are
/// not used, so route matching and error mapping stay identical to every other adapter. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct WarpAdapter;

impl ServerAdapter for WarpAdapter {
    fn id(&self) -> &'static str {
        "warp"
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        // Measured by conformance cases 8 and 9 (the test `declared_capabilities_match_the_measured_ones`), not asserted.
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.graceful_shutdown = Support::Yes;
        // Not measurable by this suite: a handler that is not `Send` cannot be written against the `Send` service contract.
        c.local_handlers = Support::No;
        // HTTPAnt terminates TLS below the adapters only where the web library accepts an already-handshaken stream; this one is edge mode only.
        c.tls = Support::No;
        // These adapters serve HTTP/1.1 only until their HTTP/2 slice; they never offer `h2` (conformance case 18).
        c.http2 = Support::No;
        // With the `http2` feature warp's connection builder serves h2c and `Accept` is private (`docs/H2C-POLICY.md`), so HTTPAnt cannot refuse it:
        // `h2c_refusable: No`, said in the capability matrix. Without the feature it is HTTP/1.1 only, as everywhere. Conformance case 19 holds both.
        c.h2c = if cfg!(feature = "http2") {
            Support::Yes
        } else {
            Support::No
        };
        c.h2c_refusable = if cfg!(feature = "http2") {
            Support::No
        } else {
            Support::Yes
        };
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let grace = opts.grace();
            let svc = opts.wrap(svc);
            opts.listener.set_nonblocking(true)?;
            let addr = opts.listener.local_addr()?;
            let listener = TcpListener::from_std(opts.listener)?;
            // warp wants a `Sync` filter and the boxed service is only `Send`: keep it behind a mutex held just long enough to clone it.
            let shared = Arc::new(Mutex::new(svc));
            let filter = warp::method()
                .and(warp::path::full())
                .and(warp::query::raw().or(warp::any().map(String::new)).unify())
                .and(warp::header::headers_cloned())
                .and(warp::addr::remote())
                .and(warp::body::stream())
                .and_then(
                    move |method: Method,
                          path: warp::path::FullPath,
                          query: String,
                          headers: HeaderMap,
                          peer: Option<std::net::SocketAddr>,
                          body| {
                        let svc = shared
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .clone();
                        async move {
                            Ok::<_, Infallible>(
                                handle(svc, method, path, query, headers, peer, body).await,
                            )
                        }
                    },
                );
            let (stop, stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                warp::serve(filter)
                    .incoming(listener)
                    .graceful(async move {
                        let _ = stopped.await;
                    })
                    .run()
                    .await;
            });
            Ok(RunningServer::new(addr, stop, task, grace))
        })
    }
}

async fn handle<S, B>(
    svc: BoxService,
    method: Method,
    path: warp::path::FullPath,
    query: String,
    headers: HeaderMap,
    peer: Option<std::net::SocketAddr>,
    body: S,
) -> warp::reply::Response
where
    S: Stream<Item = std::result::Result<B, warp::Error>> + Send + 'static,
    B: bytes::Buf + 'static,
{
    let target = if query.is_empty() {
        path.as_str().to_owned()
    } else {
        format!("{}?{query}", path.as_str())
    };
    let declared = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let bad_request = || {
        let mut res = warp::reply::reply().into_response();
        *res.status_mut() = http::StatusCode::BAD_REQUEST;
        res
    };
    let Ok(uri) = target.parse::<Uri>() else {
        return bad_request();
    };
    let mut builder = http::Request::builder().method(method).uri(uri);
    for (name, value) in &headers {
        builder = builder.header(name, value);
    }
    let body = body.map(|chunk| {
        chunk
            .map(|mut buf| buf.copy_to_bytes(buf.remaining()))
            .map_err(std::io::Error::other)
    });
    let Ok(request) = builder.body(Body::new(Declared {
        stream: Box::pin(body),
        declared,
    })) else {
        return bad_request();
    };
    let mut request: Request = request;
    if let Some(peer) = peer {
        request.extensions_mut().insert(crate::PeerAddr(peer));
    }
    let out = svc
        .oneshot(request)
        .await
        .unwrap_or_else(|e: Infallible| match e {});
    let (parts, mut body) = out.into_parts();
    let exact = body.size_hint().exact();
    // warp wants a `Sync` stream and `Body` is not: a pump task hands the frames over through a channel, one at a time.
    let (tx, mut rx) = mpsc::channel::<std::result::Result<Bytes, std::io::Error>>(1);
    tokio::spawn(async move {
        while let Some(frame) = body.frame().await {
            let item = match frame {
                Ok(frame) => match frame.into_data() {
                    Ok(data) => Ok(data),
                    Err(_) => continue,
                },
                Err(e) => Err(std::io::Error::other(e)),
            };
            let failed = item.is_err();
            if tx.send(item).await.is_err() || failed {
                break;
            }
        }
    });
    let stream = futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx));
    let mut res = warp::reply::stream(stream).into_response();
    *res.status_mut() = parts.status;
    for (name, value) in &parts.headers {
        // Framing is decided below from the body, never copied from a header.
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        res.headers_mut().append(name, value.clone());
    }
    // A body of known length keeps its `Content-Length` (and so does HEAD); an unknown one is sent chunked.
    if let Some(len) = exact {
        res.headers_mut()
            .insert(http::header::CONTENT_LENGTH, len.into());
    }
    res
}

/// A warp request body as an `http_body::Body`, with the declared length as its size hint so an over-limit body is refused on its head.
struct Declared {
    stream: Pin<Box<dyn Stream<Item = std::result::Result<Bytes, std::io::Error>> + Send>>,
    declared: Option<u64>,
}

impl http_body::Body for Declared {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, std::io::Error>>> {
        self.stream
            .poll_next_unpin(cx)
            .map(|item| item.map(|chunk| chunk.map(Frame::data)))
    }

    fn size_hint(&self) -> SizeHint {
        self.declared
            .map_or_else(SizeHint::new, SizeHint::with_exact)
    }
}
