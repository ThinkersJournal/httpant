// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use http_body::{Frame, SizeHint};
use httpant_core::http::StatusCode;
use httpant_core::http::header::{CONNECTION, HeaderValue};
use httpant_core::{Body, BoxError, BoxService, Bytes, IntoResponse, Request, Response};
use tokio::sync::watch;
use tower::ServiceExt;

/// How long shutdown waits for requests in flight, and the switch that ends them when the time is up.
///
/// An adapter gets one from [`ServeOptions::grace`](crate::ServeOptions::grace) and hands it to [`RunningServer::new`](crate::RunningServer::new);
/// the service it serves is wrapped with [`ServeOptions::wrap`](crate::ServeOptions::wrap), which carries the other end of the switch. Nothing
/// else is needed from the web library, so the grace period works the same on every adapter.
#[derive(Clone, Debug)]
pub struct Grace {
    /// `None`: wait for ever (a decision the code then shows).
    pub(crate) period: Option<Duration>,
    pub(crate) force: Arc<watch::Sender<bool>>,
}

/// The grace period when nothing says otherwise.
pub const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

impl Grace {
    /// A grace period of `period` (`None`: wait for ever) with its own switch.
    pub fn new(period: Option<Duration>) -> Self {
        Self {
            period,
            force: Arc::new(watch::channel(false).0),
        }
    }

    /// The period shutdown waits for requests in flight before ending them.
    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    /// End every request still in flight: the handler is dropped, the client is answered `503` with `Connection: close` (or, for a response
    /// already streaming, the connection is cut), and the web library's own drain can finish.
    pub(crate) fn force(&self) {
        self.force.send_replace(true);
    }

    /// Wrap `svc` so that [`Grace::force`] ends the requests it is running.
    pub fn wrap(&self, svc: BoxService) -> BoxService {
        let force = Arc::clone(&self.force);
        BoxService::new(tower::service_fn(move |req: Request| {
            let svc = svc.clone();
            let mut forced = force.subscribe();
            async move {
                let run = svc.oneshot(req);
                tokio::pin!(run);
                let res = tokio::select! {
                    res = &mut run => res.unwrap_or_else(|e: Infallible| match e {}),
                    () = wait_forced(&mut forced) => return Ok::<_, Infallible>(refused()),
                };
                let (parts, body) = res.into_parts();
                let body = Body::new(Cuttable::new(body, forced));
                Ok(Response::from_parts(parts, body))
            }
        }))
    }
}

async fn wait_forced(forced: &mut watch::Receiver<bool>) {
    // A closed channel means nobody can force any more: wait for ever.
    if forced.wait_for(|f| *f).await.is_err() {
        std::future::pending::<()>().await;
    }
}

fn refused() -> Response {
    let mut res = StatusCode::SERVICE_UNAVAILABLE.into_response();
    res.headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    res
}

/// What a forced shutdown reports to a response that was still streaming.
#[derive(Debug)]
struct ShutdownForced;

impl std::fmt::Display for ShutdownForced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the server is shutting down and the grace period is over")
    }
}

impl std::error::Error for ShutdownForced {}

/// A response body that fails once shutdown is forced, so a stream that never ends cannot keep the connection (and shutdown) open.
struct Cuttable {
    inner: Body,
    /// Completes when shutdown is forced. Kept across polls: dropping it would drop the waker it registered.
    forced: Pin<Box<dyn Future<Output = ()> + Send>>,
    cut: bool,
}

impl Cuttable {
    fn new(inner: Body, mut forced: watch::Receiver<bool>) -> Self {
        Self {
            inner,
            forced: Box::pin(async move { wait_forced(&mut forced).await }),
            cut: false,
        }
    }
}

impl http_body::Body for Cuttable {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        if !this.cut && this.forced.as_mut().poll(cx).is_ready() {
            this.cut = true;
        }
        if this.cut {
            return Poll::Ready(Some(Err(Box::new(ShutdownForced))));
        }
        Pin::new(&mut this.inner).poll_frame(cx)
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
    use std::convert::Infallible;

    use httpant_core::http;

    use super::*;

    fn hangs() -> BoxService {
        BoxService::new(tower::service_fn(|_req: Request| async {
            std::future::pending::<()>().await;
            Ok::<_, Infallible>("never".into_response())
        }))
    }

    fn get() -> Request {
        http::Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn a_request_in_flight_is_ended_with_503_and_connection_close_when_forced() {
        let grace = Grace::new(Some(Duration::from_secs(1)));
        let svc = grace.wrap(hangs());
        let pending = tokio::spawn(svc.oneshot(get()));
        tokio::task::yield_now().await;
        assert!(!pending.is_finished(), "the request is in flight");
        grace.force();
        let res = tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .expect("the request was ended after the force")
            .unwrap()
            .unwrap_or_else(|e: Infallible| match e {});
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res.headers()[CONNECTION], "close");
    }

    #[tokio::test]
    async fn a_request_that_finishes_before_the_force_is_untouched() {
        let grace = Grace::new(Some(Duration::from_secs(1)));
        let quick = BoxService::new(tower::service_fn(|_req: Request| async {
            Ok::<_, Infallible>("done".into_response())
        }));
        let res = grace
            .wrap(quick)
            .oneshot(get())
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.into_body().text(16).await.unwrap(), "done");
    }

    #[tokio::test]
    async fn a_response_still_streaming_when_forced_is_cut() {
        use http_body_util::StreamBody;
        let grace = Grace::new(Some(Duration::from_secs(1)));
        let endless = BoxService::new(tower::service_fn(|_req: Request| async {
            let body = StreamBody::new(futures_util::stream::pending::<
                Result<Frame<Bytes>, Infallible>,
            >());
            Ok::<_, Infallible>(Response::new(Body::new(body)))
        }));
        let res = grace
            .wrap(endless)
            .oneshot(get())
            .await
            .unwrap_or_else(|e: Infallible| match e {});
        let read = tokio::spawn(res.into_body().bytes(1024));
        tokio::task::yield_now().await;
        assert!(!read.is_finished(), "the body is streaming");
        grace.force();
        let err = tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .expect("the body was cut after the force")
            .unwrap()
            .unwrap_err();
        assert!(err.to_string().contains("grace period is over"), "{err}");
    }
}
