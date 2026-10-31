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

/// Every adapter that terminates TLS (declares `tls: Yes`); actix-web and warp refuse it, which `an_adapter_that_cannot_terminate_tls...` covers.
const ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
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
        // The probe offers `h2` and `http/1.1`; an adapter that serves HTTP/2 (with the `http2` feature) is read back as `h2`, salvo as `http/1.1`.
        let expected = if cfg!(feature = "http2") && *id != "salvo" {
            "h2"
        } else {
            "http/1.1"
        };
        assert_eq!(first.alpn.as_deref(), Some(expected), "{id}");
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

#[derive(Clone, Debug, Default)]
struct Events(Arc<std::sync::Mutex<Vec<httpant_server::TlsEvent>>>);

impl httpant_server::TlsAudit for Events {
    fn event(&self, event: &httpant_server::TlsEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

impl Events {
    fn take(&self) -> Vec<httpant_server::TlsEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// Wait (up to 5 s) until at least `n` events have arrived, then take them all: the client can see the alert before the server has
    /// recorded the refusal.
    async fn wait_take(&self, n: usize) -> Vec<httpant_server::TlsEvent> {
        for _ in 0..500 {
            if self.0.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.take()
    }
}

async fn start_audited(
    id: &str,
    source: impl CertSource,
    sink: Events,
    timeout: Option<Duration>,
) -> RunningServer {
    let mut acceptor = TlsAcceptor::new(source).audit(sink);
    if let Some(t) = timeout {
        acceptor = acceptor.handshake_timeout(t);
    }
    Server::with_adapter(id)
        .unwrap()
        .tls(acceptor)
        .get("/origin", || async { "ok" })
        .bind("127.0.0.1:0")
        .await
        .unwrap()
}

#[tokio::test]
async fn refused_handshakes_and_a_changed_certificate_are_reported_without_secrets() {
    use httpant_server::{RefusalReason, TlsEvent};
    for id in ADAPTERS {
        let (old, new) = (make_cert("localhost"), make_cert("localhost"));
        let source = Arc::new(Swappable(std::sync::Mutex::new(Arc::new(old.cert.clone()))));
        let events = Events::default();
        let server = start_audited(id, Shared(Arc::clone(&source)), events.clone(), None).await;
        let c = connector(&[&old.der, &new.der], false);

        // The first certificate for a name is not a "change"; serving it again is not one either.
        fetch(&server, &c, "localhost", "").await.unwrap();
        fetch(&server, &c, "localhost", "").await.unwrap();
        assert_eq!(events.take(), vec![], "{id}: events with nothing to report");

        *source.0.lock().unwrap() = Arc::new(new.cert.clone());
        fetch(&server, &c, "localhost", "").await.unwrap();
        fetch(&server, &c, "localhost", "").await.unwrap();
        let got = events.take();
        assert_eq!(got.len(), 1, "{id}: {got:?}");
        match &got[0] {
            TlsEvent::CertificateChanged {
                server_name,
                serial,
            } => {
                assert_eq!(server_name.as_deref(), Some("localhost"), "{id}");
                assert!(!serial.is_empty(), "{id}: no serial");
            }
            other => panic!("{id}: {other:?}"),
        }
        server.shutdown().await.unwrap();

        // Unknown name and failing source are refusals with their own reason, and name the name.
        let made = make_cert("localhost");
        let events = Events::default();
        let picky = Picky {
            known: Arc::new(made.cert.clone()),
            calls: AtomicUsize::new(0),
            fail: false,
        };
        let server = start_audited(id, picky, events.clone(), None).await;
        let c = connector(&[&made.der], false);
        assert!(fetch(&server, &c, "other.test", "").await.is_err());
        assert_eq!(
            events.wait_take(1).await,
            vec![TlsEvent::HandshakeRefused {
                server_name: Some("other.test".to_owned()),
                reason: RefusalReason::UnknownName,
            }],
            "{id}"
        );
        server.shutdown().await.unwrap();

        let events = Events::default();
        let failing = Picky {
            known: Arc::new(made.cert.clone()),
            calls: AtomicUsize::new(0),
            fail: true,
        };
        let server = start_audited(id, failing, events.clone(), None).await;
        assert!(fetch(&server, &c, "localhost", "").await.is_err());
        let got = events.wait_take(1).await;
        assert_eq!(
            got,
            vec![TlsEvent::HandshakeRefused {
                server_name: Some("localhost".to_owned()),
                reason: RefusalReason::SourceFailed,
            }],
            "{id}"
        );
        // The source's own message ("store unreachable") is not repeated in the event.
        assert!(!format!("{got:?}").contains("unreachable"), "{id}");
        // And no event holds key material.
        let key_body: String = made
            .key_pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        assert!(!format!("{got:?}").contains(&key_body[..32]), "{id}");
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_silent_client_and_a_plaintext_client_are_reported_as_refused() {
    use httpant_server::{RefusalReason, TlsEvent};
    for id in ADAPTERS {
        let events = Events::default();
        let server = start_audited(
            id,
            FixedCert::new(make_cert("localhost").cert),
            events.clone(),
            Some(Duration::from_millis(200)),
        )
        .await;
        let mut silent = TcpStream::connect(server.addr()).await.unwrap();
        let mut buf = [0u8; 8];
        let _ = tokio::time::timeout(Duration::from_secs(5), silent.read(&mut buf)).await;
        let got = events.wait_take(1).await;
        assert_eq!(
            got,
            vec![TlsEvent::HandshakeRefused {
                server_name: None,
                reason: RefusalReason::Timeout,
            }],
            "{id}"
        );

        let mut plain = TcpStream::connect(server.addr()).await.unwrap();
        plain
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(5), plain.read(&mut buf)).await;
        let got = events.wait_take(1).await;
        assert_eq!(got.len(), 1, "{id}: {got:?}");
        assert!(
            matches!(
                got[0],
                TlsEvent::HandshakeRefused {
                    reason: RefusalReason::Protocol,
                    ..
                }
            ),
            "{id}: {got:?}"
        );
        server.shutdown().await.unwrap();
    }
}

/// Normal certificate for every name; a challenge certificate for `localhost` while `pending`. Counts how often the challenge is asked.
struct AcmeSource {
    normal: Arc<ServerCert>,
    challenge: Arc<ServerCert>,
    pending: bool,
    asked: Arc<AtomicUsize>,
}

impl CertSource for AcmeSource {
    fn cert_for<'a>(
        &'a self,
        _name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Option<Arc<ServerCert>>, CertSourceError>> {
        Box::pin(async move { Ok(Some(Arc::clone(&self.normal))) })
    }

    fn acme_tls_cert<'a>(
        &'a self,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Option<Arc<ServerCert>>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            (self.pending && name == Some("localhost")).then(|| Arc::clone(&self.challenge))
        })
    }
}

/// A client that offers exactly `alpn` and trusts `roots`; the leaf and the negotiated protocol it saw, then whatever the server sends.
async fn handshake_with_alpn(
    server: &RunningServer,
    roots: &[&[u8]],
    alpn: &[&[u8]],
) -> std::io::Result<(Vec<u8>, Option<Vec<u8>>, Vec<u8>)> {
    let mut store = RootCertStore::empty();
    for der in roots {
        store.add(CertificateDer::from(der.to_vec())).unwrap();
    }
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(store)
    .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    let tcp = TcpStream::connect(server.addr()).await?;
    let mut tls = TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await?;
    let (_, conn) = tls.get_ref();
    let leaf = conn.peer_certificates().unwrap()[0].to_vec();
    let negotiated = conn.alpn_protocol().map(<[u8]>::to_vec);
    // The server either closes (a validation connection) or waits for a request; give it none and see which.
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(500), tls.read_to_end(&mut rest)).await;
    Ok((leaf, negotiated, rest))
}

