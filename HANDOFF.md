# HANDOFF: httpant lane

State at 2026-11-13, main `e3136df` (0.7.15 stamped by the PM). Current state only; history is in git.

## Held
- Nothing in flight except draft-free PR #81 (ROADMAP rows 16/17 done, docs only). The PM merges and stamps; never merge, publish or edit versions here.
- ROADMAP: no unblocked row. Row 15 (actix over h2) is **watch**; row 9 (`LocalHandler`) deferred until a consumer asks; row 12 (flaky suite) measured.
- Recopy `C:\Projects\tools\fragment-stamp-kit\check-fragment.py` (rejects an empty fragment) in the next kit-touching PR; no hurry.

## WAIT
- WAIT: `bash tools/crate-newer.sh actix-http 3.21.0` (exit 0 = new version): 3.21.0 (read 2026-11-15) still depends on `h2` ^0.3.27, so row 15 stays off; act when a version moves `h2` to >= 0.4.16 (`cargo tree -i h2`).
- WAIT: `bash tools/crate-newer.sh warp 0.4.4` (exit 0 = new version): 0.4.4 (2026-11-13) only fixes SSE line endings and `path_and_query`; act if a release adds an accept-loop or head-timeout hook (connection limit and header timeout are declared No on warp).

## Rules that bit
- One `changelog.d` fragment per PR, no version edit, do not merge main in after the PM stamps; rebuild from a fresh branch off origin/main after a squash-merge (cherry-pick only its own commit; read conflicts, never `git add` a file with markers).
- Worktrees are CRLF on Windows: edit scripts must detect the line ending.
- Before a push: fmt, clippy -D warnings, doc -D warnings, tests, cargo deny, MSRV 1.89. Sign posts `— lane: httpant`.
