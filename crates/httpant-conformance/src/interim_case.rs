// SPDX-License-Identifier: MIT OR Apache-2.0
//! Case 25: a handler that returns a 1xx status (ROADMAP row 20). A `1xx` is interim, not a final answer, and HTTPAnt has no way to send a
//! real interim response, so on every adapter the client gets `500` with no body, never the `1xx` (actix-web used to send it as the final
//! answer, and all six wrote `101 Switching Protocols` with no upgrade). Final statuses next to it are untouched.

use std::time::Duration;

use httpant_core::BoxFuture;

use crate::app::app;
use crate::raw::{RawRequest, send};
use crate::subject::{Setup, Subject};

type Outcome = Result<(), String>;

pub fn interim_statuses(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = s
            .start_with(app(), Setup::default())
            .await
            .map_err(|e| format!("could not start: {e}"))?;
        let addr = server.addr();
        let ask = |code: u16| async move {
            tokio::time::timeout(
                Duration::from_secs(5),
                send(
                    addr,
                    &RawRequest::new("GET", format!("/status/{code}"))
                        .header("Connection", "close"),
                ),
            )
            .await
            .map_err(|_| format!("/status/{code}: no answer within 5 s"))?
            .map_err(|e| format!("/status/{code}: request failed: {e}"))
        };
        let mut bad = Vec::new();
        for code in [100u16, 101, 102, 103, 199] {
            match ask(code).await {
                Ok(res) if res.status == 500 && res.body.is_empty() => {}
                Ok(res) => bad.push(format!(
                    "a handler's {code} reached the client as {} with {} body bytes (it must be 500, empty)",
                    res.status,
                    res.body.len()
                )),
                Err(e) => bad.push(e),
            }
        }
        // The control: final statuses either side of the interim range still arrive as sent, so the refusal is not "everything odd is 500".
        for code in [200u16, 204, 404, 599] {
            match ask(code).await {
                Ok(res) if res.status == code => {}
                Ok(res) => bad.push(format!("control: {code} arrived as {}", res.status)),
                Err(e) => bad.push(format!("control: {e}")),
            }
        }
        let _ = server.shutdown().await;
        if bad.is_empty() {
            Ok(())
        } else {
            Err(bad.join("; "))
        }
    })
}
