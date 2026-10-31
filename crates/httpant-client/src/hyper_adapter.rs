// SPDX-License-Identifier: MIT OR Apache-2.0
use std::error::Error as StdError;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use httpant_core::{
    Body, BoxError, BoxFuture, ClientCapabilities, Error, Request, Response, Result, Support,
};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use crate::ClientAdapter;

/// Connect before this, unless the builder says otherwise.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A response head must arrive within this, unless the builder or the request says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// A response body may go quiet for this long between chunks, unless the builder says otherwise.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(feature = "https")]
type Connector = hyper_rustls::HttpsConnector<HttpConnector>;
#[cfg(not(feature = "https"))]
type Connector = HttpConnector;

/// What the builder hands the adapter.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub(crate) connect_timeout: Duration,
    pub(crate) read_timeout: Duration,
    /// PEM text of extra trusted roots, checked when the adapter is built.
    pub(crate) extra_roots_pem: Vec<Vec<u8>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            extra_roots_pem: Vec::new(),
        }
    }
}

/// The reference client adapter: hyper-util's pooled client over HTTP/1.1. `http://` always; `https://` with the `https` feature (rustls,
/// the bundled Mozilla roots, TLS 1.2 and 1.3, the certificate and the host name verified).
#[derive(Clone)]
pub struct HyperClientAdapter {
    client: Client<Connector, Body>,
    connect_timeout: Duration,
    read_timeout: Duration,
}

impl HyperClientAdapter {
    /// A client with hyper-util's default connection pool and the default timeouts.
    pub fn new() -> Self {
        Self::from_settings(&Settings::default())
            .expect("the default settings have no root certificates to reject")
    }

    pub(crate) fn from_settings(settings: &Settings) -> Result<Self> {
        let mut http = HttpConnector::new();
        http.set_connect_timeout(Some(settings.connect_timeout));
        #[cfg(feature = "https")]
        let connector = {
            http.enforce_http(false);
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_tls_config(tls::client_config(&settings.extra_roots_pem)?)
                .https_or_http()
                .enable_http1()
                .wrap_connector(http)
        };
        #[cfg(not(feature = "https"))]
        let connector = {
            if !settings.extra_roots_pem.is_empty() {
                return Err(Error::Tls(
                    "add_root_pem needs https: enable the `https` feature".to_owned(),
                ));
            }
            http
        };
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
            connect_timeout: settings.connect_timeout,
            read_timeout: settings.read_timeout,
        })
    }

    /// What went wrong, said so that it ends with what to do. Never includes the URL, and so never a query string.
    fn map_error(&self, e: hyper_util::client::legacy::Error) -> Error {
        self.classify(&e)
    }

    /// Looks down the whole chain of `e`: a timed-out connect, or a TLS error (which `io::Error` hides one level further down than `source`).
    pub(crate) fn classify(&self, e: &(dyn StdError + 'static)) -> Error {
        let mut cur: Option<&(dyn StdError + 'static)> = Some(e);
        while let Some(err) = cur {
            let mut node = err;
            loop {
                if let Some(tls) = tls::describe(node) {
                    return Error::TlsHandshake(tls);
                }
                let Some(io) = node.downcast_ref::<std::io::Error>() else {
                    break;
                };
                if io.kind() == std::io::ErrorKind::TimedOut {
                    return Error::ConnectTimeout(self.connect_timeout);
                }
                match io.get_ref() {
                    Some(inner) => node = inner,
                    None => break,
                }
            }
            cur = err.source();
        }
        Error::Request(Box::new(Chain(chain(e))))
    }
}

impl Default for HyperClientAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAdapter for HyperClientAdapter {
    fn id(&self) -> &'static str {
        "hyper"
    }

    fn capabilities(&self) -> ClientCapabilities {
        let mut c = ClientCapabilities::default();
        // Measured by the client conformance cases 5-8 and the redirect probe (`declared_client_capabilities_match_the_measured_ones`).
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.connection_reuse = Support::Yes;
        c.follows_redirects = Support::No;
        c.tls = if cfg!(feature = "https") {
            Support::Yes
        } else {
            Support::No
        };
        c
    }

    fn send(&self, req: Request) -> BoxFuture<'_, Result<Response>> {
        Box::pin(async move {
            match req.uri().scheme_str() {
                Some("http") => {}
                Some("https") => {
                    if !cfg!(feature = "https") {
                        return Err(Error::SchemeNotCompiledIn {
                            scheme: "https".to_owned(),
                            feature: "https",
                        });
                    }
                }
                Some(other) => {
                    return Err(Error::UnsupportedScheme {
                        scheme: other.to_owned(),
                        adapter: "hyper",
                    });
                }
                None => {
                    return Err(Error::InvalidUrl {
                        url: crate::redact_url(&req.uri().to_string()),
                        reason: "no scheme",
                    });
                }
            }
            let res = self
                .client
                .request(req)
                .await
                .map_err(|e| self.map_error(e))?;
            Ok(res.map(|body| Body::new(IdleBody::new(Body::new(body), self.read_timeout))))
        })
    }
}

