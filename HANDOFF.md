# HANDOFF - HTTPAnt lane (current state, rewritten 2026-11-01, after #56)

You are the **HTTPAnt lane** (sonnet only). Repo `ThinkersJournal/httpant`. Read `C:\Projects\AGENT-ONBOARDING.md` once. All messages and questions
for CireSnave go to the PM (his standing rule). **claude-peers is retired; the PM and lanes talk on Synapse** (this session loads it; PONG on it after a restart).
Never merge, publish or allocate version numbers (placeholders only; the PM renumbers). Never `gh auth switch`; never run on Opus.

## What this is
Library-agnostic HTTP layer: core + server half + client half, web library chosen by id, a conformance suite every adapter must pass.
`httpant` is the root package (a thin facade); `httpant-core`, `-server`, `-client`, `-conformance`, `-bench` (unpublished) live under `crates/`.
Design: `docs/HTTPANT-DESIGN.md`. Evidence docs are in `docs/` (capability matrix, TLS probes, rocket and actix results, patent sweep, benchmarks, h2c policy).

## State (main `315e0ff`, version 0.6.2 placeholder in the HANDOFF PR)
- **Published:** 0.5.19, 0.5.21 (client https), 0.5.22 (private-address guard); tags are the PM's. Everything after is a placeholder, unpublished.
- **Merged since 0.5.22:** actix-web terminates TLS (0.5.25), benchmark harness (0.5.26), extractors (0.5.28), ConfigurAnt read (0.5.29), `LocalHandler` analysed and
  deferred (0.5.30, PM ruled 2026-11-01: defer until a consumer asks), h2c options and step 1 (0.5.31, 0.5.33), `ServerSettings` + `Server::apply` (0.5.34),
  **0.6.0 Breaking: cleartext HTTP/2 off unless `Server::h2c(true)`** (hyper, axum, poem, salvo, actix refuse by default; axum/poem/salvo through a guard on the
  connection's I/O, `preface_guard.rs`; **warp cannot refuse** because its `Accept` is private, declared `h2c_refusable: no` in the matrix; actix-web and a build without
  `http2` answer `h2c(true)` with `Error::H2cNotSupported`; conformance case 19), 0.6.1 `ServerSettings.h2c`.
- **ConfigurAnt end to end (tried 2026-11-01, scratch crate, not in the repo):** `configurant` 0.6.3 (crates.io, `rust-version` 1.95) `Loader::new(&schema).env(map).build()` then
  `config.deserialize::<App>()` with `struct App { server: ServerSettings }` fills it by dotted keys (`server.adapter`, `server.limits.max_body_bytes`, `server.h2c`); a key
  with no default stays `None`; the loader reads the env map you pass, not the process environment; a schema key `ServerSettings` lacks (tried `server.h2c` against 0.6.0)
  is rejected by `deny_unknown_fields` with a message naming it. `h2c = true` needs feature `http2` of `httpant-server` or `bind` fails with `H2cNotSupported`.
- **Waiting on nobody; no `next` row.** Watch rows: 12 (flaky suite), 15 (actix h2 needs `actix-http` on `h2` >= 0.4.16), warp TLS (needs warp to make `Accept` nameable).
  The watch conditions are the `WAIT:` lines below.
- **Not started, ask the PM first:** client slice 3 (OS root store, CONNECT proxy: the guard would judge the proxy's address; opt-in redirects).
- **Lessons:** Windows green is not Linux green. Same-named examples collide (`LNK1104`). Read main's post-merge `CI` on **both** OSes after every merge. Force-push of a
  rebased branch is denied: merge `main` into the branch. A test that predicts a doc claim can disprove it. CHANGELOG/Cargo version conflicts between PRs are routine:
  keep both entries, newest on top, `cargo metadata --locked --offline` after. Python edit scripts: write them with the Write tool, never a heredoc with `\r\n` in it
  (it produced real line breaks inside a Rust byte string once). A pending `Monitor` can die on a network error: loop and retry.

WAIT: actix-http releases a version other than 3.20.0 (row 15) | check: bash tools/crate-newer.sh actix-http 3.20.0 | wake: 2026-11-08T00:00:00Z
WAIT: warp releases a version other than 0.4.3 (warp TLS, row 4) | check: bash tools/crate-newer.sh warp 0.4.3 | wake: 2026-11-08T00:00:00Z

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`; after a merge remove worktree and
  branch (`git worktree remove`, `git worktree prune`, `git branch -D`; remote with `gh api -X DELETE`, verify absent with a control).
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder); `Cargo.lock` must match. A breaking change is the second number.
- Before every push: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings` (per feature/adapter when changed),
  `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`, `cargo test --locked --no-fail-fast`, `cargo deny check`, publish dry-run, MSRV 1.88 check. Then PR,
  watch CI, count unresolved `reviewThreads`, send `[READY] httpant#N head= merge= checks= red= threads=`. No network text in assertion messages (CodeQL).
- Subagents: pin `model: sonnet`. Scratch files go in the session scratchpad. Use the Write tool for scripts.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
