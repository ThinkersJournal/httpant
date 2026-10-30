# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.4] - 2026-10-30

- `httpant_server::read_back(addr, server_name, timeout)` (feature `tls`), WebHostAnt need 22: it connects to the **live listener**, handshakes with that SNI name and returns a `ServedCert` (issuer, subject, not-before and not-after as Unix seconds, chain length, leaf DER, TLS version, ALPN) of what is actually served, so a renewal is proved from the listener and not from the source's say-so. No request is sent. It does **not** validate the certificate (an expired or self-signed one is reported as it is), so it is for reporting only and must not decide trust. Parsing uses `x509-parser` (no default features). Tests on hyper and axum: the leaf equals the served one, a swapped certificate shows as swapped on the next read-back, a refused name and a plaintext port are errors. Not published.

## [0.5.3] - 2026-10-30

- `docs/PATENT-SWEEP.md` addendum (PM ruling): the two unread patents are claim-read, 13 more queries across Google Patents and Espacenet/PatentScope/USPTO for every idea, five more patents claim-read (12 in all), and the file says exactly what the search tool could and could not do (Espacenet's own search was not usable; no classification-code search). No verdict changes. Documentation only. Not published.

## [0.5.2] - 2026-10-30

- `docs/PATENT-SWEEP.md` is a real sweep of the 0.5.x line (10 ideas, 10 queries, 7 patents read claim by claim, none needs review) instead of the "not yet performed" placeholder. Documentation only. Not published.

## [0.5.1] - 2026-10-30

- TLS termination, first half of ROADMAP row 4 slice (b), behind the new `tls` feature of `httpant-server` (rustls with the `ring` provider only; no aws-lc, no native-tls). `CertSource` is the hole WebHostAnt/WebServAnt plug into: an async `cert_for(server_name) -> Result<Option<Arc<ServerCert>>>` called **once per handshake** with the SNI name (`None` when the client sent none; the source decides, `FixedCert` answers with its one certificate). `Ok(None)` (unknown name) and `Err` both refuse that handshake; neither falls back to another name's certificate. `TlsAcceptor::new(source).handshake_timeout(..)` (10 s default, source lookup included); `ServeOptions::tls` / `Server::tls`. TLS 1.3 and 1.2 only, ALPN `http/1.1` only (the adapters speak HTTP/1.1; `h2` waits for them). `ServerCert::from_pem` / `from_der` check that the key suits the certificate; its `Debug` shows `<redacted>` and no error carries key bytes.
- Capability `tls` (`Capabilities::tls`): `Yes` on hyper and axum (handshake below the adapter, so hyper takes the finished stream and axum a custom `Listener` that runs handshakes in their own tasks), explicit `No` on actix-web, poem, salvo and warp (edge mode only). `Server::tls` on an adapter that is not `Yes` fails with the new `Error::TlsNotSupported`; it never serves in clear. A request that arrived over TLS this server terminated gets `Origin.scheme == Https` from the connection itself (`TlsConnection` marker), whatever `X-Forwarded-Proto` says.
- `tests/tls.rs` over real sockets on hyper and axum: https served on TLS 1.3 and 1.2 with the presented certificate read back; plaintext client gets no answer; an unknown name and a failing source are refused (positive control in the same test); a swapped certificate serves the next connection; a silent client is dropped at the handshake timeout; the key is absent from `Debug` and errors; an adapter without TLS is refused. Not done (ROADMAP row 4): served-certificate read-back API (need 22), the ACME challenge-prefix handler (14) and `acme-tls/1` (13), port-80 listener with redirect (18), audit events (23), conformance case for TLS; `h2`. Not published.

## [0.5.0] - 2026-10-30

- **Breaking (pre-1.0: the second number):** edge mode. Behind a proxy that terminates TLS (Cloudflare, Caddy, WebServAnt) a handler needs the client's address and scheme, and must not believe them from just anyone. Every request now carries an `Origin { client, scheme, via_trusted_proxy }` in its extensions. `ServeOptions::trusted_proxies` / `Server::trusted_proxies` take a `TrustedProxies` (addresses and `address/prefix` networks, `parse("10.0.0.0/8")`); **nobody is trusted by default**. From a trusted peer the client is the rightmost `X-Forwarded-For` hop that is not itself a trusted proxy (hops a client invented sit to its left and are never reached) and the scheme is `X-Forwarded-Proto`; from any other peer the headers are ignored and the client is the peer. The headers are left on the request; handlers read `Origin`. Not done here: the RFC 7239 `Forwarded` header, and an `https` scheme for HTTPAnt's own listener (it serves plain HTTP until the TLS slice).
- **Adapter API (Breaking for third-party adapters):** an adapter puts a `PeerAddr` in each request's extensions before the service runs (`ServeOptions::wrap` computes `Origin` from it, and treats a request without one as having no origin). All six in-tree adapters do: hyper (accept), axum (`ConnectInfo`), actix-web (`peer_addr`), poem (`remote_addr`), salvo (`remote_addr`), warp (`addr::remote`). The conformance `Subject` trait's `start_with_grace` becomes `start_with(app, Setup)`.
- Conformance case 16 `edge-origin` on every adapter: no trusted proxy, a trusted network that does not contain the peer, a trusted network that does, and a trusted proxy that forwards nothing. Mutants `trust-any-peer` and `ignore-trusted-proxies` must fail it. Unit tests cover the chain walk, dual-stack `::ffff:` peers, prefixes and garbled headers. Not published.

## [0.4.0] - 2026-10-30

- **Breaking (pre-1.0: the second number):** `shutdown()` is bounded. A request that never finishes used to hold it for ever; now it waits for the grace period (30 s by default: `ServeOptions::shutdown_grace`, `Server::shutdown_grace`), then ends the requests still running: the handler is dropped, the client gets `503` with `Connection: close`, a response already streaming is cut, and `shutdown()` returns. `ServeOptions::no_shutdown_grace()` / `Server::no_shutdown_grace()` keep the old wait-for-ever, on purpose. If the web library has still not stopped 5 s (`FORCE_WAIT`) after that, its task is aborted and `shutdown()` returns anyway.
- **Adapter API (Breaking for third-party adapters):** `RunningServer::new` takes a fourth argument, the `Grace` from `ServeOptions::grace()`, and an adapter serves `opts.wrap(svc)` (limits and the shutdown switch together) instead of `opts.limits.wrap(svc)`. The switch is a Tower wrapper, so it works the same on every adapter and needs nothing from the web library; all six in-tree adapters are updated.
- Conformance case 15 `shutdown-grace` on every adapter: a `/hang` request is in flight, shutdown must not return before the grace period (1.5 s) is over and must return soon after it (a margin shorter than the abort fallback, so only a real forced end passes), and the abandoned client sees a 503 or a closed connection. Mutant `no-shutdown-grace` (waits for ever whatever it is asked) must fail it. Six mutations of the switch and of `shutdown()` (never force, period ignored, wrap ignores the force, body never cut, forced answer 200, forced answer keeps the connection open) are each caught by the unit tests or case 15. The suite's `Subject` gains `start_with_grace`; every other case runs with a 60 s grace so it never meets it. Not published.

## [0.3.0] - 2026-10-30

- **Breaking (pre-1.0: the second number):** the server now enforces limits by default, before any handler runs. `ServeOptions` gains `limits: Limits` (`ServeOptions::limits(..)`, `Server::limits(..)`); `Limits::default()` is conservative: request target 8 KiB (`414`), 64 header fields (`431`), 16 KiB of header names and values (`431`), request body 2 MiB (`413`). A request body over 2 MiB, which every adapter used to pass through, is now a `413` until the code raises it (`Limits::default().max_body_bytes(..)`) or takes the limits off on purpose (`Limits::unlimited()`). A body is refused on its head when its `Content-Length` says it is too big; a body that grows past the limit while streaming fails the read with `Error::BodyTooLarge` (new `BodyLimitExceeded` in `httpant-core`; `Body::bytes` maps it), which answers `413`.
- Enforced by HTTPAnt on the request each adapter hands over (`let svc = opts.limits.wrap(svc);` is one line in every adapter), so the limits mean the same thing on every adapter. They cannot stop a web library buffering what it reads first: each library's own ceiling still applies (the sizes in case 14 are below every adapter's own).
- Conformance case 14 `limits` on every adapter (at the limit is served, one over is refused, for each of the four limits), with four mutants (`no-uri-limit`, `no-header-count-limit`, `no-header-bytes-limit`, `no-body-limit`: the default limits minus one) it must fail on. The limit layer's own tests were each shown to fail when their limit is removed or off by one (ten mutations, all caught). Case 5 now declares 1.5 MiB (under the server limit) so it still tests the handler's own limit.
- `ROADMAP.md` (new): the repo's own work list, so work does not live only in HANDOFF. Not published.

