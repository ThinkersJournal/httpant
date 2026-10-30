// SPDX-License-Identifier: MIT OR Apache-2.0
//! SPIKE: actix-web terminating TLS from an HTTPAnt `CertSource`. actix only takes a `rustls::ServerConfig`, whose certificate resolver is
//! synchronous, and its workers are single-threaded runtimes. The one way to ask an async source from there is to hand the call to another
//! runtime and block the worker thread until it answers.

use std::sync::Arc;
use std::time::Duration;

use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ServerConfig, version};

use crate::tls::CertSource;

#[derive(Debug)]
struct Bridge {
    source: Arc<dyn CertSource>,
    runtime: tokio::runtime::Handle,
    wait: Duration,
}

impl std::fmt::Debug for dyn CertSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CertSource")
    }
}

impl ResolvesServerCert for Bridge {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let name = hello.server_name().map(str::to_owned);
        let (tx, rx) = std::sync::mpsc::channel();
        let source = Arc::clone(&self.source);
        self.runtime.spawn(async move {
            let _ = tx.send(source.cert_for(name.as_deref()).await);
        });
        // BLOCKS this actix worker thread until the source answers or the wait is over.
        match rx.recv_timeout(self.wait) {
            Ok(Ok(Some(cert))) => Some(cert.certified()),
            _ => None,
        }
    }
}

pub(crate) fn config(
    acceptor: &crate::TlsAcceptor,
    runtime: tokio::runtime::Handle,
) -> ServerConfig {
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&version::TLS13, &version::TLS12])
            .expect("ring supports TLS 1.3 and 1.2")
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Bridge {
                source: acceptor.source(),
                runtime,
                wait: acceptor.timeout(),
            }));
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    // One config serves every connection here, unlike the acceptor that builds one per handshake. A resumed session never reaches the
    // resolver, so a renewed certificate would not reach a client that resumes: no session cache, no tickets.
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    config
}
