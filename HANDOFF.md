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
- Merged: slices 1a-6, #15 (#7-#15). **Open PRs:** #16 warp adapter (0.1.9, in the PM's gate); percent-decoding (this branch `feat/percent-decoding`, worktree `C:\\Projects\\httpant-wt-pct`, 0.2.0 placeholder, Breaking: `PathParams::get` decodes).
- PM order (2026-10-30), one PR per item: (1) safety defaults: percent-decoding (done, in PR), then **limits in `ServeOptions`** (header / body / URI sizes, conservative defaults, a test per limit that fails when the limit is removed), then a **shutdown grace period**; (2) TLS hook with WebHostAnt (ask peer `p362rz26` what they need; HTTPAnt must not become a TLS stack); (3) benchmark harness last.
- **ROADMAP row to add (PM asked):** actually RUN rocket: an unsupported-method (PROPFIND) probe against a real rocket 0.5.x, to turn the "source reading, not run" in CHANGELOG + design section 8 into a measurement. The PM owns `PORTFOLIO-ROADMAP.md`; this is the request text.
- **Lesson (salvo):** Windows green is not Linux green. Wait for the ubuntu job before calling an adapter or a limit done.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Trailing slash rule: no normalisation (decided, design section 3). Path rule: captures decoded, literals raw (design/CHANGELOG 0.2.0).

## Next (in order)
1. Merge-waiters: remove worktrees + branches after each merge (CLAUDE.md section 3).
2. Limits in `ServeOptions`: per adapter, which can enforce header-count/size, URI length, body size at the server (some frameworks enforce their own; measure it, add a conformance case, mutants, matrix column). Then the grace period. Then TLS (ask `p362rz26`). Then benchmarks.
   Read ConfigurAnt's API before wiring settings (design section 10; UNVERIFIED).
3. Other open points: Path/Query extractors, a mutant for `Connection: close` / `Expect: 100-continue`, `LocalHandler` capability for actix.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
