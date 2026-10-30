// SPDX-License-Identifier: MIT OR Apache-2.0
//! TLS termination over real sockets, on every adapter that declares `tls: Yes`. The client is tokio-rustls speaking raw HTTP/1.1, so nothing
//! of HTTPAnt's is on the client side.
#![cfg(feature = "tls")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use httpant_core::{BoxFuture, Capabilities, Request, Support};
use httpant_server::{
    CertSource, CertSourceError, FixedCert, RunningServer, Server, ServerAdapter, ServerCert,
    TlsAcceptor,
};
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
];

struct Made {
    cert: ServerCert,
    der: Vec<u8>,
    key_pem: String,
}

fn make_cert(name: &str) -> Made {
    let ck = rcgen::generate_simple_self_signed(vec![name.to_owned()]).unwrap();
    let key_pem = ck.signing_key.serialize_pem();
    let cert = ServerCert::from_pem(ck.cert.pem().as_bytes(), key_pem.as_bytes()).unwrap();
    Made {
        cert,
        der: ck.cert.der().to_vec(),
        key_pem,
    }
}

fn connector(trusted: &[&[u8]], tls12_only: bool) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    for der in trusted {
        roots.add(CertificateDer::from(der.to_vec())).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let versions: &[&rustls::SupportedProtocolVersion] = if tls12_only {
        &[&rustls::version::TLS12]
    } else {
        &[&rustls::version::TLS13, &rustls::version::TLS12]
    };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn start(adapter: &str, source: impl CertSource) -> RunningServer {
    Server::with_adapter(adapter)
        .unwrap()
        .tls(TlsAcceptor::new(source))
        .get("/origin", |req: Request| async move {
            match req.extensions().get::<httpant_server::Origin>() {
                Some(o) => format!("{:?}", o.scheme),
                None => "none".to_owned(),
            }
        })
        .bind("127.0.0.1:0")
        .await
        .unwrap()
}

/// GET `/origin` over TLS with SNI `name`; the reply text and the leaf certificate the server presented.
async fn fetch(
    server: &RunningServer,
    connector: &TlsConnector,
    name: &str,
    extra_headers: &str,
) -> std::io::Result<(String, Vec<u8>)> {
    let tcp = TcpStream::connect(server.addr()).await?;
    let mut tls = connector
        .connect(ServerName::try_from(name.to_owned()).unwrap(), tcp)
        .await?;
    let leaf = tls.get_ref().1.peer_certificates().unwrap()[0].to_vec();
    tls.write_all(
        format!("GET /origin HTTP/1.1\r\nHost: {name}\r\nConnection: close\r\n{extra_headers}\r\n")
            .as_bytes(),
    )
    .await?;
    let mut reply = Vec::new();
    let _ = tls.read_to_end(&mut reply).await;
    Ok((String::from_utf8_lossy(&reply).into_owned(), leaf))
}

#[tokio::test]
async fn every_tls_adapter_serves_https_and_says_so() {
    for id in ADAPTERS {
        let made = make_cert("localhost");
        let server = start(id, FixedCert::new(made.cert.clone())).await;
        // A header claiming http changes nothing: the scheme is a fact about this connection, no proxy is trusted.
        for tls12 in [false, true] {
            let (reply, leaf) = fetch(
                &server,
                &connector(&[&made.der], tls12),
                "localhost",
                "X-Forwarded-Proto: http\r\n",
            )
            .await
            .unwrap_or_else(|e| panic!("{id} tls12={tls12}: {e}"));
            assert!(reply.starts_with("HTTP/1.1 200"), "{id}: not a 200");
            assert!(reply.ends_with("Https"), "{id}: scheme not https");
            assert_eq!(leaf, made.der, "{id}: served another certificate");
        }
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_plaintext_client_gets_no_http_answer_from_a_tls_listener() {
    for id in ADAPTERS {
        let server = start(id, FixedCert::new(make_cert("localhost").cert)).await;
        let mut tcp = TcpStream::connect(server.addr()).await.unwrap();
        tcp.write_all(b"GET /origin HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut got = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut got)).await;
        assert!(
            !String::from_utf8_lossy(&got).contains("HTTP/1.1 200"),
            "{id}: served in clear"
        );
        server.shutdown().await.unwrap();
    }
}

/// Knows `localhost` only; counts its calls and can be told to fail.
struct Picky {
    known: Arc<ServerCert>,
    calls: AtomicUsize,
    fail: bool,
}

impl CertSource for Picky {
    fn cert_for<'a>(
        &'a self,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Option<Arc<ServerCert>>, CertSourceError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.fail {
                return Err(CertSourceError("store unreachable".into()));
            }
            Ok((name == Some("localhost")).then(|| Arc::clone(&self.known)))
        })
    }
}

