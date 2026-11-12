// SPDX-License-Identifier: MIT OR Apache-2.0
//! The request body read timeout: a client that stops sending its body cannot hold a handler (or the drain) for ever.
//!
//! Every adapter gets it the same way, as a wrapper on the one service they all run (see [`ServeOptions::wrap`](crate::ServeOptions::wrap)):
//! the request body is wrapped so that a wait for the next piece that lasts longer than [`Limits::body_read_timeout`] fails the read with
//! [`BodyReadTimeout`], which [`Body::bytes`](httpant_core::Body::bytes) turns into [`Error::BodyTimeout`](httpant_core::Error::BodyTimeout)
//! (`408`). It is an idle time, not a total, and it runs only while somebody is waiting for data: a handler that is busy elsewhere and comes
//! back to the body later is not charged for the time it was away.
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use http_body::{Body as _, Frame, SizeHint};
use httpant_core::{Body, BodyReadTimeout, BoxError, BoxService, Bytes, Request};
use tokio::time::Sleep;
use tower::ServiceExt;

use crate::Limits;

/// Wrap `svc` so a request body that stalls for `limits.body_read_timeout` fails its read. A zero timeout wraps nothing.
pub(crate) fn wrap(limits: Limits, svc: BoxService) -> BoxService {
    let idle = limits.body_read_timeout;
    if idle.is_zero() {
        return svc;
    }
    BoxService::new(tower::service_fn(move |req: Request| {
        let svc = svc.clone();
        async move {
            let (parts, body) = req.into_parts();
            let body = if body.is_end_stream() {
                body
            } else {
                Body::new(IdleBody {
                    inner: body,
                    idle,
                    waiting: None,
                })
            };
            Ok::<_, Infallible>(
                svc.oneshot(Request::from_parts(parts, body))
                    .await
                    .unwrap_or_else(|e| match e {}),
            )
        }
    }))
}

/// A request body that fails with [`BodyReadTimeout`] once a wait for data has lasted `idle`.
struct IdleBody {
    inner: Body,
    idle: Duration,
    /// Armed by the first poll that finds no data, cleared by the next piece that comes.
    waiting: Option<Pin<Box<Sleep>>>,
}

impl http_body::Body for IdleBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(item) => {
                this.waiting = None;
                Poll::Ready(item)
            }
            Poll::Pending => {
                let idle = this.idle;
                let sleep = this
                    .waiting
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
                if sleep.as_mut().poll(cx).is_ready() {
                    Poll::Ready(Some(Err(Box::new(BodyReadTimeout::new(idle)))))
                } else {
                    Poll::Pending
                }
            }
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
    use httpant_core::http;
    use httpant_core::http::StatusCode;
    use httpant_core::{Error, IntoResponse, Response};
    use tokio::sync::mpsc;

    use super::*;

    /// A body fed from a channel: what the test sends is what arrives, when it arrives.
    struct Fed(mpsc::Receiver<Bytes>);

    impl http_body::Body for Fed {
        type Data = Bytes;
        type Error = BoxError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            self.0
                .poll_recv(cx)
                .map(|piece| piece.map(|b| Ok(Frame::data(b))))
        }
    }

    /// Reads the whole body and answers with its length, or with the error's own status.
    fn reader() -> BoxService {
        BoxService::new(tower::service_fn(|req: Request| async move {
            let res: Response = match req.into_body().bytes(1 << 30).await {
                Ok(b) => b.len().to_string().into_response(),
                Err(e @ Error::BodyTimeout { .. }) => e.into_response(),
                Err(e) => e.into_response(),
            };
            Ok::<_, Infallible>(res)
        }))
    }

    fn request(rx: mpsc::Receiver<Bytes>) -> Request {
        http::Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::new(Fed(rx)))
            .expect("request")
    }

    #[tokio::test]
    async fn a_body_that_stalls_fails_with_408() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Bytes::from_static(b"abc")).await.unwrap();
        let svc = wrap(
            Limits::default().body_read_timeout(Duration::from_millis(150)),
            reader(),
        );
        let started = std::time::Instant::now();
        let res = svc
            .oneshot(request(rx))
            .await
            .unwrap_or_else(|e| match e {});
        assert_eq!(res.status(), StatusCode::REQUEST_TIMEOUT);
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(100) && took < Duration::from_secs(3),
            "{took:?}"
        );
        drop(tx);
    }

    /// An idle time, not a total: pieces that keep coming, each inside the timeout, are read to the end however long that takes.
    #[tokio::test]
    async fn a_body_that_keeps_moving_is_never_cut() {
        let (tx, rx) = mpsc::channel(4);
        let svc = wrap(
            Limits::default().body_read_timeout(Duration::from_millis(200)),
            reader(),
        );
        let feeder = tokio::spawn(async move {
            for _ in 0..8 {
                tokio::time::sleep(Duration::from_millis(80)).await;
                tx.send(Bytes::from_static(b"xx")).await.unwrap();
            }
        });
        let res = svc
            .oneshot(request(rx))
            .await
            .unwrap_or_else(|e| match e {});
        feeder.await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn zero_switches_it_off() {
        let (tx, rx) = mpsc::channel(4);
        let svc = wrap(
            Limits::default().body_read_timeout(Duration::ZERO),
            reader(),
        );
        let feeder = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            tx.send(Bytes::from_static(b"late")).await.unwrap();
        });
        let res = svc
            .oneshot(request(rx))
            .await
            .unwrap_or_else(|e| match e {});
        feeder.await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// The time a handler spends away from the body is not charged to the client.
    #[tokio::test]
    async fn the_clock_runs_only_while_somebody_waits() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Bytes::from_static(b"abc")).await.unwrap();
        let slow_start = BoxService::new(tower::service_fn(|req: Request| async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let n = req.into_body().bytes(1 << 20).await.map_or(0, |b| b.len());
            Ok::<_, Infallible>(n.to_string().into_response())
        }));
        let svc = wrap(
            Limits::default().body_read_timeout(Duration::from_millis(150)),
            slow_start,
        );
        drop(tx);
        let res = svc
            .oneshot(request(rx))
            .await
            .unwrap_or_else(|e| match e {});
        assert_eq!(res.status(), StatusCode::OK);
    }
}
