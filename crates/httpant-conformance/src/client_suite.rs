// SPDX-License-Identifier: MIT OR Apache-2.0
//! Client conformance (design section 6): the same idea as the server cases, mirrored. Each case runs a [`Client`] against a **raw TCP
//! peer written in this file**, never against HTTPAnt's own server, so a defect cannot hide on both ends. The peer sees the exact bytes the
//! client wrote and answers with exact bytes, which is how a folded method, a collapsed header or a lossy body shows.
//!
//! Timeouts are not a case: they sit in `Client`, above the adapter, so there is no adapter defect to mutate (`httpant-client` tests cover
//! them). Redirects are measured as a capability, not required: an adapter either follows or returns the `3xx` as it is.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use httpant_client::{Client, ClientAdapter, HyperClientAdapter};
use httpant_core::http::header::{HeaderName, HeaderValue};
use httpant_core::http::{HeaderMap, Method};
use httpant_core::{
    Body, BoxFuture, ClientCapabilities, Error, Request, Response, Result as CoreResult, Support,
};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsAcceptor;

type Outcome = Result<(), String>;

/// A broken client fails the "arrives promptly" checks after this.
const PROMPT: Duration = Duration::from_secs(5);

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

/// One way a client adapter can be wrong; each reproduces a defect the survey measured in the old crate or a buffering defect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientMutant {
    /// Unknown request methods are sent as GET.
    FoldMethodToGet,
    /// Headers collapse (repeats keep the last, non-UTF8 values vanish), in both directions.
    CollapseHeaders,
    /// Request and response bodies pass through `String::from_utf8_lossy`.
    LossyBodies,
    /// Statuses outside a short list are reported as 200.
    ClampStatus,
    /// The whole response body is collected before the caller gets the response.
    BufferResponseBody,
    /// The whole request body is collected before anything is sent.
    BufferRequestBody,
    /// Every request says `Connection: close`, so no connection is reused.
    NoConnectionReuse,
    /// https: any certificate is accepted, trusted or not.
    AcceptAnyCertificate,
    /// https: the chain is checked against the roots, but the host name is not compared with the certificate.
    IgnoreHostName,
}

impl ClientMutant {
    /// Stable short id.
    pub fn id(self) -> &'static str {
        match self {
            ClientMutant::FoldMethodToGet => "client-fold-method-to-get",
            ClientMutant::CollapseHeaders => "client-collapse-headers",
            ClientMutant::LossyBodies => "client-lossy-bodies",
            ClientMutant::ClampStatus => "client-clamp-status",
            ClientMutant::BufferResponseBody => "client-buffer-response-body",
            ClientMutant::BufferRequestBody => "client-buffer-request-body",
            ClientMutant::NoConnectionReuse => "client-no-connection-reuse",
            ClientMutant::AcceptAnyCertificate => "client-accept-any-certificate",
            ClientMutant::IgnoreHostName => "client-ignore-host-name",
        }
    }
}

fn collapse(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if value.to_str().is_ok() {
            out.insert(name.clone(), value.clone());
        }
    }
    out
}

async fn collected(body: Body) -> Bytes {
    body.bytes(64 * 1024 * 1024).await.unwrap_or_default()
}

/// The reference adapter with one defect added.
struct MutantClientAdapter {
    inner: HyperClientAdapter,
    mutant: ClientMutant,
}

