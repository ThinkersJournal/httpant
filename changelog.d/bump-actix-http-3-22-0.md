kind: Bump actix-http 3.22.0 (lockfile only)
---
`Cargo.lock` takes actix-http 3.21.0 -> 3.22.0; no manifest or source change. 3.22.0 still requires `h2` ^0.3.27 (optional), so ROADMAP row 15 stays a watch. Verified: clippy -D warnings, workspace tests with all features (no failures), cargo deny.
