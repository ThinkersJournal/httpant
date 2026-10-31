# HANDOFF - HTTPAnt lane (current state, rewritten 2026-10-31)

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
- **Current TASK (PM, 2026-10-31):** client https. The client refuses `https://` today, which blocks WebHostAnt (Cloudflare API, health probes).
  Done in the design-note PR: ROADMAP row 14 and `docs/HTTPS-CLIENT-DESIGN.md` (options + recommendation: rustls/ring, bundled roots default, OS
  store opt-in, feature `https`, private-address guard in the resolver plus a literal check plus a post-connect backstop). **Stop at the PM's
  [READY]**; the implementation PRs come after the PM decides the four questions at the end of the note. WebHostAnt's needs: WebHostAnt PR 20,
  `docs/HTTPS-CLIENT-NEEDS.md` (13 needs).
- **Parked, unblocked, start when the TASK is handed over:** ROADMAP row 12, the flaky `the_whole_suite_passes_on_every_adapter` (failed once on
  Windows, not reproduced; the assertion message already names the adapter and the failed cases). Plan: build the test binary once and run just that
  test in a loop under CPU load, keep every failing output. Then row 5 (benchmarks, last). Optional: a clean actix adapter through
  `actix-server`/`actix-http` running HTTPAnt's own acceptor (`docs/TLS-ADAPTER-PROBES.md`). Row 13 (h2c) is decided: not uniform, recorded as measured.
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
