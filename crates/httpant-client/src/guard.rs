// SPDX-License-Identifier: MIT OR Apache-2.0
//! The private-address guard (design `docs/HTTPS-CLIENT-DESIGN.md`, decision 4), in three places because one is not enough:
//!
//! 1. [`GuardResolver`] filters what the name lookup returns, so the connector can only try addresses that passed (there is no second lookup
//!    for a DNS answer to change between, which is what rebinding needs).
//! 2. [`GuardConnector`] refuses an IP-literal host before connecting, because hyper-util's `HttpConnector` never sends a literal to the
//!    resolver (`SocketAddrs::try_parse` runs first).
//! 3. [`GuardConnector`] also looks at the address the connection really reached, before TLS and before any request byte is written.
//!
//! "Private" means: not a globally routable address (loopback, private networks, link-local including the cloud metadata address, carrier-grade
//! NAT, unique-local, multicast, documentation, reserved), judged on the IPv4 address inside IPv4-mapped, NAT64 and 6to4 IPv6 forms.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use httpant_core::BoxError;
use httpant_core::http::Uri;
use hyper_util::client::legacy::connect::dns::Name;
use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use tower::Service;

/// The error the guard puts into hyper's error chain; the adapter turns it into [`httpant_core::Error::PrivateAddress`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrivateAddress(pub(crate) IpAddr);

impl std::fmt::Display for PrivateAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "refused to connect to the private address {}", self.0)
    }
}

impl std::error::Error for PrivateAddress {}

/// Is `ip` an address a request to a public service should never be sent to?
pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => private_v4(a),
        IpAddr::V6(a) => private_v6(a),
    }
}

fn private_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xC0) == 64)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || (o[0] == 198 && (o[1] & 0xFE) == 18)
        || o[0] >= 240
}

fn private_v6(a: Ipv6Addr) -> bool {
    if let Some(v4) = a.to_ipv4_mapped() {
        return private_v4(v4);
    }
    let s = a.segments();
    let embedded =
        |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address: it is the one that is reached.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return private_v4(embedded(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return private_v4(embedded(s[1], s[2]));
    }
    a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x2001 && s[1] == 0)
        || (s[0] == 0x0100 && s[1..4] == [0; 3])
        || s[..6] == [0; 6]
}

/// The IP address written in the URL's host, if it is one (an IPv6 literal is bracketed in a URL).
fn literal_host(uri: &Uri) -> Option<IpAddr> {
    uri.host()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// Wraps a name resolver and, when `refuse` is set, drops the private addresses from its answer. An answer that is only private addresses is an
/// error, so the connector never falls back to a different lookup.
#[derive(Clone)]
pub(crate) struct GuardResolver<R> {
    inner: R,
    refuse: bool,
}

impl<R> GuardResolver<R> {
    pub(crate) fn new(inner: R, refuse: bool) -> Self {
        Self { inner, refuse }
    }
}

impl<R> Service<Name> for GuardResolver<R>
where
    R: Service<Name>,
    R::Response: Iterator<Item = SocketAddr> + Send + 'static,
    R::Error: Into<BoxError>,
    R::Future: Send + 'static,
{
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let lookup = self.inner.call(name);
        let refuse = self.refuse;
        Box::pin(async move {
            let found: Vec<SocketAddr> = lookup.await.map_err(Into::into)?.collect();
            if !refuse {
                return Ok(found.into_iter());
            }
            let (private, public): (Vec<_>, Vec<_>) =
                found.into_iter().partition(|a| is_private(a.ip()));
            match (public.is_empty(), private.first()) {
                (true, Some(first)) => Err(Box::new(PrivateAddress(first.ip())) as BoxError),
                _ => Ok(public.into_iter()),
            }
        })
    }
}

/// Wraps a connector. With `refuse` set it turns away an IP-literal private host before connecting, and a connection that reached a private
/// address after it (before TLS, before any byte of the request).
#[derive(Clone)]
pub(crate) struct GuardConnector<S> {
    inner: S,
    refuse: bool,
}

impl<S> GuardConnector<S> {
    pub(crate) fn new(inner: S, refuse: bool) -> Self {
        Self { inner, refuse }
    }
}