#[tokio::test]
async fn an_unknown_name_or_a_failing_source_refuses_the_handshake() {
    for id in ADAPTERS {
        let made = make_cert("localhost");
        let picky = Picky {
            known: Arc::new(made.cert.clone()),
            calls: AtomicUsize::new(0),
            fail: false,
        };
        let server = start(id, picky).await;
        // The client trusts the only certificate there is, so only the server's refusal can stop it.
        let c = connector(&[&made.der], false);
        assert!(fetch(&server, &c, "localhost", "").await.is_ok(), "{id}");
        assert!(
            fetch(&server, &c, "other.test", "").await.is_err(),
            "{id}: served an unknown name"
        );
        server.shutdown().await.unwrap();

        let failing = Picky {
            known: Arc::new(made.cert.clone()),
            calls: AtomicUsize::new(0),
            fail: true,
        };
        let server = start(id, failing).await;
        assert!(
            fetch(&server, &c, "localhost", "").await.is_err(),
            "{id}: fell back after a source failure"
        );
        server.shutdown().await.unwrap();
    }
}

/// Serves whatever it was last given: a renewal is `swap`.
struct Swappable(std::sync::Mutex<Arc<ServerCert>>);

impl CertSource for Swappable {
    fn cert_for<'a>(
        &'a self,
        _name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Option<Arc<ServerCert>>, CertSourceError>> {
        Box::pin(async move { Ok(Some(Arc::clone(&self.0.lock().unwrap()))) })
    }
}

struct Shared(Arc<Swappable>);

impl CertSource for Shared {
    fn cert_for<'a>(
        &'a self,
        n: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Option<Arc<ServerCert>>, CertSourceError>> {
        self.0.cert_for(n)
    }
}

#[tokio::test]
async fn a_renewed_certificate_serves_the_next_connection_without_a_restart() {
    for id in ADAPTERS {
        let (old, new) = (make_cert("localhost"), make_cert("localhost"));
        let source = Arc::new(Swappable(std::sync::Mutex::new(Arc::new(old.cert.clone()))));
        let server = start(id, Shared(Arc::clone(&source))).await;
        let c = connector(&[&old.der, &new.der], false);
        assert_eq!(
            fetch(&server, &c, "localhost", "").await.unwrap().1,
            old.der,
            "{id}"
        );
        *source.0.lock().unwrap() = Arc::new(new.cert.clone());
        assert_eq!(
            fetch(&server, &c, "localhost", "").await.unwrap().1,
            new.der,
            "{id}: still the old one"
        );
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_client_that_says_nothing_is_dropped_at_the_handshake_timeout() {
    for id in ADAPTERS {
        let server = Server::with_adapter(id)
            .unwrap()
            .tls(
                TlsAcceptor::new(FixedCert::new(make_cert("localhost").cert))
                    .handshake_timeout(Duration::from_millis(200)),
            )
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let mut tcp = TcpStream::connect(server.addr()).await.unwrap();
        let mut buf = [0u8; 16];
        let n = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("{id}: silent client still held after 5 s"));
        assert!(matches!(n, Ok(0) | Err(_)), "{id}");
        server.shutdown().await.unwrap();
    }
}

#[test]
fn the_private_key_is_not_in_debug_output_or_errors() {
    let made = make_cert("localhost");
    let body: String = made
        .key_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let shown = format!("{:?} {:?}", made.cert, FixedCert::new(made.cert.clone()));
    assert!(!shown.contains(&body[..32]), "key bytes in Debug output");
    assert!(shown.contains("redacted"));
    // A mismatched pair is refused, and the error does not carry the key.
    let other = make_cert("other.test");
    let chain_of_other = {
        let ck = rcgen::generate_simple_self_signed(vec!["x.test".to_owned()]).unwrap();
        ck.cert.pem()
    };
    let err = ServerCert::from_pem(chain_of_other.as_bytes(), made.key_pem.as_bytes())
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not match"), "{err}");
    assert!(!err.contains(&body[..32]));
    drop(other);
}

struct NoTls;

impl ServerAdapter for NoTls {
    fn id(&self) -> &'static str {
        "no-tls"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn serve(
        &self,
        _svc: httpant_core::BoxService,
        _opts: httpant_server::ServeOptions,
    ) -> BoxFuture<'_, httpant_core::Result<RunningServer>> {
        panic!("must not be reached: TLS was asked for and the adapter does not do it")
    }
}

