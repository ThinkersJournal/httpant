//! HTTPAnt core types (design: `docs/HTTPANT-DESIGN.md`, sections 1-3).
//!
//! The request, response, method, status, header and URI types are the [`http`] crate's, re-exported unchanged, so the information the old
//! `web-server-abstraction` adapters lost at the framework boundary has no way to be lost here. This slice holds the types only: no router,
//! handler trait or adapter yet.

mod body;
mod capabilities;
mod error;
mod response;

pub use bytes::Bytes;
pub use http;

pub use body::{Body, BoxError};
pub use capabilities::{Capabilities, DEFAULT_BODY_LIMIT, Support};
pub use error::{Error, Result};
pub use response::IntoResponse;

/// A request as every adapter hands it to the service: `http::Request` over the streaming [`Body`].
pub type Request = http::Request<Body>;
/// A response as the service returns it.
pub type Response = http::Response<Body>;
/// The one contract an adapter runs (design section 3): the whole router and its middleware compiled to a single boxed Tower service.
pub type BoxService = tower::util::BoxService<Request, Response, std::convert::Infallible>;

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn a_closure_service_boxes_and_answers() {
        let svc: BoxService = BoxService::new(tower::service_fn(|req: Request| async move {
            Ok::<_, Infallible>(format!("{} {}", req.method(), req.uri().path()).into_response())
        }));
        let req = http::Request::builder()
            .method("PROPFIND")
            .uri("/a?b=1")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.into_body().text(64).await.unwrap(), "PROPFIND /a");
    }

    #[test]
    fn capabilities_start_unknown_with_a_visible_body_limit() {
        let c = Capabilities::default();
        assert_eq!(c.graceful_shutdown, Support::Unknown);
        assert_eq!(c.default_body_limit, DEFAULT_BODY_LIMIT);
    }
}
