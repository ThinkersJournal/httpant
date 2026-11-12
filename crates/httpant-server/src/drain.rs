// SPDX-License-Identifier: MIT OR Apache-2.0
//! A refusal must reach the client: reading what the handler left of the request body before the answer is sent.
//!
//! A server that answers a request while its body is still arriving, and then closes the connection with that body unread, makes the stack
//! send a TCP reset on some systems (Windows measured: a refused `GET` with a 1 MB or 5 MB body shows the client `ConnectionAborted` and no
//! status, 30 of 30 times; 50 B and 70 KB bodies fit the socket buffers and were answered). The reset discards the answer that was already
//! on its way, so the client cannot tell a refusal from a crash. The fix has to be below the handler and the same for every adapter, so it is
//! a wrapper on the one service every adapter runs (see [`ServeOptions::wrap`](crate::ServeOptions::wrap)): when the answer is ready and
//! the request body has not been read to its end, read and discard up to [`Limits::max_drain_bytes`] of it, for at most
//! [`Limits::drain_timeout`], then send the answer. A body that is not finished by then makes the answer say `Connection: close`.
//!
//! Where it does nothing: HTTP/2 (a refused stream is reset on its own, not the connection), a request with `Expect` (the client is waiting for
//! the answer before it sends the body; reading would make the library invite it), no body at all, a declared length over the cap (draining
//! would only fail late), and a body something still holds when the answer is ready (a response that streams the request back).
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use http_body::{Body as _, Frame, SizeHint};
use httpant_core::http::Version;
use httpant_core::http::header::{CONNECTION, CONTENT_LENGTH, EXPECT, HeaderValue};
use httpant_core::{Body, BoxError, BoxService, Bytes, Request, Response};
use tower::ServiceExt;

use crate::Limits;

/// Wrap `svc` so an answer sent before the request body was read is not lost to a connection reset.
pub(crate) fn wrap(limits: Limits, svc: BoxService) -> BoxService {
    let (cap, timeout) = (limits.max_drain_bytes, limits.drain_timeout);
    if cap == 0 {
        return svc;
    }
    BoxService::new(tower::service_fn(move |req: Request| {
        let svc = svc.clone();
        async move { Ok::<_, Infallible>(serve(svc, req, cap, timeout).await) }
    }))
}

/// Whether the body of `req` is worth reading after the handler: there is one, it is not a case listed in the module documentation.
fn may_drain(req: &Request, cap: u64) -> bool {
    if req.version() == Version::HTTP_2 || req.version() == Version::HTTP_3 {
        return false;
    }
    if req.headers().contains_key(EXPECT) || req.body().is_end_stream() {
        return false;
    }
    let declared = req
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    declared.is_none_or(|n| n <= cap)
}

async fn serve(svc: BoxService, req: Request, cap: u64, timeout: Duration) -> Response {
    if !may_drain(&req, cap) {
        return svc.oneshot(req).await.unwrap_or_else(|e| match e {});
    }
    let (parts, body) = req.into_parts();
    let shared = Arc::new(Mutex::new(body));
    let req = Request::from_parts(
        parts,
        Body::new(Tracked {
            shared: Arc::clone(&shared),
        }),
    );
    let mut res = svc.oneshot(req).await.unwrap_or_else(|e| match e {});
    // The handler and its request are gone. If the response still holds the body (it streams it back), it is not ours to read.
    let Ok(body) = Arc::try_unwrap(shared) else {
        return res;
    };
    let body = body.into_inner().unwrap_or_else(PoisonError::into_inner);
    if !finished(body, cap, timeout).await {
        res.headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("close"));
    }
    res
}

/// Read and discard `body` up to `cap` bytes within `timeout`. True when it ended; false when the cap, the time or an error stopped it first.
async fn finished(mut body: Body, cap: u64, timeout: Duration) -> bool {
    let drain = async {
        let mut read = 0u64;
        while !body.is_end_stream() {
            let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
            match frame {
                None => break,
                Some(Err(_)) => return false,
                Some(Ok(frame)) => {
                    read += frame.data_ref().map_or(0, |d| d.len() as u64);
                    if read > cap {
                        return false;
                    }
                }
            }
        }
        true
    };
    tokio::time::timeout(timeout, drain).await.unwrap_or(false)
}

/// The request body as the handler sees it, with the other hand on it kept by [`serve`], so the body can still be read after the handler has
/// dropped it. Locked only for the length of one poll.
struct Tracked {
    shared: Arc<Mutex<Body>>,
}

