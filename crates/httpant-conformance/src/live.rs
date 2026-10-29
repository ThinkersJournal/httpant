// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cases 8-12: what only a live connection shows. Streaming in both directions, shutdown with a request in flight, keep-alive and
//! `Expect: 100-continue`, many requests at once, HEAD. Each waits on a [`Gates`] signal where it needs "the handler is running" or "the
//! handler may go on", never on a sleep, so no case depends on timing except the generous ceilings that make a broken adapter fail.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use httpant_core::{BoxFuture, Support};
use tokio::net::TcpStream;
use tokio::sync::{Notify, Semaphore};

use crate::app;
use crate::raw::{Conn, RawRequest, send};
use crate::subject::Subject;

type Outcome = Result<(), String>;

/// A broken adapter fails the "arrives promptly" checks after this, not after the 10 s default.
const PROMPT: Duration = Duration::from_secs(5);

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

macro_rules! serve {
    ($subject:expr) => {{
        let (app, gates) = app::app_with_gates();
        let server = $subject
            .start(app)
            .await
            .map_err(|e| format!("could not start: {e}"))?;
        (server, gates)
    }};
}

async fn open(addr: SocketAddr) -> Result<Conn, String> {
    Ok(Conn::open(addr)
        .await
        .map_err(|e| format!("could not connect: {e}"))?
        .with_timeout(PROMPT))
}

fn text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

async fn signalled(gate: &Notify) -> bool {
    tokio::time::timeout(PROMPT, gate.notified()).await.is_ok()
}

/// 8a: the first chunk of a response reaches the client while the handler is still producing the rest.
pub async fn streaming_response(s: &dyn Subject) -> Outcome {
    let (server, gates) = serve!(s);
    let mut conn = open(server.addr()).await?;
    conn.send(&RawRequest::new("GET", "/stream"))
        .await
        .map_err(|e| format!("send: {e}"))?;
    let head = conn
        .head()
        .await
        .map_err(|e| format!("response head: {e}"))?;
    ensure!(head.status == 200, "GET /stream: status {}", head.status);
    let first = conn.chunk().await.map_err(|e| {
        format!("the first response chunk did not arrive while the handler was still running (the response is buffered): {e}")
    })?;
    ensure!(
        first.as_deref() == Some(b"one".as_slice()),
        "first chunk: expected `one`, got {:?}",
        first.as_deref().map(text)
    );
    gates.stream_release.notify_one();
    let second = conn
        .chunk()
        .await
        .map_err(|e| format!("second chunk: {e}"))?;
    ensure!(
        second.as_deref() == Some(b"two".as_slice()),
        "second chunk: expected `two`, got {:?}",
        second.as_deref().map(text)
    );
    let end = conn.chunk().await.map_err(|e| format!("end: {e}"))?;
    ensure!(end.is_none(), "expected the end of the body after `two`");
    let _ = server.shutdown().await;
    Ok(())
}

/// 8b: the handler sees the first chunk of a request body while the client is still sending the rest.
pub async fn streaming_request(s: &dyn Subject) -> Outcome {
    let (server, gates) = serve!(s);
    let mut conn = open(server.addr()).await?;
    conn.write(
        b"POST /upload HTTP/1.1\r\nHost: conformance\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n",
    )
    .await
    .map_err(|e| format!("send: {e}"))?;
    ensure!(
        signalled(&gates.upload_first_chunk).await,
        "the handler did not see the first request chunk while the client was still sending (the request body is buffered)"
    );
    conn.write(b"3\r\ndef\r\n0\r\n\r\n")
        .await
        .map_err(|e| format!("send rest: {e}"))?;
    let res = conn
        .response("POST")
        .await
        .map_err(|e| format!("response: {e}"))?;
    ensure!(
        res.status == 200 && res.body == b"got 6",
        "expected 200 `got 6`, got {} `{}`",
        res.status,
        text(&res.body)
    );
    let _ = server.shutdown().await;
    Ok(())
}

pub fn streaming(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        streaming_response(s).await?;
        streaming_request(s).await
    })
}

