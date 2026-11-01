// SPDX-License-Identifier: MIT OR Apache-2.0
//! `ServerSettings` and `Server::apply`: settings that arrive as data change what the server does, on every adapter, and a setting that cannot
//! be honoured is an error, never a silent default.
use std::time::{Duration, Instant};

use httpant_client::Client;
use httpant_server::core::Error;
use httpant_server::{Limits, Server, ServerSettings};

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

fn settings(json: &str) -> ServerSettings {
    serde_json::from_str(json).expect("settings parse")
}

#[test]
fn an_empty_configuration_changes_nothing() {
    let s = settings("{}");
    assert_eq!(s, ServerSettings::default());
    assert!(s.adapter.is_none() && s.addr.is_none() && s.shutdown_grace_secs.is_none());
}

#[test]
fn a_typo_in_a_key_is_an_error_that_names_it() {
    // Control: the correctly spelt key parses (below); only the misspelling fails.
    assert!(serde_json::from_str::<ServerSettings>(r#"{"shutdown_grace_secs": 1}"#).is_ok());
    let e = serde_json::from_str::<ServerSettings>(r#"{"shutdown_grace_sec": 1}"#).unwrap_err();
    assert!(e.to_string().contains("shutdown_grace_sec"), "{e}");
    let e =
        serde_json::from_str::<ServerSettings>(r#"{"limits": {"max_body_byte": 1}}"#).unwrap_err();
    assert!(e.to_string().contains("max_body_byte"), "{e}");
}

#[tokio::test]
async fn an_unknown_adapter_is_an_error_not_a_fallback() {
    let err = Server::new()
        .apply(&settings(r#"{"adapter": "no-such-library"}"#))
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(err, Error::UnknownAdapter { .. }), "{err}");
    // Control: a known id is accepted.
    assert!(
        Server::new()
            .apply(&settings(r#"{"adapter": "hyper"}"#))
            .is_ok()
    );
}

#[tokio::test]
async fn limits_from_settings_are_enforced_on_every_adapter() {
    for id in ADAPTERS {
        let server = Server::new()
            .apply(&settings(&format!(
                r#"{{"adapter": "{id}", "limits": {{"max_body_bytes": 10}}}}"#
            )))
            .unwrap()
            .post("/echo", || async { "posted" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let base = format!("http://{}", server.addr());
        let client = Client::new();
        let small = client
            .post(format!("{base}/echo"))
            .body("0123456789")
            .send()
            .await
            .unwrap();
        assert_eq!(small.status(), 200, "{id}: a body at the limit");
        let big = client
            .post(format!("{base}/echo"))
            .body("0123456789X")
            .send()
            .await
            .unwrap();
        assert_eq!(big.status(), 413, "{id}: a body over the limit");
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn settings_left_out_keep_the_default_limits_and_earlier_calls() {
    // The body limit is set in code, the settings name only the header count: the body limit must survive and the header count must change.
    let server = Server::new()
        .limits(Limits::default().max_body_bytes(10))
        .apply(&settings(r#"{"limits": {"max_headers": 1000}}"#))
        .unwrap()
        .post("/echo", || async { "posted" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let client = Client::new();
    let url = format!("http://{}/echo", server.addr());
    let big = client.post(&url).body("0123456789X").send().await.unwrap();
    assert_eq!(big.status(), 413, "the limit set before apply survives");
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_shutdown_grace_from_settings_ends_a_request_that_never_finishes() {
    let server = Server::new()
        .apply(&settings(r#"{"shutdown_grace_secs": 1}"#))
        .unwrap()
        .get("/never", || async {
            std::future::pending::<()>().await;
            "unreachable"
        })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let url = format!("http://{}/never", server.addr());
    let pending = tokio::spawn(async move { Client::new().get(url).send().await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = Instant::now();
    server.shutdown().await.unwrap();
    // The default grace is 30 s; one second was asked for. Generous bound for a loaded CI machine, far below the default.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
    pending.abort();
}

/// What a listener answers the HTTP/2 preface plus an empty SETTINGS frame with: `true` when it is HTTP/2.
#[cfg(feature = "http2")]
async fn answers_h2c(server: &httpant_server::RunningServer) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut tcp = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    tcp.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    tcp.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
    let mut got = [0u8; 16];
    let n = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut got))
        .await
        .unwrap_or(Ok(0))
        .unwrap_or(0);
    n >= 4 && got[3] == 4 && !got[..n].starts_with(b"HTTP/")
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn h2c_from_settings_turns_cleartext_http2_on_and_off() {
    // Absent: off, as without settings (control for the two cases below).
    let server = Server::new()
        .apply(&settings("{}"))
        .unwrap()
        .get("/", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    assert!(!answers_h2c(&server).await, "h2c on without being asked");
    server.shutdown().await.unwrap();
    // True: on.
    let server = Server::new()
        .apply(&settings(r#"{"h2c": true}"#))
        .unwrap()
        .get("/", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    assert!(
        answers_h2c(&server).await,
        "h2c asked for in settings, not served"
    );
    server.shutdown().await.unwrap();
    // False after code turned it on: off again (settings replace what the builder had).
    let server = Server::new()
        .h2c(true)
        .apply(&settings(r#"{"h2c": false}"#))
        .unwrap()
        .get("/", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    assert!(
        !answers_h2c(&server).await,
        "h2c: false in settings did not turn it off"
    );
    server.shutdown().await.unwrap();
}

#[cfg(all(feature = "http2", feature = "actix-web"))]
#[tokio::test]
async fn h2c_from_settings_on_an_adapter_that_cannot_serve_it_is_a_named_error() {
    let err = Server::new()
        .apply(&settings(r#"{"adapter": "actix-web", "h2c": true}"#))
        .unwrap()
        .bind("127.0.0.1:0")
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::H2cNotSupported {
                adapter: "actix-web"
            }
        ),
        "{err}"
    );
}
