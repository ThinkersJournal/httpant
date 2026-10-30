// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt server half (design `docs/HTTPANT-DESIGN.md`, section 4): the [`Server`] builder, the [`ServerAdapter`] trait, and the
//! reference [`HyperAdapter`]. Other adapters arrive one slice at a time, each only after it passes the conformance suite.

#[cfg(feature = "actix-web")]
mod actix_adapter;
mod adapter;
#[cfg(feature = "axum")]
mod axum_adapter;
mod challenge;
mod edge;
mod grace;
mod hyper_adapter;
mod limits;
#[cfg(feature = "poem")]
mod poem_adapter;
#[cfg(all(feature = "poem", feature = "tls"))]
mod poem_tls;
mod redirect;
#[cfg(feature = "salvo")]
mod salvo_adapter;
#[cfg(all(feature = "salvo", feature = "tls"))]
mod salvo_tls;
#[cfg(feature = "tls")]
mod tls;
#[cfg(feature = "tls")]
mod tls_audit;
#[cfg(all(feature = "tls", feature = "axum"))]
mod tls_listener;
#[cfg(feature = "warp")]
mod warp_adapter;

use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "actix-web")]
pub use actix_adapter::ActixAdapter;
pub use adapter::{FORCE_WAIT, RunningServer, ServeOptions, ServerAdapter};
#[cfg(feature = "axum")]
pub use axum_adapter::AxumAdapter;
pub use challenge::{CHALLENGE_PREFIX, ChallengeResponder};
pub use edge::{BadProxy, Origin, PeerAddr, Scheme, TrustedProxies};
pub use grace::{DEFAULT_SHUTDOWN_GRACE, Grace};
pub use httpant_core as core;
use httpant_core::http::Method;
use httpant_core::{BoxService, Error, Handler, LayerFn, Result, Router, apply_layers, layer_fn};
pub use hyper_adapter::HyperAdapter;
pub use limits::Limits;
#[cfg(feature = "poem")]
pub use poem_adapter::PoemAdapter;
pub use redirect::HttpsRedirect;
#[cfg(feature = "salvo")]
pub use salvo_adapter::SalvoAdapter;
#[cfg(feature = "tls")]
pub use tls::{
    CertSource, CertSourceError, DEFAULT_HANDSHAKE_TIMEOUT, FixedCert, ServedCert, ServerCert,
    TlsAcceptor, TlsConnection, read_back,
};
#[cfg(feature = "tls")]
pub use tls_audit::{RefusalReason, TlsAudit, TlsEvent};
use tokio::net::ToSocketAddrs;
#[cfg(feature = "warp")]
pub use warp_adapter::WarpAdapter;

/// The ids of the adapters compiled into this build.
pub const ADAPTER_IDS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "actix-web")]
    "actix-web",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
    #[cfg(feature = "warp")]
    "warp",
];

