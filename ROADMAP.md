# HTTPAnt roadmap

The repo's own work list (a HANDOFF is current state and is rewritten; work that must not be lost lives here). Order is the PM's, 2026-10-30.
Status: `next` (do now, in this order), `later`, `blocked` (needs a decision or another lane), `done` (kept one release, then deleted).

| # | Row | Status | Notes |
|---|---|---|---|
| 1 | Percent-decoding of path captures | done | merged (#17), 0.2.0 (Breaking). Rule in CHANGELOG 0.2.0. |
| 2 | Limits in `ServeOptions` (target, header count and bytes, body) | done | merged (#18), 0.3.0 (Breaking: 2 MiB body default). Case 14. |
| 3 | Shutdown grace period (a request that never finishes must not hold `shutdown()` forever) | done | 0.4.0 (Breaking). 30 s default; case 15. |
| 4 | TLS hook with WebHostAnt | blocked | Ask the WebHostAnt lane what it needs first. HTTPAnt must not become a TLS stack: the hook takes an acceptor, never certificates. Client `https://` is `UnsupportedScheme` until it exists. |
| 5 | Benchmark harness (adapter vs the bare framework) | later | Last. `docs/BENCHMARKS.md` has the TechEmpower reading to compare against. |
| 6 | **Run rocket**: an unsupported-method probe (PROPFIND) against a real rocket 0.5.x | later | Turns "source reading, not run" (CHANGELOG 0.1.9, design section 8) into a measurement. If it answers 400 as the source says, rocket stays out and the design row says "measured". |
| 7 | `Path`/`Query` extractors | later | Typed access to `PathParams` and the query string. |
| 8 | Mutant for `Connection: close` / `Expect: 100-continue` (case 10 has only a keep-alive mutant) | later | Both are decided below the service, so the mutant has to live in a subject. |
| 9 | `LocalHandler` capability (non-`Send` handlers, actix) | later | Compile-time or startup error elsewhere, never a silent downgrade. |
| 10 | Read ConfigurAnt's API before wiring settings | later | Design section 10; UNVERIFIED. |
| 11 | Replace `PATENT-SWEEP.md` with a real sweep | blocked | Nothing is published until it is done (`docs/PATENT-SWEEP.md`). |