impl ClientAdapter for MutantClientAdapter {
    fn id(&self) -> &'static str {
        "hyper-mutant"
    }

    fn capabilities(&self) -> ClientCapabilities {
        self.inner.capabilities()
    }

    fn send(&self, mut req: Request) -> BoxFuture<'_, CoreResult<Response>> {
        Box::pin(async move {
            let standard = [
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::HEAD,
                Method::OPTIONS,
                Method::PATCH,
            ];
            match self.mutant {
                ClientMutant::FoldMethodToGet if !standard.contains(req.method()) => {
                    *req.method_mut() = Method::GET;
                }
                ClientMutant::CollapseHeaders => {
                    *req.headers_mut() = collapse(req.headers());
                }
                ClientMutant::LossyBodies => {
                    let (parts, body) = req.into_parts();
                    let lossy = String::from_utf8_lossy(&collected(body).await).into_owned();
                    req = Request::from_parts(parts, Body::from_bytes(lossy));
                }
                ClientMutant::BufferRequestBody => {
                    let (parts, body) = req.into_parts();
                    req = Request::from_parts(parts, Body::from_bytes(collected(body).await));
                }
                ClientMutant::NoConnectionReuse => {
                    req.headers_mut()
                        .insert("connection", HeaderValue::from_static("close"));
                }
                _ => {}
            }
            let mut res = self.inner.send(req).await?;
            match self.mutant {
                ClientMutant::ClampStatus
                    if !matches!(res.status().as_u16(), 200 | 201 | 400 | 404 | 500) =>
                {
                    *res.status_mut() = httpant_core::http::StatusCode::OK;
                }
                ClientMutant::CollapseHeaders => {
                    *res.headers_mut() = collapse(res.headers());
                }
                ClientMutant::LossyBodies => {
                    let (parts, body) = res.into_parts();
                    let lossy = String::from_utf8_lossy(&collected(body).await).into_owned();
                    res = Response::from_parts(parts, Body::from_bytes(lossy));
                }
                ClientMutant::BufferResponseBody => {
                    let (parts, body) = res.into_parts();
                    res = Response::from_parts(parts, Body::from_bytes(collected(body).await));
                }
                _ => {}
            }
            Ok(res)
        })
    }
}

/// Something the suite can build a [`Client`] from: an adapter (to be passed) or a known-bad variant of one (to be failed).
pub trait ClientSubject: Send + Sync {
    /// A name for failure messages.
    fn name(&self) -> String;

    /// A client on this subject.
    fn client(&self) -> Client;

    /// What the subject says it can do, if it is a real adapter.
    fn declared(&self) -> Option<ClientCapabilities> {
        None
    }

    /// A client on this subject that also trusts the CA certificate(s) in `root_pem`, for the https case. `None` if the subject cannot be told
    /// to trust a root (then https cannot be measured on it, and the case fails rather than skips).
    fn client_trusting(&self, _root_pem: &[u8]) -> Option<Client> {
        None
    }
}

/// The reference client adapter as a subject: it can be told to trust a root.
pub struct HyperClientSubject;

impl ClientSubject for HyperClientSubject {
    fn name(&self) -> String {
        "hyper".to_owned()
    }

    fn client(&self) -> Client {
        Client::new()
    }

    fn declared(&self) -> Option<ClientCapabilities> {
        Some(HyperClientAdapter::new().capabilities())
    }

    fn client_trusting(&self, root_pem: &[u8]) -> Option<Client> {
        Client::builder().add_root_pem(root_pem).build().ok()
    }
}

/// A real client adapter, used exactly as `Client` uses it.
pub struct ClientAdapterSubject<A>(pub A);

impl<A: ClientAdapter + Clone> ClientSubject for ClientAdapterSubject<A> {
    fn name(&self) -> String {
        self.0.id().to_owned()
    }

    fn client(&self) -> Client {
        Client::with_adapter_impl(self.0.clone())
    }

    fn declared(&self) -> Option<ClientCapabilities> {
        Some(self.0.capabilities())
    }
}

/// A deliberately broken variant of the reference client adapter.
pub struct ClientMutantSubject(pub ClientMutant);

impl ClientSubject for ClientMutantSubject {
    fn name(&self) -> String {
        format!("mutant `{}`", self.0.id())
    }

    fn client(&self) -> Client {
        Client::with_adapter_impl(MutantClientAdapter {
            inner: HyperClientAdapter::new(),
            mutant: self.0,
        })
    }

    fn client_trusting(&self, root_pem: &[u8]) -> Option<Client> {
        match self.0 {
            ClientMutant::AcceptAnyCertificate | ClientMutant::IgnoreHostName => {
                let config = lenient_tls_config(self.0, root_pem).ok()?;
                Some(Client::with_adapter_impl(
                    HyperClientAdapter::with_tls_config_for_conformance(config),
                ))
            }
            _ => None,
        }
    }
}

