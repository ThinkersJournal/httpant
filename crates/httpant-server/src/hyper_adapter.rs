// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant_core::{Body, BoxFuture, BoxService, Capabilities, Request, Result, Support};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower::ServiceExt;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// The reference adapter: hyper 1.x over HTTP/1.1, no framework on top. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct HyperAdapter;

impl ServerAdapter for HyperAdapter {
    fn id(&self) -> &'static str {
        "hyper"
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        c.local_handlers = Support::No;
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            opts.listener.set_nonblocking(true)?;
            let addr = opts.listener.local_addr()?;
            let listener = TcpListener::from_std(opts.listener)?;
            let (stop, mut stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = &mut stopped => break,
                        accepted = listener.accept() => {
                            let Ok((stream, _)) = accepted else { continue };
                            let svc = svc.clone();
                            tokio::spawn(async move {
                                let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                    let req: Request = req.map(Body::new);
                                    svc.clone().oneshot(req)
                                });
                                // A connection that errors (peer reset, malformed request) ends only that connection.
                                let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                            });
                        }
                    }
                }
            });
            Ok(RunningServer::new(addr, stop, task))
        })
    }
}
