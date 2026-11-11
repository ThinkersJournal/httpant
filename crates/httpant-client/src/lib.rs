// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt client half (design `docs/HTTPANT-DESIGN.md`, section 5): the [`Client`] builder, the [`ClientAdapter`] trait, and the reference
//! [`HyperClientAdapter`] (hyper-util; `http://` always, `https://` with the `https` feature).
//!
//! Requests and responses are the `http` crate's types over the streaming [`Body`], so nothing is lost at the boundary. The builder and the
//! timeout sit above the adapter, so they behave the same on every adapter; what an adapter can or cannot do (redirects, TLS, streaming) is
//! declared in [`httpant_core::ClientCapabilities`] and measured by the client conformance cases, never silent.

mod adapter;
mod guard;
mod hyper_adapter;
mod proxy;

pub use adapter::ClientAdapter;
pub use httpant_core as core;
use httpant_core::http::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, LOCATION,
    PROXY_AUTHORIZATION, TRANSFER_ENCODING,
};
use httpant_core::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use httpant_core::{Body, Bytes, DEFAULT_BODY_LIMIT, Error, Request, Response, Result};
pub use hyper_adapter::{
    DEFAULT_CONNECT_TIMEOUT, DEFAULT_READ_TIMEOUT, DEFAULT_TIMEOUT, HyperClientAdapter, RootStore,
};
use std::sync::Arc;
use std::time::Duration;

/// The ids of the client adapters compiled into this build.
pub const ADAPTER_IDS: &[&str] = &["hyper"];

/// Describes requests once; a client adapter (default `hyper`) sends them.
#[derive(Clone)]
pub struct Client {
    adapter: Arc<dyn ClientAdapter>,
    timeout: Duration,
    max_redirects: u32,
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
            timeout: DEFAULT_TIMEOUT,
            max_redirects: 0,
        }
    }

    /// A builder for a client with its own timeouts and trusted roots. Every limit has a finite default.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
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
            body: Payload::Empty,
            timeout: Some(self.timeout),
            max_redirects: self.max_redirects,
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

    /// `PATCH`.
    pub fn patch(&self, url: impl AsRef<str>) -> RequestBuilder {
        self.request(Method::PATCH, url)
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

/// Describes a [`Client`]'s limits and trusted roots. Nothing here can turn certificate checking off: https is verified against the bundled
/// Mozilla roots (or the operating system's store, if you choose [`RootStore::Os`] or [`RootStore::Both`]) plus the roots you add, and a
/// connection that fails the check is an error.
#[derive(Clone, Debug)]
pub struct ClientBuilder {
    timeout: Duration,
    max_redirects: u32,
    settings: hyper_adapter::Settings,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_redirects: 0,
            settings: hyper_adapter::Settings::default(),
        }
    }
}

impl ClientBuilder {
    /// Give up connecting after this (default 10 s). The error says it was the connect step.
    pub fn connect_timeout(mut self, limit: Duration) -> Self {
        self.settings.connect_timeout = limit;
        self
    }

    /// Give up if a response head has not arrived this long after the request was sent (default 30 s). A single request can set its own.
    pub fn timeout(mut self, limit: Duration) -> Self {
        self.timeout = limit;
        self
    }

    /// Follow up to `max` redirects (`301`, `302`, `303`, `307`, `308` with a `Location`); the default is `0`, which follows none and hands you the
    /// redirect response. Following is done here, above the adapter, so every adapter behaves alike. The rules: **never from `https` to `http`**
    /// (an [`Error::Redirect`]); `Authorization`, `Proxy-Authorization`, `Cookie` and `Host` are dropped when the scheme, host or port changes; `303`
    /// (and `301`/`302` after a `POST`) becomes a `GET` with no body; `307`/`308` keep the method and body, but a streamed body cannot be sent twice,
    /// so that redirect is not followed and you get it back; more than `max` redirects is an [`Error::Redirect`]. The private-address guard applies to
    /// every hop, and the head timeout to each. A single request can set its own with [`RequestBuilder::follow_redirects`].
    pub fn follow_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Give up if a response body is quiet for this long between chunks (default 30 s). The error says it was the read.
    pub fn read_timeout(mut self, limit: Duration) -> Self {
        self.settings.read_timeout = limit;
        self
    }