/// A certificate check with one defect: the chain is verified against `root_pem` by the real verifier, then the defect is applied.
#[derive(Debug)]
struct LenientVerifier {
    inner: Arc<WebPkiServerVerifier>,
    mutant: ClientMutant,
}

impl ServerCertVerifier for LenientVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let checked = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );
        match (self.mutant, checked) {
            (ClientMutant::AcceptAnyCertificate, _) => Ok(ServerCertVerified::assertion()),
            (
                ClientMutant::IgnoreHostName,
                Err(rustls::Error::InvalidCertificate(
                    CertificateError::NotValidForName
                    | CertificateError::NotValidForNameContext { .. },
                )),
            ) => Ok(ServerCertVerified::assertion()),
            (_, other) => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn lenient_tls_config(
    mutant: ClientMutant,
    root_pem: &[u8],
) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(root_pem) {
        roots
            .add(cert.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
        .build()
        .map_err(|e| e.to_string())?;
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(LenientVerifier { inner, mutant }))
        .with_no_client_auth())
}

/// Every client adapter this build has: the list CI runs the client suite over.
pub fn client_adapters() -> Vec<Box<dyn ClientSubject>> {
    vec![Box::new(HyperClientSubject)]
}

/// The reference client subject.
pub fn client_reference() -> HyperClientSubject {
    HyperClientSubject
}

// --- the raw peer -------------------------------------------------------------------------------------------------------------------------

async fn listen() -> Result<(TcpListener, SocketAddr), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("peer could not listen: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    Ok((listener, addr))
}

async fn accept(listener: &TcpListener) -> Result<TcpStream, String> {
    match tokio::time::timeout(PROMPT, listener.accept()).await {
        Ok(Ok((s, _))) => Ok(s),
        Ok(Err(e)) => Err(format!("peer accept: {e}")),
        Err(_) => Err("the client never connected".to_owned()),
    }
}

/// What the peer has read of one request so far.
struct Seen {
    head: Vec<u8>,
    rest: Vec<u8>,
}

impl Seen {
    fn lines(&self) -> Vec<&[u8]> {
        self.head
            .split(|&b| b == b'\n')
            .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn request_line(&self) -> Vec<u8> {
        self.lines().first().map(|l| l.to_vec()).unwrap_or_default()
    }

    /// Values of a header, as bytes, in order.
    fn header(&self, name: &str) -> Vec<Vec<u8>> {
        self.lines()
            .iter()
            .skip(1)
            .filter_map(|l| {
                let colon = l.iter().position(|&b| b == b':')?;
                l[..colon]
                    .eq_ignore_ascii_case(name.as_bytes())
                    .then(|| l[colon + 1..].trim_ascii().to_vec())
            })
            .collect()
    }
}

async fn read_more(s: &mut TcpStream, buf: &mut Vec<u8>) -> Result<(), String> {
    let mut chunk = [0u8; 8192];
    match tokio::time::timeout(PROMPT, s.read(&mut chunk)).await {
        Ok(Ok(0)) => Err("the client closed the connection".to_owned()),
        Ok(Ok(n)) => {
            buf.extend_from_slice(&chunk[..n]);
            Ok(())
        }
        Ok(Err(e)) => Err(format!("peer read: {e}")),
        Err(_) => Err("the client sent nothing more in time".to_owned()),
    }
}

async fn read_head(s: &mut TcpStream) -> Result<Seen, String> {
    let mut buf = Vec::new();
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(i + 4);
            return Ok(Seen { head: buf, rest });
        }
        read_more(s, &mut buf).await?;
    }
}

async fn read_body(s: &mut TcpStream, seen: &mut Seen) -> Result<Vec<u8>, String> {
    let len: usize = seen
        .header("content-length")
        .first()
        .and_then(|v| std::str::from_utf8(v).ok()?.parse().ok())
        .ok_or("the request has no Content-Length")?;
    while seen.rest.len() < len {
        read_more(s, &mut seen.rest).await?;
    }
    Ok(seen.rest[..len].to_vec())
}

