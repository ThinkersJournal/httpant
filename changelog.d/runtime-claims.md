kind: Test: the 30 s shutdown wait and certificate revocation, run
---
`readme_claims.rs` gains a test that waits out the default shutdown grace for real: a handler that never finishes holds one request on each of the six adapters (run at once, one 30 s wait); `shutdown()` returns after 29 to 40 s and the client is answered `503`. No library change. Passed on all six at `c5a8bde`.
Also `client_revocation.rs`: a private CA issues a leaf for 127.0.0.1 and revokes it in a CRL; a rustls verifier given the CRL refuses the leaf (control; without the CRL it accepts), and the HTTPAnt client, given the CA, still connects, as the README says (`rustls does not check revocation`). No library change.
