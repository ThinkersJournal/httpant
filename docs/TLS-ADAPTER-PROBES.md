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
| actix-web 4.18.1 | **edge mode only** (`tls: No`) | Read, not run. `HttpServer` has no entry point that takes an acceptor or an already-handshaken stream: TLS is `bind_rustls_0_23` / `listen_rustls_0_23` with the library's own `rustls::ServerConfig`, whose certificate resolver is synchronous (`ResolvesServerCert`), so a `CertSource` that is async and may refuse a handshake (need 5 and 8) cannot be honoured; `on_connect` only observes. A route through `actix-server` and `actix-http` directly may exist; it is a different adapter, not built. |

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
3. *Can `acme-tls/1` routing be done by every adapter?* Not tested here. Because the handshake is below the adapters on the four that
   terminate TLS, it can be done once in HTTPAnt's acceptor and does not need the libraries; actix-web and warp do not terminate TLS at all.