async fn reply(s: &mut TcpStream, bytes: &[u8]) -> Result<(), String> {
    s.write_all(bytes)
        .await
        .map_err(|e| format!("peer write: {e}"))
}

fn url(addr: SocketAddr, path: &str) -> String {
    format!("http://{addr}{path}")
}

async fn joined<T>(task: tokio::task::JoinHandle<Result<T, String>>) -> Result<T, String> {
    match tokio::time::timeout(PROMPT, task).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => Err(format!("client task: {e}")),
        Err(_) => Err("the client did not finish in time".to_owned()),
    }
}

fn err(e: Error) -> String {
    format!("client error: {e}")
}

// --- the cases ----------------------------------------------------------------------------------------------------------------------------

async fn method_fidelity(s: &dyn ClientSubject) -> Outcome {
    for method in ["PROPFIND", "TRACE"] {
        let (listener, addr) = listen().await?;
        let client = s.client();
        let m = Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let task = tokio::spawn(async move {
            let res = client
                .request(m, url(addr, "/m"))
                .send()
                .await
                .map_err(err)?;
            Ok(res.status().as_u16())
        });
        let mut conn = accept(&listener).await?;
        let seen = read_head(&mut conn).await?;
        let line = String::from_utf8_lossy(&seen.request_line()).into_owned();
        reply(
            &mut conn,
            b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
        )
        .await?;
        ensure!(
            line.starts_with(&format!("{method} /m ")),
            "{method}: the peer received the request line `{line}`"
        );
        joined(task).await?;
    }
    Ok(())
}

async fn headers(s: &dyn ClientSubject) -> Outcome {
    let (listener, addr) = listen().await?;
    let client = s.client();
    let task = tokio::spawn(async move {
        let res = client
            .get(url(addr, "/h"))
            .header("X-Multi", "a")
            .header("X-Multi", "b")
            .header("X-Raw", [0xffu8, 0xfe, b'x'])
            .send()
            .await
            .map_err(err)?;
        let cookies: Vec<Vec<u8>> = res
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|v| v.as_bytes().to_vec())
            .collect();
        let back = res
            .headers()
            .get(HeaderName::from_static("x-back"))
            .map(|v| v.as_bytes().to_vec());
        Ok((cookies, back))
    });
    let mut conn = accept(&listener).await?;
    let seen = read_head(&mut conn).await?;
    reply(
        &mut conn,
        b"HTTP/1.1 200 OK\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nX-Back: \xff\xfe\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await?;
    ensure!(
        seen.header("x-multi") == [b"a".to_vec(), b"b".to_vec()],
        "request header X-Multi: the peer received {:?}",
        seen.header("x-multi")
    );
    ensure!(
        seen.header("x-raw") == [vec![0xffu8, 0xfe, b'x']],
        "request header X-Raw: the peer received {:?}",
        seen.header("x-raw")
    );
    let (cookies, back) = joined(task).await?;
    ensure!(
        cookies == [b"a=1".to_vec(), b"b=2".to_vec()],
        "two Set-Cookie headers reached the caller as {cookies:?}"
    );
    ensure!(
        back.as_deref() == Some([0xffu8, 0xfe].as_slice()),
        "non-UTF8 response header reached the caller as {back:?}"
    );
    Ok(())
}

async fn bodies(s: &dyn ClientSubject) -> Outcome {
    let binary: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
    let (listener, addr) = listen().await?;
    let client = s.client();
    let sent = binary.clone();
    let task = tokio::spawn(async move {
        let res = client
            .post(url(addr, "/b"))
            .body(sent)
            .send()
            .await
            .map_err(err)?;
        Ok(res.bytes().await.map_err(err)?.to_vec())
    });
    let mut conn = accept(&listener).await?;
    let mut seen = read_head(&mut conn).await?;
    let got = read_body(&mut conn, &mut seen).await?;
    let mut res = b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n".to_vec();
    res.extend_from_slice(&binary);
    reply(&mut conn, &res).await?;
    ensure!(
        got == binary,
        "the request body was altered on the way ({} bytes sent, {} identical)",
        binary.len(),
        got.iter().zip(&binary).filter(|(a, b)| a == b).count()
    );
    let back = joined(task).await?;
    ensure!(
        back == binary,
        "the response body was altered on the way ({} bytes sent, {} bytes received)",
        binary.len(),
        back.len()
    );
    Ok(())
}

