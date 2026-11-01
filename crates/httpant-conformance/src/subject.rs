// SPDX-License-Identifier: MIT OR Apache-2.0
use std::time::Duration;

use httpant_core::{BoxFuture, Capabilities, LayerFn, Result, Router, apply_layers};
use httpant_server::{
    ActixAdapter, AxumAdapter, HyperAdapter, PoemAdapter, RunningServer, SalvoAdapter,
    ServeOptions, ServerAdapter, TrustedProxies, WarpAdapter,
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

    /// Start serving `app` on a free local port, set up as `setup` says.
    fn start_with(&self, app: App, setup: Setup) -> BoxFuture<'_, Result<RunningServer>>;

    /// Start serving `app` on a free local port with the default [`Setup`]: a long grace period (the cases that are not about it must never
    /// meet it) and no trusted proxy.
    fn start(&self, app: App) -> BoxFuture<'_, Result<RunningServer>> {
        self.start_with(app, Setup::default())
    }

    /// What the subject says it can do, if it is a real adapter (a mutant says nothing).
    fn declared(&self) -> Option<Capabilities> {
        None
    }
}

/// The grace period of every case that is not about the grace period.
const LONG_GRACE: Duration = Duration::from_secs(60);

/// What a case may choose about the server it starts.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The shutdown grace period (`None`: wait for ever).
    pub grace: Option<Duration>,
    /// Peers trusted to forward the client's address and scheme.
    pub trusted_proxies: TrustedProxies,
    /// Terminate TLS with this acceptor (case 17).
    pub tls: Option<httpant_server::TlsAcceptor>,
    /// Accept cleartext HTTP/2 by prior knowledge (case 19); off by default, as everywhere.
    pub h2c: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            grace: Some(LONG_GRACE),
            trusted_proxies: TrustedProxies::none(),
            tls: None,
            h2c: false,
        }
    }
}

fn options(listener: std::net::TcpListener, setup: Setup) -> ServeOptions {
    let opts = ServeOptions::new(listener)
        .trusted_proxies(setup.trusted_proxies)
        .h2c(setup.h2c);
    let opts = match setup.tls {
        Some(tls) => opts.tls(tls),
        None => opts,
    };
    match setup.grace {
        Some(period) => opts.shutdown_grace(period),
        None => opts.no_shutdown_grace(),
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

    fn start_with(&self, app: App, setup: Setup) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            // The rule `Server` applies: TLS is never started on an adapter that does not declare it.
            if setup.tls.is_some() {
                httpant_server::require_tls(&self.0)?;
            }
            // And the same for h2c: never asked of an adapter that cannot serve it.
            if setup.h2c {
                httpant_server::require_h2c(&self.0)?;
            }
            let svc = apply_layers(app.router.into_service(), app.layers);
            self.0.serve(svc, options(free_listener()?, setup)).await
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

    fn start_with(&self, app: App, mut setup: Setup) -> BoxFuture<'_, Result<RunningServer>> {
        match self.0 {
            // Waits for ever, whatever period it is asked for.
            Mutant::NoShutdownGrace => setup.grace = None,
            // Believes whatever any peer says about the client.
            Mutant::TrustAnyPeer => {
                setup.trusted_proxies = TrustedProxies::none()
                    .parse("0.0.0.0/0")
                    .and_then(|t| t.parse("::/0"))
                    .expect("valid networks");
            }
            // Believes nobody, the proxy it was told to trust included.
            Mutant::IgnoreTrustedProxies => setup.trusted_proxies = TrustedProxies::none(),
            // Asked for TLS, serves cleartext.
            Mutant::ServeTlsInClear => setup.tls = None,
            // Has a server of its own, below.
            Mutant::OffersH2WithoutServingIt => {}
            // Accepts the HTTP/2 preface whether or not it was asked to.
            Mutant::H2cOnByDefault => setup.h2c = true,
            // Asked for h2c, serves HTTP/1.1 only.
            Mutant::IgnoresH2cRequest => setup.h2c = false,
            _ => {}
        }
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
            if self.0 == Mutant::IgnoreConnectionClose {
                return mutants::serve_stripping(svc, free_listener()?, &["connection: close"]);
            }
            if self.0 == Mutant::SkipExpectContinue {
                return mutants::serve_stripping(svc, free_listener()?, &["expect: 100-continue"]);
            }
            if self.0 == Mutant::OffersH2WithoutServingIt {
                return mutants::serve_http1_only_behind_h2_offer(
                    svc,
                    options(free_listener()?, setup).limits(self.0.limits()),
                );
            }
            HyperAdapter
                .serve(
                    svc,
                    options(free_listener()?, setup).limits(self.0.limits()),
                )
                .await
        })
    }
}
