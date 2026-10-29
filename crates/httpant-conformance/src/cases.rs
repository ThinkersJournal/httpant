//! The case bodies. Each starts the shared application on the subject and speaks to it over a real socket with [`crate::raw`].

use httpant_core::BoxFuture;

use crate::app::{self, binary_body};
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
        // The request declares 10 MiB and sends only its head and 16 KiB: a server that refuses at the limit answers 413 without reading the
        // rest, and the client has nothing unread in flight that a connection reset could use to destroy the answer.
        let res = ask(
            server.addr(),
            RawRequest::new("POST", "/echo")
                .header("Content-Length", (10 * 1024 * 1024).to_string())
                .body(vec![b'x'; 16 * 1024]),
        )
        .await?;
        ensure!(
            res.status == 413,
            "a declared 10 MB body over a 1 MiB limit: expected 413, got {}",
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
