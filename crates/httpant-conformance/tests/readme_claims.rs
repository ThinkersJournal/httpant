// SPDX-License-Identifier: MIT OR Apache-2.0
//! README claims that no other test ran as written (claim-to-command audit, 2026-11-19). Each test names the README sentence it holds.
//! The conformance suite samples six status codes and two unknown methods; the README says "every status code" and "unknown methods are not
//! turned into `GET`", so the sweeps here run the whole range on every adapter this crate builds.

use std::net::SocketAddr;
use std::time::Duration;

use httpant_client::Client;
use httpant_core::http::StatusCode;
use httpant_core::{Error, IntoResponse, PathParams, Request, Response};
use httpant_server::{Limits, Origin, Server, TrustedProxies};

const IDS: &[&str] = &["hyper", "axum", "actix-web", "poem", "salvo", "warp"];

async fn status(req: Request) -> StatusCode {
    req.extensions()
        .get::<PathParams>()
        .and_then(|p| p.get("code"))
        .and_then(|c| c.parse::<u16>().ok())
        .and_then(|c| StatusCode::from_u16(c).ok())
        .unwrap_or(StatusCode::BAD_REQUEST)
}

async fn origin(req: Request) -> Response {
    match req.extensions().get::<Origin>() {
        Some(o) => format!("{}|{}", o.client, o.via_trusted_proxy).into_response(),
        None => "none".into_response(),
    }
}

async fn raw(addr: SocketAddr, request: &[u8]) -> String {
    // Blocking sockets on a separate thread: the test's own runtime has one thread and the server needs it.
    let request = request.to_vec();
    tokio::task::spawn_blocking(move || raw_blocking(addr, &request))
        .await
        .unwrap()
}

fn raw_blocking(addr: SocketAddr, request: &[u8]) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(request).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = s.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or("")
}

/// "every status code reaches the client as sent": 200 to 599, each, on every adapter. (1xx are interim, not a final answer; not run.)
#[tokio::test]
async fn every_final_status_code_reaches_the_client_as_sent_on_every_adapter() {
    for id in IDS {
        let server = Server::with_adapter(id)
            .unwrap()
            .get("/status/:code", status)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let client = Client::new();
        let mut wrong = Vec::new();
        for code in 200u16..=599 {
            let url = format!("http://{}/status/{code}", server.addr());
            match client.get(url).send().await {
                Ok(res) if res.status().as_u16() == code => {}
                Ok(res) => wrong.push(format!("{code} arrived as {}", res.status())),
                Err(e) => wrong.push(format!("{code}: {e}")),
            }
        }
        let _ = server.shutdown().await;
        assert!(
            wrong.is_empty(),
            "{id}: {} of 400 wrong: {wrong:?}",
            wrong.len()
        );
    }
}

