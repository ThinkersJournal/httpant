# HANDOFF: httpant lane

State at 2026-11-13, main `e3136df` (0.7.15 stamped by the PM). Current state only; history is in git.

## Held
- Nothing in flight except draft-free PR #81 (ROADMAP rows 16/17 done, docs only). The PM merges and stamps; never merge, publish or edit versions here.
- ROADMAP: no unblocked row. Row 15 (actix over h2) is **watch**; row 9 (`LocalHandler`) deferred until a consumer asks; row 12 (flaky suite) measured.
- Recopy `C:\Projects\tools\fragment-stamp-kit\check-fragment.py` (rejects an empty fragment) in the next kit-touching PR; no hurry.

## WAIT
- WAIT cron `c66f0d43`: on Nov 15 check `actix-http` (3.21.0 changes the h2 WAIT, row 15) and warp for a accept-loop/timeout hook (declared No for connection limit; axum/poem/warp No for header timeout).

## Rules that bit
- One `changelog.d` fragment per PR, no version edit, do not merge main in after the PM stamps; rebuild from a fresh branch off origin/main after a squash-merge (cherry-pick only its own commit; read conflicts, never `git add` a file with markers).
- Worktrees are CRLF on Windows: edit scripts must detect the line ending.
- Before a push: fmt, clippy -D warnings, doc -D warnings, tests, cargo deny, MSRV 1.89. Sign posts `— lane: httpant`.