/// Describes routes and layers once, then runs them on an adapter chosen by id (default `hyper`).
pub struct Server {
    adapter: Arc<dyn ServerAdapter>,
    router: Router,
    layers: Vec<LayerFn>,
    limits: Limits,
    grace: Option<Option<Duration>>,
    trusted_proxies: TrustedProxies,
    challenge: Option<Arc<dyn ChallengeResponder>>,
    https_redirect: Option<HttpsRedirect>,
    #[cfg(feature = "tls")]
    tls: Option<TlsAcceptor>,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    /// A server on the default adapter, hyper.
    pub fn new() -> Self {
        Self {
            adapter: Arc::new(HyperAdapter),
            router: Router::new(),
            layers: Vec::new(),
            limits: Limits::default(),
            grace: None,
            trusted_proxies: TrustedProxies::none(),
            challenge: None,
            https_redirect: None,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// A server on the adapter with this id. An unknown id is an error that lists the ids that exist.
    pub fn with_adapter(id: &str) -> Result<Self> {
        match id {
            "hyper" => Ok(Self::new()),
            #[cfg(feature = "axum")]
            "axum" => Ok(Self::with_adapter_impl(AxumAdapter)),
            #[cfg(not(feature = "axum"))]
            "axum" => Err(Error::AdapterNotCompiledIn { id: id.to_owned() }),
            #[cfg(feature = "actix-web")]
            "actix-web" => Ok(Self::with_adapter_impl(ActixAdapter)),
            #[cfg(not(feature = "actix-web"))]
            "actix-web" => Err(Error::AdapterNotCompiledIn { id: id.to_owned() }),
            #[cfg(feature = "poem")]
            "poem" => Ok(Self::with_adapter_impl(PoemAdapter)),
            #[cfg(not(feature = "poem"))]
            "poem" => Err(Error::AdapterNotCompiledIn { id: id.to_owned() }),
            #[cfg(feature = "salvo")]
            "salvo" => Ok(Self::with_adapter_impl(SalvoAdapter)),
            #[cfg(not(feature = "salvo"))]
            "salvo" => Err(Error::AdapterNotCompiledIn { id: id.to_owned() }),
            #[cfg(feature = "warp")]
            "warp" => Ok(Self::with_adapter_impl(WarpAdapter)),
            #[cfg(not(feature = "warp"))]
            "warp" => Err(Error::AdapterNotCompiledIn { id: id.to_owned() }),
            _ => Err(Error::UnknownAdapter {
                id: id.to_owned(),
                available: ADAPTER_IDS.to_vec(),
            }),
        }
    }

    /// A server on an adapter you built yourself (also how a conformance mutant is plugged in).
    pub fn with_adapter_impl(adapter: impl ServerAdapter) -> Self {
        Self {
            adapter: Arc::new(adapter),
            router: Router::new(),
            layers: Vec::new(),
            limits: Limits::default(),
            grace: None,
            trusted_proxies: TrustedProxies::none(),
            challenge: None,
            https_redirect: None,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// Serve with these limits (request target, header count and size, body size) instead of the conservative defaults, see [`Limits`].
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Peers trusted to forward the client's address and scheme, see [`ServeOptions::trusted_proxies`] and [`Origin`].
    pub fn trusted_proxies(mut self, proxies: TrustedProxies) -> Self {
        self.trusted_proxies = proxies;
        self
    }

    /// Terminate TLS on the listener with certificates from a [`CertSource`], see [`TlsAcceptor`]. An adapter that cannot terminate TLS makes
    /// `serve`/`bind` fail with [`Error::TlsNotSupported`]; it never serves in clear.
    #[cfg(feature = "tls")]
    pub fn tls(mut self, acceptor: TlsAcceptor) -> Self {
        self.tls = Some(acceptor);
        self
    }

    /// Redirect cleartext requests for the listed hosts to HTTPS (`308`, any other host `421`), see [`HttpsRedirect`]. This is the port-80
    /// server: give it the [`acme_challenge`](Self::acme_challenge) responder and the challenge prefix keeps working there.
    pub fn redirect_to_https(mut self, redirect: HttpsRedirect) -> Self {
        self.https_redirect = Some(redirect);
        self
    }

    /// Answer the ACME HTTP-01 challenge prefix (`/.well-known/acme-challenge/<token>`) from `responder` before any route, on any adapter and on
    /// cleartext and TLS listeners alike, see [`ChallengeResponder`]. HTTPAnt does no ACME itself.
    pub fn acme_challenge(mut self, responder: impl ChallengeResponder) -> Self {
        self.challenge = Some(Arc::new(responder));
        self
    }

    /// How long `shutdown()` waits for requests in flight before ending them (default 30 s), see [`ServeOptions::shutdown_grace`].
    pub fn shutdown_grace(mut self, period: Duration) -> Self {
        self.grace = Some(Some(period));
        self
    }

    /// Wait for requests in flight for ever (see [`ServeOptions::no_shutdown_grace`]).
    pub fn no_shutdown_grace(mut self) -> Self {
        self.grace = Some(None);
        self
    }

    /// Add a route for any method.
    pub fn route<M>(mut self, method: Method, path: &str, handler: impl Handler<M>) -> Self {
        self.router = self.router.route(method, path, handler);
        self
    }

    /// `GET`.
    pub fn get<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::GET, path, handler)
    }

    /// `POST`.
    pub fn post<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::POST, path, handler)
    }

