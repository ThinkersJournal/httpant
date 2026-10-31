// SPDX-License-Identifier: MIT OR Apache-2.0
//! Graceful shutdown over TLS (conformance case 9, but on the TLS listener each adapter builds for itself): after shutdown is requested a request
//! in flight completes over its connection, an idle keep-alive connection is closed, and `shutdown` returns only once the request is done.
#![cfg(feature = "tls")]

use std::sync::Arc;
use std::time::Duration;

use httpant_server::{FixedCert, Server, ServerCert, TlsAcceptor};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

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
];

const WAIT: Duration = Duration::from_secs(5);

async fn open(addr: std::net::SocketAddr, connector: &TlsConnector) -> TlsStream<TcpStream> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    connector
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap()
}

/// Read one response off a keep-alive connection: its status and body, using `Content-Length`.
async fn read_response(tls: &mut TlsStream<TcpStream>) -> (u16, String) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            if buf.len() >= i + 4 + len {
                let status = head
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap();
                let body = String::from_utf8_lossy(&buf[i + 4..i + 4 + len]).into_owned();
                return (status, body);
            }
        }
        let n = tls.read(&mut chunk).await.unwrap();
        assert!(
            n > 0,
            "the connection closed before the response was complete"
        );
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_over_tls_finishes_the_request_in_flight_and_closes_idle_connections() {
    let made = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = ServerCert::from_pem(
        made.cert.pem().as_bytes(),
        made.signing_key.serialize_pem().as_bytes(),
    )
    .unwrap();
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(made.cert.der().to_vec()))
        .unwrap();
    let connector = TlsConnector::from(Arc::new(
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    ));
    for id in ADAPTERS {
        let release = Arc::new(Notify::new());
        let (started, entered) = {
            let n = Arc::new(Notify::new());
            (Arc::clone(&n), n)
        };
        let gate = Arc::clone(&release);
        let server = Server::with_adapter(id)
            .unwrap()
            .tls(TlsAcceptor::new(FixedCert::new(cert.clone())))
            .get("/ping", || async { "pong" })
            .get("/slow", move || {
                let (gate, started) = (Arc::clone(&gate), Arc::clone(&started));
                async move {
                    started.notify_one();
                    gate.notified().await;
                    "done"
                }
            })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = server.addr();

        // An idle keep-alive connection: one request answered, the connection left open.
        let mut idle = open(addr, &connector).await;
        idle.write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(
            read_response(&mut idle).await,
            (200, "pong".to_owned()),
            "{id}"
        );

        // A request that is still running when shutdown is asked for.
        let mut busy = open(addr, &connector).await;
        busy.write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(WAIT, entered.notified())
            .await
            .unwrap_or_else(|_| panic!("{id}: the slow request never reached the handler"));

        let shutting = tokio::spawn(server.shutdown());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !shutting.is_finished(),
            "{id}: shutdown returned while a request was still running"
        );

        // The idle connection is closed by the server.
        let mut chunk = [0u8; 16];
        let closed = tokio::time::timeout(WAIT, idle.read(&mut chunk)).await;
        assert!(
            matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
            "{id}: an idle keep-alive connection was not closed by shutdown"
        );

        // The running request completes, then shutdown returns.
        release.notify_one();
        let (status, body) = tokio::time::timeout(WAIT, read_response(&mut busy))
            .await
            .unwrap_or_else(|_| panic!("{id}: the request in flight did not complete"));
        assert_eq!((status, body.as_str()), (200, "done"), "{id}");
        tokio::time::timeout(WAIT, shutting)
            .await
            .unwrap_or_else(|_| panic!("{id}: shutdown did not return after the request finished"))
            .unwrap()
            .unwrap();
    }
}
