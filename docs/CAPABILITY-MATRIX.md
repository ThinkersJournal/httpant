# HTTPAnt capability matrix

What each adapter does, **as measured by the conformance suite** (`crates/httpant-conformance`), not as claimed. The table between the markers is
rendered by the test `the_capability_matrix_document_is_current`, which fails when it is stale; `HTTPANT_BLESS=1 cargo test -p httpant-conformance`
rewrites it. Each adapter's `capabilities()` is compared with the measurement by `declared_capabilities_match_the_measured_ones`, so a declared
`yes` that the suite cannot reproduce fails the build.

<!-- matrix:start -->
| adapter | streaming request bodies | streaming response bodies | graceful shutdown | non-`Send` handlers | terminates TLS | HTTP/2 (over TLS) | cleartext h2c (measured) | default body limit (bytes) |
|---|---|---|---|---|---|---|---|---|
| hyper | yes | yes | yes | no | yes | yes | no | 2097152 |
| axum | yes | yes | yes | no | yes | yes | yes | 2097152 |
| actix-web | yes | yes | yes | no | yes | no | no | 2097152 |
| poem | yes | yes | yes | no | yes | yes | yes | 2097152 |
| salvo | yes | yes | yes | no | yes | yes | yes | 2097152 |
| warp | yes | yes | yes | no | no | no | yes | 2097152 |
<!-- matrix:end -->

Client adapters, measured by the client cases against a raw-TCP peer (`client_suite.rs`):

<!-- client-matrix:start -->
| client adapter | streaming request bodies | streaming response bodies | connection reuse | follows redirects | TLS | default body limit (bytes) |
|---|---|---|---|---|---|---|
| hyper | yes | yes | yes | no | yes | 2097152 |
<!-- client-matrix:end -->

- *connection reuse*: client case 7 (two requests to one host, one connection). *follows redirects*: a probe, not a requirement; an adapter either
  follows a `302` or returns it as it is, and half-following is an error. *TLS*: measured by client case 8 against a TLS peer written in the suite (a trusted certificate is fetched; an untrusted one, one for another host name and a self-signed one with no extra root are refused before anything is sent). It is `yes` only when the `https` feature is on, which the suite builds with. Client case 9 holds `refuse_private_addresses` to its promise: an IPv4, an IPv6 and an IPv4-in-IPv6 loopback literal and a *name* that resolves to loopback are refused with no connection made, while the same peer answers a client that was not told to refuse (the mutant that checks only the host written in the URL fails it on the name).
- *streaming request bodies* / *streaming response bodies* (server): conformance case 8. The first chunk crosses the connection while the other side is still
  producing (response) or sending (request); gated by handler signals, not by sleeps.
- *graceful shutdown*: case 9. After shutdown is requested: a request in flight completes, an idle keep-alive connection is closed, new
  connections are refused, and `shutdown()` returns only after the request completed. There is no grace period yet.
- *terminates TLS*: declared, and held to its declaration by case 17. For `yes` the case proves HTTPS on TLS 1.3 and 1.2 with the given
  certificate, the request reported as `https`, and a cleartext client getting no answer; for `no` it requires a named refusal to start with
  a TLS acceptor, so TLS that was asked for is never served in clear. The one `no` row was probed (`docs/TLS-ADAPTER-PROBES.md`): warp's
  `Accept` trait is private (run: it does not compile from outside), so it stays edge mode only. actix-web was `no` until 0.5.25: through its own
  rustls support it could be bridged only by blocking its single-threaded worker while the certificate source answers (a fast handshake took
  2035 ms behind six slow ones, against 7 ms on hyper and axum), so it now takes HTTPAnt's own acceptor and `actix_http::HttpService` instead
  (one actix thread for TLS; `tests/tls_isolation.rs` holds the property on every TLS adapter).
- *HTTP/2 (over TLS)*: declared (needs the `http2` feature of `httpant-server`), and held to its declaration by case 18. A TLS listener
  offers `h2` only if the adapter serves it. For `yes` the case proves a client offering `h2` and `http/1.1` is given `h2` and answered over
  HTTP/2 with the request reported as `https`; for `no` with TLS `yes` (actix-web, since 0.5.25: its HTTP/2 is the `h2` 0.3 line, which `cargo deny` fails on with RUSTSEC-2026-0258) it
  proves the client is given `http/1.1`, never an `h2` the adapter cannot serve. warp does not terminate TLS.
- *cleartext h2c (measured)*: a probe, **not a requirement** (nothing in the need list asks for it) and **not uniform**, shown as it is. The
  probe sends the HTTP/2 connection preface and an empty SETTINGS frame to a cleartext listener and reads what comes back: `yes` is an HTTP/2
  SETTINGS frame (the adapter's connection builder accepts h2c by prior knowledge), `no` is HTTP/1.1 text, a closed connection or silence.
  Measured 2026-10-31 with the `http2` feature on: axum, poem, salvo and warp answer in HTTP/2; hyper (which HTTPAnt builds `http1_only` for
  cleartext) and actix-web do not. Salvo was `no` before it served HTTP/2 over TLS (0.5.14): its connection builder detects the preface on any
  stream once its own `http2` feature is on, so the h2c answer moved with the feature. The `yes` rows are the probe's own control (it can see
  an HTTP/2 answer). The column does not say which behaviour is right; ROADMAP row 13 holds that decision.
- *non-`Send` handlers*: declared, not measured. The service contract is `Send`; actix-web (thread-per-core, `!Send` handlers) is the adapter this
  column is for (`docs/ACTIX-ADAPTER-NOTES.md`).
- Cases 10 (keep-alive, `Connection: close`, `Expect: 100-continue`), 11 (concurrency) and 12 (HEAD) are required of every adapter; they have no
  column because an adapter that fails them is not an adapter.
- All three parts of case 10 are shown able to fail. Keep-alive has a service-level mutant (`force-connection-close`). `Connection: close` and
  `Expect: 100-continue` are decided by the HTTP/1.1 layer below the service, so their mutants (`ignore-connection-close`,
  `skip-expect-continue`) are a copy of the reference accept loop whose stream drops that one header line before the HTTP/1.1 layer reads it
  (`mutants::serve_stripping`); the real application runs on top. A test checks each fails case 10 with the message that names its own defect.