    /// Wrap everything added so far in a Tower layer; the last layer added sees the request first.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: tower::Layer<BoxService> + Send + 'static,
        L::Service: tower::Service<
                httpant_core::Request,
                Response = httpant_core::Response,
                Error = std::convert::Infallible,
            > + Clone
            + Send
            + 'static,
        <L::Service as tower::Service<httpant_core::Request>>::Future: Send + 'static,
    {
        self.layers.push(layer_fn(layer));
        self
    }

    /// The routes and layers as the one service the adapter runs.
    pub fn into_service(self) -> BoxService {
        apply_layers(self.router.into_service(), self.layers)
    }

    /// Bind `addr` (port 0 picks a free port) and start serving.
    pub async fn bind(self, addr: impl ToSocketAddrs) -> Result<RunningServer> {
        let listener = tokio::net::TcpListener::bind(addr).await?.into_std()?;
        self.serve(listener).await
    }

    /// Serve on a socket that is already bound.
    pub async fn serve(self, listener: std::net::TcpListener) -> Result<RunningServer> {
        let adapter = Arc::clone(&self.adapter);
        let (limits, grace, proxies) = (self.limits, self.grace, self.trusted_proxies.clone());
        let opts = ServeOptions::new(listener)
            .limits(limits)
            .trusted_proxies(proxies);
        let opts = match self.challenge.clone() {
            Some(responder) => opts.acme_challenge(responder),
            None => opts,
        };
        let opts = match self.https_redirect.clone() {
            Some(redirect) => opts.redirect_to_https(redirect),
            None => opts,
        };
        #[cfg(feature = "tls")]
        let opts = match self.tls.clone() {
            None => opts,
            Some(acceptor) => {
                if adapter.capabilities().tls != httpant_core::Support::Yes {
                    return Err(Error::TlsNotSupported {
                        adapter: adapter.id(),
                    });
                }
                opts.tls(acceptor)
            }
        };
        let opts = match grace {
            None => opts,
            Some(Some(period)) => opts.shutdown_grace(period),
            Some(None) => opts.no_shutdown_grace(),
        };
        adapter.serve(self.into_service(), opts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_adapter_id_lists_the_ids_that_exist() {
        let err = Server::with_adapter("rocket")
            .err()
            .expect("rocket is not an adapter yet");
        assert_eq!(
            err.to_string(),
            format!("unknown adapter `rocket`: use one of {ADAPTER_IDS:?}")
        );
        assert!(Server::with_adapter("hyper").is_ok());
    }

    #[cfg(feature = "axum")]
    #[test]
    fn axum_is_chosen_by_id_when_compiled_in() {
        assert!(Server::with_adapter("axum").is_ok());
        assert!(ADAPTER_IDS.contains(&"axum"));
    }

    #[cfg(not(feature = "axum"))]
    #[test]
    fn axum_without_its_feature_says_which_feature_to_enable() {
        let err = Server::with_adapter("axum")
            .err()
            .expect("axum is not compiled in");
        assert_eq!(
            err.to_string(),
            "adapter `axum` is not compiled in: enable the `axum` feature"
        );
    }

    #[test]
    fn the_hyper_adapter_declares_what_it_does_not_do() {
        let c = HyperAdapter.capabilities();
        assert_eq!(c.local_handlers, httpant_core::Support::No);
        assert_eq!(c.graceful_shutdown, httpant_core::Support::Yes);
    }
}