    /// Refuse to connect to loopback, private-network, link-local (the cloud metadata address among them), carrier-grade-NAT, multicast, documentation
    /// and reserved addresses, with [`Error::PrivateAddress`]; off by default. For a client that fetches a URL someone else chose (a health probe, a
    /// webhook) so that it cannot be aimed at your own network. The check is on the address actually connected to, in three places: the name lookup
    /// keeps only public answers (so a name that resolves to a private address, or to a public and a private one, cannot slip through), an IP written
    /// in the URL is refused before connecting, and the connection is looked at once more after it is made, before TLS and before any request byte.
    /// An IPv4 address inside an IPv6 one (`::ffff:10.0.0.1`, NAT64, 6to4) is judged as the IPv4 address. A followed redirect would pass through
    /// the same connector, so the guard applies to it too. Through a proxy (not supported yet) the address reached would be the proxy's.
    pub fn refuse_private_addresses(mut self, refuse: bool) -> Self {
        self.settings.refuse_private = refuse;
        self
    }

    /// Which roots https trusts (default [`RootStore::Bundled`]). [`RootStore::Os`] and [`RootStore::Both`] read the operating system's store when
    /// the client is built, and an empty or unreadable store is an error from [`ClientBuilder::build`]. Needs the `https` feature for anything but
    /// the default.
    pub fn root_store(mut self, store: RootStore) -> Self {
        self.settings.root_store = store;
        self
    }

    /// Send `https://` requests through this HTTP proxy by CONNECT: `http://[user[:password]@]host[:port]`. The proxy sees the host and port, not the
    /// request, because TLS runs inside the tunnel. A user name and password become `Proxy-Authorization: Basic` on the CONNECT request and are never
    /// printed. Needs the `https` feature. A plain `http://` URL is an error while a proxy is set (it is never sent around the proxy), and a proxy
    /// reached over `https://` is not supported yet. An address that cannot be honoured, or a combination with
    /// [`refuse_private_addresses`](Self::refuse_private_addresses) not resolved by [`guard_judges_proxy`](Self::guard_judges_proxy), is
    /// [`Error::Proxy`] from [`ClientBuilder::build`]. No environment variable is read.
    pub fn proxy(mut self, url: impl AsRef<str>) -> Self {
        self.settings.proxy = Some(url.as_ref().to_owned());
        self
    }

    /// With a [`proxy`](Self::proxy) and [`refuse_private_addresses`](Self::refuse_private_addresses) both set, say which address the guard judges:
    /// the proxy's. The proxy looks the target up itself, so the guard cannot see the target's address, and a client that sets both without this
    /// is refused by `build` instead of being quietly less protected than it looks. Use a proxy you trust to enforce its own egress rules.
    pub fn guard_judges_proxy(mut self, yes: bool) -> Self {
        self.settings.guard_judges_proxy = yes;
        self
    }

    /// Also trust the CA certificate(s) in this PEM text (a private CA, or a test server's). Needs the `https` feature; a PEM with no certificate is
    /// an error from [`ClientBuilder::build`].
    pub fn add_root_pem(mut self, pem: impl AsRef<[u8]>) -> Self {
        self.settings.extra_roots_pem.push(pem.as_ref().to_vec());
        self
    }

    /// Build the client on the default adapter.
    pub fn build(self) -> Result<Client> {
        let adapter = HyperClientAdapter::from_settings(&self.settings)?;
        let mut client = Client::with_adapter_impl(adapter);
        client.timeout = self.timeout;
        client.max_redirects = self.max_redirects;
        Ok(client)
    }
}

/// `scheme://host[:port]/path` of `url` and nothing else: no user name or password, no query, no fragment. Those can hold a secret, so none of
/// them is ever written into an error or a `Debug`.
pub(crate) fn redact_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let url = &url[..end];
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    format!("{scheme}{host}{path}")
}

fn is_secret_header(name: &HeaderName) -> bool {
    name == AUTHORIZATION || name == PROXY_AUTHORIZATION || name == COOKIE
}

/// One request being described. Mistakes (a bad header, a bad URL) are remembered and returned by [`RequestBuilder::send`], so a chain never panics.
pub struct RequestBuilder {
    adapter: Arc<dyn ClientAdapter>,
    method: Method,
    url: String,
    headers: Result<HeaderMap>,
    body: Payload,
    timeout: Option<Duration>,
    max_redirects: u32,
}

/// The request body, kept so that a redirect can send it again when it can be.
enum Payload {
    Empty,
    /// Bytes can be sent as often as needed.
    Bytes(Bytes),
    /// A stream is sent once; `None` after it has been.
    Stream(Option<Body>),
}

