// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt client half (design `docs/HTTPANT-DESIGN.md`, section 5): the [`Client`] builder, the [`ClientAdapter`] trait, and the reference
//! [`HyperClientAdapter`] (hyper-util, plain `http://` only until the TLS hook exists).
//!
//! Requests and responses are the `http` crate's types over the streaming [`Body`], so nothing is lost at the boundary. The builder and the
//! timeout sit above the adapter, so they behave the same on every adapter; what an adapter can or cannot do (redirects, TLS, streaming) is
//! declared in [`httpant_core::ClientCapabilities`] and measured by the client conformance cases, never silent.

mod adapter;
mod hyper_adapter;

pub use adapter::ClientAdapter;
pub use httpant_core as core;
use httpant_core::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use httpant_core::{Body, Bytes, DEFAULT_BODY_LIMIT, Error, Request, Response, Result};
pub use hyper_adapter::HyperClientAdapter;
use std::sync::Arc;
use std::time::Duration;

/// The ids of the client adapters compiled into this build.
pub const ADAPTER_IDS: &[&str] = &["hyper"];

/// Describes requests once; a client adapter (default `hyper`) sends them.
#[derive(Clone)]
pub struct Client {
    adapter: Arc<dyn ClientAdapter>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// A client on the default adapter, hyper-util.
    pub fn new() -> Self {
        Self::with_adapter_impl(HyperClientAdapter::new())
    }

    /// A client on the adapter with this id. An unknown id is an error that lists the ids that exist.
    pub fn with_adapter(id: &str) -> Result<Self> {
        match id {
            "hyper" => Ok(Self::new()),
            _ => Err(Error::UnknownAdapter {
                id: id.to_owned(),
                available: ADAPTER_IDS.to_vec(),
            }),
        }
    }

    /// A client on an adapter you built yourself (also how a conformance mutant is plugged in).
    pub fn with_adapter_impl(adapter: impl ClientAdapter) -> Self {
        Self {
            adapter: Arc::new(adapter),
        }
    }