async fn statuses(s: &dyn ClientSubject) -> Outcome {
    for code in [200u16, 204, 301, 404, 418, 503] {
        let (listener, addr) = listen().await?;
        let client = s.client();
        let task = tokio::spawn(async move {
            let res = client.get(url(addr, "/s")).send().await.map_err(err)?;
            Ok((
                res.status().as_u16(),
                res.headers().contains_key("location"),
            ))
        });
        let mut conn = accept(&listener).await?;
        read_head(&mut conn).await?;
        let extra = if code == 301 {
            "Location: http://127.0.0.1:9/elsewhere\r\n"
        } else {
            ""
        };
        reply(
            &mut conn,
            format!("HTTP/1.1 {code} X\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
        let (got, _) = joined(task).await?;
        ensure!(got == code, "status {code}: the caller received {got}");
    }
    Ok(())
}

async fn streaming_response(s: &dyn ClientSubject) -> Outcome {
    let (listener, addr) = listen().await?;
    let client = s.client();
    let (got_first, first_seen) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let res = client.get(url(addr, "/stream")).send().await.map_err(err)?;
        let mut body = res.into_body();
        let first = body
            .frame()
            .await
            .and_then(|f| f.ok())
            .and_then(|f| f.into_data().ok())
            .unwrap_or_default();
        let _ = got_first.send(());
        let rest = body.bytes(1024).await.map_err(err)?;
        Ok((first.to_vec(), rest.to_vec()))
    });
    let mut conn = accept(&listener).await?;
    read_head(&mut conn).await?;
    reply(
        &mut conn,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\none\r\n",
    )
    .await?;
    ensure!(
        tokio::time::timeout(PROMPT, first_seen).await.is_ok(),
        "the caller did not get the first response chunk while the server was still sending (the response is buffered)"
    );
    reply(&mut conn, b"3\r\ntwo\r\n0\r\n\r\n").await?;
    let (first, rest) = joined(task).await?;
    ensure!(first == b"one", "first chunk: {first:?}");
    ensure!(rest == b"two", "remaining body: {rest:?}");
    Ok(())
}

async fn streaming_request(s: &dyn ClientSubject) -> Outcome {
    let (listener, addr) = listen().await?;
    let client = s.client();
    let (tx, rx) = mpsc::channel::<Bytes>(4);
    let frames = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|b| (Ok::<_, Infallible>(Frame::data(b)), rx))
    });
    let task = tokio::spawn(async move {
        let res = client
            .post(url(addr, "/up"))
            .body_stream(Body::new(StreamBody::new(frames)))
            .send()
            .await
            .map_err(err)?;
        Ok(res.status().as_u16())
    });
    tx.send(Bytes::from_static(b"abc"))
        .await
        .map_err(|e| e.to_string())?;
    let mut conn = accept(&listener).await?;
    let mut seen = read_head(&mut conn).await?;
    ensure!(
        !seen.header("transfer-encoding").is_empty(),
        "a body of unknown length was not sent chunked"
    );
    while !seen.rest.windows(3).any(|w| w == b"abc") {
        read_more(&mut conn, &mut seen.rest).await.map_err(|e| {
            format!("the peer did not receive the first request chunk while the caller was still producing the rest (the request body is buffered): {e}")
        })?;
    }
    tx.send(Bytes::from_static(b"def"))
        .await
        .map_err(|e| e.to_string())?;
    drop(tx);
    while !seen.rest.ends_with(b"0\r\n\r\n") {
        read_more(&mut conn, &mut seen.rest).await?;
    }
    ensure!(
        seen.rest.windows(3).any(|w| w == b"def"),
        "the second request chunk never arrived"
    );
    reply(
        &mut conn,
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await?;
    let status = joined(task).await?;
    ensure!(status == 200, "status {status}");
    Ok(())
}

