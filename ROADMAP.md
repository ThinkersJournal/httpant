# HTTPAnt roadmap

The repo's own work list (a HANDOFF is current state and is rewritten; work that must not be lost lives here). Order is the PM's, 2026-10-30.
Status: `next` (do now, in this order), `later`, `blocked` (needs a decision or another lane), `done` (kept one release, then deleted).

| # | Row | Status | Notes |
|---|---|---|---|
| 1 | Percent-decoding of path captures | done | merged (#17), 0.2.0 (Breaking). Rule in CHANGELOG 0.2.0. |
| 2 | Limits in `ServeOptions` (target, header count and bytes, body) | done | merged (#18), 0.3.0 (Breaking: 2 MiB body default). Case 14. |
| 3 | Shutdown grace period (a request that never finishes must not hold `shutdown()` forever) | done | 0.4.0 (Breaking). 30 s default; case 15. |
| 4 | TLS: need list from WebHostAnt (WebHostAnt#19, `docs/TLS-NEED-LIST.md`). Slices: (a) edge mode + trusted-proxy scheme/client address (needs 1-3), then ping WebHostAnt; (b) `CertSource` trait, feature `tls`, rustls only, HTTPAnt-owned acceptor below the adapters, served-cert read-back (needs 4-16, 19, 20, 22) with the ACME challenge-prefix handler (14); (c) the rest. | in progress | Slice (a) edge mode done in 0.5.0 (case 16). Slice (b) first half in 0.5.1 (PR open): `CertSource`, `tls` feature, acceptor on hyper+axum, needs 4-12, 15, 16, 19, 20 (tests/tls.rs). ACME challenge prefix (14) done in 0.5.5 (PR open). Read-back (22) done in 0.5.4. 80->443 redirect (18) done in 0.5.6 (PR open). Audit events (23) done in 0.5.7 (no rebind event because there is no rebind operation). TLS conformance case 17 done in 0.5.8 (PR open). TLS probes of the four libraries done in 0.5.9 (PR open, `docs/TLS-ADAPTER-PROBES.md`): poem and salvo terminate TLS (case 17 proves it); warp (private `Accept` trait, run) and actix-web (run as a spike, 0.5.10: bridgeable only by blocking its worker, 2035 ms vs 7 ms, and no audit events) are edge mode only. `acme-tls/1` (13) done in 0.5.11 (PR open). `h2` first slice done in 0.5.12 (PR open): hyper, axum, poem; case 18. salvo done in 0.5.14 (PR open): every adapter that terminates TLS serves h2. Left: actix-web and warp if they ever terminate TLS. HTTPAnt must not become a TLS stack (no ACME, storage, renewal). |
| 5 | Benchmark harness (adapter vs the bare framework) | later | Last. `docs/BENCHMARKS.md` has the TechEmpower reading to compare against. |
| 6 | **Run rocket**: an unsupported-method probe (PROPFIND) against a real rocket 0.5.x | done | Measured 2026-10-31 (0.5.15, `docs/ROCKET-PROBE.md`): rocket 0.5.2 answers `PROPFIND` and a made-up method `400` and a known method with no route `404`. The source reading holds; rocket stays out and the design row says "measured". |
| 7 | `Path`/`Query` extractors | later | Typed access to `PathParams` and the query string. |
| 8 | Mutant for `Connection: close` / `Expect: 100-continue` (case 10 had only a keep-alive mutant) | done | 0.5.16: `ignore-connection-close` and `skip-expect-continue`, subjects that strip the header line below the HTTP/1.1 layer (`mutants::serve_stripping`); each fails case 10 with its own message (tested). |
| 9 | `LocalHandler` capability (non-`Send` handlers, actix) | later | Compile-time or startup error elsewhere, never a silent downgrade. |
| 10 | Read ConfigurAnt's API before wiring settings | later | Design section 10; UNVERIFIED. |
| 11 | Replace `PATENT-SWEEP.md` with a real sweep | done (PM to accept) | First sweep of the 0.5.x line at 2d8a670, 7 patents read, none needs review; coverage thin (one pass of web search, no boolean database search): see its Limitations. Re-sweep for h2, ACME handler, client TLS. |
| 12 | Flaky test: `the_whole_suite_passes_on_every_adapter` failed once in a full `cargo test --workspace` on Windows (2026-10-30, at 0.4.0 PR head be84ee9), not reproduced in 5 later runs; failure message not captured | later | Evidence needed: run the workspace tests in a loop under load, keep the output, name the case. |
| 13 | Decide the cleartext HTTP/2 (h2c) policy | later | Found with the HTTP/2 slice (0.5.12) and now **measured for all six adapters** (0.5.13, `docs/CAPABILITY-MATRIX.md`): axum, poem, salvo and warp accept the h2c prior-knowledge preface on a cleartext listener (salvo since 0.5.14, when its `http2` feature came on); hyper (`http1_only`) and actix-web do not. PM 2026-10-31: not uniform is acceptable because the need does not constrain it; the difference is recorded, not hidden. Conformance does not constrain it. Decide: allow everywhere (edges can speak h2c to an origin) or refuse everywhere, then add a case. |

## First-release gate (EXPECTATIONS 6.24: easy is the product)

Written 2026-10-31 for the publish decision (PM [TASK]). "Done" means done once the two PRs named in the last column have merged: the restructure
(#37) and the packaging PR that carries this table. Nothing is published; crates.io holds only the 0.0.0 placeholders.

| # | gate item | state | evidence |
|---|---|---|---|
| G1 | A crate called `httpant` that a stranger can depend on | **done** (it did not exist: the name was claimed, the workspace had no crate by it) | #37: `httpant` is the root package, a thin facade; the four member crates keep their names and paths |
| G2 | README opens with a hello-world a stranger can copy (about ten lines) | **done** (the README was the old crate's known-issues page; that text moved to `docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md`) | `README.md`: ten lines, then install, choosing the library, what you get, **what it does not do yet** |
| G3 | The README's examples are compiled in CI, and the example is **run** | **done** | both README examples are rustdoc doctests (`cargo test`); `examples/hello.rs` is run by `cargo run --example hello -p httpant` and `-p httpant-server` in CI; `crates/httpant-conformance/tests/readme_example.rs` keeps README, example and hub card the same program (checked by breaking one) |
| G4 | Fresh-eyes trial of the **published shape** (an agent given only the README reaches the example's result) | **done, passed** (3 of 3 tasks first attempt, no errors or warnings) | `docs/FRESH-EYES-TRIAL.md`: the packaged crates behind an offline directory source, `cargo add httpant` as the README says, the agent's report quoted; it found a **wrong claim in our README** (two different errors were described as one), now fixed; its limits are stated there. Not re-run after the wording fixes |
| G5 | What the package contains is right | **done** | root `httpant` ships exactly 10 files (`include`; it was the whole repository); each member crate ships its README and both licences; `readme`, `keywords`, `categories`, `rust-version` on all four |
| G6 | The packaged crates build from their tarballs, in order | **done, and kept by CI** | `cargo publish --workspace --dry-run --locked` in CI (Linux); verified locally: all four build from their own tarballs |
| G7 | Minimum Rust version is a claim that is checked | **done** | `rust-version = "1.88"`, kept by the `msrv` CI job on the default build. Measured: the optional adapters ask for more (actix-web 1.89, salvo 1.94, every feature 1.95); cargo names the package, the README says so |
| G8 | Hub card ready | **done in this repository; the hub entry is not flipped** | `easyants-card.toml` (status `ready`, `cargo add httpant`, `examples/hello.rs` lines 2 to 11). The hub's registry entry for HTTPAnt has no `repo`, so nothing reads the card yet: at release it needs `repo = "ThinkersJournal/httpant"` and `card = "required"` (a change in the easyants repository, not here). Found while doing this: the hub only accepts `example_path` matching `^(examples\|tests)/` **at the repository root**, which a virtual-workspace Ant cannot offer; HTTPAnt became a root package for that reason |
| G9 | Errors say what to do next | **done earlier** (every `Error` message ends with it; tested) | `every_error_variant_has_a_status_and_ends_with_advice` |
| G10 | The 80% case has one obvious path, safe defaults, an escape hatch | **done earlier** | defaults: 8 KiB target, 64 headers / 16 KiB, 2 MiB body, 30 s shutdown grace; each one call to change (`Limits`, `shutdown_grace`) |
| G11 | A patent sweep that the PM accepted | **done** | `docs/PATENT-SWEEP.md`, accepted by the PM 2026-10-30 as honest and thin |

**Publish order** (what `cargo publish --workspace` computes, and what the dry run follows): `httpant-core`, then `httpant-client`, then
`httpant-server`, then `httpant`. `httpant-server` has `httpant-client` as a dev-dependency, which is why the client goes first. `httpant-conformance`
is `publish = false` and stays out. All four published names are already claimed at 0.0.0.

**What the PM still decides** (not a gap in the work): the version number the first real release takes (the repository carries `0.5.x`
placeholders, renumbered by the PM; the README promises "a breaking change bumps the second number"); the publish itself and its credentials; the
tag `v<version>` on the exact commit, pushed with the publish (`tools/tag-release.sh`); and the hub registry change above.

**For configurant row 29** (waits on a published HTTPAnt): after the restructure and the packaging PR merge, nothing technical blocks a publish.
What stands between it and a published HTTPAnt is the PM's publish decision, then the version, the order above, and the hub entry. Before this
work the real blockers were G1 (no `httpant` crate), G2 (no hello-world in the README) and G5 (a package that would have shipped the whole
repository).

**Honest limits the README states** (so the first release is not oversold): the client speaks plain `http://` only; TLS termination is in hyper,
axum, poem and salvo (actix-web and warp serve behind an edge); rocket is not supported (measured); cleartext HTTP/2 is not uniform across the
libraries; the optional adapters can need a newer compiler than the default build.
