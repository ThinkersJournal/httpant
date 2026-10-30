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
- Work list: `ROADMAP.md` (the repo's own, PM's order). Merged: slices 1a-7, percent-decoding (#17, 0.2.0), limits (#18, 0.3.0), shutdown grace (#19, 0.4.0).
- **Open PR:** #23 patent sweep addendum (branch `docs/patent-sweep-2`, worktree `C:\Projects\httpant-wt-sweep2`, 0.5.3 placeholder, docs only; PM ruling 2026-10-30). Merged: TLS first half (#21), sweep (#22).
- **Lesson (salvo):** Windows green is not Linux green. Wait for the ubuntu job before calling an adapter or a limit done.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Rules decided: no trailing-slash normalisation (design section 3); path captures decoded, literals raw (CHANGELOG 0.2.0); limits enforced by HTTPAnt on the request (0.3.0); shutdown grace enforced by a Tower wrapper (0.4.0); forwarded headers believed only from configured trusted proxies (0.5.0).
- WebHostAnt agreed the TLS plan (rustls only, feature `tls`, `CertSource` trait, HTTPAnt-owned acceptor below adapters, actix/warp likely edge-mode only; need 14 with the CertSource slice, need 22 read-back in it). Edge mode (their first integration point) is merged and they were told.
- Publish waits on the PM accepting `docs/PATENT-SWEEP.md` (thin, AI-assisted, no classification-code search; ROADMAP row 11).

## Next (in order)
1. After #23 merges: remove its worktree + branch. Then ROADMAP row 4 rest: served-cert read-back (22), ACME challenge-prefix handler (14), `acme-tls/1` (13), 80->443 (18), events (23), conformance case. Probe actix/poem/salvo/warp for real TLS ability (their `No` is recalled, not run). Then row 5 (benchmarks); the flaky-suite row needs evidence.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **No publish until `docs/PATENT-SWEEP.md` is a real sweep** (it says NOT YET PERFORMED). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
