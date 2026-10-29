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
- Merged: CI (#2), survey (#1), README known-issues (#3), benchmarks + actix notes (#4), design (#5), CODEOWNERS + PATENT-SWEEP (#6).
- Merged: slice 1a (#7, 0.1.0): `httpant-core` types, conformance skeleton, old src tree deleted; `cargo deny` is a required check.
- Merged: slice 1b (#8, 0.1.1): Router/Handler/layers, `httpant-server` (Server, ServerAdapter, HyperAdapter), conformance cases 1-7 with 9 mutants, hello example in CI.
- **Slice 2a PR open** (branch `feat/axum-adapter`, worktree `C:\Projects\httpant-wt-axum`, 0.1.2 placeholder): axum adapter behind feature `axum`; cases 1-7 pass on hyper and axum.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C. If he picks B or C, 1a is cheap to redo.
- Trailing slash rule: no normalisation (decided, design section 3).

## Next (in order)
1. Slice 2b: cases 8-12 (streaming, shutdown, keep-alive, concurrency, HEAD) on hyper and axum, first capability matrix (design section 8).
   Read ConfigurAnt's API before wiring settings (design section 10; not yet read, UNVERIFIED).
2. Slice 3 client half (hyper-util adapter, client conformance, then the real nine-line hello with `Client`); then actix (see ACTIX notes), others only if they pass.
3. Open design points: percent-decoding of path params (raw today), request header/body limits in `ServeOptions`, Path/Query extractors.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