async fn connection_reuse(s: &dyn ClientSubject) -> Outcome {
    let (listener, addr) = listen().await?;
    let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&accepted);
    // A keep-alive peer: answers every request on a connection until the client closes it.
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                while let Ok(seen) = read_head(&mut conn).await {
                    drop(seen);
                    if conn
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    let client = s.client();
    for n in 1..=2 {
        let text = tokio::time::timeout(PROMPT, async {
            client
                .get(url(addr, "/k"))
                .send()
                .await
                .map_err(err)?
                .text()
                .await
                .map_err(err)
        })
        .await
        .map_err(|_| format!("request {n} did not finish in time"))??;
        ensure!(text == "ok", "request {n}: body `{text}`");
    }
    let connections = accepted.load(std::sync::atomic::Ordering::SeqCst);
    ensure!(
        connections == 1,
        "two requests to one host used {connections} connections (expected the second to reuse the first)"
    );
    Ok(())
}

/// A TLS server written here (tokio-rustls, not HTTPAnt's own), with a self-signed certificate for `name`, answering `secure` to any request.
struct TlsPeer {
    addr: SocketAddr,
    cert_pem: String,
    requests: Arc<std::sync::atomic::AtomicUsize>,
}

async fn tls_peer(name: &str) -> Result<TlsPeer, String> {
    let made = rcgen::generate_simple_self_signed(vec![name.to_owned()])
        .map_err(|e| format!("test certificate: {e}"))?;
    let key = PrivateKeyDer::try_from(made.signing_key.serialize_der())
        .map_err(|e| format!("test key: {e}"))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("test server: {e}"))?
    .with_no_client_auth()
    .with_single_cert(vec![made.cert.der().clone()], key)
    .map_err(|e| format!("test server: {e}"))?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let (listener, addr) = listen().await?;
    let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
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
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });
    Ok(TlsPeer {
        addr,
        cert_pem: made.cert.pem(),
        requests,
    })
}

/// Case 8: https is verified. The peer here is a TLS server of our own with self-signed certificates. A client that declares `tls: yes` must
/// fetch from a peer whose certificate it was told to trust and whose name matches; refuse a peer whose certificate it was not told to trust;
/// refuse one whose certificate does not name the host it connected to; and, with no extra root, refuse a self-signed peer (the bundled roots
/// are the only trust it has). A client that declares `tls: no` must refuse the https URL and never connect.
async fn client_https(s: &dyn ClientSubject) -> Outcome {
    use std::sync::atomic::Ordering::SeqCst;
    let right = tls_peer("127.0.0.1").await?;
    let wrong = tls_peer("other.example").await?;
    let url_right = format!("https://{}/", right.addr);
    let url_wrong = format!("https://{}/", wrong.addr);

    if s.declared().map(|c| c.tls) == Some(Support::No) {
        let refused = s.client().get(&url_right).send().await;
        ensure!(
            refused.is_err(),
            "a client that declares tls: no fetched an https URL"
        );
        ensure!(
            right.requests.load(SeqCst) == 0,
            "a client that declares tls: no connected to an https URL"
        );
        return Ok(());
    }

    let trusting = |pem: &str| {
        s.client_trusting(pem.as_bytes()).ok_or_else(|| {
            "this subject cannot be told to trust a root, so https cannot be measured on it"
                .to_owned()
        })
    };

    // 1. A certificate it trusts, for the name it connected to: the body arrives, over TLS.
    let text = tokio::time::timeout(PROMPT, async {
        trusting(&right.cert_pem)?
            .get(&url_right)
            .send()
            .await
            .map_err(err)?
            .text()
            .await
            .map_err(err)
    })
    .await
    .map_err(|_| "the https request did not finish in time".to_owned())??;
    ensure!(
        text == "secure",
        "the https body was not the one the peer sent"
    );
    ensure!(
        right.requests.load(SeqCst) == 1,
        "the peer saw {} requests over TLS (expected 1)",
        right.requests.load(SeqCst)
    );

    // 2. A trusted certificate that names another host: refused at the handshake, nothing sent.
    let name_mismatch = trusting(&wrong.cert_pem)?.get(&url_wrong).send().await;
    ensure!(
        matches!(name_mismatch, Err(Error::TlsHandshake(_))),
        "a certificate for another host name was not refused with a TLS handshake error"
    );
    ensure!(
        wrong.requests.load(SeqCst) == 0,
        "a request was sent to a peer whose certificate did not name the host"
    );

    // 3. A certificate it was not told to trust (it trusts `wrong`'s, connects to `right`): refused at the handshake, nothing sent.
    let untrusted = trusting(&wrong.cert_pem)?.get(&url_right).send().await;
    ensure!(
        matches!(untrusted, Err(Error::TlsHandshake(_))),
        "an untrusted certificate was not refused with a TLS handshake error"
    );
    ensure!(
        right.requests.load(SeqCst) == 1,
        "a request was sent to a peer whose certificate was not trusted"
    );

    // 4. With no extra root at all, a self-signed peer is refused: the default trust is the bundled roots and nothing else.
    let default_trust = s.client().get(&url_right).send().await;
    ensure!(
        matches!(default_trust, Err(Error::TlsHandshake(_))),
        "a self-signed certificate was accepted with no extra root"
    );
    Ok(())
}