impl<S> Service<Uri> for GuardConnector<S>
where
    S: Service<Uri>,
    S::Response: Connection + Send + 'static,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let refuse = self.refuse;
        if refuse
            && let Some(ip) = literal_host(&dst)
            && is_private(ip)
        {
            return Box::pin(async move { Err(Box::new(PrivateAddress(ip)) as BoxError) });
        }
        let connecting = self.inner.call(dst);
        Box::pin(async move {
            let conn = connecting.await.map_err(Into::into)?;
            if refuse {
                let mut extras = httpant_core::http::Extensions::new();
                conn.connected().get_extras(&mut extras);
                match extras.get::<HttpInfo>() {
                    Some(info) if is_private(info.remote_addr().ip()) => {
                        return Err(Box::new(PrivateAddress(info.remote_addr().ip())) as BoxError);
                    }
                    Some(_) => {}
                    // Fail closed: if the address reached cannot be seen, it cannot be checked.
                    None => {
                        return Err("the address reached cannot be read, so it cannot be checked against the private-address guard".into());
                    }
                }
            }
            Ok(conn)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::client::legacy::connect::dns::GaiResolver;
    use tokio::net::TcpListener;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    #[test]
    fn the_classification_of_addresses() {
        for private in [
            "127.0.0.1",
            "127.9.9.9",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.9",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "100::1",
            // IPv4 inside IPv6: judged as the IPv4 address that is reached.
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:1::1",
            "2002:a9fe:a9fe::1",
            "::7f00:1",
        ] {
            assert!(is_private(ip(private)), "{private} must be private");
        }
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "192.0.1.1",
            "198.17.255.255",
            "198.20.0.1",
            "223.255.255.255",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(!is_private(ip(public)), "{public} must be public");
        }
    }

    #[test]
    fn a_host_is_read_as_a_literal_only_when_it_is_one() {
        let host = |u: &str| literal_host(&u.parse().unwrap());
        assert_eq!(host("http://127.0.0.1:80/"), Some(ip("127.0.0.1")));
        assert_eq!(host("https://[::1]:443/x"), Some(ip("::1")));
        assert_eq!(
            host("http://[::ffff:10.0.0.1]/"),
            Some(ip("::ffff:10.0.0.1"))
        );
        assert_eq!(host("http://localhost/"), None);
        assert_eq!(host("http://example.com:8080/"), None);
    }

    #[derive(Clone)]
    struct Fixed(Vec<SocketAddr>);

    impl Service<Name> for Fixed {
        type Response = std::vec::IntoIter<SocketAddr>;
        type Error = BoxError;
        type Future = std::future::Ready<Result<Self::Response, BoxError>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Name) -> Self::Future {
            std::future::ready(Ok(self.0.clone().into_iter()))
        }
    }

    fn name() -> Name {
        // `Name` has no public constructor; a lookup of the loopback name is the way to hold one: take it from a real URI's host.
        Name::from_str("example.invalid").unwrap()
    }

    fn sock(s: &str) -> SocketAddr {
        SocketAddr::new(ip(s), 80)
    }

    #[tokio::test]
    async fn the_resolver_keeps_only_public_answers_and_refuses_an_all_private_one() {
        // A name answering with both a public and a private address (the shape a rebinding answer takes): only the public one may be tried.
        let mixed = Fixed(vec![sock("10.0.0.1"), sock("93.184.216.34"), sock("::1")]);
        let kept: Vec<_> = GuardResolver::new(mixed.clone(), true)
            .call(name())
            .await
            .unwrap()
            .collect();
        assert_eq!(kept, vec![sock("93.184.216.34")]);

        let only_private = Fixed(vec![sock("127.0.0.1"), sock("192.168.0.9")]);
        let err = GuardResolver::new(only_private.clone(), true)
            .call(name())
            .await
            .expect_err("an all-private answer is refused");
        assert!(err.downcast_ref::<PrivateAddress>().is_some());

        // Off: the answer passes through untouched.
        let all: Vec<_> = GuardResolver::new(mixed, false)
            .call(name())
            .await
            .unwrap()
            .collect();
        assert_eq!(all.len(), 3);
    }

    async fn counting_listener() -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        tokio::spawn(async move {
            while let Ok((conn, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                std::mem::forget(conn);
            }
        });
        (addr, accepted)
    }

    #[tokio::test]
    async fn the_connector_refuses_a_literal_before_it_connects() {
        let (addr, accepted) = counting_listener().await;
        let mut guarded = GuardConnector::new(HttpConnector::new(), true);
        let err = guarded
            .call(format!("http://{addr}/").parse().unwrap())
            .await
            .expect_err("a private literal is refused");
        assert!(err.downcast_ref::<PrivateAddress>().is_some());
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(accepted.load(Ordering::SeqCst), 0, "a connection was made");
    }

    #[tokio::test]
    async fn the_connector_checks_the_address_it_really_reached_even_if_the_lookup_was_not_filtered()
     {
        // The resolver here is the plain one (no filtering), so `localhost` resolves to loopback and the connection is made: the connector's own
        // look at the address reached is the only thing that can refuse it, before TLS and before any request byte.
        let (addr, accepted) = counting_listener().await;
        let plain = HttpConnector::new_with_resolver(GaiResolver::new());
        let mut guarded = GuardConnector::new(plain, true);
        let err = guarded
            .call(
                format!("http://localhost:{}/", addr.port())
                    .parse()
                    .unwrap(),
            )
            .await
            .expect_err("a connection that reached loopback is refused");
        assert!(err.downcast_ref::<PrivateAddress>().is_some(), "{err}");
        // Positive control: with the guard off, the same connector connects to the same place.
        let mut open = GuardConnector::new(HttpConnector::new(), false);
        let before = accepted.load(Ordering::SeqCst);
        open.call(
            format!("http://localhost:{}/", addr.port())
                .parse()
                .unwrap(),
        )
        .await
        .expect("an unguarded connector connects");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(accepted.load(Ordering::SeqCst) > before);
    }
}
