use std::future::Future;

use crate::{BoxFuture, IntoResponse, Request, Response};

#[doc(hidden)]
pub struct NoArgs;
#[doc(hidden)]
pub struct WithRequest;

/// An async function (or closure) that answers a request: it takes nothing or the [`Request`] and returns anything [`IntoResponse`].
///
/// The first slice has these two shapes only; extractors (`Path`, `Query`) are not built yet. Path parameters are in the request's
/// extensions as [`PathParams`](crate::PathParams).
pub trait Handler<Marker>: Clone + Send + Sync + 'static {
    /// Run the handler.
    fn call(self, req: Request) -> BoxFuture<'static, Response>;
}

impl<F, Fut, R> Handler<NoArgs> for F
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: IntoResponse,
{
    fn call(self, _req: Request) -> BoxFuture<'static, Response> {
        Box::pin(async move { self().await.into_response() })
    }
}

impl<F, Fut, R> Handler<WithRequest> for F
where
    F: Fn(Request) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: IntoResponse,
{
    fn call(self, req: Request) -> BoxFuture<'static, Response> {
        Box::pin(async move { self(req).await.into_response() })
    }
}
