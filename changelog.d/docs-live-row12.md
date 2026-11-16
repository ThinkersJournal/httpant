kind: Conformance case 11 deadline 15 s, README case counts, ROADMAP row 12
---
The case-11 deadline in `httpant-conformance` `live.rs` goes from 8 s to 15 s (the serialising mutant still needs 20 s) and a timeout now says how many of the 1,000 requests were unanswered and the first ten; row 12 captured the 8 s deadline failing twice on hyper under load. README says 23 server and 9 client cases (was 18). Verified: fmt, clippy -D warnings, the conformance tests.
