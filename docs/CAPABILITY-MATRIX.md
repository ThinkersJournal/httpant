# HTTPAnt capability matrix

What each adapter does, **as measured by the conformance suite** (`crates/httpant-conformance`), not as claimed. The table between the markers is
rendered by the test `the_capability_matrix_document_is_current`, which fails when it is stale; `HTTPANT_BLESS=1 cargo test -p httpant-conformance`
rewrites it. Each adapter's `capabilities()` is compared with the measurement by `declared_capabilities_match_the_measured_ones`, so a declared
`yes` that the suite cannot reproduce fails the build.

<!-- matrix:start -->
| adapter | streaming request bodies | streaming response bodies | graceful shutdown | non-`Send` handlers | terminates TLS | HTTP/2 (over TLS) | default body limit (bytes) |
|---|---|---|---|---|---|---|---|
| hyper | yes | yes | yes | no | yes | yes | 2097152 |
| axum | yes | yes | yes | no | yes | yes | 2097152 |
| actix-web | yes | yes | yes | no | no | no | 2097152 |
| poem | yes | yes | yes | no | yes | yes | 2097152 |
| salvo | yes | yes | yes | no | yes | no | 2097152 |
| warp | yes | yes | yes | no | no | no | 2097152 |
<!-- matrix:end -->

Client adapters, measured by the client cases against a raw-TCP peer (`client_suite.rs`):

<!-- client-matrix:start -->
| client adapter | streaming request bodies | streaming response bodies | connection reuse | follows redirects | TLS | default body limit (bytes) |
|---|---|---|---|---|---|---|
| hyper | yes | yes | yes | no | no | 2097152 |
<!-- client-matrix:end -->

- *connection reuse*: client case 7 (two requests to one host, one connection). *follows redirects*: a probe, not a requirement; an adapter either
  follows a `302` or returns it as it is, and half-following is an error. *TLS*: declared, not measured (the reference client has no TLS connector yet).
- *streaming request bodies* / *streaming response bodies* (server): conformance case 8. The first chunk crosses the connection while the other side is still
  producing (response) or sending (request); gated by handler signals, not by sleeps.
- *graceful shutdown*: case 9. After shutdown is requested: a request in flight completes, an idle keep-alive connection is closed, new
  connections are refused, and `shutdown()` returns only after the request completed. There is no grace period yet.
- *terminates TLS*: declared, and held to its declaration by case 17. For `yes` the case proves HTTPS on TLS 1.3 and 1.2 with the given
  certificate, the request reported as `https`, and a cleartext client getting no answer; for `no` it requires a named refusal to start with
  a TLS acceptor, so TLS that was asked for is never served in clear. The two `no` rows were probed (`docs/TLS-ADAPTER-PROBES.md`): warp's
  `Accept` trait is private (run: it does not compile from outside), and actix-web, run as a spike (`spike/actix-tls`), can be bridged only by
  blocking its single-threaded worker while the certificate source answers (a fast handshake took 2035 ms behind six slow ones, against 7 ms
  on hyper and axum) and without HTTPAnt's audit events, so both stay edge mode only.
- *HTTP/2 (over TLS)*: declared (needs the `http2` feature of `httpant-server`), and held to its declaration by case 18. A TLS listener
  offers `h2` only if the adapter serves it. For `yes` the case proves a client offering `h2` and `http/1.1` is given `h2` and answered over
  HTTP/2 with the request reported as `https`; for `no` (with TLS `yes`: salvo) it proves the client is given `http/1.1`, never an `h2` the
  adapter cannot serve. Cleartext HTTP/2 (h2c) is **not** part of the column and is not uniform: hyper refuses the prior-knowledge preface on
  a cleartext listener, while axum's and poem's own builders accept it (see ROADMAP).
- *non-`Send` handlers*: declared, not measured. The service contract is `Send`; actix-web (thread-per-core, `!Send` handlers) is the adapter this
  column is for (`docs/ACTIX-ADAPTER-NOTES.md`).
- Cases 10 (keep-alive, `Connection: close`, `Expect: 100-continue`), 11 (concurrency) and 12 (HEAD) are required of every adapter; they have no
  column because an adapter that fails them is not an adapter.
- A mutant-free sub-check: in case 10, `Connection: close` and `Expect: 100-continue` have no service-level mutant (they are decided by the HTTP
  server below the service); only the keep-alive part is shown able to fail.
