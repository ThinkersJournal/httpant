# HTTPAnt

## Known issues in web-server-abstraction 1.0.2 (superseded by HTTPAnt, in progress)

**The old `web-server-abstraction` 1.0.x line is not supported: it is end of life, and security fixes go to HTTPAnt only (use `httpant`; see [SECURITY.md](SECURITY.md)).** This crate is being replaced by [HTTPAnt](https://github.com/ThinkersJournal/httpant). 1.0.2 is **not yanked**. A source read of the
crate found the defects below; each line gives its basis. Full evidence, with line numbers: [docs/OLD-CRATE-SURVEY.md](docs/OLD-CRATE-SURVEY.md).

- **Unknown request methods are served as GET** (axum, actix-web, warp, salvo, poem). Basis: a `_ => HttpMethod::GET` arm in each adapter's request conversion.
- **warp: every handler sees a GET to `/`**, whatever was sent. Basis: `convert_warp_request_to_ours` hard-codes both. Warp also never sets response headers and corrupts binary bodies (`from_utf8_lossy`).
- **axum and warp send status codes outside a short list as 200** (a 418 or 503 reaches the client as 200). Basis: a `_ => OK` arm in the status match. actix-web, salvo and poem convert status correctly.
- **salvo and poem: request bodies are always empty.** Basis: `Body::from_bytes(vec![].into()), // Empty body for now` in both adapters.
- **Middleware is stored and never applied** (all six adapters). Basis: no code path reads the stored middleware; rocket and poem have wrapper types marked `#[allow(dead_code)]`, salvo and poem print "not yet fully integrated".
- **Bodies are fully buffered**, with no streaming; axum reads with no size limit, rocket caps at 2 MB and turns a read error into an empty body.
- **No test sends a request to a running server.** Basis: `tests/integration_tests.rs` only builds servers and calls `bind`; a search of `src/`, `tests/` and `examples/` for HTTP clients found none (the unit tests in some files were not read).

Affected adapters: all six (axum, actix-web, warp, rocket, salvo, poem); the per-adapter list is in the survey. The "production-ready" wording further down this
page and in `Cargo.toml` predates this survey and is not supported by it. Nothing here is a fix: 1.0.2 on crates.io is unchanged.

## Status of this repository

The old `web-server-abstraction` source was removed from `main` in the first HTTPAnt code slice (it stays in git history, and 1.0.2 stays on
crates.io). What is here now:

- `crates/httpant-core`: the `http` crate's request/response types, a streaming `Body` with explicit size limits, `Error`, `IntoResponse`,
  `Capabilities`, a `Router` and `Handler`, and the boxed Tower service type adapters run.
- `crates/httpant-server`: the `Server` builder, the `ServerAdapter` trait, the hyper reference adapter and (features `axum`, `actix-web`, `poem`, `salvo`) the axum, actix-web, poem and salvo adapters (`cargo run --example hello -p httpant-server`).
- `crates/httpant-client`: the `Client` builder, the `ClientAdapter` trait and the hyper-util reference adapter (plain `http://` for now).
- `crates/httpant-conformance`: conformance cases 1-12 over real sockets against an independent raw-TCP HTTP/1.1 client, each with known-bad mutants it must fail on.
  Results per adapter: [`docs/CAPABILITY-MATRIX.md`](docs/CAPABILITY-MATRIX.md). Client cases 1-7 run against a raw-TCP peer. Adapters other than hyper (client) and hyper, axum, actix-web, poem and salvo (server) are not built.
- `docs/HTTPANT-DESIGN.md`: the design, including the nine-line hello-world the first adapter must make true.

Nothing is published. The design option (A, Tower core with thin adapters) is **provisional** until CireSnave decides.
