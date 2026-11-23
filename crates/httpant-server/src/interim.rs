// SPDX-License-Identifier: MIT OR Apache-2.0
//! A handler that returns a 1xx status (ROADMAP row 20; pm-ants' ruling 2026-11-22): the answer is `500`, on every adapter.
//!
//! A `1xx` is an interim response: a client that reads `100 Continue` as the whole answer waits for the real one, and `101 Switching
//! Protocols` says the connection is now something else, which nothing here ever makes it. The adapters did not agree on what to do with one
//! (measured at `cbd8e47`: five answered `500`, actix-web sent the `1xx` itself as the final answer, and all six wrote `101`), so the one
//! service they all run turns a handler's `1xx` into `500` before an adapter sees it. HTTPAnt has no API for sending a real interim response
//! (`103 Early Hints` and the like), so nothing legitimate is refused.

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};

use httpant_core::http::StatusCode;
use httpant_core::{BoxService, IntoResponse, Request};
use tower::ServiceExt;

/// Wrap `svc` so a `1xx` from a handler becomes `500`. `pass` turns it off (conformance mutant only).
pub(crate) fn wrap(pass: bool, svc: BoxService) -> BoxService {
    if pass {
        return svc;
    }
    BoxService::new(tower::service_fn(move |req: Request| {
        let svc = svc.clone();
        async move {
            let res = svc
                .oneshot(req)
                .await
                .unwrap_or_else(|e: Infallible| match e {});
            if res.status().is_informational() {
                note(res.status());
                return Ok::<_, Infallible>(StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
            Ok(res)
        }
    }))
}

/// One line on stderr the first time it happens in this process (not one per request: a handler that always does it would flood the log).
fn note(status: StatusCode) {
    static SAID: AtomicBool = AtomicBool::new(false);
    if !SAID.swap(true, Ordering::Relaxed) {
        eprintln!(
            "httpant: a handler returned status {status}, an interim (1xx) status that is not a final answer; the client was sent 500 instead (shown once per process)"
        );
    }
}
