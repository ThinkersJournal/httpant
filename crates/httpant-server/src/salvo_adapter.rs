// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use http_body::{Body as _, Frame, SizeHint};
use http_body_util::BodyExt;
use httpant_core::http;
use httpant_core::{Body, BoxFuture, BoxService, Bytes, Capabilities, Request, Result, Support};
use salvo_core::conn::tcp::TcpAcceptor;
use salvo_core::http::ResBody;
use salvo_core::{Depot, FlowCtrl, Handler, Router, Server, Service, async_trait};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower::ServiceExt;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// salvo 1 serving the one boxed service as the goal of a catch-all route: salvo's own routing, extractors and hoops are not used, so
/// route matching and error mapping stay identical to every other adapter. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct SalvoAdapter;

impl ServerAdapter for SalvoAdapter {
    fn id(&self) -> &'static str {
        "salvo"
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        // Measured by conformance cases 8 and 9 (the test `declared_capabilities_match_the_measured_ones`), not asserted.
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.graceful_shutdown = Support::Yes;
        // Not measurable by this suite: a handler that is not `Send` cannot be written against the `Send` service contract.
        c.local_handlers = Support::No;
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            opts.listener.set_nonblocking(true)?;
            let addr = opts.listener.local_addr()?;
            let acceptor = TcpAcceptor::try_from(TcpListener::from_std(opts.listener)?)?;
            let goal = Goal(Arc::new(Mutex::new(svc)));
            let router = Router::with_path("{**rest}").goal(goal);
            let server = Server::new(acceptor);
            let handle = server.handle();
            let (stop, stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                let serving = server.serve(Service::new(router));
                tokio::pin!(serving);
                tokio::select! {
                    _ = &mut serving => {}
                    _ = stopped => {
                        // Graceful: stop accepting, finish requests in flight, close idle connections, then return.
                        handle.stop_graceful(None);
                        serving.await;
                    }
                }
            });
            Ok(RunningServer::new(addr, stop, task))
        })
    }
}

/// The boxed service is `Send` only and salvo wants `Sync`: a mutex held just long enough to clone it.
struct Goal(Arc<Mutex<BoxService>>);

#[async_trait]
impl Handler for Goal {
    async fn handle(
        &self,
        req: &mut salvo_core::Request,
        _depot: &mut Depot,
        res: &mut salvo_core::Response,
        ctrl: &mut FlowCtrl,
    ) {
        ctrl.skip_rest();
        let svc = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
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
        let incoming = req.take_body();
        let Ok(request) = builder.body(Body::new(Declared {
            inner: incoming,
            declared,
        })) else {
            res.status_code(http::StatusCode::BAD_REQUEST);
            return;
        };
        let request: Request = request;
        let out = svc
            .oneshot(request)
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        let (parts, body) = out.into_parts();
        res.status_code(parts.status);
        for (name, value) in &parts.headers {
            // Framing is decided below from the body, never copied from a header.
            if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
                continue;
            }
            res.headers_mut().append(name, value.clone());
        }
        // A body of known length keeps its `Content-Length` (and so does HEAD); an unknown one is sent chunked.
        if let Some(len) = body.size_hint().exact() {
            res.headers_mut()
                .insert(http::header::CONTENT_LENGTH, len.into());
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
        res.body(ResBody::stream(stream));
    }
}

/// A salvo request body as an `http_body::Body`, with the declared length as its size hint so an over-limit body is refused on its head.
struct Declared {
    inner: salvo_core::http::ReqBody,
    declared: Option<u64>,
}

impl http_body::Body for Declared {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, std::io::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn size_hint(&self) -> SizeHint {
        self.declared
            .map_or_else(SizeHint::new, SizeHint::with_exact)
    }
}
