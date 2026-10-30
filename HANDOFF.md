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
- Merged: slices 1a-6 and #15 (#7-#15): adapters hyper, axum, actix-web (option C, no actix-tower), poem, salvo; client half; SEC-002 allow.
- **Slice 7 PR open** (branch `feat/warp-adapter`, worktree `C:\Projects\httpant-wt-warp`, 0.1.9 placeholder): warp adapter, cases 1-12 pass; rocket NOT built (source reading: rocket 0.5.2 rejects extension methods with 400; recorded in design section 8). Check with `gh pr view`.
- **Lesson (salvo):** Windows green is not Linux green. A framework that holds its listener open while draining passes on Windows and hangs a probe connect on Linux; only CI shows it. Wait for the ubuntu job before calling an adapter done.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Trailing slash rule: no normalisation (decided, design section 3).

## Next (in order)
1. After the warp PR merges: remove its worktree + branch (CLAUDE.md section 3). Roadmap's adapter list is then done (rocket excluded, with reason); next is a PM decision: TLS acceptor hook with WebHostAnt, benchmark harness, or the open design points.
   Read ConfigurAnt's API before wiring settings (design section 10; UNVERIFIED).
2. Open design points: percent-decoding of path params (raw today), request header/body limits in `ServeOptions`, Path/Query extractors,
   a shutdown grace period, a mutant for `Connection: close` / `Expect: 100-continue`, `LocalHandler` capability for actix, TLS acceptor hook.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