/// Case 9: after shutdown is requested, a request in flight still completes, an idle keep-alive connection is closed, no new connection
/// is accepted, and `shutdown` returns only once the request has completed.
pub fn graceful_shutdown(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let (server, gates) = serve!(s);
        let addr = server.addr();

        let mut busy = open(addr).await?;
        busy.send(&RawRequest::new("GET", "/slow"))
            .await
            .map_err(|e| format!("send /slow: {e}"))?;
        ensure!(
            signalled(&gates.slow_started).await,
            "the /slow handler never started"
        );
        let mut idle = open(addr).await?;
        let first = idle
            .exchange(&RawRequest::new("GET", "/m"))
            .await
            .map_err(|e| format!("keep-alive connection, first request: {e}"))?;
        ensure!(first.status == 200, "GET /m: status {}", first.status);

        let stopping = tokio::spawn(server.shutdown());

        // New connections are refused (the listener is closed), within a few seconds of the request.
        let deadline = tokio::time::Instant::now() + PROMPT;
        loop {
            if TcpStream::connect(addr).await.is_err() {
                break;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "a new connection was still accepted {PROMPT:?} after shutdown was requested"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        ensure!(
            idle.closed_by_peer(PROMPT).await,
            "an idle keep-alive connection was still open {PROMPT:?} after shutdown was requested"
        );
        ensure!(
            !stopping.is_finished(),
            "shutdown returned while a request was still in flight"
        );

        gates.slow_release.notify_one();
        let res = busy
            .response("GET")
            .await
            .map_err(|e| format!("the request in flight was cut by shutdown: {e}"))?;
        ensure!(
            res.status == 200 && res.body == b"finished",
            "the request in flight: expected 200 `finished`, got {} `{}`",
            res.status,
            text(&res.body)
        );
        match tokio::time::timeout(PROMPT, stopping).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(format!("shutdown failed: {e}")),
            Ok(Err(e)) => Err(format!("shutdown task: {e}")),
            Err(_) => Err("shutdown did not return after the last request completed".to_owned()),
        }
    })
}

