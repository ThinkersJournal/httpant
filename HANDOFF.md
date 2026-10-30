# HANDOFF - HTTPAnt lane (current state, rewritten 2026-10-29)

You are the **HTTPAnt lane** (sonnet only). Repo `ThinkersJournal/httpant`. Read `C:\Projects\AGENT-ONBOARDING.md` once. All messages and questions
for CireSnave go to the PM peer (his standing rule, this session). Never merge, publish or allocate version numbers (placeholders only; PM renumbers
with `tools/renumber-version.py`).

## What this is
Library-agnostic HTTP layer: core + server half + client half, web library chosen by id, a conformance suite every adapter must pass, Tower-family
adapters first. Crates claimed on crates.io at 0.0.0: `httpant`, `httpant-core`, `httpant-server`, `httpant-client`. Old `web-server-abstraction`
1.0.2 stays unyanked; its README (this repo) lists known issues. Design: `docs/HTTPANT-DESIGN.md`. Evidence: `docs/OLD-CRATE-SURVEY.md`,
`docs/BENCHMARKS.md`, `docs/ACTIX-ADAPTER-NOTES.md`.

## State
- Merged: CI, survey, README known-issues, benchmarks, design, CODEOWNERS/PATENT-SWEEP (#1-#6); slice 1a (#7), 1b (#8), 2a axum (#9), 2b cases 8-12 + matrix (#10), slice 3 client half + SECURITY.md (#11, 0.1.4).
- **Slice 4 PR open** (branch `feat/actix-adapter`, worktree `C:\Projects\httpant-wt-actix`, 0.1.5 placeholder): actix-web adapter (feature `actix-web`, option C, no actix-tower), cases 1-12 pass on hyper/axum/actix-web. Waiting on CI/gate; check with `gh pr view`.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Trailing slash rule: no normalisation (decided, design section 3).

## Next (in order)
1. After the actix PR merges: remove its worktree + branch (CLAUDE.md section 3). Then salvo, poem, rocket, warp one at a time, only if they pass the suite.
   Read ConfigurAnt's API before wiring settings (design section 10; UNVERIFIED).
2. Open design points: percent-decoding of path params (raw today), request header/body limits in `ServeOptions`, Path/Query extractors,
   a shutdown grace period, a mutant for `Connection: close` / `Expect: 100-continue`, `LocalHandler` capability for actix, TLS acceptor hook.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
