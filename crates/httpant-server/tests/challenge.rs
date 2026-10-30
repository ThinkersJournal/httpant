// SPDX-License-Identifier: MIT OR Apache-2.0
//! The ACME HTTP-01 challenge prefix over real sockets, on every adapter compiled in. Raw HTTP/1.1 on a TCP socket, so nothing of HTTPAnt's
//! is on the client side.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use httpant_core::BoxFuture;
use httpant_server::{ChallengeResponder, RunningServer, Server};
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

/// One pending challenge, `tok1` -> `tok1.thumbprint`; counts how often it is asked.
#[derive(Debug)]
struct Pending(Arc<AtomicUsize>);

impl ChallengeResponder for Pending {
    fn key_authorization<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Option<String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { (token == "tok1").then(|| "tok1.thumbprint".to_owned()) })
    }
}

async fn start(id: &str, asked: Option<Arc<AtomicUsize>>) -> RunningServer {
    let server = Server::with_adapter(id)
        .unwrap()
        .get("/.well-known/acme-challenge/tok1", || async {
            "from the app"
        })
        .get("/other", || async { "other" });
    let server = match asked {
        Some(asked) => server.acme_challenge(Pending(asked)),
        None => server,
    };
    server.bind("127.0.0.1:0").await.unwrap()
}

/// Send one request and return the whole reply as text (head and body).
async fn ask(server: &RunningServer, method: &str, path: &str) -> String {
    let mut tcp = TcpStream::connect(server.addr()).await.unwrap();
    tcp.write_all(
        format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut reply = Vec::new();
    let _ = tcp.read_to_end(&mut reply).await;
    String::from_utf8_lossy(&reply).into_owned()
}

fn body(reply: &str) -> &str {
    reply.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

#[tokio::test]
async fn a_pending_token_is_answered_before_the_apps_own_route() {
    for id in ADAPTERS {
        let asked = Arc::new(AtomicUsize::new(0));
        let server = start(id, Some(asked)).await;
        let get = ask(&server, "GET", "/.well-known/acme-challenge/tok1").await;
        assert!(get.starts_with("HTTP/1.1 200"), "{id}: {get:?}");
        assert_eq!(body(&get), "tok1.thumbprint", "{id}: the app's route won");
        let lower = get.to_ascii_lowercase();
        assert!(
            lower.contains("content-type: application/octet-stream"),
            "{id}"
        );
        assert!(lower.contains("cache-control: no-store"), "{id}");

        let head = ask(&server, "HEAD", "/.well-known/acme-challenge/tok1").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{id}: {head:?}");
        assert_eq!(body(&head), "", "{id}: HEAD carried a body");
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn unknown_bad_and_misused_challenge_requests_are_refused_and_other_paths_pass() {
    for id in ADAPTERS {
        let asked = Arc::new(AtomicUsize::new(0));
        let server = start(id, Some(Arc::clone(&asked))).await;
        let unknown = ask(&server, "GET", "/.well-known/acme-challenge/nope").await;
        assert!(unknown.starts_with("HTTP/1.1 404"), "{id}: {unknown:?}");
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "{id}: the unknown token was asked"
        );

        // A token that is not base64url is refused without asking the responder at all.
        for bad in ["a.b", "..", "a%2Fb", "a/b"] {
            let reply = ask(
                &server,
                "GET",
                &format!("/.well-known/acme-challenge/{bad}"),
            )
            .await;
            assert!(reply.starts_with("HTTP/1.1 404"), "{id} {bad}: {reply:?}");
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "{id}: a bad token reached the responder"
        );

        let post = ask(&server, "POST", "/.well-known/acme-challenge/tok1").await;
        assert!(post.starts_with("HTTP/1.1 405"), "{id}: {post:?}");
        assert!(
            post.to_ascii_lowercase().contains("allow: get, head"),
            "{id}"
        );

        // Everything outside the prefix is the application's.
        let other = ask(&server, "GET", "/other").await;
        assert_eq!(body(&other), "other", "{id}");
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn without_a_responder_the_prefix_belongs_to_the_application() {
    for id in ADAPTERS {
        let server = start(id, None).await;
        let reply = ask(&server, "GET", "/.well-known/acme-challenge/tok1").await;
        assert_eq!(body(&reply), "from the app", "{id}: {reply:?}");
        server.shutdown().await.unwrap();
    }
}
