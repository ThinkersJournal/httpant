// SPDX-License-Identifier: MIT OR Apache-2.0
//! The client's explicit proxy (ROADMAP row 14 slice 3b), measured against a CONNECT proxy and a TLS origin written here with tokio and rustls,
//! not with HTTPAnt's own server: the request must really travel inside the tunnel, with the proxy seeing only the CONNECT line.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

use httpant_client::Client;
use httpant_core::Error;
use rustls::pki_types::PrivateKeyDer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

struct Origin {
    addr: SocketAddr,
    cert_pem: String,
    requests: Arc<AtomicUsize>,
}

/// A TLS server with a self-signed certificate for 127.0.0.1 that answers `secure` to any request.
async fn origin() -> Origin {
    let made = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let key = PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![made.cert.der().clone()], key)
    .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let counter = Arc::clone(&counter);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                counter.fetch_add(1, SeqCst);
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });
    Origin {
        addr,
        cert_pem: made.cert.pem(),
        requests,
    }
}

struct Proxy {
    addr: SocketAddr,
    /// The CONNECT request heads it was sent, as text.
    seen: Arc<Mutex<Vec<String>>>,
}

/// A CONNECT proxy. With `refuse` it answers 407; otherwise it connects to the requested host and port and copies bytes both ways.
async fn proxy(refuse: bool) -> Proxy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match client.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                log.lock().unwrap().push(head.clone());
                if refuse {
                    let _ = client
                        .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    return;
                }
                let target = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let Ok(mut upstream) = tokio::net::TcpStream::connect(&target).await else {
                    let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
                    return;
                };
                let _ = client
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await;
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            });
        }
    });
    Proxy { addr, seen }
}

#[tokio::test]
async fn an_https_request_travels_through_the_tunnel_with_the_credentials_on_the_connect() {
    let origin = origin().await;
    let proxy = proxy(false).await;
    let client = Client::builder()
        .add_root_pem(origin.cert_pem.as_bytes())
        .proxy(format!("http://user:secret@{}", proxy.addr))
        .build()
        .unwrap();
    let text = client
        .get(format!("https://{}/", origin.addr))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "secure");
    assert_eq!(origin.requests.load(SeqCst), 1);
    let seen = proxy.seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "one CONNECT for one request");
    assert!(
        seen[0].starts_with(&format!("CONNECT {} HTTP/1.1", origin.addr)),
        "{}",
        seen[0]
    );
    let auth = seen[0]
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.eq_ignore_ascii_case("proxy-authorization"))
                .map(|(_, v)| v.trim().to_owned())
        })
        .expect("the CONNECT carried no Proxy-Authorization");
    assert_eq!(auth, "Basic dXNlcjpzZWNyZXQ=");
    assert!(
        !seen[0].contains("GET /"),
        "the proxy must see no request, only the CONNECT"
    );
}

#[tokio::test]
async fn without_a_proxy_the_same_request_does_not_touch_it() {
    let origin = origin().await;
    let proxy = proxy(false).await;
    let direct = Client::builder()
        .add_root_pem(origin.cert_pem.as_bytes())
        .build()
        .unwrap();
    let text = direct
        .get(format!("https://{}/", origin.addr))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "secure");
    assert!(proxy.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_proxy_that_refuses_the_tunnel_is_a_proxy_error_and_the_origin_is_never_reached() {
    let origin = origin().await;
    let proxy = proxy(true).await;
    let client = Client::builder()
        .add_root_pem(origin.cert_pem.as_bytes())
        .proxy(format!("http://{}", proxy.addr))
        .build()
        .unwrap();
    let err = client
        .get(format!("https://{}/", origin.addr))
        .send()
        .await
        .expect_err("a 407 on CONNECT is an error");
    assert!(matches!(err, Error::Proxy(_)), "{err}");
    assert_eq!(origin.requests.load(SeqCst), 0);
}

#[tokio::test]
async fn a_plain_http_url_is_refused_and_never_sent_around_the_proxy() {
    let proxy = proxy(false).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while listener.accept().await.is_ok() {
            counter.fetch_add(1, SeqCst);
        }
    });
    let client = Client::builder()
        .proxy(format!("http://{}", proxy.addr))
        .build()
        .unwrap();
    let err = client
        .get(format!("http://{target}/"))
        .send()
        .await
        .expect_err("http through a proxy is refused");
    assert!(matches!(err, Error::Proxy(_)), "{err}");
    assert!(proxy.seen.lock().unwrap().is_empty());
    assert_eq!(
        hits.load(SeqCst),
        0,
        "the target was reached around the proxy"
    );
}

#[tokio::test]
async fn the_private_address_guard_with_a_proxy_must_say_which_address_it_judges() {
    let proxy = proxy(false).await;
    let url = format!("http://{}", proxy.addr);

    // Both set, no choice made: refused when building, so nothing is quietly less protected than it looks.
    let Err(err) = Client::builder()
        .proxy(&url)
        .refuse_private_addresses(true)
        .build()
    else {
        panic!("guard plus proxy needs a choice");
    };
    assert!(matches!(err, Error::Proxy(_)), "{err}");

    // The choice made: the guard judges the proxy's address. This proxy is on loopback, so it is refused as a private address, before any byte.
    let client = Client::builder()
        .proxy(&url)
        .refuse_private_addresses(true)
        .guard_judges_proxy(true)
        .build()
        .unwrap();
    let err = client
        .get("https://example.invalid/")
        .send()
        .await
        .expect_err("a loopback proxy is a private address");
    assert!(matches!(err, Error::PrivateAddress { .. }), "{err}");
    assert!(proxy.seen.lock().unwrap().is_empty());
}

#[test]
fn a_proxy_address_that_cannot_be_honoured_is_refused_when_building() {
    for bad in [
        "https://proxy.example:3128",
        "proxy.example",
        "http://proxy.example/x",
    ] {
        let Err(err) = Client::builder().proxy(bad).build() else {
            panic!("an unusable proxy address");
        };
        assert!(matches!(err, Error::Proxy(_)), "{bad}: {err}");
        assert!(
            !err.to_string().contains("proxy.example"),
            "the address leaked: {err}"
        );
    }
}
