// SPDX-License-Identifier: MIT OR Apache-2.0
//! Redirect cleartext requests to HTTPS (WebHostAnt need 18: the port-80 listener). It is a wrapper like the others, so it works on every
//! adapter. Two things keep it from being an open redirect or a loop: the `Host` must be one the caller listed (anything else is `421`, so a
//! forged `Host` cannot make this server bounce a victim to a host of the attacker's choosing), and a request that already arrived over
//! HTTPS (this server's own TLS, or a trusted proxy that says so) passes through untouched.

use std::convert::Infallible;
use std::sync::Arc;

use httpant_core::http::{HeaderValue, StatusCode, header};
use httpant_core::{BoxFuture, BoxService, IntoResponse, Request, Response};
use tower::ServiceExt;

use crate::{Origin, Scheme};

/// Which hosts to redirect to HTTPS, and on which port. Register it with [`Server::redirect_to_https`](crate::Server::redirect_to_https).
///
/// A matching request gets `308 Permanent Redirect` (the method and body are kept) to `https://<host><path and query>`, with `:<port>`
/// only when the port is not 443. The ACME challenge prefix is answered before this (see [`ChallengeResponder`](crate::ChallengeResponder)),
/// so HTTP-01 keeps working on the port that redirects everything else.
#[derive(Clone, Debug)]
pub struct HttpsRedirect {
    hosts: Arc<[String]>,
    port: u16,
}

impl HttpsRedirect {
    /// Redirect requests for these hosts (names or IP literals, compared without case and without a port) to HTTPS on port 443. A request
    /// for any other host is answered `421 Misdirected Request`. An empty list therefore refuses everything.
    pub fn new(hosts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let hosts = hosts
            .into_iter()
            .filter_map(|h| host_name(h.as_ref()))
            .collect::<Vec<_>>();
        Self {
            hosts: hosts.into(),
            port: 443,
        }
    }

    /// Redirect to this HTTPS port instead of 443.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// The `Location` for a request to `host` with this target, if `host` is one of the listed hosts.
    fn location(&self, host: &str, target: &str) -> Option<String> {
        let name = host_name(host)?;
        self.hosts.contains(&name).then(|| {
            if self.port == 443 {
                format!("https://{name}{target}")
            } else {
                format!("https://{name}:{}{target}", self.port)
            }
        })
    }
}

/// The host of a `Host` header value: lower-cased, without a port or a trailing dot, IPv6 literals keeping their brackets. `None` for text
/// that is not a plausible host (empty, or anything beyond letters, digits, `-`, `.`, and a bracketed IPv6 literal).
fn host_name(value: &str) -> Option<String> {
    let value = value.trim();
    let host = if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let (inner, after) = (&rest[..end], &rest[end + 1..]);
        if !after.is_empty() && !after.strip_prefix(':').is_some_and(all_digits) {
            return None;
        }
        if inner.is_empty()
            || !inner
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        {
            return None;
        }
        format!("[{}]", inner.to_ascii_lowercase())
    } else {
        let name = match value.rsplit_once(':') {
            Some((name, port)) if all_digits(port) => name,
            Some(_) => return None,
            None => value,
        };
        let name = name.strip_suffix('.').unwrap_or(name);
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        {
            return None;
        }
        name.to_ascii_lowercase()
    };
    Some(host)
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Wrap `svc` so cleartext requests for the listed hosts are redirected. Must sit inside the [`Origin`] wrapper: it reads the scheme from it.
pub(crate) fn wrap(redirect: Option<HttpsRedirect>, svc: BoxService) -> BoxService {
    let Some(redirect) = redirect else {
        return svc;
    };
    BoxService::new(tower::service_fn(move |req: Request| {
        let (svc, redirect) = (svc.clone(), redirect.clone());
        let step: BoxFuture<'static, Response> = Box::pin(async move {
            let already_https = req
                .extensions()
                .get::<Origin>()
                .is_some_and(|o| o.scheme == Scheme::Https);
            if already_https {
                return svc
                    .oneshot(req)
                    .await
                    .unwrap_or_else(|e: Infallible| match e {});
            }
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .or_else(|| req.uri().authority().map(|a| a.as_str()));
            let Some(host) = host else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let target = req.uri().path_and_query().map_or("/", |p| p.as_str());
            match redirect.location(host, target) {
                Some(location) => match HeaderValue::from_str(&location) {
                    Ok(location) => {
                        let mut res = StatusCode::PERMANENT_REDIRECT.into_response();
                        res.headers_mut().insert(header::LOCATION, location);
                        res
                    }
                    Err(_) => StatusCode::BAD_REQUEST.into_response(),
                },
                None => StatusCode::MISDIRECTED_REQUEST.into_response(),
            }
        });
        async move { Ok::<_, Infallible>(step.await) }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_are_normalised_and_junk_is_refused() {
        assert_eq!(host_name("Example.COM").as_deref(), Some("example.com"));
        assert_eq!(
            host_name("example.com:8080").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_name("example.com.").as_deref(), Some("example.com"));
        assert_eq!(host_name("[::1]:80").as_deref(), Some("[::1]"));
        assert_eq!(host_name("[2001:DB8::1]").as_deref(), Some("[2001:db8::1]"));
        assert_eq!(host_name("127.0.0.1:80").as_deref(), Some("127.0.0.1"));
        for bad in [
            "",
            ":",
            "a b",
            "a/b",
            "a@b",
            "a.com:x",
            "a.com:123456",
            "[::1",
            "[]",
            "[zz]:1",
            "a.com\r\nX: y",
            "a_b.com",
            "evil.com#@good.com",
            "a.com:80:80",
        ] {
            assert_eq!(host_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_listed_hosts_get_a_location_and_the_port_is_shown_only_when_not_443() {
        let r = HttpsRedirect::new(["example.com", "WWW.Example.com", "[::1]"]);
        assert_eq!(
            r.location("EXAMPLE.com:80", "/a?b=1").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(
            r.location("www.example.com", "/").as_deref(),
            Some("https://www.example.com/")
        );
        assert_eq!(
            r.location("[::1]:80", "/x").as_deref(),
            Some("https://[::1]/x")
        );
        assert_eq!(r.location("evil.com", "/"), None);
        assert_eq!(r.location("example.com.evil.com", "/"), None);
        assert_eq!(r.location("sub.example.com", "/"), None);
        let r = HttpsRedirect::new(["example.com"]).port(8443);
        assert_eq!(
            r.location("example.com", "//evil.com/x").as_deref(),
            Some("https://example.com:8443//evil.com/x")
        );
        assert_eq!(
            HttpsRedirect::new(Vec::<String>::new()).location("example.com", "/"),
            None
        );
    }
}
