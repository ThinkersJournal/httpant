# Survey of the old `web-server-abstraction` 1.0.3 crate

Measured at `origin/main` 1d53fad, by reading source (nothing was built or run).

**Read in full:** `Cargo.toml`; `src/core.rs` lines 1-200; the six adapter files' non-test code (axum, warp, actix-web, rocket,
salvo, poem); `tests/integration_tests.rs`; the poem test tail. **Struct definitions only:** `src/types.rs`.
**Not read:** the rest of `core.rs` (the `WebServer` builder, lines 200-528), `src/mock` beyond its size, the in-file unit tests of
axum/actix/rocket/salvo/warp, `src/*/tests.rs`, `examples/`, and every non-adapter module (auth, security, session, ...).
Claims below are about what was read. Line numbers are in `origin/main`.

## Verdict

Discard the code, keep the ideas. The shape (one handler type, adapters chosen by feature, a mock adapter for tests) is right.
The adapters lose information at the framework boundary, and the types cannot express what HTTP needs. HTTPAnt's core should be built
on the `http` crate's types, which the old crate already depends on but wraps.

## Core types (`src/types.rs`, `src/core.rs`)

- `Headers` wraps `HashMap<String,String>` (types.rs:681): repeated headers (`Set-Cookie`) cannot be represented. Discard.
- `StatusCode(pub u16)` (types.rs:641) duplicates `http::StatusCode`. Discard.
- `HttpMethod` is a closed enum of nine methods. Discard for `http::Method` (extension methods like `PROPFIND` exist).
- `Body` wraps one `Bytes` (types.rs:757): no streaming. Discard.
- `Request.extensions` is `HashMap<String,String>` ("Simplified for now", types.rs:19). `http::Extensions` is the real thing.
- `Middleware`/`Next` (core.rs:18-36): idea reusable, type is not Tower-compatible.
- `Route::match_dynamic_path` (core.rs:97): hand-rolled `:param` / `*wild` router. Keep the syntax idea, not the code.
- `Cargo.toml` pulls `auth-framework`, `sqlx`, `flate2`, `sha1`, `uuid`, `serde_yaml`, `libc`, `regex`, `rand` unconditionally. Auth,
  sessions, database, FFI, security and static-file modules are out of scope for a transport layer. Not read, only listed by
  filename and line count; "out of scope" is a scoping call, not a quality finding.

## Adapters

Each adapter is its own route table plus conversion functions; none shares a conversion layer. Ten lines max each.

**axum** (axum.rs, read in full)
- Sound: `Router` composition, `axum::serve`.
- Unknown request method becomes GET (:149). Response status outside 8 listed codes becomes 200 (:205-215): a 418 or 503 is sent as 200.
- Non-UTF8 request header values dropped (:158). Middleware stored, never applied (:94, :129). `bind` only records the address.
- Body read with `usize::MAX` limit, fully buffered (:164). `path_params` always empty (:171).

**warp** (warp.rs, read in full)
- Request conversion hard-codes `method = GET` and `uri = "/"` (:169-170). **Every handler sees a GET to `/`**, whatever was sent.
  Routing itself uses the real method and path (:103-118), so a handler cannot know which request it is serving.
- Unknown method becomes GET (:111). Status outside 5 listed codes becomes 200 (:140-147).
- Response body goes through `String::from_utf8_lossy` (:137): binary bodies are corrupted. Response headers are never set at all
  (the reply is `with_status(body_string, status)`). Middleware stored, never applied.

**actix-web** (actix_web.rs, read through the conversion code)
- Status converted correctly via `from_u16` (:374). Response headers dropped if they fail to parse (:357).
- The route registration `match` has a fallback arm that registers any other method as a GET route (:203-218); `TRACE` and
  `CONNECT` are the variants that reach it, so a route declared `TRACE` is served on GET.
