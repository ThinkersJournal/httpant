// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cleartext HTTP/2 (h2c) through the public API: off unless `Server::h2c(true)`, on every adapter whose library can refuse it, a real HTTP/2
//! client served when it is asked for, HTTP/1.1 unaffected either way, and a named error where the adapter cannot serve it.
#![cfg(feature = "http2")]

use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use httpant_client::Client;
use httpant_core::Bytes;
use httpant_server::core::Error;
use httpant_server::{RunningServer, Server};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The adapters that refuse the preface unless asked, and serve HTTP/2 when asked.
const SWITCHABLE: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
];

const WAIT: Duration = Duration::from_secs(5);

async fn start(id: &str, h2c: bool) -> Result<RunningServer, Error> {
    Server::with_adapter(id)
        .unwrap()
        .h2c(h2c)
        .get("/", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
}

/// What the listener answers the HTTP/2 preface plus an empty SETTINGS frame with: its first bytes, or none.
async fn answer_to_preface(server: &RunningServer) -> Vec<u8> {
    let mut tcp = TcpStream::connect(server.addr()).await.unwrap();
    tcp.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    tcp.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
    let mut got = [0u8; 16];
    let n = tokio::time::timeout(WAIT, tcp.read(&mut got))
        .await
        .unwrap_or(Ok(0))
        .unwrap_or(0);
    got[..n].to_vec()
}

fn is_http2_settings(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[3] == 4 && !bytes.starts_with(b"HTTP/")
}

async fn http1_works(server: &RunningServer, id: &str) {
    let res = Client::new()
        .get(format!("http://{}/", server.addr()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{id}: HTTP/1.1 on the same listener");
    assert_eq!(res.text().await.unwrap(), "ok", "{id}");
}

#[tokio::test]
async fn h2c_is_off_by_default_and_http1_is_unaffected() {
    for id in SWITCHABLE {
        let server = start(id, false).await.unwrap();
        let answer = answer_to_preface(&server).await;
        assert!(
            !is_http2_settings(&answer),
            "{id}: answered the HTTP/2 preface with HTTP/2 although h2c was not asked for"
        );
        http1_works(&server, id).await;
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_real_http2_client_is_served_when_h2c_is_asked_for() {
    for id in SWITCHABLE {
        let server = start(id, true).await.unwrap();
        let tcp = TcpStream::connect(server.addr()).await.unwrap();
        let (mut sender, conn) = http2::handshake(TokioExecutor::new(), TokioIo::new(tcp))
            .await
            .unwrap();
        tokio::spawn(conn);
        let req = hyper::Request::get(format!("http://{}/", server.addr()))
            .body(Empty::<Bytes>::new())
            .unwrap();
        let res = tokio::time::timeout(WAIT, sender.send_request(req))
            .await
            .unwrap_or_else(|_| panic!("{id}: no answer over h2c"))
            .unwrap_or_else(|e| panic!("{id}: h2c request failed: {e}"));
        assert_eq!(res.status(), 200, "{id}");
        assert_eq!(res.version(), hyper::Version::HTTP_2, "{id}");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok", "{id}");
        // The same listener still answers HTTP/1.1.
        http1_works(&server, id).await;
        server.shutdown().await.unwrap();
    }
}

/// warp's accept path is private (`docs/H2C-POLICY.md`), so HTTPAnt cannot refuse the preface for it: this pins what the capability matrix
/// says, in both modes, so the day warp makes it possible this test is the one that changes.
#[cfg(feature = "warp")]
#[tokio::test]
async fn warp_serves_h2c_whether_or_not_it_was_asked_for() {
    for asked in [false, true] {
        let server = start("warp", asked).await.unwrap();
        let answer = answer_to_preface(&server).await;
        assert!(is_http2_settings(&answer), "warp, h2c({asked}): {answer:?}");
        http1_works(&server, "warp").await;
        server.shutdown().await.unwrap();
    }
}

/// actix-web has no HTTP/2, so it always refuses, and asking for h2c is an error that names it, not a quiet HTTP/1.1 server.
#[cfg(feature = "actix-web")]
#[tokio::test]
async fn actix_web_refuses_h2c_and_cannot_be_asked_for_it() {
    let server = start("actix-web", false).await.unwrap();
    assert!(!is_http2_settings(&answer_to_preface(&server).await));
    http1_works(&server, "actix-web").await;
    server.shutdown().await.unwrap();
    let err = start("actix-web", true).await.map(|_| ()).unwrap_err();
    assert!(
        matches!(
            err,
            Error::H2cNotSupported {
                adapter: "actix-web"
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("h2c"), "{err}");
}