## [0.2.0] - 2026-10-30

- **Breaking (pre-1.0: the second number):** `PathParams::get` now returns the **percent-decoded** value; before it returned the raw text. `PathParams` is no longer a tuple struct (`PathParams(pub Vec<..>)`): use `get`, `get_raw` (the text as sent) and `iter`. Rule, decided with the router: captures are decoded segment by segment (`+` stays a plus); a capture that cannot be decoded safely is a `400` before any handler runs, with a one-line reason: a `%` not followed by two hex digits, decoded bytes that are not UTF-8, an encoded `/`, backslash or NUL, an encoded `.` or `..` segment. Literals in a route are matched **raw**: `/h%65llo` is not `/hello` (404), so a layer that looks at the raw path cannot be sidestepped by an encoded spelling.
- Conformance case 13 `path-encoding` on every adapter (12 request targets, answers and refusals), with the mutant `decode-before-routing` (a framework that turns `%2F` and `%2e` into a real separator or dot segment before routing) it must fail on. The router's own tests were each shown to fail when their rule is removed (six mutations: no separator refusal, no dot refusal, lossy UTF-8, a bad escape kept literally, literals decoded, `+` read as a space). Not published.

## [0.1.9] - 2026-10-30

- Slice 7: warp adapter (`WarpAdapter`, feature `warp` of `httpant-server`, `Server::with_adapter("warp")`), warp 0.4 with default features off. One filter matches every request and hands it to the boxed service, so routing and error mapping are HTTPAnt's. Request bodies stream through (declared `Content-Length` is the size hint); response bodies stream through a one-frame channel (warp wants a `Sync` stream, `Body` is not); a body of known length keeps its `Content-Length`. warp closes its listener when it begins draining and waits for connections through hyper-util's `GracefulShutdown`. Conformance cases 1-12 pass on hyper, axum, actix-web, poem, salvo and warp; the matrix gains the row.
- **rocket is not built, on a source reading:** rocket 0.5.2 knows nine request methods (`rocket_http::Method`) and `Request::from_hyp` turns any other into a `BadMethod` error, so a `PROPFIND` request never reaches a route (rocket answers 400). Conformance case 1 requires extension methods, so the adapter could not pass; its source carries the comment "TODO: Allow made-up methods?". This is a reading of rocket's source, not a run; nothing was built. Recorded in the design note's section 8. Not published.

