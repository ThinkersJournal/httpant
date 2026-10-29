// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::sync::{Arc, Mutex, PoisonError};

use httpant_core::{Body, BoxFuture, BoxService, Capabilities, Request, Result, Support};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower::ServiceExt;

use crate::{RunningServer, ServeOptions, ServerAdapter};

/// axum 0.8 serving the one boxed service as its fallback: axum's own routing, extractors and `Router` are not used, so route matching and
/// error mapping stay identical to every other adapter. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxumAdapter;

impl ServerAdapter for AxumAdapter {
    fn id(&self) -> &'static str {
        "axum"
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
            let listener = TcpListener::from_std(opts.listener)?;
            // axum wants a `Sync` service and the boxed one is only `Send`: keep it behind a mutex held just long enough to clone it.
            let shared = Arc::new(Mutex::new(svc));
            let app = axum::Router::new().fallback_service(tower::service_fn(
                move |req: axum::extract::Request| {
                    let svc = shared
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    async move {
                        let req: Request = req.map(Body::new);
                        let res = svc
                            .oneshot(req)
                            .await
                            .unwrap_or_else(|e: Infallible| match e {});
                        Ok::<_, Infallible>(res.map(axum::body::Body::new))
                    }
                },
            ));
            let (stop, stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = stopped.await;
                    })
                    .await;
            });
            Ok(RunningServer::new(addr, stop, task))
        })
    }
}