/// Probe, not a requirement: does the adapter follow a `302`? `Ok(true)` means it did (the caller got the final `200`), `Ok(false)` that it
/// returned the `302` as it is. Anything else is an error: half-following is a defect either way.
async fn follows_redirect(s: &dyn ClientSubject) -> Result<bool, String> {
    let (listener, addr) = listen().await?;
    let client = s.client();
    let task = tokio::spawn(async move {
        let res = client.get(url(addr, "/old")).send().await.map_err(err)?;
        Ok(res.status().as_u16())
    });
    let mut conn = accept(&listener).await?;
    read_head(&mut conn).await?;
    reply(
        &mut conn,
        b"HTTP/1.1 302 Found\r\nLocation: /new\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await?;
    // If the client follows it will connect again for `/new`; if not, the first status is the answer.
    let second = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
    if let Ok(Ok((mut again, _))) = second {
        let seen = read_head(&mut again).await?;
        let line = String::from_utf8_lossy(&seen.request_line()).into_owned();
        ensure!(
            line.starts_with("GET /new "),
            "a followed redirect went to `{line}`"
        );
        reply(
            &mut again,
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await?;
        let status = joined(task).await?;
        ensure!(status == 200, "a followed redirect ended with {status}");
        return Ok(true);
    }
    let status = joined(task).await?;
    ensure!(status == 302, "an unfollowed 302 was reported as {status}");
    Ok(false)
}

// --- registry ------------------------------------------------------------------------------------------------------------------------------

/// One client conformance case and the broken variants it must fail on.
#[derive(Clone, Copy)]
pub struct ClientCase {
    /// Number (1-based, contiguous).
    pub number: u8,
    /// Stable short id.
    pub id: &'static str,
    /// What a conforming client adapter does.
    pub requirement: &'static str,
    /// The deliberately broken variants this case must fail on.
    pub mutants: &'static [ClientMutant],
    run: for<'a> fn(&'a dyn ClientSubject) -> BoxFuture<'a, Outcome>,
}

impl ClientCase {
    /// Run this case against `subject`.
    pub async fn run(&self, subject: &dyn ClientSubject) -> Outcome {
        (self.run)(subject).await
    }
}

macro_rules! run_fn {
    ($f:ident) => {
        |s| Box::pin($f(s))
    };
}

/// The client cases.
pub const CLIENT_CASES: [ClientCase; 8] = [
    ClientCase {
        number: 1,
        id: "client-method-fidelity",
        requirement: "PROPFIND and TRACE reach the peer as sent",
        mutants: &[ClientMutant::FoldMethodToGet],
        run: run_fn!(method_fidelity),
    },
    ClientCase {
        number: 2,
        id: "client-headers",
        requirement: "repeated request headers and non-UTF8 values reach the peer as sent; two Set-Cookie and a non-UTF8 response header reach the caller",
        mutants: &[ClientMutant::CollapseHeaders],
        run: run_fn!(headers),
    },
    ClientCase {
        number: 3,
        id: "client-bodies",
        requirement: "a binary request body and a binary response body are byte-identical",
        mutants: &[ClientMutant::LossyBodies],
        run: run_fn!(bodies),
    },
    ClientCase {
        number: 4,
        id: "client-statuses",
        requirement: "200, 204, 301, 404, 418 and 503 reach the caller unchanged (a 301 is reported, not followed, by an adapter that declares it does not follow)",
        mutants: &[ClientMutant::ClampStatus],
        run: run_fn!(statuses),
    },
    ClientCase {
        number: 5,
        id: "client-streaming-response",
        requirement: "the first chunk of a response reaches the caller while the server is still sending the rest",
        mutants: &[ClientMutant::BufferResponseBody],
        run: run_fn!(streaming_response),
    },
    ClientCase {
        number: 6,
        id: "client-streaming-request",
        requirement: "the first chunk of a request body reaches the peer while the caller is still producing the rest",
        mutants: &[ClientMutant::BufferRequestBody],
        run: run_fn!(streaming_request),
    },
    ClientCase {
        number: 7,
        id: "client-connection-reuse",
        requirement: "two requests to one host use one connection",
        mutants: &[ClientMutant::NoConnectionReuse],
        run: run_fn!(connection_reuse),
    },
    ClientCase {
        number: 8,
        id: "client-https",
        requirement: "https is verified: a trusted certificate for the right host name is fetched over TLS; an untrusted one, one for another host name, and a self-signed one with no extra root are refused at the handshake before anything is sent",
        mutants: &[
            ClientMutant::AcceptAnyCertificate,
            ClientMutant::IgnoreHostName,
        ],
        run: run_fn!(client_https),
    },
];

/// Run every client case against `subject`.
pub async fn run_client_suite(subject: &dyn ClientSubject) -> Vec<(u8, Outcome)> {
    let mut out = Vec::new();
    for case in &CLIENT_CASES {
        out.push((case.number, case.run(subject).await));
    }
    out
}

/// The both-halves check: the reference passes and every listed mutant fails.
pub async fn check_client_case(case: &ClientCase, reference: &dyn ClientSubject) -> Outcome {
    case.run(reference).await.map_err(|e| {
        format!(
            "client case {} `{}` failed on {}: {e}",
            case.number,
            case.id,
            reference.name()
        )
    })?;
    for &mutant in case.mutants {
        if case.run(&ClientMutantSubject(mutant)).await.is_ok() {
            return Err(format!(
                "client case {} `{}` PASSED on mutant `{}`: the case cannot fail, so it proves nothing",
                case.number,
                case.id,
                mutant.id()
            ));
        }
    }
    Ok(())
}

/// What the client suite measured about a subject.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientMeasured {
    /// Case 6.
    pub streaming_request_bodies: Support,
    /// Case 5.
    pub streaming_response_bodies: Support,
    /// Case 7.
    pub connection_reuse: Support,
    /// The redirect probe.
    pub follows_redirects: Support,
    /// Case 8.
    pub tls: Support,
}

fn support(r: &Outcome) -> Support {
    if r.is_ok() { Support::Yes } else { Support::No }
}

/// Run the client capability probes against `subject`.
pub async fn measure_client(s: &dyn ClientSubject) -> ClientMeasured {
    ClientMeasured {
        streaming_request_bodies: support(&streaming_request(s).await),
        streaming_response_bodies: support(&streaming_response(s).await),
        connection_reuse: support(&connection_reuse(s).await),
        follows_redirects: match follows_redirect(s).await {
            Ok(true) => Support::Yes,
            Ok(false) => Support::No,
            Err(_) => Support::Unknown,
        },
        tls: support(&client_https(s).await),
    }
}
