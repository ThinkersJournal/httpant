# HTTPAnt design

**Design only. Nothing here is built.** Written 2026-10-29 by the HTTPAnt lane against `origin/main` 0404afa. **UNVERIFIED** marks any fact not read from
source or measured. Evidence it builds on: `docs/OLD-CRATE-SURVEY.md` (the old crate, PR #1), `docs/ACTIX-ADAPTER-NOTES.md` and `docs/BENCHMARKS.md` (PR #4).
Those two PRs are not yet on `main`; where this file cites them, read them on their branches.

## Hello-world first

A stranger serves "hello" and fetches it. Nine lines, intended, not built:

```rust
use httpant::{Client, Server};

#[tokio::main]
async fn main() -> httpant::Result<()> {
    let server = Server::new().get("/", || async { "hello" }).bind("127.0.0.1:0").await?;
    let text = Client::new().get(format!("http://{}/", server.addr())).send().await?.text().await?;
    assert_eq!(text, "hello");
    Ok(())
}
```

Choosing the web library is one more call, by id, and the rest of the program does not change:

```rust
let server = Server::with_adapter("axum")?.get("/", || async { "hello" }).bind(addr).await?;   // id from settings, not from code
```

An unknown or not-compiled-in id is an error that lists the ids that are available and the cargo feature that enables each.
Rules this hello-world commits us to (EXPECTATIONS 6.24): the 80% case has one obvious path; a handler returns anything with `IntoResponse`
(a `&str` here); errors say what to do next; the example is compiled in CI.

## 1. What HTTPAnt is, and is not

HTTP is a settled protocol, so the types are not ours to invent: **HTTPAnt's request, response, method, status, header and URI types are the
`http` crate's** (`http` 1.5.0 on crates.io, 2026-07-29), and its body is an `http_body::Body` over `bytes::Bytes` (`http-body` 1.1.0,
`http-body-util` 0.1.5). The old crate wrapped or re-implemented these and lost information doing it (survey: unknown methods served as GET,
status codes collapsed to 200, repeated headers impossible). Re-using them makes those defects unrepresentable.

It is **one interface to many web libraries, in two halves**:

- **Server half:** you describe routes and handlers once; an *adapter* runs them on axum, hyper, actix-web, etc.
- **Client half:** you describe requests once; a *client adapter* sends them with hyper-util, reqwest, etc.