#[tokio::test]
async fn an_adapter_that_cannot_terminate_tls_is_refused_never_served_in_clear() {
    assert_ne!(NoTls.capabilities().tls, Support::Yes);
    let err = Server::with_adapter_impl(NoTls)
        .tls(TlsAcceptor::new(FixedCert::new(
            make_cert("localhost").cert,
        )))
        .bind("127.0.0.1:0")
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            httpant_core::Error::TlsNotSupported { adapter: "no-tls" }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn read_back_reports_the_certificate_the_listener_really_serves() {
    for id in ADAPTERS {
        let (old, new) = (make_cert("localhost"), make_cert("localhost"));
        let source = Arc::new(Swappable(std::sync::Mutex::new(Arc::new(old.cert.clone()))));
        let server = start(id, Shared(Arc::clone(&source))).await;
        let t = Duration::from_secs(5);
        let first = httpant_server::read_back(server.addr(), "localhost", t)
            .await
            .unwrap();
        assert_eq!(first.leaf_der, old.der, "{id}");
        assert_eq!(first.chain_len, 1, "{id}");
        // rcgen's self-signed certificate is its own issuer, and is valid now (not_after in the future).
        assert_eq!(first.issuer, first.subject, "{id}");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(first.not_before <= now && first.not_after > now, "{id}");
        assert_eq!(first.alpn.as_deref(), Some("http/1.1"), "{id}");
        assert!(
            first.protocol.contains("TLSv1_3") || first.protocol.contains("1.3"),
            "{id}: {}",
            first.protocol
        );
        // After a renewal the read-back shows the new certificate: it is read from the listener, not from the source's say-so.
        *source.0.lock().unwrap() = Arc::new(new.cert.clone());
        let second = httpant_server::read_back(server.addr(), "localhost", t)
            .await
            .unwrap();
        assert_eq!(second.leaf_der, new.der, "{id}");
        assert_ne!(second.leaf_der, first.leaf_der, "{id}");
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn read_back_of_a_refused_name_or_a_plaintext_port_is_an_error() {
    let made = make_cert("localhost");
    let picky = Picky {
        known: Arc::new(made.cert.clone()),
        calls: AtomicUsize::new(0),
        fail: false,
    };
    let server = start("hyper", picky).await;
    let t = Duration::from_secs(5);
    assert!(
        httpant_server::read_back(server.addr(), "localhost", t)
            .await
            .is_ok()
    );
    assert!(
        httpant_server::read_back(server.addr(), "other.test", t)
            .await
            .is_err()
    );
    server.shutdown().await.unwrap();

    let plain = Server::new()
        .get("/", || async { "hi" })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    assert!(
        httpant_server::read_back(plain.addr(), "localhost", t)
            .await
            .is_err()
    );
    plain.shutdown().await.unwrap();
}
