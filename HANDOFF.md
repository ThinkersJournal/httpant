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
- Work list: `ROADMAP.md` (the repo's own, PM's order). Merged: slices 1a-7 (#7-#16; adapters hyper, axum, actix-web, poem, salvo, warp; rocket not built, source reading).
- **Open PRs:** limits (this branch `feat/server-limits`, worktree `C:\\Projects\\httpant-wt-limits`, 0.3.0 placeholder, Breaking: 2 MiB body default).
- **Lesson (salvo):** Windows green is not Linux green. Wait for the ubuntu job before calling an adapter or a limit done.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Rules decided: no trailing-slash normalisation (design section 3); path captures decoded, literals raw (CHANGELOG 0.2.0); limits enforced by HTTPAnt on the request, not by each framework (CHANGELOG 0.3.0).

## Next (in order)
1. After each merge: remove the worktree + branch (CLAUDE.md section 3).
2. `ROADMAP.md` row 3 (grace period), then 4 (TLS: ask the WebHostAnt lane first), then 5.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