## [0.1.8] - 2026-10-30

- Portfolio check: a line-scoped `[[allow]]` for SEC-002 on `src/types/tests.rs` in `.coderipper.toml`, with its reason. The four gitleaks hits are in the history of a file deleted in 0.1.0 and are the public RFC 6455 section 1.3 WebSocket handshake sample (key and derived accept value) used as a test vector, not secrets. No code change. Not published.

## [0.1.7] - 2026-10-30

- Slice 6: salvo adapter (`SalvoAdapter`, feature `salvo` of `httpant-server`, `Server::with_adapter("salvo")`), salvo_core 1.0 with default features off. The one boxed service is the goal of a catch-all salvo route, so routing and error mapping are HTTPAnt's. Request bodies stream through (declared `Content-Length` is the size hint); a response body of known length keeps its `Content-Length`, unknown length is chunked. Conformance cases 1-12 pass on hyper, axum, actix-web, poem and salvo; the matrix gains the row. salvo keeps its listening socket open until it has finished draining, so on Linux CI (not on Windows) a client could still connect during shutdown and case 9 failed; the adapter hands salvo an acceptor it can close at shutdown, and counts requests (not only connections) so `shutdown()` returns when the last response has been sent. Not published.

## [0.1.6] - 2026-10-30

- Slice 5: poem adapter (`PoemAdapter`, feature `poem` of `httpant-server`, `Server::with_adapter("poem")`), poem 3.1 with default features off. poem serves the one boxed service as its only endpoint, so routing and error mapping are HTTPAnt's. Request bodies stream through (declared `Content-Length` is the size hint, as on actix); a response body of known length keeps its `Content-Length` (case 12 found the first draft sent every response chunked); unknown length is chunked. Conformance cases 1-12 pass on hyper, axum, actix-web and poem; the matrix gains the row. Not published.

## [0.1.5] - 2026-10-30

