kind: dependency refresh: warp 0.4.4, actix-http 3.21.0 (lockfile only)
---
Cargo.lock moves warp 0.4.3 to 0.4.4 (SSE line-ending fix, absent path_and_query normalised) and actix-http 3.20.4 to 3.21.0. No source change; actix-http still pulls h2 0.3 only behind its off `http2` feature, so ROADMAP row 15 is unchanged. Clippy, doc-free tests and cargo deny pass.
