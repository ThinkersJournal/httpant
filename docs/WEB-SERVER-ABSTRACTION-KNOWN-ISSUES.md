# Known issues in web-server-abstraction 1.0.2 (superseded by HTTPAnt)

Moved here from the repository README, unchanged apart from the links, when the README became HTTPAnt's own front page.

**The old `web-server-abstraction` 1.0.x line is not supported: it is end of life, and security fixes go to HTTPAnt only (use `httpant`; see [SECURITY.md](../SECURITY.md)).** This crate is being replaced by [HTTPAnt](https://github.com/ThinkersJournal/httpant). 1.0.2 is **not yanked**. A source read of the
crate found the defects below; each line gives its basis. Full evidence, with line numbers: [OLD-CRATE-SURVEY.md](OLD-CRATE-SURVEY.md).

- **Unknown request methods are served as GET** (axum, actix-web, warp, salvo, poem). Basis: a `_ => HttpMethod::GET` arm in each adapter's request conversion.
- **warp: every handler sees a GET to `/`**, whatever was sent. Basis: `convert_warp_request_to_ours` hard-codes both. Warp also never sets response headers and corrupts binary bodies (`from_utf8_lossy`).
- **axum and warp send status codes outside a short list as 200** (a 418 or 503 reaches the client as 200). Basis: a `_ => OK` arm in the status match. actix-web, salvo and poem convert status correctly.
- **salvo and poem: request bodies are always empty.** Basis: `Body::from_bytes(vec![].into()), // Empty body for now` in both adapters.
- **Middleware is stored and never applied** (all six adapters). Basis: no code path reads the stored middleware; rocket and poem have wrapper types marked `#[allow(dead_code)]`, salvo and poem print "not yet fully integrated".
- **Bodies are fully buffered**, with no streaming; axum reads with no size limit, rocket caps at 2 MB and turns a read error into an empty body.
- **No test sends a request to a running server.** Basis: `tests/integration_tests.rs` only builds servers and calls `bind`; a search of `src/`, `tests/` and `examples/` for HTTP clients found none (the unit tests in some files were not read).

Affected adapters: all six (axum, actix-web, warp, rocket, salvo, poem); the per-adapter list is in the survey. The "production-ready" wording further down this
page and in `Cargo.toml` predates this survey and is not supported by it. Nothing here is a fix: 1.0.2 on crates.io is unchanged.
