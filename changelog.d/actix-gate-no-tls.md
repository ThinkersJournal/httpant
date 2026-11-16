kind: Internal: the actix-web adapter builds warning-free without `tls`; CI keeps that build
---
`--features actix-web` without `tls` failed `clippy -D warnings` (unused `gate`) because the workspace build always enables `tls` through the conformance crate and so never built that combination. The connection-limit check still runs on both paths; CI now clippy-builds `httpant-server` with `actix-web` alone. Verified: that command fails on origin/main 00cef75 and passes here; all-features clippy and the actix tests pass.

— lane: httpant
