// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;

use tower::Layer;
use tower::Service;

use crate::{BoxService, Request, Response};

/// A Tower layer, erased so a list of them can be held and applied later.
pub type LayerFn = Box<dyn FnOnce(BoxService) -> BoxService + Send>;

/// Erase any Tower `Layer` whose service answers HTTPAnt requests.
pub fn layer_fn<L>(layer: L) -> LayerFn
where
    L: Layer<BoxService> + Send + 'static,
    L::Service: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    <L::Service as Service<Request>>::Future: Send + 'static,
{
    Box::new(move |inner| BoxService::new(layer.layer(inner)))
}

/// Wrap `svc` in `layers`, in order: the first layer is innermost and the last is outermost (the last one added sees the request first).
pub fn apply_layers(svc: BoxService, layers: Vec<LayerFn>) -> BoxService {
    layers.into_iter().fold(svc, |inner, layer| layer(inner))
}
