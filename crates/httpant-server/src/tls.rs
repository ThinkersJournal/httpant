// SPDX-License-Identifier: MIT OR Apache-2.0
//! TLS termination (feature `tls`; rustls only). HTTPAnt serves a connection with a certificate it is *handed*: the [`CertSource`] gets, stores
//! and renews certificates (WebHostAnt, WebServAnt, or a file pair); HTTPAnt never does ACME, storage or renewal. The source is asked once per
//! handshake with the server name the client sent, so a replaced certificate serves the next connection and the one before it is undisturbed.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use httpant_core::{BoxFuture, Error, Result};
use rustls::crypto::ring as provider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ServerConfig, version};
use tokio::net::TcpStream;
use tokio_rustls::LazyConfigAcceptor;
use tokio_rustls::server::TlsStream;

/// Request extension an adapter sets on every request that arrived over TLS this server terminated, so [`Origin`](crate::Origin) says `https`
/// without believing any header.
#[derive(Clone, Copy, Debug)]
pub struct TlsConnection;

/// Default time a client has to finish the handshake (a client that connects and says nothing must not hold the connection).
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// A certificate chain and its private key. `Debug` never shows the key, and the key bytes are not reachable from outside this type.
#[derive(Clone)]
pub struct ServerCert {
    certified: Arc<CertifiedKey>,
}

impl ServerCert {
    /// From PEM text: the chain (leaf first) and a PKCS#8, PKCS#1 or SEC1 private key. Errors name what is wrong, never the key.
    pub fn from_pem(chain_pem: &[u8], key_pem: &[u8]) -> Result<Self> {
        let chain = CertificateDer::pem_slice_iter(chain_pem)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Tls(format!("certificate chain is not valid PEM: {e}")))?;
        if chain.is_empty() {
            return Err(Error::Tls("certificate chain holds no certificate".into()));
        }
        let key = PrivateKeyDer::from_pem_slice(key_pem)
            .map_err(|e| Error::Tls(format!("private key is not valid PEM: {e}")))?;
        Self::from_der(chain, key)
    }

    /// From DER parts.
    pub fn from_der(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self> {
        let signing = provider::sign::any_supported_type(&key).map_err(|_| {
            Error::Tls("private key is not a supported type (RSA, ECDSA or Ed25519)".into())
        })?;
        let certified = CertifiedKey::new(chain, signing);
        certified
            .keys_match()
            .map_err(|_| Error::Tls("private key does not match the certificate".into()))?;
        Ok(Self {
            certified: Arc::new(certified),
        })
    }

    /// The leaf certificate, DER.
    pub fn leaf_der(&self) -> &[u8] {
        self.certified
            .end_entity_cert()
            .map_or(&[][..], |c| c.as_ref())
    }
}

impl fmt::Debug for ServerCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerCert")
            .field("certificates", &self.certified.cert.len())
            .field("key", &"<redacted>")
            .finish()
    }
}

/// A source could not produce a certificate. The handshake is refused; no other certificate is served in its place.
#[derive(Debug)]
pub struct CertSourceError(pub String);

impl fmt::Display for CertSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "certificate source failed: {}", self.0)
    }
}

impl std::error::Error for CertSourceError {}

/// Where certificates come from. Called once per handshake, so it may answer differently from one call to the next (renewal) and may be async.
pub trait CertSource: Send + Sync + 'static {
    /// The certificate for `server_name`, the name in the client's SNI extension (`None` when it sent none, for example a connection by IP
    /// address: the source decides, and [`FixedCert`] answers with its one certificate). `Ok(None)` is an unknown name and `Err` a failure;
    /// either refuses that handshake and neither falls back to another name's certificate.
    fn cert_for<'a>(
        &'a self,
        server_name: Option<&'a str>,
    ) -> BoxFuture<'a, std::result::Result<Option<Arc<ServerCert>>, CertSourceError>>;
}

/// A source with one certificate for every name (a file pair, or a self-signed certificate in tests).
#[derive(Clone, Debug)]
pub struct FixedCert(Arc<ServerCert>);

impl FixedCert {
    /// Serve `cert` for every name.
    pub fn new(cert: ServerCert) -> Self {
        Self(Arc::new(cert))
    }
}

impl CertSource for FixedCert {
    fn cert_for<'a>(
        &'a self,
        _server_name: Option<&'a str>,
    ) -> BoxFuture<'a, std::result::Result<Option<Arc<ServerCert>>, CertSourceError>> {
        Box::pin(async move { Ok(Some(Arc::clone(&self.0))) })
    }
}

#[derive(Debug)]
struct One(Arc<CertifiedKey>);

impl ResolvesServerCert for One {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}

/// Terminates TLS on accepted connections with certificates from a [`CertSource`]. TLS 1.3 and 1.2 only (rustls offers nothing older), ALPN
/// `http/1.1` (the adapters speak HTTP/1.1 only; `h2` waits for them).
#[derive(Clone)]
pub struct TlsAcceptor {
    source: Arc<dyn CertSource>,
    handshake_timeout: Duration,
}

impl fmt::Debug for TlsAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAcceptor")
            .field("handshake_timeout", &self.handshake_timeout)
            .finish_non_exhaustive()
    }
}

impl TlsAcceptor {
    /// Terminate TLS with certificates from `source`.
    pub fn new(source: impl CertSource) -> Self {
        Self {
            source: Arc::new(source),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
        }
    }

    /// How long a client has to finish the handshake, source lookup included (default [`DEFAULT_HANDSHAKE_TIMEOUT`]).
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Run the handshake on `tcp`. Errors (timeout, unknown name, source failure, protocol error) end only this connection.
    pub async fn accept(&self, tcp: TcpStream) -> io::Result<TlsStream<TcpStream>> {
        let handshake = async {
            let start = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), tcp).await?;
            let name = start.client_hello().server_name().map(str::to_owned);
            let cert = self
                .source
                .cert_for(name.as_deref())
                .await
                .map_err(io::Error::other)?
                .ok_or_else(|| {
                    io::Error::other(format!("no certificate for server name {name:?}"))
                })?;
            start.into_stream(Arc::new(config_for(&cert)?)).await
        };
        tokio::time::timeout(self.handshake_timeout, handshake)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TLS handshake timed out"))?
    }
}

fn config_for(cert: &ServerCert) -> io::Result<ServerConfig> {
    let mut config = ServerConfig::builder_with_provider(Arc::new(provider::default_provider()))
        .with_protocol_versions(&[&version::TLS13, &version::TLS12])
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(One(Arc::clone(&cert.certified))));
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}