- Slice 4: actix-web adapter (`ActixAdapter`, feature `actix-web` of `httpant-server`, `Server::with_adapter("actix-web")`), actix-web 4.18 with default features off. Option C of `docs/ACTIX-ADAPTER-NOTES.md`: no Tower middleware inside actix and no `actix-tower`. actix's workers run on a thread of their own; the `!Send` request payload is moved chunk by chunk, with back-pressure, into a `Send` body by a pump task on the worker, so nothing is buffered. The declared `Content-Length` is the body's size hint, so an over-limit body is refused on its head (case 5 found the first draft waited for bytes that never came). Conformance cases 1-12 pass on hyper, axum and actix-web; the matrix gains the row. `non-Send handlers` stays `no` (the `LocalHandler` capability is not built). Not published.

## [0.1.4] - 2026-10-30

- Slice 3: the client half. New crate `httpant-client`: `Client` (`get`/`post`/`put`/`delete`/`head`/`request`, repeatable raw-byte headers, byte or streaming bodies, `timeout`), the `ClientAdapter` trait, and the hyper-util reference adapter (plain `http://`; `https://` is a named `UnsupportedScheme` error until the TLS hook exists; redirects are returned, not followed, and declared so). Response bodies are read only with a limit (`bytes`/`text` use the 2 MiB default, `*_limited` take one). `httpant-core` gains `ClientCapabilities` and the errors `InvalidUrl`, `InvalidHeader`, `UnsupportedScheme`, `Timeout`, `Request`.
- Client conformance (`httpant-conformance`): 7 cases against a raw-TCP peer written in the suite, never HTTPAnt's own server: method fidelity, headers, bodies, statuses, streaming response, streaming request, connection reuse; 7 known-bad client mutants, each shown to fail its case for the intended reason. A redirect probe measures follow/no-follow. The client matrix is in `docs/CAPABILITY-MATRIX.md`, rendered by the same stale-document test, and declared client capabilities must equal the measured ones. Timeouts live above the adapter, so they are tested in `httpant-client`, not as a case.
- The hello-world example now fetches with `Client` (no raw socket).
- `SECURITY.md` (new): supported versions, quoted from the portfolio support text, and a private-report route. README: `web-server-abstraction` 1.0.x is end of life and unsupported; use `httpant`. Not published.

## [0.1.3] - 2026-10-30

- Slice 2b: conformance cases 8-12 on every adapter. 8 streaming (response chunks and request chunks cross the wire while the other side is still producing them, gated by handler signals, not sleeps), 9 graceful shutdown, 10 keep-alive / `Connection: close` / `Expect: 100-continue`, 11 concurrency (1,000 requests, 250 at a time, each answered with its own number, handlers overlap), 12 HEAD. Six new known-bad mutants, each shown to fail its case. First capability matrix (`docs/CAPABILITY-MATRIX.md`), rendered from the suite by a test that fails when the document is stale; each adapter's declared `Capabilities` must equal what the suite measured.
- **Fixed a defect case 9 found in the 0.1.2 hyper adapter:** `shutdown()` only stopped accepting; idle keep-alive connections stayed open and kept being served, and `shutdown()` did not wait for requests in flight. It now drains: the listener closes, in-flight requests finish, idle connections close, and it returns when they have. (No grace period yet.)
- `Router`: `HEAD` runs the `GET` handler when no `HEAD` route exists (the server drops the body); `405 Allow` lists `HEAD` beside `GET`.
- The suite's raw client gains `Conn`, a connection that stays open (chunk-by-chunk reads, keep-alive, interim `100`, who-closes-when). Not published.

## [0.1.2] - 2026-10-30

- Slice 2a: axum adapter (`AxumAdapter`, feature `axum` of `httpant-server`, `Server::with_adapter("axum")`); axum 0.8 serves the one boxed service as its fallback, so routing and error mapping are HTTPAnt's. The conformance suite now runs cases 1-7 on every adapter (hyper and axum both pass). Not published.

## [0.1.1] - 2026-10-29

- Slice 1b (provisional option A): `httpant-core` gains `Router` (`:name`/`*rest`, exact-method matching, 405 with `Allow`, no path normalisation), the `Handler` trait, Tower layer helpers; `BoxService` is now cloneable. New `httpant-server` with `Server`, the `ServerAdapter` trait and the hyper reference adapter. `httpant-conformance` now runs cases 1-7 over real sockets, each with known-bad mutants that it must fail on (9 mutants). The hello-world example runs in CI and escapes control characters in anything it prints from the network (CodeQL log injection), with a test. Not published.

## [0.1.0] - 2026-10-29

