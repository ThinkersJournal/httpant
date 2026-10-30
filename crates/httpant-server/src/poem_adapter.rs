// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use futures_util::StreamExt;
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::BodyExt;
use httpant_core::http;
use httpant_core::{Body, BoxFuture, BoxService, Bytes, Capabilities, Request, Result, Support};
use poem::listener::TcpAcceptor;
use tokio::sync::oneshot;
use tower::ServiceExt;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// poem 3 serving the one boxed service as its only endpoint: poem's routing, extractors and middleware are not used, so route matching
/// and error mapping stay identical to every other adapter. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoemAdapter;

impl ServerAdapter for PoemAdapter {
    fn id(&self) -> &'static str {
        "poem"
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
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let grace = opts.grace();
            let svc = opts.wrap(svc);
            opts.listener.set_nonblocking(true)?;
            let addr = opts.listener.local_addr()?;
            let acceptor = TcpAcceptor::from_std(opts.listener)?;
            // poem wants a `Sync` endpoint and the boxed service is only `Send`: keep it behind a mutex held just long enough to clone it.
            let shared = Arc::new(Mutex::new(svc));
            let endpoint = poem::endpoint::make(move |req: poem::Request| {
                let svc = shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                async move { Ok::<_, Infallible>(handle(svc, req).await) }
            });
            let (stop, stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                let _ = poem::Server::new_with_acceptor(acceptor)
                    .run_with_graceful_shutdown(
                        endpoint,
                        async move {
                            let _ = stopped.await;
                        },
                        None,
                    )
                    .await;
            });
            Ok(RunningServer::new(addr, stop, task, grace))
        })
    }
}

async fn handle(svc: BoxService, mut req: poem::Request) -> poem::Response {
    let declared = req
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut builder = http::Request::builder()
        .method(req.method().clone())
        .uri(req.uri().clone());
    for (name, value) in req.headers() {
        builder = builder.header(name, value);
    }
    let peer = req.remote_addr().as_socket_addr().copied();
    let stream = req.take_body().into_bytes_stream();
    let Ok(request) = builder.body(Body::new(StreamBody {
        stream: Box::pin(stream),
        declared,
    })) else {
        return poem::Response::builder()
            .status(http::StatusCode::BAD_REQUEST)
            .finish();
    };
    let mut request: Request = request;
    if let Some(peer) = peer {
        request.extensions_mut().insert(crate::PeerAddr(peer));
    }
    let res = svc
        .oneshot(request)
        .await
        .unwrap_or_else(|e: Infallible| match e {});
    let (parts, body) = res.into_parts();
    let mut out = poem::Response::builder().status(parts.status);
    for (name, value) in &parts.headers {
        // Framing is decided below from the body, never copied from a header.
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        out = out.header(name, value);
    }
    // A body of known length keeps its `Content-Length` (and so does HEAD); an unknown one is sent chunked.
    if let Some(len) = body.size_hint().exact() {
        out = out.header(http::header::CONTENT_LENGTH, len);
    }
    let stream = futures_util::stream::unfold(body, |mut body| async move {
        loop {
            match body.frame().await? {
                Ok(frame) => match frame.into_data() {
                    Ok(data) => return Some((Ok::<Bytes, std::io::Error>(data), body)),
                    Err(_) => continue,
                },
                Err(e) => return Some((Err(std::io::Error::other(e)), body)),
            }
        }
    });
    out.body(poem::Body::from_bytes_stream(stream))
}

/// A poem request body as an `http_body::Body`, with the declared length as its size hint so an over-limit body is refused on its head.
struct StreamBody {
    stream: Pin<
        Box<dyn futures_util::Stream<Item = std::result::Result<Bytes, std::io::Error>> + Send>,
    >,
    declared: Option<u64>,
}

impl http_body::Body for StreamBody {
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
