// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTP/2 over TLS (ALPN `h2`) with a real HTTP/2 client (hyper's), on the adapters that declare it.
#![cfg(all(feature = "tls", feature = "http2"))]

use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty, Full};
use httpant_core::Bytes;
use httpant_server::{FixedCert, RunningServer, Server, ServerCert, TlsAcceptor};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// The adapters that serve HTTP/2.
const H2_ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "poem")]
    "poem",
];

struct Made {
    cert: ServerCert,
    der: Vec<u8>,
}

fn made() -> Made {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let cert = ServerCert::from_pem(
        ck.cert.pem().as_bytes(),
        ck.signing_key.serialize_pem().as_bytes(),
    )
    .unwrap();
    Made {
        cert,
        der: ck.cert.der().to_vec(),
    }
}

fn connector(der: &[u8], alpn: &[&[u8]]) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(der.to_vec())).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    TlsConnector::from(Arc::new(config))
}

async fn start(id: &str, m: &Made) -> RunningServer {
    Server::with_adapter(id)
        .unwrap()
        .tls(TlsAcceptor::new(FixedCert::new(m.cert.clone())))
        .get("/origin", |req: httpant_core::Request| async move {
            match req.extensions().get::<httpant_server::Origin>() {
                Some(o) => format!("{:?}", o.scheme),
                None => "none".to_owned(),
            }
        })
        .post("/echo", || async { "posted" })
        .bind("127.0.0.1:0")
        .await
        .unwrap()
}

/// The protocol the server picks when the client offers `h2` and `http/1.1`.
async fn negotiated(server: &RunningServer, m: &Made) -> Option<Vec<u8>> {
    let tcp = TcpStream::connect(server.addr()).await.unwrap();
    let tls = connector(&m.der, &[b"h2", b"http/1.1"])
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec)
}

async fn h2_sender(server: &RunningServer, m: &Made) -> http2::SendRequest<Empty<Bytes>> {
    let tcp = TcpStream::connect(server.addr()).await.unwrap();
    let tls = connector(&m.der, &[b"h2"])
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    let (sender, conn) = http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
        .await
        .unwrap();
    tokio::spawn(conn);
    sender
}

#[tokio::test]
async fn an_http2_client_is_served_over_h2_and_an_http1_client_still_over_http1() {
    for id in H2_ADAPTERS {
        let m = made();
        let server = start(id, &m).await;
        assert_eq!(
            negotiated(&server, &m).await.as_deref(),
            Some(&b"h2"[..]),
            "{id}"
        );

        let mut sender = h2_sender(&server, &m).await;
        let req = hyper::Request::get("https://localhost/origin")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let res = sender.send_request(req).await.unwrap();
        assert_eq!(res.status(), 200, "{id}");
        assert_eq!(res.version(), hyper::Version::HTTP_2, "{id}");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            &body[..],
            b"Https",
            "{id}: the request must be reported as https over h2"
        );

        // Several streams at once on the one connection.
        let mut sends = Vec::new();
        for _ in 0..8 {
            let mut s = sender.clone();
            sends.push(tokio::spawn(async move {
                let req = hyper::Request::get("https://localhost/origin")
                    .body(Empty::<Bytes>::new())
                    .unwrap();
                s.send_request(req).await.unwrap().status()
            }));
        }
        for s in sends {
            assert_eq!(s.await.unwrap(), 200, "{id}");
        }

        // A client that offers only http/1.1 gets HTTP/1.1 from the same listener.
        let tcp = TcpStream::connect(server.addr()).await.unwrap();
        let mut tls = connector(&m.der, &[b"http/1.1"])
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        assert_eq!(
            tls.get_ref().1.alpn_protocol(),
            Some(&b"http/1.1"[..]),
            "{id}"
        );
        tls.write_all(b"GET /origin HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut reply = Vec::new();
        let _ = tls.read_to_end(&mut reply).await;
        assert!(
            reply.starts_with(b"HTTP/1.1 200"),
            "{id}: not a 200 over http/1.1"
        );
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn the_server_limits_apply_over_http2() {
    for id in H2_ADAPTERS {
        let m = made();
        let server = start(id, &m).await;
        let tcp = TcpStream::connect(server.addr()).await.unwrap();
        let tls = connector(&m.der, &[b"h2"])
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        let (mut sender, conn) = http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .unwrap();
        tokio::spawn(conn);
        // 3 MiB against the 2 MiB default: refused on its declared length, before any handler.
        let req = hyper::Request::post("https://localhost/echo")
            .header("content-length", 3 * 1024 * 1024)
            .body(Full::new(Bytes::from(vec![0u8; 3 * 1024 * 1024])))
            .unwrap();
        let res = tokio::time::timeout(Duration::from_secs(10), sender.send_request(req))
            .await
            .expect("no timeout")
            .unwrap();
        assert_eq!(res.status(), 413, "{id}");
        server.shutdown().await.unwrap();
    }
}

/// The hyper adapter keeps a cleartext listener on HTTP/1.1. **Not yet uniform:** with the `http2` feature, axum's and poem's own connection
/// builders accept the HTTP/2 prior-knowledge preface (h2c) on a cleartext listener, which hyper here refuses. Nothing in WebHostAnt's need list
/// asks for h2c, so conformance does not constrain it (ROADMAP: decide the h2c policy); this test pins only what hyper chose.
#[tokio::test]
async fn a_cleartext_listener_stays_http1_even_with_the_http2_feature() {
    for id in ["hyper"] {
        let server = Server::with_adapter(id)
            .unwrap()
            .get("/", || async { "ok" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        // The HTTP/2 connection preface ("prior knowledge"): an HTTP/2 server would answer with a SETTINGS frame; HTTP/1.1 answers in text.
        let mut tcp = TcpStream::connect(server.addr()).await.unwrap();
        tcp.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
            .await
            .unwrap();
        let mut got = [0u8; 16];
        let n = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut got))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        assert!(
            n == 0 || got[..n].starts_with(b"HTTP/1.1"),
            "{id}: a cleartext listener answered the HTTP/2 preface with {:?}",
            &got[..n.min(9)]
        );
        server.shutdown().await.unwrap();
    }
}
