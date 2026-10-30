// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use httpant_core::http::HeaderMap;
use httpant_core::{BoxService, Request};
use tower::ServiceExt;

/// The address of the peer that opened the connection a request arrived on. Every adapter puts one in the request's extensions before the
/// service runs (conformance case 16); [`Origin`] is what handlers should read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerAddr(pub SocketAddr);

/// The scheme the client used. Behind an edge that terminates TLS this is what the edge says, and only when the edge is a trusted proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// Plain HTTP.
    Http,
    /// HTTPS.
    Https,
}

/// Where a request really came from, placed in the request's extensions by [`ServeOptions::wrap`](crate::ServeOptions::wrap).
///
/// When the peer is one of the configured [trusted proxies](TrustedProxies), the client address is taken from `X-Forwarded-For` (the
/// rightmost entry that is not itself a trusted proxy) and the scheme from `X-Forwarded-Proto`. From any other peer those headers are ignored:
/// `client` is the peer and `scheme` is [`Scheme::Http`] (HTTPAnt serves plain HTTP until it terminates TLS itself). The headers are left on
/// the request untouched; read this, not them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    /// The client's address.
    pub client: IpAddr,
    /// The scheme the client used.
    pub scheme: Scheme,
    /// Whether the connection came from a trusted proxy (so `client` and `scheme` are what it forwarded, if it forwarded any).
    pub via_trusted_proxy: bool,
}

/// Peers allowed to say, with `X-Forwarded-For` and `X-Forwarded-Proto`, who the client is. Empty by default: nobody is trusted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    nets: Vec<(IpAddr, u8)>,
}

/// A trusted-proxy entry that is not an address or `address/prefix`.
#[derive(Debug, PartialEq, Eq)]
pub struct BadProxy(pub String);

impl std::fmt::Display for BadProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is not an IP address or `address/prefix` (for example `10.0.0.0/8` or `::1`)",
            self.0
        )
    }
}

impl std::error::Error for BadProxy {}

impl TrustedProxies {
    /// Nobody is trusted.
    pub fn none() -> Self {
        Self::default()
    }

    /// Trust one address.
    pub fn address(mut self, ip: IpAddr) -> Self {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        self.nets.push((ip, bits));
        self
    }

    /// Trust a network: every address that shares the first `prefix` bits of `ip`.
    pub fn network(mut self, ip: IpAddr, prefix: u8) -> Result<Self, BadProxy> {
        let max = if ip.is_ipv4() { 32 } else { 128 };
        if prefix > max {
            return Err(BadProxy(format!("{ip}/{prefix}")));
        }
        self.nets.push((ip, prefix));
        Ok(self)
    }

    /// Trust an address (`203.0.113.7`, `::1`) or a network (`10.0.0.0/8`) written as text.
    pub fn parse(self, text: &str) -> Result<Self, BadProxy> {
        let bad = || BadProxy(text.to_string());
        match text.split_once('/') {
            Some((ip, prefix)) => {
                let ip = IpAddr::from_str(ip.trim()).map_err(|_| bad())?;
                let prefix = prefix.trim().parse::<u8>().map_err(|_| bad())?;
                self.network(ip, prefix)
            }
            None => Ok(self.address(IpAddr::from_str(text.trim()).map_err(|_| bad())?)),
        }
    }

    /// Whether `ip` is trusted.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = unmap(ip);
        self.nets
            .iter()
            .any(|&(net, prefix)| in_network(unmap(net), prefix, ip))
    }
}

/// `::ffff:1.2.3.4` is `1.2.3.4`: a dual-stack listener reports IPv4 peers that way.
fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

fn in_network(net: IpAddr, prefix: u8, ip: IpAddr) -> bool {
    // Both are unmapped, so a v4 net can only match a v4 address. The prefix of a mapped v6 net was written against 128 bits, which is
    // not what `unmap` leaves: a mapped net is only ever written as a plain v4 address here, so no shift is needed.
    let (a, b, bits) = match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(i)) => {
            (u128::from(u32::from(n)), u128::from(u32::from(i)), 32u32)
        }
        (IpAddr::V6(n), IpAddr::V6(i)) => (u128::from(n), u128::from(i), 128u32),
        _ => return false,
    };
    let prefix = u32::from(prefix).min(bits);
    if prefix == 0 {
        return true;
    }
    let shift = bits - prefix;
    (a >> shift) == (b >> shift)
}

fn forwarded_for(headers: &HeaderMap) -> Vec<IpAddr> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|hop| IpAddr::from_str(hop.trim()).ok().map(unmap))
        .collect()
}

fn forwarded_proto(headers: &HeaderMap) -> Option<Scheme> {
    // The last value is the one the nearest trusted proxy appended.
    let last = headers
        .get_all("x-forwarded-proto")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .next_back()?;
    if last.eq_ignore_ascii_case("https") {
        Some(Scheme::Https)
    } else if last.eq_ignore_ascii_case("http") {
        Some(Scheme::Http)
    } else {
        None
    }
}

