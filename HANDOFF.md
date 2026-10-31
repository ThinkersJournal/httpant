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
- **Open PR:** FIRST-RELEASE GATE, restructure PR (branch `feat/first-release-gate`, worktree `C:\Projects\httpant-wt-gate`, 0.5.17 placeholder): `httpant` becomes the ROOT package (PM-approved: "one PR for the restructure, nothing else in it"). Follow-ups, in this order, each its own PR: (2) README hello-world + metadata (readme, keywords, categories, rust-version 1.88, `include`), per-crate README + licence copies, the card `easyants-card.toml` (`example_path = "examples/hello.rs"`, `example_lines = [2, 11]`), the old crate's known-issues text moved to `docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md`; saved drafts are in the session scratchpad `gate-full/` (README.new.md, Cargo.full.toml, KNOWN-ISSUES.moved.md; the README snippet must change to the fmt-stable 10-line example); (3) CI package checks (`cargo publish --workspace --dry-run`), an MSRV job (1.88 default build: measured OK; all adapters need 1.95: salvo 1.94, actix-web 1.89); (4) the fresh-eyes trial of the packaged form (sonnet subagent, only the README), then the ROADMAP gate table and `[READY]` to the PM. Publish order: httpant-core, then httpant-server and httpant-client, then httpant (conformance is publish = false). Also open at the PM: #36 case-10 mutants (0.5.16). The actix spike code is on remote branch `spike/actix-tls` (e799dc8, not for merge; result in `docs/TLS-ADAPTER-PROBES.md`). Merged: TLS first half (#21), sweep (#22, #23), read-back (#24), ACME prefix (#25), 80->443 redirect (#26), audit events (#27), case 17 (#28), poem + salvo TLS (#29), actix result (#30), acme-tls/1 (#31), HTTP/2 first slice (#32), h2c measured (#33), salvo HTTP/2 (#34), rocket measured (#35, 0.5.15).
- **Lesson (salvo):** Windows green is not Linux green. Wait for the ubuntu job before calling an adapter or a limit done.
- **Option A (Tower core, thin adapters) is PROVISIONAL**: PM told me to proceed 2026-10-29 while CireSnave decides A/B/C.
- Rules decided: no trailing-slash normalisation (design section 3); path captures decoded, literals raw (CHANGELOG 0.2.0); limits enforced by HTTPAnt on the request (0.3.0); shutdown grace enforced by a Tower wrapper (0.4.0); forwarded headers believed only from configured trusted proxies (0.5.0).
- WebHostAnt agreed the TLS plan (rustls only, feature `tls`, `CertSource` trait, HTTPAnt-owned acceptor below adapters, actix/warp likely edge-mode only; need 14 with the CertSource slice, need 22 read-back in it). Edge mode (their first integration point) is merged and they were told.
- Publish is the PM's call: the sweep (`docs/PATENT-SWEEP.md`) is accepted as honest and thin; the PM reads what is already on crates.io first.

## Next (in order)
1. After each PR merges: remove its worktree + branch. HTTP/2 is done for every adapter that terminates TLS; row 13 (h2c) is decided by the PM (not uniform, recorded as measured). Next from the ROADMAP (rows 6 and 8 are done): row 12 (flaky suite: loop `the_whole_suite_passes_on_every_adapter` under load and keep the output, name the case), then row 5 (benchmarks, last). Optionally an actix-web adapter through `actix-server`/`actix-http` that runs HTTPAnt's own acceptor (the clean route; not built, `docs/TLS-ADAPTER-PROBES.md`). Run `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps` before every push (CI does; it caught a broken link on #26). Then row 5 (benchmarks); the flaky-suite row needs evidence.

## Standing constraints
- `CARGO_BUILD_JOBS=2`, one build at a time. Primary checkout stays on `main`; every branch in `C:\Projects\httpant-wt-<topic>`.
- No `#[ignore]`. Never weaken `deny.toml`. Every pushed change bumps version + CHANGELOG (placeholder). Cargo.lock must match the version
  (`cargo metadata --locked --offline`).
- **Publish is the PM's decision** (the sweep is accepted as honest and thin). actix-tower is not a hard dependency without a decision.
- Stale files to fix when touched: `CONTRIBUTING.md`, `.github/copilot-instructions.md` still describe the old crate.
