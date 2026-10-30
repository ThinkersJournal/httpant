// SPDX-License-Identifier: MIT OR Apache-2.0
//! Case 17: TLS termination. The client here is tokio-rustls speaking raw HTTP/1.1, not HTTPAnt, so a defect cannot hide on both ends.
//!
//! What an adapter has to do depends on what it declares: one that declares `tls: Yes` must serve HTTPS (TLS 1.3 and 1.2), present the
//! certificate it was given, report the request as `https`, and answer a cleartext client with no HTTP reply; one that declares `tls: No` must
//! refuse to start with a TLS acceptor at all (it never serves in clear when TLS was asked for); one that has not established it (`Unknown`)
//! fails. A subject that declares nothing (a mutant) is held to the first.

use std::sync::Arc;
use std::time::Duration;

use httpant_core::{BoxFuture, Support};
use httpant_server::{FixedCert, ServerCert, TlsAcceptor};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::app;
use crate::raw::{RawRequest, send_with_timeout};
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

fn connector(
    root_der: &[u8],
    versions: &[&'static rustls::SupportedProtocolVersion],
) -> Result<TlsConnector, String> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(root_der.to_vec()))
        .map_err(|e| format!("test root: {e}"))?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(versions)
    .map_err(|e| format!("test client: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// GET `/origin` over TLS: the status, the body, and the leaf certificate the server presented.
async fn https_origin(
    addr: std::net::SocketAddr,
    connector: &TlsConnector,
) -> Result<(u16, String, Vec<u8>), String> {
    let work = async {
        let tcp = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("connect: {e}"))?;
        let name = ServerName::try_from("localhost").map_err(|e| format!("name: {e}"))?;
        let mut tls = connector
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS handshake: {e}"))?;
        let leaf = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .map(|c| c.to_vec())
            .ok_or("the server presented no certificate")?;
        tls.write_all(
            &RawRequest::new("GET", "/origin")
                .header("connection", "close")
                .to_bytes(),
        )
        .await
        .map_err(|e| format!("send: {e}"))?;
        let mut reply = Vec::new();
        let _ = tls.read_to_end(&mut reply).await;
        let text = String::from_utf8_lossy(&reply).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").ok_or("no response head")?;
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or("no status line")?;
        Ok::<_, String>((status, body.to_owned(), leaf))
    };
    tokio::time::timeout(WAIT, work)
        .await
        .map_err(|_| "https request timed out".to_owned())?
}

fn test_cert() -> Result<(ServerCert, Vec<u8>), String> {
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
        .map_err(|e| format!("test certificate: {e}"))?;
    let cert = ServerCert::from_pem(
        made.cert.pem().as_bytes(),
        made.signing_key.serialize_pem().as_bytes(),
    )
    .map_err(|e| format!("test certificate: {e}"))?;
    Ok((cert, made.cert.der().to_vec()))
}

/// Case 17.
pub fn tls_terminate(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        let (cert, der) = test_cert()?;
        let setup = Setup {
            tls: Some(TlsAcceptor::new(FixedCert::new(cert))),
            ..Setup::default()
        };
        let (app, _gates) = app::app_with_gates();
        let declared = s.declared().map(|c| c.tls);
        let started = s.start_with(app, setup).await;
        match declared {
            Some(Support::No) => {
                return match started {
                    Err(e) if e.to_string().contains("cannot terminate TLS") => Ok(()),
                    Err(e) => Err(format!(
                        "declares `tls: No` but the refusal was not the named one: {e}"
                    )),
                    Ok(server) => {
                        let _ = server.shutdown().await;
                        Err("declares `tls: No` yet started with a TLS acceptor: it would serve in clear".to_owned())
                    }
                };
            }
            Some(Support::Unknown) => {
                if let Ok(server) = started {
                    let _ = server.shutdown().await;
                }
                return Err(
                    "the TLS capability is not established: declare `Yes` or `No`".to_owned(),
                );
            }
            Some(Support::Yes) | None => {}
        }
        let server = started.map_err(|e| format!("could not start with TLS: {e}"))?;
        let addr = server.addr();
        let outcome = async {
            for (label, versions) in [
                ("TLS 1.3", &[&rustls::version::TLS13][..]),
                ("TLS 1.2", &[&rustls::version::TLS12][..]),
            ] {
                let (status, body, leaf) = https_origin(addr, &connector(&der, versions)?)
                    .await
                    .map_err(|e| format!("{label}: {e}"))?;
                ensure!(status == 200, "{label}: GET /origin: status {status}");
                ensure!(
                    body == "127.0.0.1 https direct",
                    "{label}: the request must be reported as https from the connection: expected `127.0.0.1 https direct`, got `{body}`"
                );
                ensure!(leaf == der, "{label}: the server presented another certificate than the one it was given");
            }
            // A cleartext client gets no HTTP answer from a TLS listener.
            if let Ok(res) = send_with_timeout(addr, &RawRequest::new("GET", "/origin"), Duration::from_secs(2)).await {
                ensure!(res.status != 200, "a cleartext request was answered `200` on a TLS listener");
            }
            Ok(())
        }
        .await;
        let _ = server.shutdown().await;
        outcome
    })
}
