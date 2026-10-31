# HANDOFF - HTTPAnt lane (current state, rewritten 2026-10-31, after #51)

You are the **HTTPAnt lane** (sonnet only). Repo `ThinkersJournal/httpant`. Read `C:\Projects\AGENT-ONBOARDING.md` once. All messages and questions
for CireSnave go to the PM (his standing rule). **claude-peers is retired; the PM and lanes talk on Synapse** (this session loads it; PONG on it after a restart).
Never merge, publish or allocate version numbers (placeholders only; the PM renumbers). Never `gh auth switch`; never run on Opus.

## What this is
Library-agnostic HTTP layer: core + server half + client half, web library chosen by id, a conformance suite every adapter must pass.
`httpant` is the root package (a thin facade); `httpant-core`, `-server`, `-client`, `-conformance`, `-bench` (unpublished) live under `crates/`.
Design: `docs/HTTPANT-DESIGN.md`. Evidence docs are in `docs/` (capability matrix, TLS probes, rocket and actix results, patent sweep, benchmarks, h2c policy).

## State (main `bf70c80`, version 0.5.31 placeholder)
- **Published:** 0.5.19, 0.5.21 (client https), 0.5.22 (private-address guard); tags are the PM's. Everything after is a placeholder, unpublished.
- **Merged since 0.5.22:** 0.5.23 flaky-suite measurement, 0.5.24 patent re-sweep, 0.5.25 actix-web terminates TLS (own acceptor, HTTP/1.1 only), 0.5.26 benchmark
  harness (`docs/BENCHMARKS.md`: .69 warp to .93 axum/poem of the bare library, caveats stated), 0.5.28 extractors (`Path`, `Query`, `PathParams`, `FromRequest`),
  0.5.29 ConfigurAnt read (recommend no dependency; plain serde `ServerSettings` + `Server::apply`, wiring awaits the PM), 0.5.30 `LocalHandler` analysed
  (recommend defer until a consumer asks), 0.5.31 `docs/H2C-POLICY.md` (options A refuse / B per adapter / C opt-in off by default; recommends C after reading
  whether axum, poem, salvo and warp can refuse the preface; not built).
- **Row 4 (TLS):** slices (a) edge mode + trusted proxies and (b) CertSource/acceptor are done (PM accepted, 2026-10-31). Left: warp TLS and actix h2.
  **What unblocks warp TLS:** a warp release that makes its `Accept` trait nameable from outside (`warp::serve(..).incoming(acceptor)` needs it; in 0.4.3 the trait
  sits in a private module, `E0603`, `docs/TLS-ADAPTER-PROBES.md`). Latest warp on crates.io 2026-10-31: 0.4.3 (2026-05-04), i.e. nothing new. Re-run the probe on a new warp.
  **What unblocks actix h2 (row 15):** `actix-http` depending on `h2` >= 0.4.16. Checked 2026-10-31 on crates.io: `actix-http` 3.20.0 (2026-10-25, the latest, already in
  `Cargo.lock`) requires `h2 ^0.3.27` (optional) and `http ^0.2.7`; `h2` itself is at 0.4.20. So still blocked; the check is `cargo tree -i h2` after a new `actix-http`.
- **Waiting on the PM:** decision on row 13 (h2c), row 10 (build settings or not), row 9 (defer or build). Not mine: hub registry flip, publish and tags.
- **Watch:** row 12 (flaky suite; failure message names adapter and cases), row 15 (`cargo tree -i h2` when `actix-http` moves to `h2` >= 0.4.16).
- **Not started, ask the PM first:** client slice 3 (OS root store, CONNECT proxy: the guard would judge the proxy's address; opt-in redirects).
- **First task of the next session:** PM named row 15 next; it is blocked (above), so re-check `actix-http` and `warp` on crates.io, then look for the PM's answer on
  rows 9, 10, 13 (`gh pr list`, Synapse); if none, idle (no `next` row).
- **Lessons:** Windows green is not Linux green. Same-named examples collide (`LNK1104`). Read main's post-merge `CI` on **both** OSes after every merge. Force-push of
  a rebased branch is denied: merge `main` into the branch. A test that predicts a doc claim can disprove it (the `%FF` lossy decode). Conflicts in CHANGELOG/Cargo
  version lines are routine between docs PRs: keep both entries, newest version on top, `cargo metadata --locked --offline` after.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`; after a merge remove worktree and
  branch (`git worktree remove`, `git worktree prune`, `git branch -D`; remote with `gh api -X DELETE`, verify absent with a control).
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder); `Cargo.lock` must match.
- Before every push: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings` (per feature/adapter when changed),
  `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`, `cargo test --locked --no-fail-fast`, `cargo deny check`, publish dry-run, MSRV 1.88 check. Then PR,
  watch CI, count unresolved `reviewThreads`, send `[READY] httpant#N head= merge= checks= red= threads=`. No network text in assertion messages (CodeQL).
- Subagents: pin `model: sonnet`. Scratch files go in the session scratchpad. Use the Write tool for scripts (heredocs with mixed quotes broke once).
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