- Request `uri` is rebuilt as `path?query` (:320). Middleware stored, never applied (no code reads `self.middleware`).
- Body buffered by the `web::Bytes` extractor (actix's own default limit applies; unverified what it is).

**rocket** (rocket.rs, read through :320)
- Status: `Status::from_code(code)`; if it returns `None` nothing is set and the response stays 200 (:226-228).
- Request body read to a 2 MB cap; a read error becomes an empty body (:188-191). Request URI contains the path only; the query is dropped (:203-206).
- Response body read with `block_in_place` + `block_on` (:242), fully buffered.
- `MiddlewareFairing` is `#[allow(dead_code)]` and never attached (:255-285). Route path syntax: Rocket uses `<id>`; no translation from
  `:id` was seen (unverified for dynamic routes).

**salvo** (salvo.rs, read through :290)
- **Request body is always empty**: `Body::from_bytes(vec![].into()), // Empty body for now` (:239). A POST body never reaches a handler.
- Status converted via `from_u16` (:255). Routes declared `TRACE`/`CONNECT` are skipped with a `println!` (:101-104).
- Unknown request method becomes GET (:207). Response headers use `insert`, overwriting repeated ones (:263).
- Middleware: `println!("... not yet fully integrated")` (:110).

**poem** (poem.rs, read through :370)
- **Request body is always empty** (:312, same comment). Middleware wrapper is `#[allow(dead_code)]` and its loop only prints (:211-263).
- A response header whose name fails to parse is sent as `x-custom-header` (:336-338): wrong name, value kept.
- Built-in `NormalizePath(Trim)` is always applied (:131-133): `/a/` and `/a` are the same route, a behaviour the other adapters do not share.
- `CONNECT` routes are registered as "any method" (:117-120). Unknown request method becomes GET (:280).

**Doc claims:** `actix_web.rs`, `rocket.rs`, `salvo.rs`, `poem.rs` each say at line 3-4 "production-ready ... Includes full
request/response conversion, middleware integration". For all six adapters the middleware is stored and not applied; for warp,
salvo and poem the request conversion is not full (items above). The README and `Cargo.toml` "✅ production-ready" comments are live
claims this survey does not support; correct them when the crate is replaced.

## Tests

`tests/integration_tests.rs` (268 lines, read in full): every test builds a server and calls `bind("127.0.0.1:0")`; **none serves a
request**. Their comments say so ("In a real implementation, we would ..."). `test_status_codes` builds a route returning
`StatusCode(418)` and never checks what a client receives. The in-file adapter tests I saw (axum, poem) check construction and
that the disabled-feature stubs return errors. Positive control for "no test sends a request": a grep of `src/`, `tests/`,
`examples/` for `TcpStream|reqwest|hyper::|oneshot|tower::ServiceExt` finds no client; its only hits for `.run()` are the server
entry points and one test that calls `run()` with the feature off. The unread unit-test files could still contain one.

## The conformance suite's first failing cases

These are the defects above, restated as tests the new suite must contain. Each needs a known-bad control (a deliberately broken
mock adapter that makes the test fail), so the test is shown able to fail.

1. **Method fidelity:** a handler for `PROPFIND` (and `TRACE`) receives that method, and a request with an unregistered method gets 405/404, never the GET handler. Old: every adapter folds unknown methods to GET; actix and salvo mishandle `TRACE`/`CONNECT` routes.
2. **Status fidelity:** every status from 100-599 that the framework permits arrives at the client unchanged (418, 503 at minimum). Old: axum and warp send 200.
3. **Middleware applied:** a middleware that adds a header and one that short-circuits both take effect on a real request. Old: stored, never applied, in all six.
4. **Headers:** a response with two `Set-Cookie` values arrives as two headers; a request header with non-UTF8 bytes is not silently dropped. Old: `HashMap<String,String>`.
5. **Bodies:** a request body of 10 MB is rejected or streamed by an explicit limit rather than unbounded buffering or silent truncation; a binary response body arrives byte-identical; a POST body reaches the handler. Old: unbounded (axum), 2 MB silent (rocket), empty (salvo, poem), lossy UTF-8 (warp).
6. **Request identity:** the handler sees the real method, path and query. Old: warp always GET `/`; rocket drops the query.
7. **Path normalisation:** `/a` vs `/a/` behaves identically on every adapter or the difference is a declared capability. Old: poem trims, others do not.

## Recommendation for task 2

1. New crate from scratch on `http` / `http-body` / `bytes`; take the mock-adapter idea and `:param` syntax.
2. Axum/hyper first. Add each other framework only when it passes the same suite.
3. Do not carry the auth/session/db/ffi/security modules or their dependencies.
4. Treat actix/rocket/warp/salvo/poem as candidates, not shipped adapters, until a conformance run says otherwise.
