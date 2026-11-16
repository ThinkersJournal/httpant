# HANDOFF: httpant lane

State at 2026-11-16, main `7890b6c` (0.7.19 area; the PM stamps). Current state only; history is in git.

## Held
- **PR #84** (coderipper 0.47.5 pin move, PIN-MOVE-0.47.5): pushed, [READY] sent, Portfolio job read (no findings, 36/47 rules ran; gaps reported). Waiting for the PM to stamp and merge. After merge: remove worktree `httpant-wt-pin` and branch `ci/pin-coderipper-0.47.5`, verify the remote branch absent with a control. Never merge, publish or edit versions.
- ROADMAP: no unblocked row. Row 15 (actix over h2) **watch**; row 12 (flaky `the_whole_suite_passes_on_every_adapter`) **watch**: first captured failure was case 11 on hyper, 8 s deadline (`live.rs`); if it recurs raise to 15 s and log unanswered; row 9 (`LocalHandler`) deferred. Remote `spike/actix-tls` kept on purpose (cited by CHANGELOG and TLS-ADAPTER-PROBES).
- Recopy `C:/Projects/tools/fragment-stamp-kit/check-fragment.py` in the next kit-touching PR; no hurry.

## WAIT
- WAIT: `bash tools/crate-newer.sh actix-http 3.21.0` (exit 0 = new version): 3.21.0 (read 2026-11-15) still depends on `h2` ^0.3.27, so row 15 stays off; act when a version moves `h2` to >= 0.4.16 (`cargo tree -i h2`).
- WAIT: `bash tools/crate-newer.sh warp 0.4.4` (exit 0 = new version): 0.4.4 (2026-11-13) only fixes SSE line endings and `path_and_query`; act if a release adds an accept-loop or head-timeout hook (connection limit and header timeout are declared No on warp).

## Rules that bit
- One `changelog.d` fragment per PR, no version edit, do not merge main in after the PM stamps; rebuild from a fresh branch off origin/main after a squash-merge (cherry-pick only its own commit; read conflicts, never `git add` a file with markers).
- Worktrees are CRLF on Windows: edit scripts must detect the line ending.
- Before a push: fmt, clippy -D warnings, doc -D warnings, tests, cargo deny, MSRV 1.89. Sign posts `— lane: httpant`.
