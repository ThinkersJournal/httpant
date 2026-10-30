// SPIKE MEASUREMENT (not for merge): how long does a slow certificate source hold up other connections? Run with --nocapture.
#![cfg(feature = "tls")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use httpant_core::BoxFuture;
use httpant_server::{CertSource, CertSourceError, Server, ServerCert, TlsAcceptor};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const SLOW: Duration = Duration::from_millis(700);

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
async fn a_slow_source_and_the_time_a_fast_handshake_takes_meanwhile() {
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
    for id in ["hyper", "axum", "actix-web"] {
        let server = Server::with_adapter(id)
            .unwrap()
            .tls(TlsAcceptor::new(Source(Arc::new(cert.clone()))))
            .get("/", || async { "ok" })
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = server.addr();
        let alone = handshake(addr, &connector, "localhost").await;
        // Six handshakes held by the slow source (more than the two actix workers).
        let slow: Vec<_> = (0..6)
            .map(|_| {
                let c = connector.clone();
                tokio::spawn(async move { handshake(addr, &c, "slow.test").await })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let during = handshake(addr, &connector, "localhost").await;
        let mut slowest = Duration::ZERO;
        for s in slow {
            slowest = slowest.max(s.await.unwrap());
        }
        println!(
            "STALL {id:>10}: fast handshake alone {:>5} ms, while 6 slow ones were in flight {:>5} ms (slow ones took up to {} ms)",
            alone.as_millis(),
            during.as_millis(),
            slowest.as_millis()
        );
        server.shutdown().await.unwrap();
    }
}