/// Work out the [`Origin`] of a request from its peer and headers. `None` when the adapter gave no peer.
pub(crate) fn origin_of(
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
    trusted: &TrustedProxies,
) -> Option<Origin> {
    let peer_ip = unmap(peer?.ip());
    if !trusted.contains(peer_ip) {
        return Some(Origin {
            client: peer_ip,
            scheme: Scheme::Http,
            via_trusted_proxy: false,
        });
    }
    // Walk the chain from the nearest hop outwards, skipping hops that are themselves trusted: the first one that is not is the client.
    // Entries a client invented sit to the left of the first trusted proxy's own entry, so they are never reached.
    let client = forwarded_for(headers)
        .into_iter()
        .rev()
        .find(|ip| !trusted.contains(*ip))
        .unwrap_or(peer_ip);
    Some(Origin {
        client,
        scheme: forwarded_proto(headers).unwrap_or(Scheme::Http),
        via_trusted_proxy: true,
    })
}

/// Wrap `svc` so that every request carries its [`Origin`] in its extensions.
pub(crate) fn wrap(trusted: TrustedProxies, svc: BoxService) -> BoxService {
    BoxService::new(tower::service_fn(move |mut req: Request| {
        let svc = svc.clone();
        let trusted = trusted.clone();
        async move {
            let peer = req.extensions().get::<PeerAddr>().map(|p| p.0);
            if let Some(origin) = origin_of(peer, req.headers(), &trusted) {
                req.extensions_mut().insert(origin);
            }
            svc.oneshot(req).await.map_err(|e: Infallible| match e {})
        }
    }))
}

#[cfg(test)]
mod tests {
    use httpant_core::http::HeaderValue;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn peer(s: &str) -> Option<SocketAddr> {
        Some(SocketAddr::new(ip(s), 5555))
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn proxies() -> TrustedProxies {
        TrustedProxies::none()
            .parse("10.0.0.0/8")
            .unwrap()
            .parse("::1")
            .unwrap()
    }

    #[test]
    fn an_untrusted_peer_cannot_set_the_client_or_the_scheme() {
        let h = headers(&[
            ("x-forwarded-for", "1.2.3.4"),
            ("x-forwarded-proto", "https"),
        ]);
        let o = origin_of(peer("198.51.100.9"), &h, &proxies()).unwrap();
        assert_eq!(o.client, ip("198.51.100.9"));
        assert_eq!(o.scheme, Scheme::Http);
        assert!(!o.via_trusted_proxy);
    }

    #[test]
    fn nobody_is_trusted_by_default() {
        let h = headers(&[("x-forwarded-for", "1.2.3.4")]);
        let o = origin_of(peer("10.1.1.1"), &h, &TrustedProxies::none()).unwrap();
        assert_eq!(o.client, ip("10.1.1.1"));
    }

    #[test]
    fn a_trusted_peer_sets_the_client_and_the_scheme() {
        let h = headers(&[
            ("x-forwarded-for", "203.0.113.9"),
            ("x-forwarded-proto", "https"),
        ]);
        let o = origin_of(peer("10.2.3.4"), &h, &proxies()).unwrap();
        assert_eq!(o.client, ip("203.0.113.9"));
        assert_eq!(o.scheme, Scheme::Https);
        assert!(o.via_trusted_proxy);
    }

    #[test]
    fn entries_the_client_invented_are_never_reached() {
        // The client sent `6.6.6.6`; the proxy at 10.0.0.5 appended the client's real address.
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 203.0.113.9, 10.0.0.7")]);
        let o = origin_of(peer("10.0.0.5"), &h, &proxies()).unwrap();
        assert_eq!(o.client, ip("203.0.113.9"));
    }

    #[test]
    fn a_trusted_peer_that_forwards_nothing_is_its_own_client() {
        let o = origin_of(peer("10.0.0.5"), &HeaderMap::new(), &proxies()).unwrap();
        assert_eq!(o.client, ip("10.0.0.5"));
        assert_eq!(o.scheme, Scheme::Http);
        assert!(o.via_trusted_proxy);
    }

    #[test]
    fn a_garbled_forwarded_header_is_ignored_not_trusted() {
        let h = headers(&[
            ("x-forwarded-for", "not-an-ip"),
            ("x-forwarded-proto", "gopher"),
        ]);
        let o = origin_of(peer("10.0.0.5"), &h, &proxies()).unwrap();
        assert_eq!(o.client, ip("10.0.0.5"));
        assert_eq!(o.scheme, Scheme::Http);
    }

    #[test]
    fn an_ipv4_peer_on_a_dual_stack_socket_matches_an_ipv4_network() {
        assert!(proxies().contains(ip("::ffff:10.9.9.9")));
        assert!(!proxies().contains(ip("::ffff:11.0.0.1")));
    }

    #[test]
    fn networks_match_on_their_prefix_and_bad_text_is_refused() {
        let t = TrustedProxies::none().parse("192.168.1.0/24").unwrap();
        assert!(t.contains(ip("192.168.1.200")));
        assert!(!t.contains(ip("192.168.2.1")));
        assert!(TrustedProxies::none().parse("192.168.1.0/33").is_err());
        assert!(TrustedProxies::none().parse("banana").is_err());
        assert!(TrustedProxies::none().parse("1.2.3.4/x").is_err());
    }

    #[test]
    fn no_peer_gives_no_origin() {
        assert_eq!(origin_of(None, &HeaderMap::new(), &proxies()), None);
    }
}
