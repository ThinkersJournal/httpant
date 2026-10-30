// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use http_body::{Body as _, Frame, SizeHint};
use httpant_core::http::StatusCode;
use httpant_core::{
    Body, BodyLimitExceeded, BoxFuture, BoxService, Bytes, DEFAULT_BODY_LIMIT, IntoResponse,
    Request, Response,
};
use tower::ServiceExt;

/// What the server accepts before a handler ever runs. The defaults are conservative; raise them on purpose, or take them off with
/// [`Limits::unlimited`] (a decision the code then shows).
///
/// The limits are enforced by HTTPAnt on the request each adapter hands over, so they mean the same thing on every adapter. They cannot stop
/// a web library from buffering what it reads before it gets that far: each library's own ceiling still applies (conformance case 14 uses sizes
/// below every adapter's own).
///
/// | limit | over it | default |
/// |---|---|---|
/// | request target (path and query) length | `414` | 8 KiB |
/// | number of header fields | `431` | 64 |
/// | total size of header names and values | `431` | 16 KiB |
/// | request body, declared or streamed | `413` | 2 MiB |
///
/// A body over its limit is refused at once when its `Content-Length` says so; a body that grows past it while streaming (chunked, or a lying
/// `Content-Length`) fails the read with [`httpant_core::Error::BodyTooLarge`], which the handler's error mapping answers with `413`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Longest request target (path and query) accepted, in bytes. `None`: no limit.
    pub max_uri_len: Option<usize>,
    /// Most header fields accepted (a repeated field counts once per occurrence). `None`: no limit.
    pub max_headers: Option<usize>,
    /// Largest total of header name and value bytes accepted. `None`: no limit.
    pub max_header_bytes: Option<usize>,
    /// Largest request body accepted, in bytes. `None`: no limit.
    pub max_body_bytes: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_uri_len: Some(8 * 1024),
            max_headers: Some(64),
            max_header_bytes: Some(16 * 1024),
            max_body_bytes: Some(DEFAULT_BODY_LIMIT as u64),
        }
    }
}

impl Limits {
    /// No limit of any kind: the web library's own ceilings are all that is left. Say so in the code that asks for it.
    pub fn unlimited() -> Self {
        Self {
            max_uri_len: None,
            max_headers: None,
            max_header_bytes: None,
            max_body_bytes: None,
        }
    }

    /// Longest request target accepted.
    pub fn max_uri_len(mut self, bytes: usize) -> Self {
        self.max_uri_len = Some(bytes);
        self
    }

    /// Most header fields accepted.
    pub fn max_headers(mut self, count: usize) -> Self {
        self.max_headers = Some(count);
        self
    }

    /// Largest total of header name and value bytes accepted.
    pub fn max_header_bytes(mut self, bytes: usize) -> Self {
        self.max_header_bytes = Some(bytes);
        self
    }

    /// Largest request body accepted (a streaming upload endpoint raises this on purpose).
    pub fn max_body_bytes(mut self, bytes: u64) -> Self {
        self.max_body_bytes = Some(bytes);
        self
    }