/// "Unknown methods are not turned into `GET`": a route that exists only for GET, asked with a method nobody registered, on every adapter.
#[tokio::test]
async fn an_unknown_method_is_never_answered_as_get_on_every_adapter() {
    for id in IDS {
        let server = Server::with_adapter(id)
            .unwrap()
            .get("/", || async { "hello" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let control = raw(
            server.addr(),
            b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert!(
            status_line(&control).contains(" 200"),
            "{id}: control GET: {control:?}"
        );
        for method in ["PROPFIND", "MKCOL", "BREW", "PURGE"] {
            let line = raw(
                server.addr(),
                format!("{method} / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await;
            assert!(
                !line.contains("hello") && !status_line(&line).contains(" 200"),
                "{id}: {method} was answered as GET: {line:?}"
            );
            assert!(
                status_line(&line).contains(" 405")
                    || status_line(&line).contains(" 404")
                    || status_line(&line).contains(" 501"),
                "{id}: {method} got {:?}, expected a refusal",
                status_line(&line)
            );
        }
        let _ = server.shutdown().await;
    }
}

/// "Safe defaults ...: target 8 KiB, 64 header fields, 16 KiB of headers, body 2 MiB (`Limits`)": the numbers, then the enforcement on every
/// adapter. (The 30 s is checked as a constant and wired by `adapter.rs`; waiting it out is not run.)
#[test]
fn the_readme_default_numbers_are_the_defaults() {
    let d = Limits::default();
    assert_eq!(d.max_uri_len, Some(8 * 1024));
    assert_eq!(d.max_headers, Some(64));
    assert_eq!(d.max_header_bytes, Some(16 * 1024));
    assert_eq!(d.max_body_bytes, Some(2 * 1024 * 1024));
    assert_eq!(
        httpant_server::DEFAULT_SHUTDOWN_GRACE,
        Duration::from_secs(30)
    );
}

#[tokio::test]
async fn the_default_limits_refuse_what_the_readme_says_on_every_adapter() {
    for id in IDS {
        let server = Server::with_adapter(id)
            .unwrap()
            .get("/", || async { "ok" })
            .post("/", || async { "ok" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let a = server.addr();
        let ok = raw(a, b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await;
        assert!(status_line(&ok).contains(" 200"), "{id}: control: {ok:?}");
        let long = format!(
            "GET /{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            "a".repeat(8 * 1024 + 1)
        );
        let r = raw(a, long.as_bytes()).await;
        assert!(
            status_line(&r).contains(" 414")
                || status_line(&r).contains(" 431")
                || status_line(&r).contains(" 400"),
            "{id}: target 8 KiB + 1: {r:?}"
        );
        let mut many = String::from("GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
        for i in 0..80 {
            many.push_str(&format!("x-{i}: 1\r\n"));
        }
        many.push_str("\r\n");
        let r = raw(a, many.as_bytes()).await;
        assert!(
            !status_line(&r).contains(" 200"),
            "{id}: 80 header fields were accepted: {r:?}"
        );
        let fat = format!(
            "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\nx-big: {}\r\n\r\n",
            "b".repeat(17 * 1024)
        );
        let r = raw(a, fat.as_bytes()).await;
        assert!(
            !status_line(&r).contains(" 200"),
            "{id}: 17 KiB of headers were accepted: {r:?}"
        );
        let big = Client::new()
            .post(format!("http://{a}/"))
            .body(vec![0u8; 2 * 1024 * 1024 + 1])
            .send()
            .await;
        match big {
            Ok(res) => assert_eq!(res.status().as_u16(), 413, "{id}: body 2 MiB + 1"),
            Err(e) => panic!("{id}: body 2 MiB + 1 gave an error, not 413: {e}"),
        }
        let small = Client::new()
            .post(format!("http://{a}/"))
            .body(vec![0u8; 2 * 1024 * 1024])
            .send()
            .await
            .unwrap();
        assert_eq!(small.status().as_u16(), 200, "{id}: body of exactly 2 MiB");
        let _ = server.shutdown().await;
    }
}

/// "nobody is trusted by default" and "only from peers you list".
#[tokio::test]
async fn a_forwarded_for_header_is_ignored_unless_the_peer_is_listed() {
    let plain = Server::new()
        .get("/", origin)
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let said = Client::new()
        .get(format!("http://{}/", plain.addr()))
        .header("x-forwarded-for", "203.0.113.9")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(said, "127.0.0.1|false", "default: nobody is trusted");
    let _ = plain.shutdown().await;
    let proxies = TrustedProxies::none().address("127.0.0.1".parse().unwrap());
    let behind = Server::new()
        .trusted_proxies(proxies)
        .get("/", origin)
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let said = Client::new()
        .get(format!("http://{}/", behind.addr()))
        .header("x-forwarded-for", "203.0.113.9")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(said, "203.0.113.9|true", "listed peer: believed");
    let _ = behind.shutdown().await;
}

/// "The client follows no redirects unless you ask".
#[tokio::test]
async fn the_client_follows_no_redirect_unless_asked() {
    let server = Server::new()
        .get("/a", || async {
            let mut r = StatusCode::FOUND.into_response();
            r.headers_mut().insert("location", "/b".parse().unwrap());
            r
        })
        .get("/b", || async { "end" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let url = format!("http://{}/a", server.addr());
    assert_eq!(
        Client::new()
            .get(&url)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        302
    );
    let followed = Client::builder()
        .follow_redirects(3)
        .build()
        .unwrap()
        .get(&url)
        .send()
        .await
        .unwrap();
    assert_eq!(followed.status().as_u16(), 200);
    assert_eq!(followed.text().await.unwrap(), "end");
    let _ = server.shutdown().await;
}

/// "`.bearer(token)` keeps the token out of `Debug` and out of every error, as does the query string of a URL."
#[tokio::test]
async fn the_bearer_token_and_the_query_never_appear_in_debug_or_errors() {
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let req = Client::new()
        .get(format!("http://{dead}/p?apikey=QUERYSECRET"))
        .bearer("TOKENSECRET");
    let shown = format!("{req:?}");
    assert!(
        !shown.contains("TOKENSECRET"),
        "Debug shows the token: {shown}"
    );
    assert!(
        !shown.contains("QUERYSECRET"),
        "Debug shows the query: {shown}"
    );
    let err = req.send().await.expect_err("nothing listens there");
    let text = format!("{err} / {err:?}");
    assert!(
        !text.contains("TOKENSECRET") && !text.contains("QUERYSECRET"),
        "an error shows a secret: {text}"
    );
    let bad = Client::new()
        .get("http://[::bad/p?apikey=QUERYSECRET")
        .bearer("TOKENSECRET")
        .send()
        .await
        .expect_err("not a URL");
    let text = format!("{bad} / {bad:?}");
    assert!(
        !text.contains("TOKENSECRET") && !text.contains("QUERYSECRET"),
        "an invalid-URL error shows a secret: {text}"
    );
}

/// "`refuse_private_addresses(true)` refuses loopback"; "the guard then judges the proxy, which `build` makes you acknowledge".
#[tokio::test]
async fn the_private_address_guard_and_the_proxy_acknowledgement_behave_as_written() {
    let server = Server::new()
        .get("/", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let guarded = Client::builder()
        .refuse_private_addresses(true)
        .build()
        .unwrap();
    let err = guarded
        .get(format!("http://{}/", server.addr()))
        .send()
        .await
        .expect_err("loopback must be refused");
    assert!(
        matches!(err, Error::PrivateAddress { .. })
            || format!("{err}").to_lowercase().contains("address"),
        "unexpected: {err}"
    );
    assert!(
        Client::builder()
            .proxy("http://127.0.0.1:3128")
            .refuse_private_addresses(true)
            .build()
            .is_err(),
        "proxy + guard built without the acknowledgement"
    );
    assert!(
        Client::builder()
            .proxy("http://127.0.0.1:3128")
            .refuse_private_addresses(true)
            .guard_judges_proxy(true)
            .build()
            .is_ok(),
        "proxy + guard + acknowledgement did not build"
    );
    let _ = server.shutdown().await;
}

/// "Two different mistakes get two different messages": the unknown-id message, verbatim from the README.
#[test]
fn the_unknown_adapter_message_is_the_readme_one() {
    let e = Server::with_adapter("nonexistent")
        .err()
        .expect("no such adapter");
    let text = e.to_string();
    assert!(
        text.starts_with("unknown adapter `nonexistent`: use one of ["),
        "{text}"
    );
    assert!(text.contains("\"hyper\""), "{text}");
}