    /// The adapter this client sends with.
    pub fn adapter_id(&self) -> &'static str {
        self.adapter.id()
    }

    /// A request with any method.
    pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
        RequestBuilder {
            adapter: Arc::clone(&self.adapter),
            method,
            url: url.as_ref().to_owned(),
            headers: Ok(HeaderMap::new()),
            body: Body::empty(),
            timeout: None,
        }
    }

    /// `GET`.
    pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// `POST`.
    pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// `PUT`.
    pub fn put(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// `DELETE`.
    pub fn delete(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// `HEAD`.
    pub fn head(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }
}

/// One request being described. Mistakes (a bad header, a bad URL) are remembered and returned by [`RequestBuilder::send`], so a chain never panics.
pub struct RequestBuilder {
    adapter: Arc<dyn ClientAdapter>,
    method: Method,
    url: String,
    headers: Result<HeaderMap>,
    body: Body,
    timeout: Option<Duration>,
}

impl RequestBuilder {
    /// Add a header; name and value are bytes, and a name added twice is sent twice.
    pub fn header(mut self, name: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Self {
        if let Ok(headers) = &mut self.headers {
            let (name, value) = (name.as_ref(), value.as_ref());
            match (HeaderName::from_bytes(name), HeaderValue::from_bytes(value)) {
                (Ok(n), Ok(v)) => {
                    headers.append(n, v);
                }
                _ => {
                    self.headers = Err(Error::InvalidHeader {
                        name: String::from_utf8_lossy(name).into_owned(),
                    });
                }
            }
        }
        self
    }

    /// A body of exactly these bytes (sent with a `Content-Length`).
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Body::from_bytes(body);
        self
    }

    /// A streaming body, sent as it is produced.
    pub fn body_stream(mut self, body: Body) -> Self {
        self.body = body;
        self
    }

    /// Give up if the response head has not arrived within `timeout` (reading the body afterwards is not covered).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Send the request.
    pub async fn send(self) -> Result<ClientResponse> {
        let uri: Uri = self.url.parse().map_err(|_| Error::InvalidUrl {
            url: self.url.clone(),
            reason: "not a URL",
        })?;
        if uri.scheme().is_none() || uri.authority().is_none() {
            return Err(Error::InvalidUrl {
                url: self.url,
                reason: "scheme and host are required",
            });
        }
        let mut req = Request::new(self.body);
        *req.method_mut() = self.method;
        *req.uri_mut() = uri;
        *req.headers_mut() = self.headers?;
        let sending = self.adapter.send(req);
        let res = match self.timeout {
            Some(limit) => tokio::time::timeout(limit, sending)
                .await
                .map_err(|_| Error::Timeout(limit))??,
            None => sending.await?,
        };
        Ok(ClientResponse { inner: res })
    }
}

/// A response: status and headers now, the body as a stream or, with an explicit limit, as bytes.
#[derive(Debug)]
pub struct ClientResponse {
    inner: Response,
}

impl ClientResponse {
    /// The status exactly as the peer sent it.
    pub fn status(&self) -> StatusCode {
        self.inner.status()
    }

    /// The headers, repeats kept.
    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    /// The body as a stream.
    pub fn into_body(self) -> Body {
        self.inner.into_body()
    }

    /// The whole body, at most [`DEFAULT_BODY_LIMIT`] bytes.
    pub async fn bytes(self) -> Result<Bytes> {
        self.bytes_limited(DEFAULT_BODY_LIMIT).await
    }

    /// The whole body, at most `limit` bytes (`Error::BodyTooLarge` beyond that).
    pub async fn bytes_limited(self, limit: usize) -> Result<Bytes> {
        self.inner.into_body().bytes(limit).await
    }

    /// The body as UTF-8 text, at most [`DEFAULT_BODY_LIMIT`] bytes.
    pub async fn text(self) -> Result<String> {
        self.text_limited(DEFAULT_BODY_LIMIT).await
    }

    /// The body as UTF-8 text, at most `limit` bytes.
    pub async fn text_limited(self, limit: usize) -> Result<String> {
        self.inner.into_body().text(limit).await
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn a_url_without_scheme_or_host_is_an_invalid_url_error() {
        for bad in ["", "no-scheme", "/just/a/path", "http://"] {
            let err = Client::new().get(bad).send().await.unwrap_err();
            assert!(matches!(err, Error::InvalidUrl { .. }), "{bad:?}: {err}");
        }
    }

    #[tokio::test]
    async fn https_is_a_named_error_not_a_silent_downgrade() {
        let err = Client::new()
            .get("https://127.0.0.1:1/")
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "scheme `https` is not supported by the `hyper` client adapter: use http:// or another adapter"
        );
    }

    #[tokio::test]
    async fn a_bad_header_is_returned_by_send_and_never_panics() {
        for (name, value) in [("bad name", "v"), ("x-ok", "line\r\nbreak")] {
            let err = Client::new()
                .get("http://127.0.0.1:1/")
                .header(name, value)
                .send()
                .await
                .unwrap_err();
            assert!(matches!(err, Error::InvalidHeader { .. }), "{err}");
        }
    }

    #[test]
    fn an_unknown_adapter_id_lists_the_ones_that_exist() {
        let err = Client::with_adapter("nope").err().expect("unknown id");
        assert_eq!(
            err.to_string(),
            "unknown adapter `nope`: use one of [\"hyper\"]"
        );
    }

    #[tokio::test]
    async fn a_peer_that_never_answers_ends_in_a_timeout_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _silent = tokio::spawn(async move {
            let (_conn, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let started = std::time::Instant::now();
        let err = Client::new()
            .get(format!("http://{addr}/"))
            .timeout(Duration::from_millis(200))
            .send()
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Timeout(_)), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_refused_connection_is_a_request_error() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = free.local_addr().unwrap();
        drop(free);
        let err = Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Request(_)), "{err}");
    }
}
