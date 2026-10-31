# Actix-web adapter notes (input to the design note)

Requested by CireSnave via the PM, 2026-10-28: read `actix-tower` 0.1.4 BEFORE designing the actix adapter; record actix's own service
trait, `!Send` futures and runtime, and how HTTPAnt's handler type expresses them. Sources: crate files downloaded from static.crates.io on
2026-10-28 (nothing built or run). Versions from the crates.io API: actix-web 4.18.1 (updated 2026-10-23), actix-service 2.0.3, actix-rt 2.15.0.

## Actix's own model

- `actix_service::Service<Req>` (actix-service 2.0.3 `src/lib.rs:92`): `poll_ready(&self, ..)` and `call(&self, req) -> Self::Future`, with
  `Future: Future<Output = Result<Response, Error>>` and **no `Send` bound** on the trait or the future. Tower's `Service` takes `&mut self`
  for both; this is why the two do not compose directly.
- `actix-rt` 2.15.0 (`src/lib.rs:1-9`): "Tokio-based single-threaded async runtime ... it has been chosen to use !Send futures". It builds
  `tokio::runtime::Builder::new_current_thread()` with a `LocalSet` (`src/runtime.rs`). Each worker is its own runtime and futures never move
  between threads, so handlers may hold `Rc`, `RefCell` and actix's `HttpRequest`.
- Consequence: a handler contract that demands `Send` futures can still be run by actix (a `Send` future is also fine on a local runtime),
  but a handler written against actix's `!Send` types cannot be run on a multi-threaded runtime. Unverified: whether `HttpServer` can be
  driven from a multi-threaded runtime; not tested.

## actix-tower 0.1.4

- crates.io name `actix_tower`; five versions published 2026-07-04 to 2026-07-09; Apache-2.0 only; MSRV 1.80; one repository
  (`rstosp/actix_tower`); **148 total downloads** at fetch time. A very young crate: not a hard dependency without a decision.
- Provides `TowerLayer` / `TowerLayerCompat`, a Tower `Layer` used as an actix `Transform` (`src/compat/tower/layer.rs`).
- Its own doc comment (layer.rs): the bridge **buffers the entire request body** before the inner actix service sees it, "because Actix's
  `HttpRequest` is `!Send` and cannot be held across the Tower future boundary"; default limit 4 MiB, over which it answers 413.
- Same doc comment: a Tower middleware that **short-circuits** (returns a response without calling the inner service) produces a **500**,
  because the original request cannot be recovered; the crate's advice is to reject requests with actix-native middleware.
- Service state is `Rc<RefCell<TS>>` (service.rs) and its doc says not to use it on a multi-threaded executor.
- Its README claims a "zero-allocation hot path (no Box::pin, no dyn Future)"; `ActixServiceWrapper::call` in the same crate keeps
  `Box::pin(async move ..)` (service.rs, with a comment saying so). Claim and code disagree; we rely on neither.
- Uses `http` 1.x types, so the HTTPAnt boundary is an `http::Request` <-> `ServiceRequest` mapping.

## How HTTPAnt's handler type could express this (proposal for the design note, not decided)

1. Public API: handlers are `Send + 'static` (the common case; matches axum/hyper/salvo/poem). The actix adapter runs them on its worker
   threads, which a `Send` future permits.
2. A separate, opt-in `LocalHandler` (not required to be `Send`) that only adapters declaring the capability `local-handlers` accept
   (actix). Attempting it on another adapter is a compile-time or startup error, never a silent downgrade.
3. Tower middleware (the core's middleware story) reaches actix in one of three ways:
   - **A** use `actix-tower` (young, buffers bodies, 500 on short-circuit);
   - **B** our own small bridge, with the buffering limit stated as a declared capability;
   - **C** no Tower middleware inside actix: HTTPAnt middleware wraps the handler before actix sees the request.
   Recommendation for the first actix slice: **C**, since it adds no dependency and has no 500 trap; revisit A or B when actix-tower has a
   history. Unverified: that C avoids double buffering; the adapter spike must show it.
4. Every limit above is a row in "what each adapter can NOT do", with a conformance test and a known-bad control.

## `LocalHandler` (ROADMAP row 9): what building it would take, read 2026-10-31 at `ac365e9`

Read, not built; nothing was run. Question: could a handler whose future is not `Send` (actix's `HttpRequest`, `Rc`, `RefCell`) be registered
and run on the actix adapter only?

- **The one service is `Send` and everything is wrapped around it.** `Server::into_service` makes the router and `.layer()` layers one
  `BoxService` (a `BoxCloneService`, `Send`), and `ServeOptions::wrap` (`crates/httpant-server/src/adapter.rs`) wraps that service in the body
  limits, the shutdown switch, the ACME challenge responder, the trusted-proxy `Origin` and the HTTPS redirect, in that order. Every adapter
  calls `wrap` (conformance cases 14, 15, 16). A `!Send` handler cannot be inside that service.
- **So a local route would bypass all of it** unless each wrapper is rewritten for `!Send` services. Without that, a local route would ignore
  the body limit, answer on the port-80 redirect listener, miss the ACME prefix and see no `Origin`: a silent hole in the guarantees the
  other routes have. That is the cost the earlier notes did not count.
- **Routing would be split.** Local and ordinary routes on one path need one 405/`Allow` answer and one 404; today `Router` owns that
  (`crates/httpant-core/src/router.rs`) and stores `Send` handlers only. Either a second router for local routes with a merge rule, or a generic
  router over a handler kind.
- **Construction is per worker.** actix runs a factory closure on each worker thread, so a local handler is a `Fn() -> Handler` factory (its
  state cannot be shared across threads); the adapter must also run the factory for the TLS path, which is one actix thread (0.5.25), and the
  cleartext path, which is two workers.
- **The error side is simple:** `capabilities().local_handlers` already exists (`No` on five adapters, `No` on actix today) and a startup
  error `LocalHandlersNotSupported` on `bind` is a few lines.

Recommendation: do **not** build it without a consumer. No consumer has asked for it (nothing in this repository names a user, and no message to this lane did; WebHostAnt's need lists live in that
repository and were not re-read for this), and a handler that holds `Rc` can almost always hold `Arc` instead, which the existing `Send` path already runs on
actix. If a consumer needs actix-native state, the smaller route is to run HTTPAnt's service as a **tower service inside their own actix `App`**
(an actix adapter for embedding, the reverse direction), which keeps the wrappers where they are. Estimated size of the in-adapter route:
a generic router or a second one, `!Send` re-implementations of five wrappers, per-worker factories on two serve paths, and a conformance case with
mutants for each wrapper on local routes: a PR the size of the TLS adapter work, for a feature with no named user.
