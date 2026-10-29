# HTTPAnt capability matrix

What each adapter does, **as measured by the conformance suite** (`crates/httpant-conformance`), not as claimed. The table between the markers is
rendered by the test `the_capability_matrix_document_is_current`, which fails when it is stale; `HTTPANT_BLESS=1 cargo test -p httpant-conformance`
rewrites it. Each adapter's `capabilities()` is compared with the measurement by `declared_capabilities_match_the_measured_ones`, so a declared
`yes` that the suite cannot reproduce fails the build.

<!-- matrix:start -->
| adapter | streaming request bodies | streaming response bodies | graceful shutdown | non-`Send` handlers | default body limit (bytes) |
|---|---|---|---|---|---|
| hyper | yes | yes | yes | no | 2097152 |
| axum | yes | yes | yes | no | 2097152 |
<!-- matrix:end -->

- *streaming request bodies* / *streaming response bodies*: conformance case 8. The first chunk crosses the connection while the other side is still
  producing (response) or sending (request); gated by handler signals, not by sleeps.
- *graceful shutdown*: case 9. After shutdown is requested: a request in flight completes, an idle keep-alive connection is closed, new
  connections are refused, and `shutdown()` returns only after the request completed. There is no grace period yet.
- *non-`Send` handlers*: declared, not measured. The service contract is `Send`; actix-web (thread-per-core, `!Send` handlers) is the adapter this
  column is for (`docs/ACTIX-ADAPTER-NOTES.md`).
- Cases 10 (keep-alive, `Connection: close`, `Expect: 100-continue`), 11 (concurrency) and 12 (HEAD) are required of every adapter; they have no
  column because an adapter that fails them is not an adapter.
- A mutant-free sub-check: in case 10, `Connection: close` and `Expect: 100-continue` have no service-level mutant (they are decided by the HTTP
  server below the service); only the keep-alive part is shown able to fail.
