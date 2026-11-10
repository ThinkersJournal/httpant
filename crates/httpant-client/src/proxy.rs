// SPDX-License-Identifier: MIT OR Apache-2.0
//! An explicit HTTP proxy for `https://` URLs, by CONNECT (design `docs/HTTPS-CLIENT-DESIGN.md`, need 9).
//!
//! [`Routed`] wraps the TCP connector. Without a proxy it is a pass-through. With one, an `https://` destination is reached by connecting to the
//! proxy and asking it for a CONNECT tunnel (hyper-util's `Tunnel`); TLS then runs inside the tunnel, so the proxy sees the host and port and
//! nothing else. A plain `http://` destination is refused, never sent around the proxy: a request the caller meant to go through the proxy must
//! not quietly leave by another route.
//!
//! The private-address guard sits inside the connector the tunnel dials, so it judges the **proxy's** address (the proxy does the target's
//! lookup); [`Settings`](crate::hyper_adapter::Settings) refuses "guard on" with "proxy on" unless the caller says that is what is meant.

#[cfg(feature = "https")]
use std::future::Future;
#[cfg(feature = "https")]
use std::pin::Pin;
#[cfg(feature = "https")]
use std::task::{Context, Poll};

use httpant_core::BoxError;
use httpant_core::http::{HeaderValue, Uri};
#[cfg(feature = "https")]
use hyper::rt::{Read, Write};
#[cfg(feature = "https")]
use hyper_util::client::legacy::connect::proxy::Tunnel;
#[cfg(feature = "https")]
use tower::Service;

/// A proxy failure. Its `Display` is fixed text (never the proxy's or the target's address, and never a credential); the cause stays in the
/// `source` chain so the adapter can still find a private-address refusal under it.
#[derive(Debug)]
pub(crate) struct ProxyError {
    pub(crate) message: String,
    source: Option<BoxError>,
}

impl ProxyError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|e| e as _)
    }
}

/// A parsed `http://[user[:password]@]host[:port]` proxy address.
// Without the `https` feature a proxy is only validated (then refused), so the parsed parts are read by nothing.
#[cfg_attr(not(feature = "https"), allow(dead_code))]
#[derive(Clone)]
pub(crate) struct ProxyTarget {
    pub(crate) uri: Uri,
    pub(crate) authorization: Option<HeaderValue>,
}

impl std::fmt::Debug for ProxyTarget {
    // The address and any credential stay out of `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyTarget")
            .field("credentials", &self.authorization.is_some())
            .finish_non_exhaustive()
    }
}

/// Read a proxy URL. Only `http://` proxies are supported (the leg to the proxy is clear; TLS to the proxy itself is a later option), with no path
/// or query. A user name and password in the URL become a `Proxy-Authorization: Basic` header on the CONNECT request, and are never printed.
pub(crate) fn parse(url: &str) -> Result<ProxyTarget, ProxyError> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(ProxyError::new(
            "the proxy address must start with http:// (a proxy reached over https is not supported yet)",
        ));
    };
    let (authority, tail) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    if !(tail.is_empty() || tail == "/") {
        return Err(ProxyError::new(
            "the proxy address must be only host and port, with no path, query or fragment",
        ));
    }
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let host_ok = !hostport.is_empty() && !hostport.starts_with(':');
    if !host_ok {
        return Err(ProxyError::new("the proxy address has no host"));
    }
    let uri: Uri = format!("http://{hostport}")
        .parse()
        .map_err(|_| ProxyError::new("the proxy address is not a valid host and port"))?;
    let authorization = match userinfo {
        None => None,
        Some(info) => {
            let (user, pass) = info.split_once(':').unwrap_or((info, ""));
            let user = percent_decode(user)?;
            let pass = percent_decode(pass)?;
            let mut value = HeaderValue::from_str(&format!(
                "Basic {}",
                base64(format!("{user}:{pass}").as_bytes())
            ))
            .map_err(|_| ProxyError::new("the proxy credentials cannot be sent as a header"))?;
            value.set_sensitive(true);
            Some(value)
        }
    };
    Ok(ProxyTarget { uri, authorization })
}

fn percent_decode(s: &str) -> Result<String, ProxyError> {
    let bad = || ProxyError::new("the proxy credentials hold a bad percent-escape");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or_else(bad)?;
            let hex = std::str::from_utf8(hex).map_err(|_| bad())?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| bad())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| bad())
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(feature = "https")]
/// The connector the client's TLS layer wraps: direct, or through a CONNECT tunnel.
#[derive(Clone)]
pub(crate) struct Routed<C> {
    direct: C,
    tunnel: Option<Tunnel<C>>,
}

#[cfg(feature = "https")]
impl<C: Clone> Routed<C> {
    pub(crate) fn new(connector: C, proxy: Option<ProxyTarget>) -> Self {
        let tunnel = proxy.map(|p| {
            let t = Tunnel::new(p.uri, connector.clone());
            match p.authorization {
                Some(auth) => t.with_auth(auth),
                None => t,
            }
        });
        Self {
            direct: connector,
            tunnel,
        }
    }
}

#[cfg(feature = "https")]
impl<C> Service<Uri> for Routed<C>
where
    C: Service<Uri> + Clone + Send + 'static,
    C::Response: Read + Write + Unpin + Send + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Send + 'static,
{
    type Response = C::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<C::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        match &mut self.tunnel {
            Some(t) => t.poll_ready(cx).map_err(|e| wrap(e)),
            None => self.direct.poll_ready(cx).map_err(Into::into),
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let Some(tunnel) = &mut self.tunnel else {
            let connecting = self.direct.call(dst);
            return Box::pin(async move { connecting.await.map_err(Into::into) });
        };
        if dst.scheme_str() != Some("https") {
            return Box::pin(async {
                Err(Box::new(ProxyError::new(
                    "a plain http:// URL cannot go through a proxy yet; use https://, or build the client without a proxy",
                )) as BoxError)
            });
        }
        let tunnelling = tunnel.call(dst);
        Box::pin(async move { tunnelling.await.map_err(wrap) })
    }
}

#[cfg(feature = "https")]
/// Keep the cause in the chain, with fixed text on top.
fn wrap<E: Into<BoxError>>(e: E) -> BoxError {
    let cause: BoxError = e.into();
    Box::new(ProxyError {
        message: format!("the proxy refused or could not open the tunnel ({cause})"),
        source: Some(cause),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
    }

    #[test]
    fn a_proxy_address_is_read_and_its_credentials_kept_out_of_debug() {
        let p = parse("http://user:p%40ss@proxy.example:3128").unwrap();
        assert_eq!(p.uri.to_string(), "http://proxy.example:3128/");
        let auth = p.authorization.as_ref().unwrap();
        assert!(auth.is_sensitive());
        assert_eq!(auth.to_str().unwrap(), "Basic dXNlcjpwQHNz");
        let shown = format!("{p:?}");
        assert!(
            !shown.contains("proxy.example") && !shown.contains("dXNl"),
            "{shown}"
        );
        assert!(
            parse("http://proxy.example:3128/")
                .unwrap()
                .authorization
                .is_none()
        );
    }

    #[test]
    fn a_proxy_address_that_cannot_be_honoured_is_an_error() {
        for bad in [
            "https://proxy.example:3128",
            "proxy.example:3128",
            "http://",
            "http://:3128",
            "http://proxy.example/path",
            "http://proxy.example?x=1",
            "http://u:%zz@proxy.example",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
