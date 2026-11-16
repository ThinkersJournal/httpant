kind: Internal: break two module cycles (no behaviour change)
---
The conformance crate's `app`, `subject` and `mutants` modules no longer import each other in a ring (the shared `BODY_LIMIT` moved to a new `consts` module), and the actix adapter's `handle` is generic over the in-flight guard it holds, so `actix_adapter` no longer names `actix_tls`. Verified: fmt, clippy, tests with `tls` and without.

— lane: httpant