/// Case 10: connections are reused, `Connection: close` is honoured, `Expect: 100-continue` gets its interim response.
pub fn connection_handling(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let (server, _gates) = serve!(s);
        let addr = server.addr();

        // Keep-alive: two requests, one connection.
        let mut conn = open(addr).await?;
        for n in 1..=2 {
            let res = conn
                .exchange(&RawRequest::new("GET", "/m"))
                .await
                .map_err(|e| format!("request {n} on one connection: {e}"))?;
            ensure!(
                res.status == 200 && res.body == b"GET",
                "request {n} on one connection: {} `{}`",
                res.status,
                text(&res.body)
            );
        }

        // Connection: close is honoured.
        let mut closing = open(addr).await?;
        let res = closing
            .exchange(&RawRequest::new("GET", "/m").header("Connection", "close"))
            .await
            .map_err(|e| format!("Connection: close request: {e}"))?;
        ensure!(
            res.status == 200,
            "Connection: close: status {}",
            res.status
        );
        ensure!(
            closing.closed_by_peer(PROMPT).await,
            "the server kept the connection open after `Connection: close`"
        );

        // Expect: 100-continue: the interim response comes before the body is sent.
        let mut expect = open(addr).await?;
        expect
            .write(
                b"POST /echo HTTP/1.1\r\nHost: conformance\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\n",
            )
            .await
            .map_err(|e| format!("send head: {e}"))?;
        let interim = expect
            .head()
            .await
            .map_err(|e| format!("no `100 Continue` before the body was sent: {e}"))?;
        ensure!(
            interim.status == 100,
            "expected `100 Continue`, got {}",
            interim.status
        );
        expect
            .write(b"hello")
            .await
            .map_err(|e| format!("send body: {e}"))?;
        let res = expect
            .response("POST")
            .await
            .map_err(|e| format!("final response: {e}"))?;
        ensure!(
            res.status == 200 && res.body == b"hello",
            "after 100-continue: expected 200 `hello`, got {} `{}`",
            res.status,
            text(&res.body)
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

/// How many requests case 11 sends, and how many are in flight at once.
const REQUESTS: usize = 1000;
const IN_FLIGHT: usize = 250;

/// Case 11: 1,000 requests, each asking for its own number back, 250 at a time, answered correctly (no response goes to the wrong
/// client) and fast (each handler pauses, so an adapter that handles one request at a time cannot finish in time).
pub fn concurrency(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let (server, _gates) = serve!(s);
        let addr = server.addr();
        let permits = Arc::new(Semaphore::new(IN_FLIGHT));
        let mut tasks = Vec::with_capacity(REQUESTS);
        for n in 0..REQUESTS {
            let permits = Arc::clone(&permits);
            tasks.push(tokio::spawn(async move {
                let _slot = permits.acquire().await.map_err(|e| e.to_string())?;
                let res = send(addr, &RawRequest::new("GET", format!("/delay/{n}")))
                    .await
                    .map_err(|e| format!("request {n}: {e}"))?;
                if res.status != 200 || res.body != n.to_string().as_bytes() {
                    return Err(format!(
                        "request {n}: got {} `{}`",
                        res.status,
                        text(&res.body)
                    ));
                }
                Ok::<_, String>(())
            }));
        }
        let all = async {
            let mut failures = Vec::new();
            for t in tasks {
                match t.await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => failures.push(e),
                    Err(e) => failures.push(e.to_string()),
                }
            }
            failures
        };
        let failures = tokio::time::timeout(Duration::from_secs(8), all)
            .await
            .map_err(|_| format!("{REQUESTS} requests were not all answered within 8 s"))?;
        ensure!(
            failures.is_empty(),
            "{} of {REQUESTS} requests failed; first: {}",
            failures.len(),
            failures[0]
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

/// Case 12: HEAD returns the headers GET returns and no body, and leaves the connection usable.
pub fn head(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let (server, _gates) = serve!(s);
        let mut conn = open(server.addr()).await?;
        let get = conn
            .exchange(&RawRequest::new("GET", "/hello"))
            .await
            .map_err(|e| format!("GET /hello: {e}"))?;
        ensure!(
            get.status == 200 && get.body == b"hello",
            "GET /hello: {} `{}`",
            get.status,
            text(&get.body)
        );
        let head = conn
            .exchange(&RawRequest::new("HEAD", "/hello"))
            .await
            .map_err(|e| format!("HEAD /hello: {e}"))?;
        ensure!(head.status == 200, "HEAD /hello: status {}", head.status);
        ensure!(head.body.is_empty(), "HEAD /hello carried a body");
        for name in ["content-length", "content-type", "x-hello"] {
            ensure!(
                !get.header_values(name).is_empty(),
                "the GET response has no `{name}` (the case is wrong)"
            );
            ensure!(
                head.header_values(name) == get.header_values(name),
                "HEAD `{name}`: expected {:?}, got {:?}",
                get.header_values(name)
                    .iter()
                    .map(|v| text(v))
                    .collect::<Vec<_>>(),
                head.header_values(name)
                    .iter()
                    .map(|v| text(v))
                    .collect::<Vec<_>>()
            );
        }
        // Stray body bytes after a HEAD response would corrupt the next exchange on this connection.
        let next = conn
            .exchange(&RawRequest::new("GET", "/m"))
            .await
            .map_err(|e| format!("GET after HEAD on the same connection: {e}"))?;
        ensure!(
            next.status == 200 && next.body == b"GET",
            "GET after HEAD: {} `{}`",
            next.status,
            text(&next.body)
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

/// What the suite measured about a subject, as the three [`Support`] values the capability matrix reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    /// Case 8b.
    pub streaming_request_bodies: Support,
    /// Case 8a.
    pub streaming_response_bodies: Support,
    /// Case 9.
    pub graceful_shutdown: Support,
}

fn support(r: &Outcome) -> Support {
    if r.is_ok() { Support::Yes } else { Support::No }
}

/// Run the capability probes against `subject`.
pub async fn measure(s: &dyn Subject) -> Measured {
    Measured {
        streaming_request_bodies: support(&streaming_request(s).await),
        streaming_response_bodies: support(&streaming_response(s).await),
        graceful_shutdown: support(&graceful_shutdown(s).await),
    }
}
