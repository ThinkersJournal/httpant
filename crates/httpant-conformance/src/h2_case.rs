// SPDX-License-Identifier: MIT OR Apache-2.0
//! Case 18: ALPN and HTTP/2. A TLS listener must offer exactly what the adapter serves. The client here is hyper's HTTP/2 client and
//! tokio-rustls, not HTTPAnt, so a defect cannot hide on both ends.
//!
//! An adapter that declares `http2: Yes` must negotiate `h2` with a client that offers `h2` and `http/1.1`, and answer a request over
//! HTTP/2; one that declares `http2: No` (but terminates TLS) must negotiate `http/1.1` and answer a request over it, never `h2` it cannot
//! serve; one that does not terminate TLS has nothing to negotiate (case 17 holds it to refusing the acceptor); `Unknown` fails. A subject
//! that declares nothing (a mutant) is held to HTTP/2.

use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use httpant_core::{BoxFuture, Bytes, Support};
use httpant_server::{FixedCert, ServerCert, TlsAcceptor};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::app;
use crate::subject::{Setup, Subject};

type Outcome = Result<(), String>;

const WAIT: Duration = Duration::from_secs(5);

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

fn offering_h2_and_http1(root_der: &[u8]) -> Result<TlsConnector, String> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(root_der.to_vec()))
        .map_err(|e| format!("test root: {e}"))?;
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("test client: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Case 18.
pub fn http2_alpn(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .map_err(|e| format!("test certificate: {e}"))?;
        let cert = ServerCert::from_pem(
            made.cert.pem().as_bytes(),
            made.signing_key.serialize_pem().as_bytes(),
        )
        .map_err(|e| format!("test certificate: {e}"))?;
        let der = made.cert.der().to_vec();
        let (tls, http2) = match s.declared() {
            Some(c) => (c.tls, c.http2),
            None => (Support::Yes, Support::Yes),
        };
        if tls != Support::Yes {
            // Nothing to negotiate: case 17 holds this adapter to refusing a TLS acceptor.
            return Ok(());
        }
        ensure!(
            http2 != Support::Unknown,
            "the HTTP/2 capability is not established: declare `Yes` or `No`"
        );
        let setup = Setup {
            tls: Some(TlsAcceptor::new(FixedCert::new(cert))),
            ..Setup::default()
        };
        let (app, _gates) = app::app_with_gates();
        let server = s
            .start_with(app, setup)
            .await
            .map_err(|e| format!("could not start with TLS: {e}"))?;
        let addr = server.addr();
        let connector = offering_h2_and_http1(&der)?;
        let outcome = async {
            let tcp = TcpStream::connect(addr)
                .await
                .map_err(|e| format!("connect: {e}"))?;
            let mut tls = connector
                .connect(
                    ServerName::try_from("localhost").map_err(|e| format!("name: {e}"))?,
                    tcp,
                )
                .await
                .map_err(|e| format!("TLS handshake: {e}"))?;
            let chosen = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
            if http2 == Support::Yes {
                ensure!(
                    chosen.as_deref() == Some(&b"h2"[..]),
                    "declares `http2: Yes` but a client offering h2 and http/1.1 was given {:?}",
                    chosen.as_deref().map(String::from_utf8_lossy)
                );
                let (mut sender, conn) = http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
                    .await
                    .map_err(|e| format!("HTTP/2 handshake after negotiating h2: {e}"))?;
                tokio::spawn(conn);
                let req = hyper::Request::get("https://localhost/origin")
                    .body(Empty::<Bytes>::new())
                    .map_err(|e| format!("request: {e}"))?;
                let res = tokio::time::timeout(WAIT, sender.send_request(req))
                    .await
                    .map_err(|_| "GET /origin over h2 timed out".to_owned())?
                    .map_err(|e| format!("GET /origin over h2: {e}"))?;
                ensure!(res.status() == 200, "GET /origin over h2: status {}", res.status());
                ensure!(res.version() == hyper::Version::HTTP_2, "the reply was not HTTP/2");
                let body = res
                    .into_body()
                    .collect()
                    .await
                    .map_err(|e| format!("body: {e}"))?
                    .to_bytes();
                ensure!(
                    &body[..] == b"127.0.0.1 https direct",
                    "over h2 the request must be reported as https from the connection: expected `127.0.0.1 https direct`, got `{}`",
                    String::from_utf8_lossy(&body)
                );
            } else {
                ensure!(
                    chosen.as_deref() != Some(&b"h2"[..]),
                    "declares `http2: No` yet offered h2, which it cannot serve"
                );
                tls.write_all(b"GET /origin HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .await
                    .map_err(|e| format!("send: {e}"))?;
                let mut reply = Vec::new();
                let _ = tokio::time::timeout(WAIT, tls.read_to_end(&mut reply)).await;
                let text = String::from_utf8_lossy(&reply).into_owned();
                ensure!(
                    text.starts_with("HTTP/1.1 200") && text.ends_with("127.0.0.1 https direct"),
                    "over HTTP/1.1 after not choosing h2: expected a 200 reporting `127.0.0.1 https direct`"
                );
            }
            Ok(())
        }
        .await;
        let _ = server.shutdown().await;
        outcome
    })
}
