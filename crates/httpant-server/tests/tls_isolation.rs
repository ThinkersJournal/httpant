// SPDX-License-Identifier: MIT OR Apache-2.0
//! A slow certificate source costs only its own handshake. Every adapter that terminates TLS runs HTTPAnt's own acceptor, and the handshakes run
//! as tasks of their own, so while several handshakes wait on the source another client's handshake is not held up. (Measured when actix-web was
//! tried through its own rustls support, whose certificate resolver is synchronous: a fast handshake took 2035 ms behind six slow ones, against
//! 7 ms here; `docs/TLS-ADAPTER-PROBES.md`.) The limit below is half of the source's delay, so an adapter that blocks a thread on the source fails.
#![cfg(feature = "tls")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use httpant_core::BoxFuture;
use httpant_server::{CertSource, CertSourceError, Server, ServerCert, TlsAcceptor};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const SLOW: Duration = Duration::from_millis(1500);
const SLOW_CLIENTS: usize = 6;

/// Every adapter that terminates TLS.
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

struct Source(Arc<ServerCert>);

impl CertSource for Source {
    fn cert_for<'a>(
        &'a self,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Option<Arc<ServerCert>>, CertSourceError>> {
        Box::pin(async move {
            if name == Some("slow.test") {
                tokio::time::sleep(SLOW).await;
            }
            Ok(Some(Arc::clone(&self.0)))
        })
    }
}

async fn handshake(addr: std::net::SocketAddr, c: &TlsConnector, name: &str) -> Duration {
    let t = Instant::now();
    let tcp = TcpStream::connect(addr).await.unwrap();
    let _tls = c
        .connect(ServerName::try_from(name.to_owned()).unwrap(), tcp)
        .await
        .unwrap();
    t.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_certificate_source_does_not_hold_up_another_clients_handshake() {
    let made =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "slow.test".into()]).unwrap();
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
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    ));
    for id in ADAPTERS {
        let server = Server::with_adapter(id)
            .unwrap()
            .tls(TlsAcceptor::new(Source(Arc::new(cert.clone()))))
            .get("/", || async { "ok" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = server.addr();
        // More slow handshakes than any adapter has worker threads.
        let slow: Vec<_> = (0..SLOW_CLIENTS)
            .map(|_| {
                let c = connector.clone();
                tokio::spawn(async move { handshake(addr, &c, "slow.test").await })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let during = handshake(addr, &connector, "localhost").await;
        let mut slowest = Duration::ZERO;
        for s in slow {
            slowest = slowest.max(s.await.unwrap());
        }
        // Positive control: the slow handshakes really were slow, so the fast one was measured while they were in flight.
        assert!(
            slowest >= SLOW,
            "{id}: the slow handshakes finished in {slowest:?}, so nothing was held"
        );
        assert!(
            during < SLOW / 2,
            "{id}: a fast handshake took {during:?} while {SLOW_CLIENTS} slow ones waited on the certificate source"
        );
        server.shutdown().await.unwrap();
    }
}
