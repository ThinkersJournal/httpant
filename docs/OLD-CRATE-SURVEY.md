# Survey of the old `web-server-abstraction` 1.0.3 crate

Read at `origin/main` 1d53fad (read only; nothing was built or run). Method: read `Cargo.toml`, `src/core.rs`,
`src/adapters/axum.rs` in full; grep of the other five adapters for fallbacks, `println!`, middleware, `unwrap_or_default`;
`src/types.rs` struct definitions. **Everything not listed as read is unverified.** The grep is a pattern probe, not a read of each file.

## Verdict

Discard the code, keep the ideas. The shape (one handler type, adapters by feature, a mock adapter for tests) is right.
The implementation loses information at the adapter boundary in every adapter I looked at, and the types cannot express
what HTTP needs. HTTPAnt's core should be built on the `http` crate's types, which the old crate already depends on but wraps.

## Core (`src/types.rs`, `src/core.rs`)

- `Headers` is `HashMap<String, String>` (types.rs:681). Repeated headers (`Set-Cookie`) cannot be represented. Discard.
- `StatusCode(pub u16)` (types.rs:641) duplicates `http::StatusCode`. Discard.
- `HttpMethod` is a closed enum; every adapter maps unknown methods to GET (axum.rs:149, actix_web.rs:308, warp.rs:111,
  salvo.rs:207, poem.rs:280). A `PROPFIND` request is silently served as a GET. Discard.
- `Body` wraps a single `Bytes` (types.rs:757): no streaming. Discard.
- `Request.extensions` is `HashMap<String,String>` ("Simplified for now", types.rs:19). `http::Extensions` is the real thing.
- `Middleware`/`Next` (core.rs:18-36) takes the handler by value; reusable idea, but the type is not Tower-compatible.
- `Route::match_dynamic_path` (core.rs:97): a hand-rolled router. Reuse the idea of `:param`/`*wild`; do not reuse the code.
- `Cargo.toml` pulls `auth-framework`, `sqlx`, `rustls`, `flate2`, `sha1`, `uuid`, `serde_yaml`, `libc`, `regex`, `rand`
  unconditionally or near it. Auth, sessions, database, FFI, benchmarks, security and static-file modules (~5k lines in `src/`)
  are out of scope for a transport layer. Discard from the core; revisit as separate crates only if a consumer asks.

## Adapters (ten lines each, max)

**axum** (259 lines, read in full)
- Sound: request/response conversion shape; `Router` composition; `axum::serve` for running.
- Bug: response status is matched against 8 codes, everything else becomes 200 (axum.rs:205-215). A 418 or 503 reaches the client as 200.
- Bug: non-UTF8 header values are silently dropped on the request (axum.rs:158); response headers that fail to parse are dropped.
- Bug: `middleware` is stored and never applied (axum.rs:94 says so in a comment). `bind` only records the address.
- Bug: body fully buffered with `usize::MAX` limit (axum.rs:164): unbounded memory, no streaming.
- `path_params` is always empty (axum.rs:171).
- Reuse: nothing verbatim. Becomes the first HTTPAnt adapter, rewritten on `http::Request<Body>` which axum already uses.

**actix-web** (471 lines; grep only)
- Unknown methods route as GET (actix_web.rs:203-215). Status/middleware handling not read.
- Uses its own `Logger`; custom middleware stored, integration not read. Verdict: rewrite or drop. Actix is not Tower-based
  and its types are `!Send`, so the conformance suite will show whether it fits.

**warp** (242 lines; grep only)
- Status mapped through 5 codes, else 200 (warp.rs:138-146). Response body via `body_string`, so binary bodies are suspect.
- Middleware stored, not applied (warp.rs:85-87). Verdict: rewrite or drop. Warp 0.4 is a thin hyper wrapper; low value.

**rocket** (423 lines; grep only)
- Middleware is a fairing that prints "Processing request through middleware" (rocket.rs:274,283); not a chain. Verdict: rewrite or drop.

**salvo** (325 lines; grep only)
- Own `println!("Warning: TRACE/CONNECT methods not fully supported")` (salvo.rs:103); middleware "registered but not yet fully integrated" (salvo.rs:110).

**poem** (474 lines; grep only)
- Same "not yet fully integrated" message (poem.rs:127); wrapper loop that only prints (poem.rs:251-254).

All five non-axum adapters claim "full request/response conversion, middleware integration" in their module docs
(line 4 of actix_web, rocket, salvo, poem). The grep contradicts the middleware claim for poem, salvo, rocket. **The README and
`Cargo.toml` comments ("✅ production-ready") are live claims that this survey does not support; correct them when the crate is replaced.**

## Tests and CI

- No CI, no branch protection (HANDOFF). Test files read: none. `tests/integration_tests.rs` (268 lines) and the in-file unit
  tests are unread; the adapter tests I saw (axum.rs:228-258) assert construction only and say "should succeed without panicking".
  They are not conformance tests: none starts a server and sends a request. Unverified whether any test in the crate does.

## Recommendation for task 2

1. New crate layout from scratch on `http`/`http-body`/`bytes`; take the old crate's **mock adapter idea** and the `:param` route syntax.
2. Axum/hyper first (it is the one adapter whose upstream types already match). Add others only as each passes the same conformance suite.
3. Drop the auth/session/db/ffi/security modules from the line of this crate; do not carry their dependencies.
4. Treat actix/rocket/warp/salvo/poem as "adapter candidates", not shipped adapters, until a conformance run says otherwise.
