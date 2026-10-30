// SPDX-License-Identifier: MIT OR Apache-2.0
use std::time::Duration;

use httpant_core::{BoxFuture, Capabilities, LayerFn, Result, Router, apply_layers};
use httpant_server::{
    ActixAdapter, AxumAdapter, HyperAdapter, PoemAdapter, RunningServer, SalvoAdapter,
    ServeOptions, ServerAdapter, WarpAdapter,
};

use crate::mutants::{self, Mutant};

/// What the suite asks a subject to serve: routes and layers, as an application would describe them.
pub struct App {
    /// The routes.
    pub router: Router,
    /// Layers in application order: the last is outermost.
    pub layers: Vec<LayerFn>,
}

/// Something the suite can start a server on: an adapter (to be passed) or a known-bad variant of one (to be failed).
pub trait Subject: Send + Sync {
    /// A name for failure messages.
    fn name(&self) -> String;

    /// Start serving `app` on a free local port, with this shutdown grace period (`None`: wait for ever).
    fn start_with_grace(
        &self,
        app: App,
        grace: Option<Duration>,
    ) -> BoxFuture<'_, Result<RunningServer>>;

    /// Start serving `app` on a free local port. The grace period is long (the cases that are not about it must never meet it).
    fn start(&self, app: App) -> BoxFuture<'_, Result<RunningServer>> {
        self.start_with_grace(app, Some(LONG_GRACE))
    }

    /// What the subject says it can do, if it is a real adapter (a mutant says nothing).
    fn declared(&self) -> Option<Capabilities> {
        None
    }
}

/// The grace period of every case that is not about the grace period.
const LONG_GRACE: Duration = Duration::from_secs(60);

fn options(listener: std::net::TcpListener, grace: Option<Duration>) -> ServeOptions {
    match grace {
        Some(period) => ServeOptions::new(listener).shutdown_grace(period),
        None => ServeOptions::new(listener).no_shutdown_grace(),
    }
}

fn free_listener() -> std::io::Result<std::net::TcpListener> {
    std::net::TcpListener::bind("127.0.0.1:0")
}

/// A real adapter, run exactly as `Server` runs it.
pub struct AdapterSubject<A>(pub A);

impl<A: ServerAdapter> Subject for AdapterSubject<A> {
    fn name(&self) -> String {
        format!("adapter `{}`", self.0.id())
    }

    fn declared(&self) -> Option<Capabilities> {
        Some(self.0.capabilities())
    }

    fn start_with_grace(
        &self,
        app: App,
        grace: Option<Duration>,
    ) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let svc = apply_layers(app.router.into_service(), app.layers);
            self.0.serve(svc, options(free_listener()?, grace)).await
        })
    }
}

/// The reference subject: the hyper adapter.
pub fn reference() -> AdapterSubject<HyperAdapter> {
    AdapterSubject(HyperAdapter)
}

/// Every adapter this build of HTTPAnt has, each wrapped as a subject: the list CI runs the whole suite over.
pub fn adapters() -> Vec<Box<dyn Subject>> {
    vec![
        Box::new(AdapterSubject(HyperAdapter)),
        Box::new(AdapterSubject(AxumAdapter)),
        Box::new(AdapterSubject(ActixAdapter)),
        Box::new(AdapterSubject(PoemAdapter)),
        Box::new(AdapterSubject(SalvoAdapter)),
        Box::new(AdapterSubject(WarpAdapter)),
    ]
}

/// A deliberately broken variant of the reference adapter. A case that passes on its mutant is itself a failing test.
pub struct MutantSubject(pub Mutant);

impl Subject for MutantSubject {
    fn name(&self) -> String {
        format!("mutant `{}`", self.0.id())
    }

    fn start_with_grace(
        &self,
        app: App,
        grace: Option<Duration>,
    ) -> BoxFuture<'_, Result<RunningServer>> {
        // The mutant that waits for ever ignores the period it was asked for.
        let grace = if self.0 == Mutant::NoShutdownGrace {
            None
        } else {
            grace
        };
        Box::pin(async move {
            let svc = if self.0 == Mutant::NoMiddleware {
                app.router.into_service()
            } else {
                apply_layers(app.router.into_service(), app.layers)
            };
            let svc = mutants::apply(self.0, svc);
            if self.0 == Mutant::AbortOnShutdown {
                return mutants::serve_aborting(svc, free_listener()?);
            }
            HyperAdapter
                .serve(
                    svc,
                    options(free_listener()?, grace).limits(self.0.limits()),
                )
                .await
        })
    }
}
