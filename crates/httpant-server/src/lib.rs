//! HTTPAnt server half (design `docs/HTTPANT-DESIGN.md`, section 4): the [`Server`] builder, the [`ServerAdapter`] trait, and the
//! reference [`HyperAdapter`]. Other adapters arrive one slice at a time, each only after it passes the conformance suite.

mod adapter;
mod hyper_adapter;

use std::sync::Arc;

pub use adapter::{RunningServer, ServeOptions, ServerAdapter};
pub use httpant_core as core;
use httpant_core::http::Method;
use httpant_core::{BoxService, Error, Handler, LayerFn, Result, Router, apply_layers, layer_fn};
pub use hyper_adapter::HyperAdapter;
use tokio::net::ToSocketAddrs;

/// The ids of the adapters compiled into this build.
pub const ADAPTER_IDS: &[&str] = &["hyper"];

/// Describes routes and layers once, then runs them on an adapter chosen by id (default `hyper`).
pub struct Server {
    adapter: Arc<dyn ServerAdapter>,
    router: Router,
    layers: Vec<LayerFn>,
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
        }
    }

    /// A server on the adapter with this id. An unknown id is an error that lists the ids that exist.
    pub fn with_adapter(id: &str) -> Result<Self> {
        match id {
            "hyper" => Ok(Self::new()),
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
        }
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
        adapter
            .serve(self.into_service(), ServeOptions::new(listener))
            .await
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
            "unknown adapter `rocket`: use one of [\"hyper\"]"
        );
        assert!(Server::with_adapter("hyper").is_ok());
    }

    #[test]
    fn the_hyper_adapter_declares_what_it_does_not_do() {
        let c = HyperAdapter.capabilities();
        assert_eq!(c.local_handlers, httpant_core::Support::No);
        assert_eq!(c.graceful_shutdown, httpant_core::Support::Unknown);
    }
}
