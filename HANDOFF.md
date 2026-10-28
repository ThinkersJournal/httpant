# HANDOFF - HTTPAnt lane (current state, written by the PM 2026-10-28)

You are the **HTTPAnt lane**. Read `C:\Projects\AGENT-ONBOARDING.md` once. Messages: Synapse and claude-peers (PM role `pm`). Models: sonnet only.
Repo: `ThinkersJournal/httpant` (was `ciresnave/web-server-abstraction`, transferred and renamed 2026-10-28; GitHub redirects the old URL).

## What this is and why now
The portfolio's library-agnostic HTTP layer: **core + server half + client half**, the web library chosen by id behind one interface, a tested
adapter conformance suite, Tower-family adapters first. Crate names claimed on crates.io at 0.0.0: `httpant`, `httpant-core`, `httpant-server`,
`httpant-client`. The old crate `web-server-abstraction` 1.0.2 (published 2025-08-27) is end-of-lifed with a README pointer (no yank) once HTTPAnt can replace it.
**CireSnave 2026-10-28 (first-hand):** *"WebServAnt would sit nicely atop that. I agree. Are we starting HTTPAnt now? If not, why not? HTTP is a
well established protocol that we should be able to define a crate for relatively easily."* and, on WebHostAnt, *"Caddy is a web server. Does it
support our goals of web server library agnosticism? ... That's why I pointed you toward web-server-abstraction in the past."*
Consumers waiting on you: WebHostAnt (in-process TLS and the edge interface; its own lane started today), MCPAnt, SecretAnt's web page, AlarmAnt,
fmp-rs, lightbulb, the Humboldt installer's local wizard. A later **WebServAnt** (a finished library-agnostic web server program) will sit on top.

## What exists
The old single crate: `src/` with adapters for axum (default), actix-web, rocket, warp, salvo, poem; `DESIGN.md`; `tests/`; `examples/`. Cargo.toml
says several adapters "may have limited functionality" after framework upgrades. **No CI and no branch protection exist: your first PR adds CI
(fmt, clippy -D warnings, build+test on ubuntu and windows, cargo deny); I set protection after its first run.**

## Tasks, in order
1. Survey the existing crate (read, do not run builds until CI exists): what is sound, what to reuse, what to discard; ten lines max per adapter.
2. `docs/HTTPANT-DESIGN.md`, hello-world FIRST (EXPECTATIONS 6.24): a stranger serves "hello" and fetches it in <= 10 lines. Then core types
   (request, response, headers, streaming body, handler, errors), the server half, the client half, adapter selection by id, the conformance suite
   (every adapter must pass the same tests, each with a known-bad control), what each adapter can NOT do (websockets, streaming, graceful shutdown),
   TLS as a capability (WebHostAnt owns certificates; you expose the hook), middleware/Tower story, migration from web-server-abstraction.
   Mark every unverified fact. Options A/B/C with a recommendation for CireSnave, plain sentences.
3. First code slice after the design merges: core types + conformance harness + ONE adapter (axum/hyper) + the ten-line example in CI.
Ant-first: use ConfigurAnt/PersistAnt/CapAnt where they fit; the other Ants are 0.0.0 placeholders: stub behind a trait and say so. One PR at a
time, red-first, version bump every PR (the package becomes `httpant`; PM allocates numbers). Memory: CARGO_BUILD_JOBS=2, one build at a time.
Patent sweep before any publish (HTTP middleware/routing are crowded fields): queries into `PATENT-SWEEP.md`, claim nothing clear.

## State
main = old 1.0.2 content + this HANDOFF (1.0.3, docs only, not published). No open PRs or issues. No lane existed before this one.