#[tokio::test]
async fn acme_tls_alpn_is_answered_with_the_challenge_certificate_and_closed() {
    use httpant_server::{RefusalReason, TlsEvent};
    for id in ADAPTERS {
        let (normal, challenge) = (make_cert("localhost"), make_cert("localhost"));
        let asked = Arc::new(AtomicUsize::new(0));
        let events = Events::default();
        let server = start_audited(
            id,
            AcmeSource {
                normal: Arc::new(normal.cert.clone()),
                challenge: Arc::new(challenge.cert.clone()),
                pending: true,
                asked: Arc::clone(&asked),
            },
            events.clone(),
            None,
        )
        .await;

        // A validator offers only `acme-tls/1`: it gets the challenge certificate, that protocol, and then the connection is closed.
        let (leaf, negotiated, rest) =
            handshake_with_alpn(&server, &[&challenge.der], &[b"acme-tls/1"])
                .await
                .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(leaf, challenge.der, "{id}: not the challenge certificate");
        assert_eq!(negotiated.as_deref(), Some(&b"acme-tls/1"[..]), "{id}");
        assert!(
            rest.is_empty(),
            "{id}: the validation connection was served as HTTP"
        );
        assert_eq!(
            events.wait_take(1).await,
            vec![TlsEvent::AcmeChallengeServed {
                server_name: Some("localhost".to_owned())
            }],
            "{id}: a validation is not a refusal"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 1, "{id}");

        // An ordinary client is unaffected: the normal certificate, HTTP, and the source is not asked about challenges.
        let before = asked.load(Ordering::SeqCst);
        let c = connector(&[&normal.der], false);
        let (reply, leaf) = fetch(&server, &c, "localhost", "").await.unwrap();
        assert_eq!(
            leaf, normal.der,
            "{id}: an ordinary client got the challenge certificate"
        );
        assert!(reply.starts_with("HTTP/1.1 200"), "{id}: not a 200");
        assert_eq!(
            asked.load(Ordering::SeqCst),
            before,
            "{id}: asked about a challenge without acme-tls/1"
        );
        server.shutdown().await.unwrap();

        // Nothing pending: the same validator is refused, not given the normal certificate under a protocol it did not agree on.
        let events = Events::default();
        let server = start_audited(
            id,
            AcmeSource {
                normal: Arc::new(normal.cert.clone()),
                challenge: Arc::new(challenge.cert.clone()),
                pending: false,
                asked: Arc::new(AtomicUsize::new(0)),
            },
            events.clone(),
            None,
        )
        .await;
        assert!(
            handshake_with_alpn(&server, &[&normal.der, &challenge.der], &[b"acme-tls/1"])
                .await
                .is_err(),
            "{id}: served a validator with nothing pending"
        );
        let got = events.wait_take(1).await;
        assert!(
            matches!(
                got[..],
                [TlsEvent::HandshakeRefused {
                    reason: RefusalReason::Protocol,
                    ..
                }]
            ),
            "{id}: {got:?}"
        );
        server.shutdown().await.unwrap();
    }
}
