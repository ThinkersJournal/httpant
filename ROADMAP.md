# HTTPAnt roadmap

The repo's own work list (a HANDOFF is current state and is rewritten; work that must not be lost lives here). Order is the PM's, 2026-10-30.
Status: `next` (do now, in this order), `later`, `blocked` (needs a decision or another lane), `done` (kept one release, then deleted).

| # | Row | Status | Notes |
|---|---|---|---|
| 1 | Percent-decoding of path captures | done | merged (#17), 0.2.0 (Breaking). Rule in CHANGELOG 0.2.0. |
| 2 | Limits in `ServeOptions` (target, header count and bytes, body) | done | merged (#18), 0.3.0 (Breaking: 2 MiB body default). Case 14. |
| 3 | Shutdown grace period (a request that never finishes must not hold `shutdown()` forever) | done | 0.4.0 (Breaking). 30 s default; case 15. |
| 4 | TLS: need list from WebHostAnt (WebHostAnt#19, `docs/TLS-NEED-LIST.md`). Slices: (a) edge mode + trusted-proxy scheme/client address (needs 1-3), then ping WebHostAnt; (b) `CertSource` trait, feature `tls`, rustls only, HTTPAnt-owned acceptor below the adapters, served-cert read-back (needs 4-16, 19, 20, 22) with the ACME challenge-prefix handler (14); (c) the rest. | in progress | Slice (a) edge mode done in 0.5.0 (case 16). Slice (b) first half in 0.5.1 (PR open): `CertSource`, `tls` feature, acceptor on hyper+axum, needs 4-12, 15, 16, 19, 20 (tests/tls.rs). ACME challenge prefix (14) done in 0.5.5 (PR open). Read-back (22) done in 0.5.4. 80->443 redirect (18) done in 0.5.6 (PR open). Audit events (23) done in 0.5.7 (no rebind event because there is no rebind operation). TLS conformance case 17 done in 0.5.8 (PR open). TLS probes of the four libraries done in 0.5.9 (PR open, `docs/TLS-ADAPTER-PROBES.md`): poem and salvo terminate TLS (case 17 proves it); warp (private `Accept` trait, run) and actix-web (run as a spike, 0.5.10: bridgeable only by blocking its worker, 2035 ms vs 7 ms, and no audit events) are edge mode only. Left: `acme-tls/1` (13), `h2`. HTTPAnt must not become a TLS stack (no ACME, storage, renewal). |
| 5 | Benchmark harness (adapter vs the bare framework) | later | Last. `docs/BENCHMARKS.md` has the TechEmpower reading to compare against. |
| 6 | **Run rocket**: an unsupported-method probe (PROPFIND) against a real rocket 0.5.x | later | Turns "source reading, not run" (CHANGELOG 0.1.9, design section 8) into a measurement. If it answers 400 as the source says, rocket stays out and the design row says "measured". |
| 7 | `Path`/`Query` extractors | later | Typed access to `PathParams` and the query string. |
| 8 | Mutant for `Connection: close` / `Expect: 100-continue` (case 10 has only a keep-alive mutant) | later | Both are decided below the service, so the mutant has to live in a subject. |
| 9 | `LocalHandler` capability (non-`Send` handlers, actix) | later | Compile-time or startup error elsewhere, never a silent downgrade. |
| 10 | Read ConfigurAnt's API before wiring settings | later | Design section 10; UNVERIFIED. |
| 11 | Replace `PATENT-SWEEP.md` with a real sweep | done (PM to accept) | First sweep of the 0.5.x line at 2d8a670, 7 patents read, none needs review; coverage thin (one pass of web search, no boolean database search): see its Limitations. Re-sweep for h2, ACME handler, client TLS. |
| 12 | Flaky test: `the_whole_suite_passes_on_every_adapter` failed once in a full `cargo test --workspace` on Windows (2026-10-30, at 0.4.0 PR head be84ee9), not reproduced in 5 later runs; failure message not captured | later | Evidence needed: run the workspace tests in a loop under load, keep the output, name the case. |
