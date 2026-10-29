use bytes::Bytes;
use http::StatusCode;
use http::header::{CONTENT_TYPE, HeaderValue};

use crate::{Body, Error, Response};

/// Anything that can become a [`Response`].
pub trait IntoResponse {
    /// Convert into a response.
    fn into_response(self) -> Response;
}

fn with_type(body: Body, content_type: &'static str) -> Response {
    let mut res = Response::new(body);
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    res
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        Response::new(Body::empty())
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        let mut res = Response::new(Body::empty());
        *res.status_mut() = self;
        res
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        with_type(
            Body::from_bytes(Bytes::from_static(self.as_bytes())),
            "text/plain; charset=utf-8",
        )
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        with_type(Body::from_bytes(self), "text/plain; charset=utf-8")
    }
}

impl IntoResponse for Bytes {
    fn into_response(self) -> Response {
        with_type(Body::from_bytes(self), "application/octet-stream")
    }
}

impl IntoResponse for Vec<u8> {
    fn into_response(self) -> Response {
        Bytes::from(self).into_response()
    }
}

impl<T: IntoResponse> IntoResponse for (StatusCode, T) {
    fn into_response(self) -> Response {
        let mut res = self.1.into_response();
        *res.status_mut() = self.0;
        res
    }
}

impl<T: IntoResponse, E: IntoResponse> IntoResponse for Result<T, E> {
    fn into_response(self) -> Response {
        match self {
            Ok(v) => v.into_response(),
            Err(e) => e.into_response(),
        }
    }
}

/// The default error mapper: the status from [`Error::status`] and its canonical reason as the body. Internal detail (the message, a
/// wrapped source error) is never put in the body.
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status();
        (status, status.canonical_reason().unwrap_or("Error")).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn text(res: Response) -> String {
        res.into_body().text(1024).await.unwrap()
    }

    #[tokio::test]
    async fn str_is_200_text_plain() {
        let res = "hello".into_response();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[CONTENT_TYPE], "text/plain; charset=utf-8");
        assert_eq!(text(res).await, "hello");
    }

    #[tokio::test]
    async fn status_with_body_keeps_both() {
        let res = (StatusCode::IM_A_TEAPOT, "short and stout").into_response();
        assert_eq!(res.status().as_u16(), 418);
        assert_eq!(text(res).await, "short and stout");
    }

    #[tokio::test]
    async fn bare_status_has_empty_body() {
        let res = StatusCode::SERVICE_UNAVAILABLE.into_response();
        assert_eq!(res.status().as_u16(), 503);
        assert_eq!(text(res).await, "");
    }

    #[tokio::test]
    async fn bytes_are_not_reinterpreted_as_text() {
        let res = vec![0xffu8, 0x00, 0xfe].into_response();
        assert_eq!(res.headers()[CONTENT_TYPE], "application/octet-stream");
        assert_eq!(
            res.into_body().bytes(8).await.unwrap().as_ref(),
            &[0xff, 0x00, 0xfe]
        );
    }

    #[tokio::test]
    async fn result_maps_either_arm() {
        let ok: Result<&'static str, Error> = Ok("fine");
        assert_eq!(text(ok.into_response()).await, "fine");
        let err: Result<&'static str, Error> = Err(Error::BodyTooLarge { limit: 1 });
        let res = err.into_response();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn default_error_mapper_leaks_no_internal_detail() {
        let secret = "db password hunter2 at 10.0.0.7";
        let res = Error::Body(secret.into()).into_response();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = text(res).await;
        assert_eq!(body, "Bad Request");
        assert!(!body.contains("hunter2"));
    }

    #[test]
    fn every_error_variant_has_a_status_and_ends_with_advice() {
        let all = [
            Error::BodyTooLarge { limit: 1 },
            Error::Body("x".into()),
            Error::NotUtf8,
            Error::AdapterNotCompiledIn {
                id: "rocket".into(),
            },
        ];
        for e in all {
            assert!(
                e.status().is_client_error() || e.status().is_server_error(),
                "{e}"
            );
            assert!(
                e.to_string().contains(':') || e.to_string().contains(';'),
                "no advice in: {e}"
            );
        }
        assert_eq!(
            Error::AdapterNotCompiledIn {
                id: "rocket".into()
            }
            .to_string(),
            "adapter `rocket` is not compiled in: enable the `rocket` feature"
        );
    }
}
