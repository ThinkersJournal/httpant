// SPDX-License-Identifier: MIT OR Apache-2.0
//! The case bodies. Each starts the shared application on the subject and speaks to it over a real socket with [`crate::raw`].

use httpant_core::BoxFuture;

use crate::app::{self, BODY_LIMIT, binary_body};
use crate::raw::{RawRequest, RawResponse, send};
use crate::subject::Subject;

type Outcome = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

async fn ask(addr: std::net::SocketAddr, req: RawRequest) -> Result<RawResponse, String> {
    send(addr, &req)
        .await
        .map_err(|e| format!("request failed: {e}"))
}

fn header_lines(res: &RawResponse) -> String {
    res.headers
        .iter()
        .map(|(k, v)| {
            format!(
                "{}: {}",
                String::from_utf8_lossy(k),
                String::from_utf8_lossy(v)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn text(res: &RawResponse) -> String {
    String::from_utf8_lossy(&res.body).into_owned()
}

macro_rules! serve {
    ($subject:expr) => {
        $subject
            .start(app::app())
            .await
            .map_err(|e| format!("could not start: {e}"))?
    };
}

pub fn method_fidelity(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        let addr = server.addr();
        for method in ["PROPFIND", "TRACE"] {
            let res = ask(addr, RawRequest::new(method, "/m")).await?;
            ensure!(
                res.status == 200 && text(&res) == method,
                "{method} /m: expected 200 `{method}`, got {} `{}`",
                res.status,
                text(&res)
            );
        }
        let res = ask(addr, RawRequest::new("MKCOL", "/m")).await?;
        ensure!(
            res.status == 405,
            "MKCOL on a known path: expected 405, got {} `{}`",
            res.status,
            text(&res)
        );
        ensure!(
            !res.header_values("allow").is_empty(),
            "405 without an Allow header"
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn status_fidelity(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        for code in [200u16, 204, 301, 404, 418, 503] {
            let res = ask(
                server.addr(),
                RawRequest::new("GET", format!("/status/{code}")),
            )
            .await?;
            ensure!(
                res.status == code,
                "status {code}: the client received {}",
                res.status
            );
        }
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn middleware_applied(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        let res = ask(server.addr(), RawRequest::new("GET", "/m")).await?;
        ensure!(
            res.header_values("x-layer") == [b"a".as_slice()],
            "header layer did not take effect: {}",
            header_lines(&res)
        );
        let res = ask(
            server.addr(),
            RawRequest::new("GET", "/m").header("x-block", "1"),
        )
        .await?;
        ensure!(
            res.status == 403 && text(&res) == "blocked",
            "short-circuiting layer did not take effect: {} `{}`",
            res.status,
            text(&res)
        );
        ensure!(
            res.header_values("x-layer") == [b"a".as_slice()],
            "outer layer missing on the short-circuited response"
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn headers(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        let res = ask(server.addr(), RawRequest::new("GET", "/cookies")).await?;
        let cookies = res.header_values("set-cookie");
        ensure!(
            cookies == [b"a=1".as_slice(), b"b=2".as_slice()],
            "expected two Set-Cookie lines, got {cookies:?}"
        );
        let res = ask(
            server.addr(),
            RawRequest::new("GET", "/hdr").header("x-raw", [0xffu8, 0xfe]),
        )
        .await?;
        ensure!(
            text(&res) == "fffe",
            "non-UTF8 header bytes: handler saw `{}`",
            text(&res)
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn bodies(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        let res = ask(
            server.addr(),
            RawRequest::new("POST", "/echo").body("hello"),
        )
        .await?;
        ensure!(
            res.status == 200 && res.body == b"hello",
            "POST body: got {} `{}`",
            res.status,
            text(&res)
        );
        // The request declares 1.5 MiB (over the handler's 1 MiB, under the server's 2 MiB default limit, so it is the handler's limit being tested) and sends only its head and 16 KiB: a server that refuses at the limit answers 413 without reading the
        // rest, and the client has nothing unread in flight that a connection reset could use to destroy the answer.
        let res = ask(
            server.addr(),
            RawRequest::new("POST", "/echo")
                .header("Content-Length", (BODY_LIMIT + BODY_LIMIT / 2).to_string())
                .body(vec![b'x'; 16 * 1024]),
        )
        .await?;
        ensure!(
            res.status == 413,
            "a declared 1.5 MiB body over a 1 MiB handler limit: expected 413, got {}",
            res.status
        );
        let res = ask(server.addr(), RawRequest::new("GET", "/bin")).await?;
        ensure!(
            res.body == binary_body(),
            "binary response differs ({} bytes received)",
            res.body.len()
        );
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn request_identity(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        for (method, target, want) in [
            ("GET", "/who/abc?x=1&y=2", "GET /who/abc x=1&y=2"),
            ("POST", "/who/abc?z=3", "POST /who/abc z=3"),
        ] {
            let res = ask(server.addr(), RawRequest::new(method, target)).await?;
            ensure!(
                text(&res) == want,
                "{method} {target}: handler saw `{}`, expected `{want}`",
                text(&res)
            );
        }
        let _ = server.shutdown().await;
        Ok(())
    })
}

pub fn path_rule(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        for (target, want) in [("/a", 200u16), ("/a/", 404), ("/b", 404), ("/b/", 200)] {
            let res = ask(server.addr(), RawRequest::new("GET", target)).await?;
            ensure!(
                res.status == want,
                "GET {target}: expected {want}, got {}",
                res.status
            );
        }
        let _ = server.shutdown().await;
        Ok(())
    })
}

/// Case 13: a percent-encoded path reaches the handler decoded, and the encodings that could smuggle a different path are refused.
pub fn path_encoding(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        for (target, status, body) in [
            ("/param/a%20b", 200u16, Some("a b|a%20b")),
            ("/param/caf%C3%A9", 200, Some("caf\u{e9}|caf%C3%A9")),
            ("/param/a+b", 200, Some("a+b|a+b")),
            ("/file/x/a%20b/c", 200, Some("x/a b/c|x/a%20b/c")),
            // Refused before any handler runs: bad escape, not UTF-8, encoded separators and NUL, encoded dot segments.
            ("/param/%zz", 400, None),
            ("/param/%ff", 400, None),
            ("/param/a%2Fb", 400, None),
            ("/param/a%5Cb", 400, None),
            ("/param/%00", 400, None),
            ("/param/%2e%2e", 400, None),
            ("/file/a/%2e%2e/b", 400, None),
            // Literals are matched raw: the encoded spelling of a route is not that route.
            ("/%61", 404, None),
        ] {
            let res = ask(server.addr(), RawRequest::new("GET", target)).await?;
            ensure!(
                res.status == status,
                "GET {target}: expected {status}, got {} `{}`",
                res.status,
                text(&res)
            );
            if let Some(body) = body {
                ensure!(
                    text(&res) == body,
                    "GET {target}: the handler saw `{}`, expected `{body}`",
                    text(&res)
                );
            }
        }
        let _ = server.shutdown().await;
        Ok(())
    })
}

/// Case 14: the server's default limits hold. The sizes are chosen below every adapter's own ceiling, so it is HTTPAnt's limit that answers.
pub fn limits(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let server = serve!(s);
        let addr = server.addr();
        let d = httpant_server::Limits::default();
        let max_uri = d.max_uri_len.expect("a default");
        let max_header_bytes = d.max_header_bytes.expect("a default");
        let max_body = d.max_body_bytes.expect("a default");

        // Request target: at the limit is served, one over is 414.
        for (extra, want) in [(0usize, 200u16), (1, 414)] {
            let target = format!("/m?q={}", "a".repeat(max_uri - "/m?q=".len() + extra));
            let res = ask(addr, RawRequest::new("GET", target.as_bytes())).await?;
            ensure!(
                res.status == want,
                "a request target of {} bytes (limit {max_uri}): expected {want}, got {}",
                target.len(),
                res.status
            );
        }

        // Header fields: a few are served, far more than the default are 431.
        for (count, want) in [
            (20usize, 200u16),
            (d.max_headers.expect("a default") + 10, 431),
        ] {
            let mut req = RawRequest::new("GET", "/m");
            for i in 0..count {
                req = req.header(format!("x-limit-{i}"), "v");
            }
            let res = ask(addr, req).await?;
            ensure!(
                res.status == want,
                "{count} header fields: expected {want}, got {}",
                res.status
            );
        }

        // Header bytes: one field under the limit is served, one over is 431.
        for (size, want) in [
            (max_header_bytes - 1024, 200u16),
            (max_header_bytes + 1024, 431),
        ] {
            let res = ask(
                addr,
                RawRequest::new("GET", "/m").header("x-big", "b".repeat(size)),
            )
            .await?;
            ensure!(
                res.status == want,
                "a header field of {size} bytes (limit {max_header_bytes} for all): expected {want}, got {}",
                res.status
            );
        }

        // Body, declared: refused on the head. The request announces 3 MiB and sends 16 KiB (a server that refuses early and closes with
        // unread data can reset the connection; a 413 that arrives is the answer).
        let res = ask(
            addr,
            RawRequest::new("POST", "/sink")
                .header("Content-Length", (max_body + 1024 * 1024).to_string())
                .body(vec![b'x'; 16 * 1024]),
        )
        .await?;
        ensure!(
            res.status == 413,
            "a declared body of {} bytes (limit {max_body}): expected 413, got {}",
            max_body + 1024 * 1024,
            res.status
        );

        // Body, streamed (chunked, so there is nothing to announce): exactly the limit is served, one byte over is 413.
        for (total, want) in [(max_body, 200u16), (max_body + 1, 413)] {
            let mut conn = crate::raw::Conn::open(addr)
                .await
                .map_err(|e| format!("could not connect: {e}"))?
                .with_timeout(std::time::Duration::from_secs(5));
            conn.write(
                b"POST /sink HTTP/1.1\r\nHost: conformance\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .await
            .map_err(|e| format!("send head: {e}"))?;
            let piece = 64 * 1024u64;
            let mut sent = 0u64;
            while sent < total {
                let n = piece.min(total - sent);
                let mut chunk = format!("{n:x}\r\n").into_bytes();
                chunk.extend(std::iter::repeat_n(b'x', n as usize));
                chunk.extend_from_slice(b"\r\n");
                // The server may answer and stop reading at the limit; a write that fails after that is the answer being in flight.
                if conn.write(&chunk).await.is_err() {
                    break;
                }
                sent += n;
            }
            let _ = conn.write(b"0\r\n\r\n").await;
            let res = conn
                .response("POST")
                .await
                .map_err(|e| format!("a streamed body of {total} bytes: no answer: {e}"))?;
            ensure!(
                res.status == want,
                "a streamed body of {total} bytes (limit {max_body}): expected {want}, got {} `{}`",
                res.status,
                text(&res)
            );
        }
        let _ = server.shutdown().await;
        Ok(())
    })
}