impl Payload {
    /// The body for one attempt; `None` if a stream has already been used.
    fn take_for_send(&mut self) -> Option<Body> {
        match self {
            Payload::Empty => Some(Body::empty()),
            Payload::Bytes(b) => Some(Body::from_bytes(b.clone())),
            Payload::Stream(s) => s.take(),
        }
    }

    fn replayable(&self) -> bool {
        !matches!(self, Payload::Stream(_))
    }
}

impl RequestBuilder {
    /// Add a header; name and value are bytes, and a name added twice is sent twice.
    pub fn header(mut self, name: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Self {
        if let Ok(headers) = &mut self.headers {
            let (name, value) = (name.as_ref(), value.as_ref());
            match (HeaderName::from_bytes(name), HeaderValue::from_bytes(value)) {
                (Ok(n), Ok(mut v)) => {
                    if is_secret_header(&n) {
                        v.set_sensitive(true);
                    }
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

    /// `Authorization: Bearer <token>`. The value is marked sensitive, so `Debug` prints it as sensitive and it is never in an error.
    pub fn bearer(mut self, token: impl AsRef<str>) -> Self {
        if let Ok(headers) = &mut self.headers {
            match HeaderValue::from_str(&format!("Bearer {}", token.as_ref())) {
                Ok(mut v) => {
                    v.set_sensitive(true);
                    headers.insert(AUTHORIZATION, v);
                }
                Err(_) => {
                    self.headers = Err(Error::InvalidHeader {
                        name: "authorization".to_owned(),
                    });
                }
            }
        }
        self
    }

    /// A body of exactly these bytes (sent with a `Content-Length`).
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Payload::Bytes(body.into());
        self
    }

    /// A streaming body, sent as it is produced.
    pub fn body_stream(mut self, body: Body) -> Self {
        self.body = Payload::Stream(Some(body));
        self
    }

    /// Follow up to `max` redirects for this request, whatever the client was built with (`0` follows none). The rules are on
    /// [`ClientBuilder::follow_redirects`].
    pub fn follow_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Give up if the response head has not arrived within `timeout` (default 30 s; the body has its own read timeout on the client).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Send the request.
    pub async fn send(mut self) -> Result<ClientResponse> {
        let mut uri = parse_url(&self.url)?;
        let mut headers = self.headers?;
        let mut hops = 0u32;
        loop {
            let Some(body) = self.body.take_for_send() else {
                // Only reachable after a followed redirect, which is never taken for a stream.
                return Err(Error::Redirect(
                    "a streamed request body cannot be sent again".to_owned(),
                ));
            };
            let mut req = Request::new(body);
            *req.method_mut() = self.method.clone();
            *req.uri_mut() = uri.clone();
            *req.headers_mut() = headers.clone();
            let sending = self.adapter.send(req);
            let res = match self.timeout {
                Some(limit) => tokio::time::timeout(limit, sending)
                    .await
                    .map_err(|_| Error::Timeout(limit))??,
                None => sending.await?,
            };
            if self.max_redirects == 0 || !is_redirect(res.status()) {
                return Ok(ClientResponse { inner: res });
            }
            let Some(location) = res
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
            else {
                // A redirect status with no usable Location says nowhere to go: the caller gets the response.
                return Ok(ClientResponse { inner: res });
            };
            let status = res.status();
            let to_get = status == StatusCode::SEE_OTHER
                || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND)
                    && self.method == Method::POST);
            if !to_get && !self.body.replayable() {
                // 307/308 must resend the body, and a stream is gone: hand the redirect back rather than send something else.
                return Ok(ClientResponse { inner: res });
            }
            if hops >= self.max_redirects {
                return Err(Error::Redirect(format!(
                    "more than {} redirects",
                    self.max_redirects
                )));
            }
            let next = resolve_location(&uri, &location)
                .map_err(|reason| Error::Redirect(format!("the Location header {reason}")))?;
            if uri.scheme_str() == Some("https") && next.scheme_str() == Some("http") {
                return Err(Error::Redirect(
                    "refused to follow a redirect from https to http".to_owned(),
                ));
            }
            if origin(&uri) != origin(&next) {
                for name in [AUTHORIZATION, PROXY_AUTHORIZATION, COOKIE, HOST] {
                    headers.remove(name);
                }
            }
            if to_get {
                if self.method != Method::HEAD {
                    self.method = Method::GET;
                }
                self.body = Payload::Empty;
                for name in [
                    CONTENT_TYPE,
                    CONTENT_LENGTH,
                    CONTENT_ENCODING,
                    TRANSFER_ENCODING,
                ] {
                    headers.remove(name);
                }
            }
            uri = next;
            hops += 1;
        }
    }
}

fn parse_url(url: &str) -> Result<Uri> {
    let uri: Uri = url.parse().map_err(|_| Error::InvalidUrl {
        url: redact_url(url),
        reason: "not a URL",
    })?;
    if uri.scheme().is_none() || uri.authority().is_none() {
        return Err(Error::InvalidUrl {
            url: redact_url(url),
            reason: "scheme and host are required",
        });
    }
    Ok(uri)
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

/// Scheme, host and effective port: what "the same place" means for sending credentials again.
fn origin(uri: &Uri) -> (String, String, u16) {
    let scheme = uri.scheme_str().unwrap_or_default().to_ascii_lowercase();
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    (
        scheme,
        uri.host().unwrap_or_default().to_ascii_lowercase(),
        port,
    )
}

/// Resolve a `Location` value against the URL that returned it (RFC 3986 section 5, without the fragment, which a client does not send).
/// The reason it cannot be used is returned as text that holds none of the URL.
fn resolve_location(base: &Uri, location: &str) -> std::result::Result<Uri, &'static str> {
    let location = location.split('#').next().unwrap_or_default().trim();
    if location.is_empty() {
        return Err("is empty");
    }
    let scheme = base.scheme_str().ok_or("has no base scheme")?;
    let authority = base.authority().ok_or("has no base host")?;
    let text = if let Some((s, _)) = location.split_once("://") {
        if s.is_empty()
            || !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        {
            return Err("is not a usable URL");
        }
        location.to_owned()
    } else if let Some(rest) = location.strip_prefix("//") {
        format!("{scheme}://{rest}")
    } else if location.starts_with('/') {
        format!("{scheme}://{authority}{}", remove_dot_segments(location))
    } else if let Some(query) = location.strip_prefix('?') {
        format!("{scheme}://{authority}{}?{query}", base.path())
    } else {
        let path = base.path();
        let dir = &path[..=path.rfind('/').unwrap_or(0)];
        let dir = if dir.is_empty() { "/" } else { dir };
        format!(
            "{scheme}://{authority}{}",
            remove_dot_segments(&format!("{dir}{location}"))
        )
    };
    let uri: Uri = text.parse().map_err(|_| "is not a usable URL")?;
    match uri.scheme_str() {
        Some("http" | "https") if uri.authority().is_some() => Ok(uri),
        _ => Err("names a scheme this client does not use"),
    }
}

/// `.` and `..` segments removed from a path that may carry a query.
fn remove_dot_segments(path_and_query: &str) -> String {
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *seg {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut joined = out.join("/");
    if !joined.starts_with('/') {
        joined.insert(0, '/');
    }
    match query {
        Some(q) => format!("{joined}?{q}"),
        None => joined,
    }
}

impl std::fmt::Debug for RequestBuilder {
    /// The method, the URL without user name, query or fragment, and header **names** only; never a value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self
            .headers
            .as_ref()
            .map(|h| h.keys().map(HeaderName::as_str).collect())
            .unwrap_or_default();
        f.debug_struct("RequestBuilder")
            .field("method", &self.method)
            .field("url", &redact_url(&self.url))
            .field("header_names", &names)
            .field("timeout", &self.timeout)
            .finish()
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
        self.inner
            .into_body()
            .bytes(limit)
            .await
            .map_err(idle_timeout_error)
    }

    /// The body as UTF-8 text, at most [`DEFAULT_BODY_LIMIT`] bytes.
    pub async fn text(self) -> Result<String> {
        self.text_limited(DEFAULT_BODY_LIMIT).await
    }

    /// The body as UTF-8 text, at most `limit` bytes.
    pub async fn text_limited(self, limit: usize) -> Result<String> {
        let bytes = self.bytes_limited(limit).await?;
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::NotUtf8)
    }
}

/// A body that went quiet reports [`Error::ReadTimeout`], not a bare body error.
fn idle_timeout_error(e: Error) -> Error {
    if let Error::Body(inner) = &e
        && let Some(idle) = inner.downcast_ref::<hyper_adapter::IdleTimeout>()
    {
        return Error::ReadTimeout(idle.0);
    }
    e
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

    #[cfg(not(feature = "https"))]
    #[tokio::test]
    async fn https_without_the_feature_names_the_feature_and_never_downgrades() {
        let err = Client::new()
            .get("https://127.0.0.1:1/")
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "scheme `https` is not compiled in: enable the `https` feature"
        );
    }