/// The text of an error and of everything under it.
fn chain(e: &(dyn StdError + 'static)) -> String {
    let mut out = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        out.push_str(": ");
        out.push_str(&c.to_string());
        cur = c.source();
    }
    out
}

#[derive(Debug)]
struct Chain(String);

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for Chain {}

/// The error a response body reports when it was quiet for longer than the read timeout.
#[derive(Debug)]
pub(crate) struct IdleTimeout(pub(crate) Duration);

impl std::fmt::Display for IdleTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the response went quiet for {:?}: raise the read timeout or check the peer",
            self.0
        )
    }
}

impl StdError for IdleTimeout {}

/// A body that fails with [`IdleTimeout`] if no frame arrives for `limit`.
struct IdleBody {
    inner: Body,
    limit: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
}

impl IdleBody {
    fn new(inner: Body, limit: Duration) -> Self {
        Self {
            inner,
            limit,
            sleep: Box::pin(tokio::time::sleep(limit)),
        }
    }
}

impl HttpBody for IdleBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(item) => {
                this.sleep
                    .as_mut()
                    .reset(tokio::time::Instant::now() + this.limit);
                Poll::Ready(item)
            }
            Poll::Pending => match this.sleep.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Some(Err(Box::new(IdleTimeout(this.limit))))),
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(feature = "https")]
mod tls {
    use std::sync::Arc;

    use httpant_core::{Error, Result};
    use rustls::RootCertStore;
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;

    /// TLS 1.2 and 1.3, the ring provider (as the server), the bundled Mozilla roots plus any PEM roots the caller added. A PEM that holds no
    /// certificate is an error: the client never ends up trusting less than was asked, nor more.
    pub(super) fn client_config(extra_roots_pem: &[Vec<u8>]) -> Result<rustls::ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for pem in extra_roots_pem {
            let mut found = 0usize;
            for cert in CertificateDer::pem_slice_iter(pem) {
                let cert = cert.map_err(|e| Error::Tls(format!("root certificate PEM: {e}")))?;
                roots
                    .add(cert)
                    .map_err(|e| Error::Tls(format!("root certificate: {e}")))?;
                found += 1;
            }
            if found == 0 {
                return Err(Error::Tls(
                    "the PEM given to add_root_pem holds no certificate: pass the CA certificate, not its key".to_owned(),
                ));
            }
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(format!("client protocol versions: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(config)
    }

    /// The text of `e` if it is a rustls error.
    pub(super) fn describe(e: &(dyn std::error::Error + 'static)) -> Option<String> {
        e.downcast_ref::<rustls::Error>().map(|e| e.to_string())
    }
}

#[cfg(not(feature = "https"))]
mod tls {
    pub(super) fn describe(_: &(dyn std::error::Error + 'static)) -> Option<String> {
        None
    }
}
