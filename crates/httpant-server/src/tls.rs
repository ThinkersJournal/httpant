// SPDX-License-Identifier: MIT OR Apache-2.0
//! TLS termination (feature `tls`; rustls only). HTTPAnt serves a connection with a certificate it is *handed*: the [`CertSource`] gets, stores
//! and renews certificates (WebHostAnt, WebServAnt, or a file pair); HTTPAnt never does ACME, storage or renewal. The source is asked once per
//! handshake with the server name the client sent, so a replaced certificate serves the next connection and the one before it is undisturbed.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crate::tls_audit::{self, RefusalReason, Served, TlsAudit, TlsEvent};
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
    audit: Option<Arc<dyn TlsAudit>>,
    served: Arc<Served>,
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
            audit: None,
            served: Arc::default(),
        }
    }

    /// Report [`TlsEvent`]s (refused handshakes, a changed certificate) to `sink`, for the audit trail.
    pub fn audit(mut self, sink: impl TlsAudit) -> Self {
        self.audit = Some(Arc::new(sink));
        self
    }

    fn emit(&self, event: TlsEvent) {
        if let Some(sink) = &self.audit {
            sink.event(&event);
        }
    }

    /// How long a client has to finish the handshake, source lookup included (default [`DEFAULT_HANDSHAKE_TIMEOUT`]).
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Run the handshake on `tcp`. Errors (timeout, unknown name, source failure, protocol error) end only this connection, and are reported to
    /// the audit sink if there is one.
    pub async fn accept<IO>(&self, tcp: IO) -> io::Result<TlsStream<IO>>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        // The name the client sent, once its hello has been read: kept outside the handshake so a timeout can still say which name it was.
        let hello_name = std::sync::Mutex::new(None::<Option<String>>);
        let handshake = async {
            let start = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), tcp)
                .await
                .map_err(|e| (RefusalReason::Protocol, e))?;
            let name = start.client_hello().server_name().map(str::to_owned);
            *hello_name.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
            let cert = match self.source.cert_for(name.as_deref()).await {
                Ok(Some(cert)) => cert,
                Ok(None) => {
                    let why = io::Error::other(format!("no certificate for server name {name:?}"));
                    return Err((RefusalReason::UnknownName, why));
                }
                Err(e) => return Err((RefusalReason::SourceFailed, io::Error::other(e))),
            };
            let config = config_for(&cert).map_err(|e| (RefusalReason::Protocol, e))?;
            let stream = start
                .into_stream(Arc::new(config))
                .await
                .map_err(|e| (RefusalReason::Protocol, e))?;
            // Only a handshake that completed counts as the certificate being served.
            if self.audit.is_some() && self.served.changed(&name, cert.leaf_der()) {
                self.emit(TlsEvent::CertificateChanged {
                    server_name: name.as_deref().map(tls_audit::clean_name),
                    serial: tls_audit::serial_hex(cert.leaf_der()),
                });
            }
            Ok(stream)
        };
        let refused = |reason: RefusalReason| {
            let name = hello_name
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .flatten();
            self.emit(TlsEvent::HandshakeRefused {
                server_name: name.as_deref().map(tls_audit::clean_name),
                reason,
            });
        };
        match tokio::time::timeout(self.handshake_timeout, handshake).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err((reason, why))) => {
                refused(reason);
                Err(why)
            }
            Err(_) => {
                refused(RefusalReason::Timeout);
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "TLS handshake timed out",
                ))
            }
        }
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

/// What a live TLS listener actually presented for a server name, read back over a real handshake (need 22: the state is what is served,
/// not what the [`CertSource`] says it holds).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServedCert {
    /// The server name asked for (sent as SNI).
    pub server_name: String,
    /// The leaf's issuer, as an RFC 4514-style string.
    pub issuer: String,
    /// The leaf's subject.
    pub subject: String,
    /// Start of validity, seconds since the Unix epoch.
    pub not_before: i64,
    /// End of validity, seconds since the Unix epoch.
    pub not_after: i64,
    /// Certificates in the chain the server sent.
    pub chain_len: usize,
    /// The leaf certificate, DER.
    pub leaf_der: Vec<u8>,
    /// The TLS version negotiated, `"TLSv1.3"` or `"TLSv1.2"`.
    pub protocol: String,
    /// The ALPN protocol negotiated (for example `http/1.1`), if any.
    pub alpn: Option<String>,
}

/// Connect to `addr`, handshake with SNI `server_name`, read what the listener presents, and hang up. No request is sent. **The
/// certificate is not validated** (that is the point: this reports what is served, including an expired or self-signed certificate), so the
/// result must never be used to decide whether to trust a peer. The handshake is bounded by `timeout`.
pub async fn read_back(
    addr: std::net::SocketAddr,
    server_name: &str,
    timeout: Duration,
) -> io::Result<ServedCert> {
    let name = rustls::pki_types::ServerName::try_from(server_name.to_owned())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a valid server name"))?;
    let provider = Arc::new(provider::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&version::TLS13, &version::TLS12])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnything(provider)))
        .with_no_client_auth();
    // Offer what the server offers, so the ALPN it would choose is visible.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let probe = async {
        let tcp = TcpStream::connect(addr).await?;
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await?;
        let (_, conn) = tls.get_ref();
        let chain = conn
            .peer_certificates()
            .ok_or_else(|| io::Error::other("the server presented no certificate"))?;
        let leaf = chain
            .first()
            .ok_or_else(|| io::Error::other("the server presented an empty chain"))?;
        let (_, x509) = x509_parser::parse_x509_certificate(leaf.as_ref())
            .map_err(|e| io::Error::other(format!("served certificate does not parse: {e}")))?;
        Ok(ServedCert {
            server_name: server_name.to_owned(),
            issuer: x509.issuer().to_string(),
            subject: x509.subject().to_string(),
            not_before: x509.validity().not_before.timestamp(),
            not_after: x509.validity().not_after.timestamp(),
            chain_len: chain.len(),
            leaf_der: leaf.as_ref().to_vec(),
            protocol: conn
                .protocol_version()
                .map_or_else(String::new, |v| format!("{v:?}")),
            alpn: conn
                .alpn_protocol()
                .map(|p| String::from_utf8_lossy(p).into_owned()),
        })
    };
    tokio::time::timeout(timeout, probe)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TLS read-back timed out"))?
}

/// Accepts every certificate: only [`read_back`] uses it, to look at what is served.
#[derive(Debug)]
struct AcceptAnything(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAnything {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