- **Restart of the version line, taken knowingly (PM 2026-10-29).** The `httpant-*` crates are a new project, not a continuation of `web-server-abstraction` 1.0.x, so the number drops from 1.0.10 to 0.1.0; the old 1.0.x line stays end-of-life on crates.io, not yanked. The version-drop is deliberate.
- Slice 1a of HTTPAnt (provisional option A): workspace with `httpant-core` (http types, streaming `Body` with explicit limits, `Error`, `IntoResponse`, `Capabilities`, boxed Tower service type) and `httpant-conformance` (skeleton: case registry for cases 1-7, raw-TCP HTTP/1.1 client). Removed the old `web-server-abstraction` source tree from `main` (kept in git history); `cargo deny` is now green. Not published.

## [1.0.10] - 2026-10-29

- Added .github/CODEOWNERS and docs/PATENT-SWEEP.md (an honest "not yet performed" record; nothing may be published until it is replaced by a real sweep). Not published.

## [1.0.9] - 2026-10-29

- Added docs/HTTPANT-DESIGN.md: design for HTTPAnt (core, server half, client half, conformance suite, options A/B/C). Docs only, not published.

## [1.0.8] - 2026-10-28

- Added docs/BENCHMARKS.md (TechEmpower R23, Rust JSON and Fortunes) and docs/ACTIX-ADAPTER-NOTES.md (actix service model, actix-tower 0.1.4). Docs only, not published.

## [1.0.7] - 2026-10-28

- README: known-issues section for web-server-abstraction 1.0.2 (superseded by HTTPAnt), from the survey. Docs only, not published.

## [1.0.6] - 2026-10-28

- Added docs/OLD-CRATE-SURVEY.md: survey of this crate for HTTPAnt (what to keep, what to discard, first conformance cases). Docs only, not published.

## [1.0.5] - 2026-10-28

- Added CI (fmt, clippy -D warnings, build+test on ubuntu and windows, doc, cargo deny) and deny.toml, same shape as mcpant; committed Cargo.lock; cargo fmt; h2 updated past RUSTSEC-2026-0258. Not published.

## [1.0.3] - 2026-10-28

- Repository moved to ThinkersJournal/httpant (HTTPAnt); HANDOFF for the new lane. Docs only, not published.

## [1.0.2] - 2025-08-26

### Fixed

- **Documentation Accuracy**: Updated all documentation to reflect the true status of framework adapters
- **Version Consistency**: Corrected version references throughout documentation
- **Framework Status**: All adapters (Rocket, Salvo, Poem) are confirmed working and production-ready

### Changed

- **README.md**: Updated framework status table to show all adapters as complete
- **Copilot Instructions**: Corrected outdated status indicators for framework adapters
- **Cargo.toml**: Updated comments to reflect actual working status of all frameworks

## [1.0.1] - 2025-08-26

### Fixed

- Code quality improvements across all modules
- Resolved clippy warnings and style issues
- Fixed pattern matching and import optimization
- Enhanced error handling consistency
- Updated framework adapter documentation accuracy

### Changed

- Improved test coverage reporting (107/107 tests passing)
- Enhanced middleware performance optimizations
- Better dead code annotations for future features

## [1.0.0] - 2025-08-21

### Added

- Completed all framework adapters (Rocket, Salvo, Poem)
- Stable API with full backward compatibility guarantees
- Mountable interface support for modular applications
- Advanced authentication integration
- Full WebSocket support across all framework adapters
- Comprehensive documentation and examples

### Changed

- Refined middleware API for better ergonomics
- Improved error handling and reporting
- Enhanced performance in core components

### Fixed

- Compilation issues with Rocket adapter
- Memory leaks in long-running connection handling
- Thread safety issues in state management

## [0.3.0] - 2025-06-15

### Added

- Warp adapter implementation
- Advanced routing features (path parameters, wildcards)
- Basic WebSocket support
- HTTP method convenience functions (get, post, put, delete, patch)
- Scaffolding for Rocket, Salvo, and Poem adapters

### Changed

- Improved request/response handling
- Enhanced middleware composition
- More efficient path matching algorithm

## [0.2.0] - 2025-04-10

### Added

- Complete Axum adapter
- Actix-Web adapter
- Enhanced middleware ecosystem (9 middleware types)
- Performance benchmarks and profiling infrastructure
- Improved testing utilities

### Changed

- Refactored core abstractions for better performance
- Enhanced error handling with more context
- Improved documentation

## [0.1.0] - 2025-02-28

### Added

- Initial release
- Core abstractions and API design
- Mock adapter for testing
- Basic middleware system
- Documentation and examples
