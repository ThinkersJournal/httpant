// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use actix_web::body::{BodyStream, SizedStream};
use actix_web::dev::ServerHandle;
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use futures_util::StreamExt;
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::BodyExt;
use httpant_core::http::{self, Method, Uri};
use httpant_core::{
    Body, BoxFuture, BoxService, Bytes, Capabilities, Error, Request, Result, Support,
};
use tokio::sync::{mpsc, oneshot};
use tower::ServiceExt;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// actix-web 4 serving the one boxed service as its default service: actix's own routing and extractors are not used, so route matching and
/// error mapping stay identical to every other adapter.
///
/// actix runs its own single-threaded workers (`actix-rt`) on a thread of its own, so the request payload, which is `!Send`, never crosses a
/// thread: a pump task on the worker moves it chunk by chunk, with back-pressure, into the `Send` [`Body`] the service reads. Nothing is
/// buffered, unlike `actix-tower` (design `docs/ACTIX-ADAPTER-NOTES.md`, option C: no Tower middleware inside actix).
#[derive(Clone, Copy, Debug, Default)]
pub struct ActixAdapter;

impl ServerAdapter for ActixAdapter {
    fn id(&self) -> &'static str {
        "actix-web"
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        // Measured by conformance cases 8 and 9 (the test `declared_capabilities_match_the_measured_ones`), not asserted.
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.graceful_shutdown = Support::Yes;
        // Handlers are `Send` futures; a handler written against actix's `!Send` types is the later `LocalHandler` capability.
        c.local_handlers = Support::No;
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let addr = opts.listener.local_addr()?;
            let listener = opts.listener;
            let (handle_tx, handle_rx) = oneshot::channel::<std::io::Result<ServerHandle>>();
            let (done_tx, done_rx) = oneshot::channel::<()>();
            std::thread::Builder::new()
                .name("httpant-actix".into())
                .spawn(move || {
                    actix_web::rt::System::new().block_on(async move {
                        let built = HttpServer::new(move || {
                            let svc = svc.clone();
                            App::new().default_service(web::to(
                                move |req: HttpRequest, payload: web::Payload| {
                                    handle(svc.clone(), req, payload)
                                },
                            ))
                        })
                        .workers(2)
                        .disable_signals()
                        .listen(listener)
                        .map(HttpServer::run);
                        match built {
                            Ok(server) => {
                                let _ = handle_tx.send(Ok(server.handle()));
                                let _ = server.await;
                            }
                            Err(e) => {
                                let _ = handle_tx.send(Err(e));
                            }
                        }
                    });
                    let _ = done_tx.send(());
                })?;
            let handle = handle_rx
                .await
                .map_err(|_| Error::Io(std::io::Error::other("actix thread ended early")))??;
            let (stop, stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                // A dropped `RunningServer` stops the server too, as in the other adapters.
                let _ = stopped.await;
                // Graceful: stop accepting, finish requests in flight, close idle connections.
                handle.stop(true).await;
                let _ = done_rx.await;
            });
            Ok(RunningServer::new(addr, stop, task))
        })
    }
}

async fn handle(svc: BoxService, req: HttpRequest, payload: web::Payload) -> HttpResponse {
    let Ok(request) = to_request(&req, payload) else {
        return HttpResponse::BadRequest().finish();
    };
    let res = svc
        .oneshot(request)
        .await
        .unwrap_or_else(|e: Infallible| match e {});
    let (parts, body) = res.into_parts();
    let status = actix_web::http::StatusCode::from_u16(parts.status.as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
    let mut out = HttpResponse::build(status);
    let sized = body.size_hint().exact();
    for (name, value) in &parts.headers {
        // actix writes the framing headers itself from the body it is given.
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out.append_header((n, v));
        }
    }
    let stream = futures_util::stream::unfold(body, |mut body| async move {
        loop {
            match body.frame().await? {
                Ok(frame) => match frame.into_data() {
                    Ok(data) => return Some((Ok::<_, Box<dyn std::error::Error>>(data), body)),
                    Err(_) => continue,
                },
                Err(e) => return Some((Err(e as Box<dyn std::error::Error>), body)),
            }
        }
    });
    match sized {
        Some(len) => out.body(SizedStream::new(len, stream)),
        None => out.body(BodyStream::new(stream)),
    }
}

fn to_request(req: &HttpRequest, mut payload: web::Payload) -> std::result::Result<Request, ()> {
    let method = Method::from_bytes(req.method().as_str().as_bytes()).map_err(|_| ())?;
    let uri: Uri = req.uri().to_string().parse().map_err(|_| ())?;
    let mut builder = http::Request::builder().method(method).uri(uri);
    for (name, value) in req.headers() {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    // Capacity 1: the pump reads the next chunk only when the service has taken the last, so a slow handler slows the sender.
    let (tx, rx) = mpsc::channel::<std::result::Result<Bytes, std::io::Error>>(1);
    actix_web::rt::spawn(async move {
        while let Some(chunk) = payload.next().await {
            let chunk = chunk.map_err(std::io::Error::other);
            let failed = chunk.is_err();
            if tx.send(chunk).await.is_err() || failed {
                break;
            }
        }
    });
    // The declared length is the size hint, so a body over the handler's limit is refused on its head, as on the other adapters.
    let declared = req
        .headers()
        .get(actix_web::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    builder
        .body(Body::new(ChannelBody(rx, declared)))
        .map_err(|_| ())
}

/// The `Send` end of the payload pump.
struct ChannelBody(
    mpsc::Receiver<std::result::Result<Bytes, std::io::Error>>,
    Option<u64>,
);

impl http_body::Body for ChannelBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, std::io::Error>>> {
        self.0
            .poll_recv(cx)
            .map(|item| item.map(|chunk| chunk.map(Frame::data)))
    }

    fn size_hint(&self) -> SizeHint {
        self.1.map_or_else(SizeHint::new, SizeHint::with_exact)
    }
}
