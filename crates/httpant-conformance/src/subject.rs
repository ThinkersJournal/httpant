// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant_core::{BoxFuture, LayerFn, Result, Router, apply_layers};
use httpant_server::{AxumAdapter, HyperAdapter, RunningServer, ServeOptions, ServerAdapter};

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

    /// Start serving `app` on a free local port.
    fn start(&self, app: App) -> BoxFuture<'_, Result<RunningServer>>;
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

    fn start(&self, app: App) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let svc = apply_layers(app.router.into_service(), app.layers);
            self.0.serve(svc, ServeOptions::new(free_listener()?)).await
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
    ]
}

/// A deliberately broken variant of the reference adapter. A case that passes on its mutant is itself a failing test.
pub struct MutantSubject(pub Mutant);

impl Subject for MutantSubject {
    fn name(&self) -> String {
        format!("mutant `{}`", self.0.id())
    }

    fn start(&self, app: App) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let svc = if self.0 == Mutant::NoMiddleware {
                app.router.into_service()
            } else {
                apply_layers(app.router.into_service(), app.layers)
            };
            let svc = mutants::apply(self.0, svc);
            HyperAdapter
                .serve(svc, ServeOptions::new(free_listener()?))
                .await
        })
    }
}