impl Tracked {
    fn inner(&self) -> std::sync::MutexGuard<'_, Body> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl http_body::Body for Tracked {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        Pin::new(&mut *self.inner()).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner().is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner().size_hint()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use httpant_core::http;
    use httpant_core::http::StatusCode;
    use httpant_core::{BoxFuture, IntoResponse};

    use super::*;

    /// A body of `chunks` pieces of `size` bytes that counts what was taken from it.
    struct Counted {
        left: usize,
        size: usize,
        taken: Arc<AtomicU64>,
    }

    impl http_body::Body for Counted {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            if self.left == 0 {
                return Poll::Ready(None);
            }
            self.left -= 1;
            let size = self.size;
            self.taken.fetch_add(size as u64, Ordering::SeqCst);
            Poll::Ready(Some(Ok(Frame::data(Bytes::from(vec![0u8; size])))))
        }

        fn is_end_stream(&self) -> bool {
            self.left == 0
        }
    }

    fn refuse() -> BoxService {
        BoxService::new(tower::service_fn(|_req: Request| async {
            Ok::<_, Infallible>(StatusCode::FORBIDDEN.into_response())
        }))
    }

    fn request() -> http::request::Builder {
        http::Request::builder().method("GET").uri("/")
    }

    fn with_body(
        builder: http::request::Builder,
        chunks: usize,
        size: usize,
        taken: &Arc<AtomicU64>,
    ) -> Request {
        builder
            .body(Body::new(Counted {
                left: chunks,
                size,
                taken: Arc::clone(taken),
            }))
            .expect("request")
    }

    fn call(limits: Limits, req: Request) -> BoxFuture<'static, Response> {
        let svc = wrap(limits, refuse());
        Box::pin(async move { svc.oneshot(req).await.unwrap_or_else(|e| match e {}) })
    }

    #[tokio::test]
    async fn an_unread_body_is_read_to_its_end_before_the_refusal_is_sent() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(request(), 40, 1024, &taken);
        let res = call(Limits::default(), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(taken.load(Ordering::SeqCst), 40 * 1024);
        assert!(res.headers().get(CONNECTION).is_none());
    }

    #[tokio::test]
    async fn a_body_past_the_cap_stops_the_drain_and_the_answer_closes_the_connection() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(request(), 100, 1024, &taken);
        let res = call(Limits::default().max_drain_bytes(10 * 1024), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let n = taken.load(Ordering::SeqCst);
        assert!(
            (10 * 1024..=12 * 1024).contains(&n),
            "stopped near the cap, took {n}"
        );
        assert_eq!(res.headers().get(CONNECTION).unwrap(), "close");
    }

    #[tokio::test]
    async fn draining_off_reads_nothing() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(request(), 40, 1024, &taken);
        let res = call(Limits::default().max_drain_bytes(0), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(taken.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_expect_request_is_not_read() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(request().header("expect", "100-continue"), 40, 1024, &taken);
        let res = call(Limits::default(), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(taken.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn http_2_is_not_read() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(request().version(Version::HTTP_2), 40, 1024, &taken);
        let res = call(Limits::default(), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(taken.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_declared_length_over_the_cap_is_not_read() {
        let taken = Arc::new(AtomicU64::new(0));
        let req = with_body(
            request().header("content-length", "999999999"),
            40,
            1024,
            &taken,
        );
        let res = call(Limits::default(), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(taken.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_request_without_a_body_costs_nothing() {
        let req = http::Request::builder()
            .method("GET")
            .uri("/")
            .body(Body::empty())
            .expect("request");
        let res = call(Limits::default(), req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(res.headers().get(CONNECTION).is_none());
    }

    /// A handler that reads the whole body itself leaves nothing to drain, and the answer is not touched.
    #[tokio::test]
    async fn a_body_the_handler_read_is_left_alone() {
        let taken = Arc::new(AtomicU64::new(0));
        let reads = BoxService::new(tower::service_fn(|req: Request| async move {
            let n = req.into_body().bytes(1 << 30).await.map_or(0, |b| b.len());
            Ok::<_, Infallible>(n.to_string().into_response())
        }));
        let svc = wrap(Limits::default(), reads);
        let req = with_body(request(), 5, 100, &taken);
        let res = svc.oneshot(req).await.unwrap_or_else(|e| match e {});
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(CONNECTION).is_none());
        assert_eq!(taken.load(Ordering::SeqCst), 500);
    }
}