    #[tokio::test]
    async fn a_scheme_nobody_serves_is_a_named_error() {
        let err = Client::new()
            .get("ftp://127.0.0.1:1/")
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "scheme `ftp` is not supported by the `hyper` client adapter: use http:// or https://"
        );
    }

    #[test]
    fn a_redacted_url_keeps_scheme_host_and_path_and_nothing_else() {
        for (given, shown) in [
            (
                "http://example.com/a/b?token=SECRET#frag",
                "http://example.com/a/b",
            ),
            (
                "https://user:pw@example.com:8443/x?k=v",
                "https://example.com:8443/x",
            ),
            ("https://example.com?token=SECRET", "https://example.com"),
            ("/just/a/path?token=SECRET", "/just/a/path"),
            ("no-scheme#SECRET", "no-scheme"),
        ] {
            assert_eq!(redact_url(given), shown, "{given}");
        }
    }

    #[tokio::test]
    async fn no_error_prints_the_query_string_or_the_credentials_of_a_url() {
        // An unusable URL, an unsupported scheme and a refused connection, each with a secret in the URL.
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let refused = format!(
            "http://user:SECRETPW@{}/x?token=SECRETQ",
            free.local_addr().unwrap()
        );
        drop(free);
        for url in [
            "/just/a/path?token=SECRETQ".to_owned(),
            "ftp://user:SECRETPW@127.0.0.1:1/x?token=SECRETQ".to_owned(),
            refused,
        ] {
            let err = Client::new().get(&url).send().await.unwrap_err();
            for text in [err.to_string(), format!("{err:?}")] {
                assert!(
                    !text.contains("SECRETQ") && !text.contains("SECRETPW"),
                    "an error carried the secret part of a URL"
                );
            }
        }
    }

    #[test]
    fn a_bearer_token_is_sensitive_and_is_not_in_the_debug_output() {
        let request = Client::new()
            .get("http://127.0.0.1:1/x?k=SECRETQ")
            .bearer("SECRETTOKEN")
            .header("cookie", "SECRETCOOKIE")
            .header("x-plain", "visible");
        let headers = request.headers.as_ref().unwrap();
        assert!(headers[AUTHORIZATION].is_sensitive());
        assert!(headers[COOKIE].is_sensitive());
        assert!(!headers["x-plain"].is_sensitive());
        for text in [format!("{request:?}"), format!("{headers:?}")] {
            assert!(
                !text.contains("SECRETTOKEN")
                    && !text.contains("SECRETCOOKIE")
                    && !text.contains("SECRETQ"),
                "a secret reached Debug output"
            );
        }
        assert!(format!("{request:?}").contains("authorization"));
    }

    #[tokio::test]
    async fn a_429_with_retry_after_arrives_unchanged_and_a_post_is_sent_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut conn, _)) = listener.accept().await {
                let counter = Arc::clone(&counter);
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = conn.read(&mut buf).await {
                        if n == 0 {
                            return;
                        }
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = conn
                            .write_all(
                                b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n",
                            )
                            .await;
                    }
                });
            }
        });
        let res = Client::new()
            .post(format!("http://{addr}/limited"))
            .body("x")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 429);
        assert_eq!(res.headers()["retry-after"], "7");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            seen.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the client retried"
        );
    }

    #[tokio::test]
    async fn a_body_that_goes_quiet_ends_in_a_read_timeout_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _quiet = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = conn.read(&mut buf).await;
            let _ = conn
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")
                .await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let client = Client::builder()
            .read_timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let res = client.get(format!("http://{addr}/")).send().await.unwrap();
        let err = res.text().await.unwrap_err();
        assert!(matches!(err, Error::ReadTimeout(_)), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_timed_out_connect_says_it_was_the_connect_step() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("tcp connect error")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let adapter = HyperClientAdapter::new();
        let timed_out = Outer(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connection timed out",
        ));
        assert!(matches!(
            adapter.classify(&timed_out),
            Error::ConnectTimeout(d) if d == DEFAULT_CONNECT_TIMEOUT
        ));
        let refused = Outer(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ));
        assert!(matches!(adapter.classify(&refused), Error::Request(_)));
    }

    #[test]
    fn a_root_pem_without_a_certificate_is_an_error_and_never_silently_ignored() {
        let err = Client::builder()
            .add_root_pem("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n")
            .build()
            .err()
            .expect("a key is not a root certificate");
        assert!(
            matches!(err, Error::Tls(_)),
            "a PEM that holds no certificate must be refused: {err}"
        );
    }

    #[cfg(feature = "https")]
    #[test]
    fn the_os_store_builds_and_is_never_a_silent_empty_trust() {
        // On a machine with any system roots (CI runners and developer machines have them) all three choices build; where the store is
        // empty, `Os` and `Both` must say so rather than build a client that trusts nothing.
        Client::builder()
            .root_store(RootStore::Bundled)
            .build()
            .expect("bundled roots always build");
        for store in [RootStore::Os, RootStore::Both] {
            match Client::builder().root_store(store).build() {
                Ok(_) => {}
                Err(Error::Tls(m)) => assert!(m.contains("certificate store"), "{m}"),
                Err(other) => panic!("an empty store must be a Tls error: {other}"),
            }
        }
    }

    #[cfg(not(feature = "https"))]
    #[test]
    fn a_root_store_choice_without_https_is_an_error_not_ignored() {
        let err = Client::builder()
            .root_store(RootStore::Os)
            .build()
            .err()
            .expect("needs the https feature");
        assert!(matches!(err, Error::Tls(_)), "{err}");
    }

    #[test]
    fn a_location_is_resolved_against_the_url_that_sent_it() {
        let base: Uri = "https://a.example:8443/x/y/z?q=1".parse().unwrap();
        for (location, expected) in [
            ("https://b.example/p", "https://b.example/p"),
            ("//b.example/p", "https://b.example/p"),
            ("/abs", "https://a.example:8443/abs"),
            ("/a/./b/../c", "https://a.example:8443/a/c"),
            ("rel", "https://a.example:8443/x/y/rel"),
            ("../up", "https://a.example:8443/x/up"),
            ("../../../../way/up", "https://a.example:8443/way/up"),
            ("?n=2", "https://a.example:8443/x/y/z?n=2"),
            ("/p?a=b#frag", "https://a.example:8443/p?a=b"),
        ] {
            let got = resolve_location(&base, location).unwrap();
            assert_eq!(got.to_string(), expected, "{location}");
        }
        for bad in [
            "",
            "   ",
            "#only",
            "ftp://b.example/",
            "://x",
            "javascript://x",
        ] {
            assert!(resolve_location(&base, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_same_place_means_same_scheme_host_and_port() {
        let u = |s: &str| -> Uri { s.parse().unwrap() };
        assert_eq!(
            origin(&u("https://A.example/")),
            origin(&u("https://a.example:443/x"))
        );
        assert_ne!(
            origin(&u("https://a.example/")),
            origin(&u("http://a.example/"))
        );
        assert_ne!(
            origin(&u("https://a.example/")),
            origin(&u("https://a.example:8443/"))
        );
        assert_ne!(
            origin(&u("https://a.example/")),
            origin(&u("https://b.example/"))
        );
    }

    #[tokio::test]
    async fn a_client_built_to_refuse_private_addresses_does_and_the_default_client_does_not() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _peer = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut conn, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = conn.read(&mut buf).await;
                let _ = conn
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await;
            }
        });
        // Control: the default client reaches loopback.
        let open = Client::new().get(format!("http://{addr}/")).send().await;
        assert_eq!(open.unwrap().text().await.unwrap(), "ok");
        let guarded = Client::builder()
            .refuse_private_addresses(true)
            .build()
            .unwrap();
        for url in [
            format!("http://{addr}/x?token=SECRETQ"),
            format!("http://localhost:{}/x?token=SECRETQ", addr.port()),
            "http://[::1]:1/x?token=SECRETQ".to_owned(),
            "http://[::ffff:169.254.169.254]/latest/meta-data?token=SECRETQ".to_owned(),
        ] {
            let err = guarded.get(&url).send().await.unwrap_err();
            assert!(matches!(err, Error::PrivateAddress { .. }), "{url}: {err}");
            for text in [err.to_string(), format!("{err:?}")] {
                assert!(!text.contains("SECRETQ"), "the error carried the query");
            }
        }
    }

    #[test]
    fn every_timeout_has_a_finite_default() {
        for d in [
            DEFAULT_CONNECT_TIMEOUT,
            DEFAULT_TIMEOUT,
            DEFAULT_READ_TIMEOUT,
        ] {
            assert!(d > Duration::ZERO && d <= Duration::from_secs(120));
        }
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
