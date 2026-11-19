kind: Test: the 30 s shutdown wait, run
---
`readme_claims.rs` gains a test that waits out the default shutdown grace for real: a handler that never finishes holds one request on each of the six adapters (run at once, one 30 s wait); `shutdown()` returns after 29 to 40 s and the client is answered `503`. No library change. Passed on all six at `c5a8bde`.