    /// Wrap `svc` so a request over any limit is answered before `svc` sees it. Every adapter calls this on the service it is given.
    pub fn wrap(self, svc: BoxService) -> BoxService {
        if self == Self::unlimited() {
            return svc;
        }
        BoxService::new(tower::service_fn(move |req: Request| {
            let svc = svc.clone();
            let limits = self;
            let step: BoxFuture<'static, Response> = Box::pin(async move {
                match limits.check(&req) {
                    Some(refusal) => refusal.into_response(),
                    None => svc
                        .oneshot(limits.limit_body(req))
                        .await
                        .unwrap_or_else(|e: Infallible| match e {}),
                }
            });
            async move { Ok::<_, Infallible>(step.await) }
        }))
    }

    /// The status to refuse `req` with, if it is over a limit that can be judged from its head.
    fn check(&self, req: &Request) -> Option<StatusCode> {
        if let Some(max) = self.max_uri_len {
            let len = req.uri().path_and_query().map_or(0, |p| p.as_str().len());
            if len > max {
                return Some(StatusCode::URI_TOO_LONG);
            }
        }
        if let Some(max) = self.max_headers
            && req.headers().len() > max
        {
            return Some(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
        }
        if let Some(max) = self.max_header_bytes {
            let total: usize = req
                .headers()
                .iter()
                .map(|(name, value)| name.as_str().len() + value.len())
                .sum();
            if total > max {
                return Some(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
            }
        }
        if let Some(max) = self.max_body_bytes
            && req.body().size_hint().lower() > max
        {
            return Some(StatusCode::PAYLOAD_TOO_LARGE);
        }
        None
    }

    /// Cap the bytes the handler can read from the body.
    fn limit_body(&self, req: Request) -> Request {
        let Some(max) = self.max_body_bytes else {
            return req;
        };
        let (parts, body) = req.into_parts();
        Request::from_parts(
            parts,
            Body::new(CappedBody {
                inner: body,
                remaining: max,
                limit: max,
            }),
        )
    }
}

/// A request body that fails with [`BodyLimitExceeded`] once more than `limit` bytes have come out of it.
struct CappedBody {
    inner: Body,
    remaining: u64,
    limit: u64,
}

impl http_body::Body for CappedBody {
    type Data = Bytes;
    type Error = httpant_core::BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let len = data.len() as u64;
                    if len > this.remaining {
                        this.remaining = 0;
                        return Poll::Ready(Some(Err(Box::new(BodyLimitExceeded::new(
                            this.limit,
                        )))));
                    }
                    this.remaining -= len;
                }
                Poll::Ready(Some(Ok(frame)))
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use httpant_core::Body;
    use httpant_core::http;

    use super::*;

    /// A service that reads the whole body with a limit above anything in these tests and answers its length: what a handler would see.
    fn echo_len() -> BoxService {
        BoxService::new(tower::service_fn(|req: Request| async move {
            let res = match req.into_body().bytes(usize::MAX / 2).await {
                Ok(bytes) => format!("{}", bytes.len()).into_response(),
                Err(e) => e.into_response(),
            };
            Ok::<_, Infallible>(res)
        }))
    }

    async fn status_of(limits: Limits, req: Request) -> (StatusCode, String) {
        let res = limits
            .wrap(echo_len())
            .oneshot(req)
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        let status = res.status();
        (status, res.into_body().text(1024).await.unwrap_or_default())
    }

    fn get(uri: &str) -> http::request::Builder {
        http::Request::builder().method("GET").uri(uri)
    }

    #[tokio::test]
    async fn the_request_target_limit() {
        let limits = Limits::default().max_uri_len(10);
        let at = get("/12345678").body(Body::empty()).unwrap(); // 9 bytes
        let edge = get("/123456789").body(Body::empty()).unwrap(); // 10 bytes: allowed
        let over = get("/1234567890").body(Body::empty()).unwrap(); // 11 bytes
        assert_eq!(status_of(limits, at).await.0, StatusCode::OK);
        assert_eq!(status_of(limits, edge).await.0, StatusCode::OK);
        assert_eq!(status_of(limits, over).await.0, StatusCode::URI_TOO_LONG);
        // The query counts: the limit is on the target, not the path.
        let long_query = get("/a?q=12345678").body(Body::empty()).unwrap();
        assert_eq!(
            status_of(limits, long_query).await.0,
            StatusCode::URI_TOO_LONG
        );
    }

    #[tokio::test]
    async fn the_header_count_limit() {
        let limits = Limits::default().max_headers(3);
        let with = |n: usize| {
            let mut b = get("/");
            for i in 0..n {
                b = b.header(format!("x-{i}"), "v");
            }
            b.body(Body::empty()).unwrap()
        };
        assert_eq!(status_of(limits, with(3)).await.0, StatusCode::OK);
        assert_eq!(
            status_of(limits, with(4)).await.0,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        );
        // A repeated field counts once per occurrence.
        let repeated = get("/")
            .header("a", "1")
            .header("a", "2")
            .header("a", "3")
            .header("a", "4")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status_of(limits, repeated).await.0,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn the_header_bytes_limit() {
        let limits = Limits::default().max_header_bytes(10);
        let ok = get("/")
            .header("abcde", "12345")
            .body(Body::empty())
            .unwrap(); // 5 + 5 = 10
        let over = get("/")
            .header("abcde", "123456")
            .body(Body::empty())
            .unwrap(); // 11
        assert_eq!(status_of(limits, ok).await.0, StatusCode::OK);
        assert_eq!(
            status_of(limits, over).await.0,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn a_declared_body_over_the_limit_is_refused_before_the_handler() {
        let limits = Limits::default().max_body_bytes(4);
        let ok = http::Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::from_bytes("1234"))
            .unwrap();
        let over = http::Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::from_bytes("12345"))
            .unwrap();
        assert_eq!(
            status_of(limits, ok).await,
            (StatusCode::OK, "4".to_owned())
        );
        assert_eq!(
            status_of(limits, over).await.0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn a_declared_body_over_the_limit_is_refused_even_when_the_handler_never_reads_it() {
        let ignores_body = BoxService::new(tower::service_fn(|_req: Request| async {
            Ok::<_, Infallible>("ran".into_response())
        }));
        let post = |len: usize| {
            http::Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::from_bytes(vec![0u8; len]))
                .unwrap()
        };
        let limits = Limits::default().max_body_bytes(4);
        let wrapped = limits.wrap(ignores_body);
        let ok = wrapped
            .clone()
            .oneshot(post(4))
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        assert_eq!(ok.status(), StatusCode::OK);
        let over = wrapped
            .oneshot(post(5))
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        assert_eq!(over.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_streamed_body_that_outgrows_the_limit_fails_the_read_with_413() {
        use http_body_util::StreamBody;
        // No size hint (as chunked, or a lying Content-Length): only the running count can catch it.
        let chunks = |n: usize| {
            let frames: Vec<Result<Frame<Bytes>, Infallible>> =
                (0..n).map(|_| Ok(Frame::data(Bytes::from("12")))).collect();
            Body::new(StreamBody::new(futures_util::stream::iter(frames)))
        };
        let limits = Limits::default().max_body_bytes(4);
        let post = |body| {
            http::Request::builder()
                .method("POST")
                .uri("/")
                .body(body)
                .unwrap()
        };
        assert_eq!(
            status_of(limits, post(chunks(2))).await,
            (StatusCode::OK, "4".to_owned())
        );
        let (status, body) = status_of(limits, post(chunks(3))).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    }

    #[tokio::test]
    async fn unlimited_takes_every_limit_off() {
        let long = "a".repeat(60_000); // the `http` crate itself refuses a URI over 65,534 bytes
        let req = get(&format!("/{long}"))
            .header("x", "y".repeat(100_000))
            .body(Body::from_bytes(vec![0u8; 5 * 1024 * 1024]))
            .unwrap();
        assert_eq!(
            status_of(Limits::unlimited(), req).await,
            (StatusCode::OK, (5 * 1024 * 1024).to_string())
        );
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let d = Limits::default();
        assert_eq!(d.max_uri_len, Some(8192));
        assert_eq!(d.max_headers, Some(64));
        assert_eq!(d.max_header_bytes, Some(16384));
        assert_eq!(d.max_body_bytes, Some(2 * 1024 * 1024));
    }
}