It is not a web framework (no ORM, auth, sessions, templating; the old crate's `auth`/`session`/`database`/`ffi` modules are discarded, survey) and not
a web *server program* (that is the later **WebServAnt**, which sits on top). TLS certificates and edge/hosting belong to WebHostAnt.

## 2. Crates

| Crate | Holds | Depends on |
|---|---|---|
| `httpant-core` | re-exports of `http`/`http-body`, `Body`, `Error`, `IntoResponse`, `Handler`, `Router`, middleware (Tower `Layer`) glue, the `Capabilities` type | `http`, `http-body-util`, `bytes`, `tower-service`/`tower-layer` (UNVERIFIED which tower crates exactly) |
| `httpant-server` | `Server` builder, the `ServerAdapter` trait, adapter registry, built-in `hyper` reference adapter | core |
| `httpant-client` | `Client`, the `ClientAdapter` trait, built-in hyper-util client adapter | core |
| `httpant` | facade: features pick adapters (`axum`, `actix-web`, `salvo`, ...); default = hyper | the three above |
| `httpant-conformance` | the test suite every adapter must pass (section 6); not published to depend on | core |

The four names `httpant`, `httpant-core`, `httpant-server`, `httpant-client` are claimed on crates.io at 0.0.0 (HANDOFF). Each non-default adapter lives behind
a feature of `httpant`, so a user pays only for the framework they pick (compile time of axum, actix-web, rocket etc. is large; UNVERIFIED how large).
Adapters may later move to their own crates (`httpant-axum`, ...) if a framework's release cadence needs it: those names are unclaimed.
One version number for all crates (portfolio rule).

## 3. Core types

```rust
pub type Request  = http::Request<Body>;
pub type Response = http::Response<Body>;

pub struct Body(/* boxed http_body::Body<Data = Bytes>; streaming by construction */);
impl Body { pub fn empty() -> Self; pub fn from_bytes(b: impl Into<Bytes>) -> Self; pub fn from_stream(..) -> Self;
            pub async fn bytes(self, limit: usize) -> Result<Bytes>; pub async fn text(self, limit: usize) -> Result<String>; }
```

- **No unbounded read exists.** `bytes`/`text` take an explicit limit (the old axum adapter read with `usize::MAX`). A default limit is set on the server
  and is visible in `Capabilities`. Streaming a body chunk by chunk is the primary representation; buffering is a convenience.
- **Handler:** any `async fn` (or closure) taking zero or more extractors and returning `impl IntoResponse`. The first slice ships only `Request` and
  `Path`/`Query` extractors, not a large extractor zoo (UNVERIFIED how much users will want; the survey shows the old crate had many modules nobody could call).
  `IntoResponse` is implemented for `&str`, `String`, `Bytes`, `StatusCode`, `(StatusCode, T)`, `Response`, `Result<T, Error>`.
- **The universal form is a Tower service:** every handler and the whole router compile to
  `tower_service::Service<Request, Response = Response, Error = Infallible>` boxed as `BoxService`. An adapter's only job is to run that one service
  and to feed it requests; it never sees routes. That keeps route matching, middleware and error mapping identical across adapters.
- **Send-ness:** the public `Handler` requires `Send + 'static` futures (matches hyper/axum/salvo/poem; works on actix since a `Send` future is also fine on
  its single-threaded workers). An opt-in `LocalHandler` (not `Send`) is accepted only by adapters declaring `local-handlers` (actix), per
  `docs/ACTIX-ADAPTER-NOTES.md`. Trying it elsewhere is a startup error, never a silent downgrade.
- **Errors:** one `httpant::Error` enum (`#[non_exhaustive]`), each variant's message ends with what to do next ("adapter `rocket` is not compiled in:
  enable the `rocket` feature"). A handler error becomes a response through a single, user-replaceable mapper; the default never puts internal detail in the body.
- **Routing:** `:name` and `*rest` path syntax (kept from the old crate), exact-method matching on `http::Method` (extension methods work), `405` with an `Allow`
  header for a known path with the wrong method, `404` otherwise. Trailing-slash behaviour is **one documented rule, same on every adapter** (the old poem
  adapter alone trimmed). The rule chosen: no normalisation; `/a` and `/a/` are different paths. UNVERIFIED whether users want the opposite; decide at review.

## 4. Server half

```rust
Server::new()                      // default adapter: hyper
    .layer(TraceLayer::new_for_http())     // any tower Layer
    .get("/users/:id", get_user)
    .post("/users", create_user)
    .bind("127.0.0.1:8080").await?       // a RunningServer; also accepts an already-bound socket
```

```rust
pub trait ServerAdapter: Send + Sync + 'static {
    fn id(&self) -> &'static str;                       // "hyper", "axum", "actix-web", ...
    fn framework_version(&self) -> &'static str;        // the version of the library actually linked
    fn capabilities(&self) -> Capabilities;
    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<Result<RunningServer>>;
}
```

`ServeOptions`: listener (TCP address or an already-bound socket, so WebHostAnt/systemd can hand one over), body limit, header limits, shutdown signal,
optional TLS acceptor (section 7). `RunningServer`: `addr()`, `shutdown().await`, `join().await`. Binding to port 0 and reading the real address back is
part of the contract (the hello-world, and every conformance test, depends on it).

**Adapters are thin**: convert the framework's request to `http::Request<Body>` and the `Response` back, nothing else. The old adapters each had their
own route table and conversion code (survey); in this design a conversion bug is a bug in one small function the conformance suite exercises.

## 5. Client half

```rust
let resp = Client::new().post("http://svc/users").json(&user)?.timeout(secs(5)).send().await?;
resp.status(); resp.headers(); resp.text().await?; resp.json::<T>().await?;
```

`ClientAdapter` takes an `http::Request<Body>` and returns an `http::Response<Body>`; the builder, JSON, timeouts and redirects policy sit above it in core so
they behave the same on every adapter. Candidate adapters: `hyper-util` client (reference, 0.1.21), `reqwest` (0.13.5) (UNVERIFIED that reqwest can
accept/return `http` bodies with streaming both ways without buffering; the first spike must show it). Redirect following, cookie jars and proxies are
**declared capabilities**, not silent behaviours: a client adapter that cannot do a thing says so in `Capabilities`.
Client TLS roots are configuration, not baked in (ConfigurAnt supplies them); the default is the platform/webpki roots chosen by the adapter and reported by `Capabilities`.

## 6. Adapter conformance suite

The suite is `httpant-conformance`: a list of named cases, run against each adapter, **over real sockets**. It uses its own minimal raw-TCP HTTP/1.1
client to test servers (and a raw-TCP server to test clients), **never the adapter under test or HTTPAnt's own client**, so a defect cannot hide on both ends.
Each adapter's CI job runs the whole suite; an adapter that fails a case it does not declare an exemption for does not ship.

**Known-bad controls.** Every case ships with a *mutant*: a deliberately broken variant of the reference adapter (folds unknown methods to GET, clamps status
to a short list, drops repeated headers, empty request bodies, ...). A case must **fail on its mutant** and pass on the reference; a case that passes on its
mutant is itself a failing test. The first seven mutants are the old crate's measured defects (survey, "conformance suite's first failing cases"):

| # | Case | Mutant (old defect it reproduces) |
|---|---|---|
| 1 | method fidelity: `PROPFIND`, `TRACE` reach the handler as sent; unknown method on a known path gets 405, never the GET handler | folds unknown methods to GET |
| 2 | status fidelity: 200, 204, 301, 404, 418, 503 arrive unchanged | maps unlisted codes to 200 |
| 3 | middleware applied: a layer adding a header and a layer short-circuiting both take effect | middleware stored, never run |
| 4 | headers: two `Set-Cookie` arrive as two lines; non-UTF8 request header bytes are visible to the handler | `HashMap<String,String>` |
| 5 | bodies: POST body reaches the handler; a 10 MB body is refused at the limit with 413; a binary response is byte-identical | empty request body; lossy UTF-8; unbounded read |
| 6 | request identity: handler sees real method, path and query | hard-coded GET `/`; query dropped |
| 7 | path rule: `/a` and `/a/` behave per the one documented rule | trims trailing slash |
| 8 | streaming: a response streamed in chunks arrives chunked and incrementally (first chunk before the last is produced) | buffers the whole body |
| 9 | graceful shutdown: an in-flight request completes after `shutdown()`; a new connection is then refused | drops in-flight |
| 10 | HTTP/1.1 keep-alive and `Expect: 100-continue` / `Connection: close` honoured | closes after each response |
| 11 | concurrency: 1,000 concurrent requests all answered correctly | serialises or drops |
| 12 | `HEAD` returns the headers of `GET` and no body | sends a body |
| 13 | a percent-encoded path reaches the handler decoded; a bad escape, non-UTF-8, an encoded `/`, backslash, NUL or dot segment is a 400; the encoded spelling of a literal is a 404 | decodes `%2F` or `%2e` before routing |

Cases 8-12 are expected to expose differences between frameworks; the result is a **capability matrix** (section 8), not a hidden exception.
A case an adapter cannot pass is recorded as `Unsupported` in its `Capabilities`, and the suite then asserts that using the feature returns the named error
(a feature must fail loudly, never partially work).

Client conformance mirrors this against the raw-TCP server (method/headers/body fidelity, redirects as declared, timeouts, streaming, connection reuse).

## 7. TLS as a capability

HTTPAnt never reads, stores or renews certificates. WebHostAnt owns that. The hook is a single trait:

```rust
pub trait TlsAcceptor: Send + Sync + 'static {   // given an accepted TCP stream, returns an encrypted stream
    fn accept(&self, io: TcpStream) -> BoxFuture<io::Result<BoxedIo>>;
}
```

`ServeOptions::tls(Arc<dyn TlsAcceptor>)`; an adapter that cannot take a pre-built acceptor declares `tls: false` in `Capabilities` and the server refuses to
start with one (never serves cleartext when TLS was asked for). Certificate reload without restart is a separate declared capability (`tls-reload`) because the
acceptor can swap its config internally. The default acceptor implementation (rustls) lives in WebHostAnt or an opt-in feature, not in core. UNVERIFIED: which of the
frameworks accept an externally built acceptor versus requiring their own TLS config types (axum/hyper do via hyper-util serving a stream, UNVERIFIED for the rest).

## 8. What each adapter can NOT do

Filled by the conformance run, not by assertion. What is known today (source-read, nothing run); every blank is UNVERIFIED until slice 2:

| Adapter | Known limit | Source |
|---|---|---|
| hyper (reference) | none known; websocket upgrade not provided by hyper itself (UNVERIFIED) | - |
| axum | rides hyper; same limits | survey: axum.rs |
| actix-web | `!Send` handlers, single-threaded workers, Tower middleware only via a buffering bridge (`actix-tower` buffers the body, 500 on short-circuit) | `docs/ACTIX-ADAPTER-NOTES.md` |
| poem, salvo, warp | none found by the conformance run (cases 1-12 pass); salvo keeps its listening socket open while draining, so the adapter wraps its acceptor to close it first | `docs/CAPABILITY-MATRIX.md`, `salvo_adapter.rs` |
| rocket | **cannot do extension methods** (source reading, nothing run): rocket 0.5.2 has nine request methods and answers 400 to any other before a route is tried (`Request::from_hyp`, `BadMethod`), so conformance case 1 (`PROPFIND`) cannot pass. Not built | rocket 0.5.2 `src/request/request.rs`, `rocket_http` `method.rs` |

`Capabilities` fields (initial): `websockets`, `request_streaming`, `response_streaming`, `graceful_shutdown`, `http2`, `trailers`, `tls`, `tls_reload`,
`local_handlers`, `max_body_default`, `extension_methods`, `connect_method`. Each is `Supported | Unsupported(reason) | Unknown`, and the conformance run turns every
`Unknown` into one of the first two. **Websockets** are not in the first slice: upgrade semantics differ across frameworks; they get their own design note once
two adapters pass the basic suite.

## 9. Middleware and Tower

Tower is the middleware standard: any `tower::Layer<BoxService>` applies via `.layer()`, and the core ships none of its own beyond routing (the old crate's
logging/cors/auth middleware are discarded; `tower-http` has trace, cors, compression, timeout, request-id). Middleware runs **inside** HTTPAnt, wrapping the
router service, before any framework sees the request, so it behaves identically on every adapter and needs no per-framework bridge. The cost is that the
framework's own native middleware is not used; that is intended (agnosticism over framework features). A framework-native escape hatch exists per adapter
(`Server::with_adapter("axum")?.native(|router| ..)`), clearly outside the portable surface; code using it is not portable and the type says so.

## 10. Ant-first

- **ConfigurAnt:** adapter id, bind address, body and header limits, timeouts come from settings; the builder takes a `ServerConfig` that ConfigurAnt can fill.
  ConfigurAnt is at 0.5.4 (PM message); how its API looks is UNVERIFIED and I will read it before slice 1.
- **PersistAnt/CapAnt:** no use identified in the HTTP layer. Not forced in.
- **Other Ants (0.0.0 placeholders):** not depended on; a trait is stubbed where a seam exists (TLS acceptor above, `Settings` source) and the stub is labelled.

## 11. Migration from `web-server-abstraction`

| Old | New |
|---|---|
| `WebServer::new().route(path, HttpMethod::GET, h)` | `Server::new().route(path, Method::GET, h)` or `.get(path, h)` |
| `HttpMethod` enum | `http::Method` |
| `types::{Request, Response, StatusCode, Headers}` | `http::{Request, Response, StatusCode, HeaderMap}` |
| `Body` (single `Bytes`) | streaming `Body` with explicit-limit `bytes()` |
| `Middleware`/`Next` | Tower `Layer` |
| `WebServer::with_axum_adapter()` | `Server::with_adapter("axum")?` |
| auth/session/database/ffi/security/monitoring modules | not carried; separate crates if a consumer asks |

The old crate stays unyanked and gets a README pointer once HTTPAnt can replace it (HANDOFF; PR #3 adds the known-issues note). No compatibility shim is planned: the
old types are the defects. A shim would reintroduce them. Unverified: who depends on `web-server-abstraction` 1.0.2 (crates.io reverse dependencies not looked up).

## 12. Slices

1. **Core + conformance harness + hyper reference adapter + the nine-line example in CI.** Mutants 1-7. `cargo deny` becomes required here (HANDOFF exception ends).
   Built in 1b as: `BoxService` is a `BoxCloneService` (an adapter clones it per connection); `httpant-conformance` depends on core and server (a `Subject` starts
   an `App` on an adapter; mutants are wrappers around the service); an oversize body is tested by declaring 10 MiB and sending 16 KiB, because a server that refuses
   early and closes with unread data resets the connection on Windows and can destroy the 413 (observed, os error 10053).
   PM 2026-10-29 split it: **1a** (done in the first code PR) = core types + conformance skeleton (case registry, raw-TCP client), provisionally on option A; **1b** = router/handler, hyper reference adapter, the runnable cases 1-7 with their mutants, and the example in CI. The old source tree was removed in 1a, so `cargo deny` is green from 1a.
2. **axum adapter**, then cases 8-12 for both; first capability matrix.
3. **Client half** with hyper-util adapter and client conformance.
4. **actix-web adapter** (Send handlers; `LocalHandler` capability).
5. **salvo, poem, rocket, warp** one at a time, only if they pass.
6. TLS acceptor hook with WebHostAnt; benchmark harness (adapter vs bare framework; TechEmpower R23 has no actix or warp entries, `docs/BENCHMARKS.md`).

Each slice is one PR, red-first (the conformance case and its mutant are written failing before the adapter code), CI green on ubuntu and windows.

## 13. Options for CireSnave

**A. Tower-service core, thin adapters, reference hyper adapter (recommended).** One `BoxService` is the contract; adapters only translate and run it; middleware is
Tower and portable. Pro: smallest adapters, one place for routing/middleware, conformance suite can be exact, matches the Tower-family-first order. Con: frameworks'
native middleware and extractors are not available portably; actix needs the `LocalHandler` carve-out.

**B. Framework-native routing per adapter.** Each adapter registers routes with its framework's own router. Pro: native performance and features. Con: this is how the old
crate drifted: six route tables, six conversion paths, and the defects in the survey; the same behaviour on every framework becomes much harder to guarantee.

**C. Compatibility-first: keep the old crate's API and fix it underneath.** Pro: existing users keep compiling. Con: the API is the problem (closed `HttpMethod`, string
`Headers`, buffered `Body`); who depends on the old crate was not looked up (UNVERIFIED), so the benefit is unmeasured.

Recommendation: **A**, with the hyper adapter as reference and axum second.
