# Which web libraries can sit behind HTTPAnt's TLS acceptor

Question (WebHostAnt's need list, questions 1 to 3; ROADMAP row 4): can the library serve connections that HTTPAnt has already put through
its own TLS handshake (`TlsAcceptor`, a `CertSource` asked once per handshake, with refusal on an unknown name or a source failure)? The
design note had this as UNVERIFIED for every library but axum and hyper. Probed 2026-10-30 against the versions in `Cargo.lock`: actix-web
4.18.1, poem 3.1.12, salvo_core 1.0.1, warp 0.4.3. "Run" means the result below came from compiling or running code, not from reading.

| library | verdict | how it was established |
|---|---|---|
| poem 3.1.12 | **terminates TLS** (`tls: Yes`) | Run. `poem::listener::Acceptor` is a public trait over any `AsyncRead + AsyncWrite` stream; `poem_tls.rs` implements it with a task that accepts TCP, runs the handshake in its own task, and hands finished streams over. Conformance case 17 and `tests/tls.rs` pass on it (TLS 1.3 and 1.2, the given certificate, `https` reported, refusal on unknown name and source failure, renewal, handshake timeout, read-back, audit events). |
| salvo_core 1.0.1 | **terminates TLS** (`tls: Yes`) | Run. `salvo_core::conn::Acceptor` is public; `salvo_tls.rs` wraps the existing acceptor and gives each connection a `HandshakeStream` whose handshake is HTTPAnt's, so salvo's own lazy-handshake machinery and its handshake timeout apply. Same tests, same result. |
| warp 0.4.3 | **edge mode only** (`tls: No`) | Run. `warp::serve(..).incoming(acceptor)` takes an acceptor, but its `Accept` trait is in a private module: an attempt to implement it from outside fails with `error[E0603]: module server is private` ("trait Accept is not publicly re-exported"). Warp's own `Server::tls` is an empty stub (`pub fn tls(self) -> ... {}`) in this version. Nothing outside warp can plug a TLS stream in. |
| actix-web 4.18.1 | **edge mode only** (`tls: No`) | **Run** (spike, branch `spike/actix-tls` at e799dc8, not merged). `HttpServer` has no entry point for an acceptor or an already-handshaken stream; TLS is `listen_rustls_0_23(lst, rustls::ServerConfig)` and the certificate resolver in that config is synchronous (`ResolvesServerCert::resolve`). The spike bridged it anyway: the resolver hands `CertSource::cert_for` to the caller's multi-thread runtime and **blocks the actix worker thread** on a channel until it answers (an unknown name or a failing source returns `None`, which refuses the handshake). Results below. |

### The actix-web spike, run 2026-10-30

With `actix-web/rustls-0_23` on (adds `actix-tls` 3.6.1 and a few small crates; no aws-lc, ring only) and the bridge in `actix_tls.rs`:

- **Works:** conformance case 17 (HTTPS on TLS 1.3 and 1.2, the given certificate, `https` reported, a cleartext client gets no answer); refusal of an unknown name and of a failing source; read-back; a silent client dropped at the timeout (actix's own `tls_handshake_timeout`, set to ours).
- **Needed a fix:** a renewed certificate was not served to a client that resumed its session (`tests/tls.rs:234`, `a_renewed_certificate_serves...`: "still the old one"), because one shared `ServerConfig` keeps a session cache and a resumed session never reaches the resolver. Fixed by `NoServerSessionStorage` and no tickets. The other adapters build a config per handshake and never had this.
- **Fails, and cannot be fixed from outside actix:**
  1. *Isolation.* The resolver runs on an actix worker, which is a single-threaded runtime, so the wait for the source blocks that worker. Measured (`tests/tls_stall_probe.rs`, `--nocapture`, 2 workers, a source that takes 700 ms for one name): a fast handshake takes **8 ms** on hyper and **7 ms** on axum whether or not six slow ones are in flight, and **2035 ms** on actix-web while they are (6 x 700 ms over 2 workers). One slow or hung certificate source freezes every connection on the worker. Whether established connections' requests stall as well was not measured.
  2. *Audit events (need 23).* actix-tls runs the handshake, so HTTPAnt's acceptor never sees it: `HandshakeRefused` with `Timeout` or `Protocol` and `CertificateChanged` are not sent (`tests/tls.rs:463` and `:543` fail with an empty event list). Only the resolver sees the unknown-name and source-failure cases.
- **Verdict:** actix-web can be made to honour an async, refusing `CertSource`, but only by blocking its worker, and without the audit trail. That breaks the property the other four adapters have (a slow source costs only its own handshake), so it stays `tls: No`. The spike is kept on its branch as the evidence; a clean route would be an adapter built on `actix-server` and `actix-http` that runs HTTPAnt's own acceptor (not built).

Why "edge mode only" is a fine answer for warp and actix-web: behind a proxy that terminates TLS (Cloudflare, Caddy, WebServAnt), the edge
mode of 0.5.0 gives them the client address and scheme from trusted forwarded headers, and case 17 holds them to refusing a TLS acceptor by
name, never serving cleartext when TLS was asked for.

Re-run this when a pinned version changes: warp 0.4 is the line whose `tls()` is a stub, and actix-web may grow an acceptor hook. The
conformance case is the guard: an adapter that starts claiming `tls: Yes` is held to the full HTTPS scenario.

## What the answers mean for WebHostAnt's questions

1. *Which adapters take a per-handshake certificate callback?* hyper, axum, poem, salvo: all four through HTTPAnt's acceptor, not the
   library's own TLS, so the behaviour is the same on each (case 17).
2. *A TLS library choice inside an adapter that the trait must not contradict?* None is used: HTTPAnt's acceptor is rustls (ring) below the
   adapters. actix-web's own rustls and warp's stub are not used.
3. *Can `acme-tls/1` routing be done by every adapter?* Yes on the four that terminate TLS, and it is done once, in HTTPAnt's acceptor
   (0.5.11, `CertSource::acme_tls_cert`), so it does not need the libraries: `tests/tls.rs` runs it on hyper, axum, poem and salvo. actix-web
   and warp do not terminate TLS, so a validation on port 443 reaches whatever edge does.
