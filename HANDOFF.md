# HANDOFF - HTTPAnt lane (current state, rewritten 2026-10-31, after #46)

You are the **HTTPAnt lane** (sonnet only). Repo `ThinkersJournal/httpant`. Read `C:\Projects\AGENT-ONBOARDING.md` once. All messages and questions
for CireSnave go to the PM peer (his standing rule). Never merge, publish or allocate version numbers (placeholders only; the PM renumbers with
`tools/renumber-version.py`). Never `gh auth switch`; never run on Opus.

## What this is
Library-agnostic HTTP layer: core + server half + client half, web library chosen by id, a conformance suite every adapter must pass.
`httpant` is the root package (a thin facade); `httpant-core`, `-server`, `-client`, `-conformance` live under `crates/`. **0.5.19 is published**
(httpant-core, -client, -server, httpant; tag `v0.5.19` on main `1e926bd`, PM, 2026-10-31). Design: `docs/HTTPANT-DESIGN.md`. Evidence docs are in
`docs/` (capability matrix, TLS probes, rocket and actix results, patent sweep, fresh-eyes trial).

## State
- Work list: `ROADMAP.md` (PM's order). The first-release gate (G1 to G11) is done and published. Open at the PM: the hub registry flip
  (`repo = "ThinkersJournal/httpant"`, `card = "required"` in the easyants repository) goes to CireSnave via the board.
- **Published:** 0.5.19 (first release), 0.5.21 (client https, verified; feature `https`), 0.5.22 (private-address guard, `refuse_private_addresses`), all four crates, tags `v0.5.19`, `v0.5.21`, `v0.5.22` (PM). WebHostAnt's Cloudflare REST calls and health probe can now use them; their needs are WebHostAnt PR 20 (`docs/HTTPS-CLIENT-NEEDS.md`). Design: `docs/HTTPS-CLIENT-DESIGN.md`.
- **Not started, no PM ruling, none blocks WebHostAnt:** client slice 3 (OS root store opt-in, proxy with CONNECT, opt-in redirects; the proxy slice must say the guard would then judge the proxy's address). Ask the PM before starting.
- **Merged since 0.5.22 (placeholders, not published):** 0.5.23 flaky-suite measurement, 0.5.24 patent re-sweep record (`docs/PATENT-SWEEP.md`, PM GO), 0.5.25 actix-web terminates TLS through HTTPAnt's own acceptor (HTTP/1.1 only), 0.5.26 benchmark harness (`crates/httpant-bench`, unpublished; table and caveats in `docs/BENCHMARKS.md`: HTTPAnt served .69 (warp) to .93 (axum, poem) of the bare library's req/s; the load generator may be the limit, spread not kept).
- **Roadmap is empty of `next` rows.** Watch rows: 12 (flaky suite: measured, not reproduced; the failure message names adapter and cases), 15 (actix over HTTP/2 when `actix-http` moves to `h2` >= 0.4.16; `cargo tree -i h2`), 13 (h2c decided: not uniform, recorded as measured). Later rows 7 (`Path`/`Query` extractors), 9 (`LocalHandler`), 10 (ConfigurAnt) need the PM to order them. Optional follow-ups to row 5: a stronger load generator, more rounds with spread, a route with a real handler.
- **Option A (Tower core, thin adapters) is provisional**: CireSnave has not decided A/B/C.
- **Lessons:** Windows green is not Linux green (salvo). Two examples with the same name in one workspace write the same `target/.../examples/<name>.exe`
  and race on Windows (`LNK1104`; fixed in 0.5.19 by `server_hello`); read cargo's `output filename collision` warning. Read main's post-merge CI on
  **both** OSes after every merge. A force-push of a rebased branch is denied by the classifier: push the rebased branch under a new name.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`; remove worktree
  and branch after a merge (`git worktree remove`, `git worktree prune`, `git branch -D`; the remote branch with `gh api -X DELETE`, verify absent).
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder); `Cargo.lock` must match
  (`cargo metadata --locked --offline`).
- Before every push: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings` (also per feature and per adapter when they changed),
  `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`, `cargo test --locked --no-fail-fast`. Then PR, watch CI, count unresolved review threads
  by GraphQL, send `[READY] httpant#N head=... checks=... threads=...`. Do not print network text in test assertion messages (CodeQL).
- Subagents: pin `model: sonnet`. Scratch files go in the session scratchpad, not `rm`.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
