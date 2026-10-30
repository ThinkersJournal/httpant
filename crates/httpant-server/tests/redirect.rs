// SPDX-License-Identifier: MIT OR Apache-2.0
//! The port-80 redirect over real sockets, on every adapter compiled in. Raw HTTP/1.1, so nothing of HTTPAnt's is on the client side.

use httpant_core::BoxFuture;
use httpant_server::{ChallengeResponder, HttpsRedirect, RunningServer, Server, TrustedProxies};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "actix-web")]
    "actix-web",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
    #[cfg(feature = "warp")]
    "warp",
];

#[derive(Debug)]
struct Pending;

impl ChallengeResponder for Pending {
    fn key_authorization<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move { (token == "tok1").then(|| "tok1.thumbprint".to_owned()) })
    }
}

fn server(id: &str, redirect: HttpsRedirect, proxy_trusted: bool) -> Server {
    let s = Server::with_adapter(id)
        .unwrap()
        .get("/app", || async { "the app" })
        .post("/app", || async { "posted" })
        .acme_challenge(Pending)
        .redirect_to_https(redirect);
    if proxy_trusted {
        s.trusted_proxies(TrustedProxies::none().parse("127.0.0.1").unwrap())
    } else {
        s
    }
}

async fn ask(
    srv: &RunningServer,
    method: &str,
    path: &str,
    host: Option<&str>,
    extra: &str,
) -> String {
    let mut tcp = TcpStream::connect(srv.addr()).await.unwrap();
    let host = host.map_or(String::new(), |h| format!("Host: {h}\r\n"));
    tcp.write_all(
        format!(
            "{method} {path} HTTP/1.1\r\n{host}{extra}Connection: close\r\nContent-Length: 0\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut reply = Vec::new();
    let _ = tcp.read_to_end(&mut reply).await;
    String::from_utf8_lossy(&reply).into_owned()
}

fn location(reply: &str) -> Option<&str> {
    reply
        .lines()
        .find_map(|l| {
            l.strip_prefix("location: ")
                .or_else(|| l.strip_prefix("Location: "))
        })
        .map(str::trim)
}

#[tokio::test]
async fn a_listed_host_is_redirected_with_the_method_and_target_kept() {
    for id in ADAPTERS {
        let srv = server(id, HttpsRedirect::new(["example.com"]), false)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        for method in ["GET", "POST"] {
            let reply = ask(&srv, method, "/app?x=1", Some("Example.COM:80"), "").await;
            assert!(
                reply.starts_with("HTTP/1.1 308"),
                "{id} {method}: {reply:?}"
            );
            assert_eq!(
                location(&reply),
                Some("https://example.com/app?x=1"),
                "{id} {method}"
            );
        }
        srv.shutdown().await.unwrap();

        let srv = server(id, HttpsRedirect::new(["example.com"]).port(8443), false)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let reply = ask(&srv, "GET", "/app", Some("example.com"), "").await;
        assert_eq!(
            location(&reply),
            Some("https://example.com:8443/app"),
            "{id}"
        );
        srv.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn an_unlisted_or_missing_host_is_never_redirected() {
    for id in ADAPTERS {
        let srv = server(id, HttpsRedirect::new(["example.com"]), false)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        for host in ["evil.com", "example.com.evil.com", "sub.example.com"] {
            let reply = ask(&srv, "GET", "/app", Some(host), "").await;
            assert!(reply.starts_with("HTTP/1.1 421"), "{id} {host}: {reply:?}");
            assert!(
                location(&reply).is_none(),
                "{id} {host}: redirected a forged host"
            );
        }
        let reply = ask(&srv, "GET", "/app", None, "").await;
        assert!(!reply.starts_with("HTTP/1.1 308"), "{id}: {reply:?}");
        assert!(location(&reply).is_none(), "{id}");
        srv.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn the_challenge_path_is_answered_not_redirected() {
    for id in ADAPTERS {
        let srv = server(id, HttpsRedirect::new(["example.com"]), false)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        // Even for a host that is not listed and for one that is: the challenge is answered first.
        for host in ["example.com", "other.test"] {
            let reply = ask(
                &srv,
                "GET",
                "/.well-known/acme-challenge/tok1",
                Some(host),
                "",
            )
            .await;
            assert!(reply.starts_with("HTTP/1.1 200"), "{id} {host}: {reply:?}");
            assert!(reply.ends_with("tok1.thumbprint"), "{id} {host}: {reply:?}");
        }
        srv.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_request_that_already_came_over_https_passes_through() {
    for id in ADAPTERS {
        // The peer (127.0.0.1) is a trusted proxy that says the client used https: no redirect, no loop.
        let srv = server(id, HttpsRedirect::new(["example.com"]), true)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let via = ask(
            &srv,
            "GET",
            "/app",
            Some("example.com"),
            "X-Forwarded-Proto: https\r\n",
        )
        .await;
        assert!(
            via.starts_with("HTTP/1.1 200") && via.ends_with("the app"),
            "{id}: {via:?}"
        );
        // A proxy that says http is redirected like anyone else, and the same header from an untrusted peer is ignored.
        let plain = ask(
            &srv,
            "GET",
            "/app",
            Some("example.com"),
            "X-Forwarded-Proto: http\r\n",
        )
        .await;
        assert!(plain.starts_with("HTTP/1.1 308"), "{id}: {plain:?}");
        srv.shutdown().await.unwrap();

        let srv = server(id, HttpsRedirect::new(["example.com"]), false)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let forged = ask(
            &srv,
            "GET",
            "/app",
            Some("example.com"),
            "X-Forwarded-Proto: https\r\n",
        )
        .await;
        assert!(
            forged.starts_with("HTTP/1.1 308"),
            "{id}: an untrusted peer skipped the redirect"
        );
        srv.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn without_a_redirect_configured_the_app_answers() {
    for id in ADAPTERS {
        let srv = Server::with_adapter(id)
            .unwrap()
            .get("/app", || async { "the app" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let reply = ask(&srv, "GET", "/app", Some("evil.com"), "").await;
        assert!(reply.ends_with("the app"), "{id}: {reply:?}");
        srv.shutdown().await.unwrap();
    }
}
