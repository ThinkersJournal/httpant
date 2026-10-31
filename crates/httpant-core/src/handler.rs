// SPDX-License-Identifier: MIT OR Apache-2.0
use std::future::Future;

use crate::extract::Extracted;
use crate::{BoxFuture, FromRequest, IntoResponse, Request, Response};

#[doc(hidden)]
pub struct NoArgs;
#[doc(hidden)]
pub struct WithRequest;

/// An async function (or closure) that answers a request and returns anything [`IntoResponse`]. It takes nothing, the [`Request`], or from one
/// to four extractors ([`Path`](crate::Path), [`Query`](crate::Query), [`PathParams`](crate::PathParams), or your own [`FromRequest`]), which
/// are built before the handler runs; one that fails is answered instead, and the handler does not run. Path parameters are also in the
/// request's extensions as [`PathParams`](crate::PathParams).
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

macro_rules! extractors {
    ($($t:ident),+) => {
        #[allow(non_snake_case)]
        impl<F, Fut, R, $($t),+> Handler<Extracted<($($t,)+)>> for F
        where
            F: Fn($($t),+) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = R> + Send + 'static,
            R: IntoResponse,
            $($t: FromRequest),+
        {
            fn call(self, req: Request) -> BoxFuture<'static, Response> {
                Box::pin(async move {
                    $(let $t = match $t::from_request(&req) {
                        Ok(v) => v,
                        Err(res) => return *res,
                    };)+
                    drop(req);
                    self($($t),+).await.into_response()
                })
            }
        }
    };
}

extractors!(T1);
extractors!(T1, T2);
extractors!(T1, T2, T3);
extractors!(T1, T2, T3, T4);
