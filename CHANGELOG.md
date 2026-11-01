# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.33] - 2026-11-01

- ROADMAP row 13, step 1 (PM ruled 2026-11-01: opt-in, off by default): `docs/H2C-POLICY.md` records, from reading the sources (not run), that axum 0.8.9, poem 3.1.12, salvo_core 1.0.1 and warp 0.4.3 have no setting to refuse the h2c preface; axum, poem and salvo can be handled by wrapping their public accept path, warp cannot (private `Accept`). Docs only, no code change. Not published.

## [0.5.32] - 2026-10-31

- HANDOFF rewritten to current state (0.5.23 to 0.5.31 merged, rows 9, 10 and 13 waiting on the PM, peers retired for Synapse). ROADMAP row 4 says slices (a) and (b) are done and no longer says "PR open" for work that has merged; row 15 and the warp TLS blocker re-checked (HANDOFF). Docs only, no code change. Not published.

## [0.5.31] - 2026-10-31

- ROADMAP row 13: `docs/H2C-POLICY.md` sets out the cleartext HTTP/2 options (refuse everywhere, leave per adapter, opt-in off by default) with the compatibility consequence of each and a recommendation (opt-in, after reading whether axum, poem, salvo and warp can refuse the preface). Options only: nothing built, nothing run. Docs only, no code change. Not published.

## [0.5.30] - 2026-10-31

- ROADMAP row 9: `LocalHandler` analysed (read, not built) and the result written into `docs/ACTIX-ADAPTER-NOTES.md`: the one `Send` service carries every wrapper (limits, shutdown, ACME prefix, `Origin`, redirect), so a `!Send` route would silently bypass them; recommendation to defer until a consumer names it. Docs only, no code change. Not published.

## [0.5.29] - 2026-10-31

- ROADMAP row 10: ConfigurAnt's API read (not run) at its `bb7545e` and the result written into design section 10, replacing "UNVERIFIED": settings can reach HTTPAnt through a plain serde struct with no dependency on ConfigurAnt (whose `rust-version` is 1.95). A recommendation, nothing built. Docs only, no code change. Not published.

## [0.5.28] - 2026-10-31

- **Extractors (ROADMAP row 7):** a handler can take `Path<T>`, `Query<T>` and `PathParams` (and your own type through the new `FromRequest` trait) as arguments, one to four of them, built before the handler runs. `Path<T>` and `Query<T>` deserialize with serde into a struct or map (`Option` fields are optional); numbers and other text-parsed types are parsed. A value of the wrong kind, a missing required query field, or query bytes that are not UTF-8 are answered `400` before the handler runs, with a message that never repeats what the client sent (tested); a `Path` field the route does not capture is the handler's mistake and is `500`, naming no field. A failing extractor means the handler does not run (tested). The same on every adapter (`tests/extractors.rs` runs hyper, axum, actix-web, poem, salvo and warp). `httpant-core` gains two dependencies already in the lock file (`serde`, `serde_urlencoded`); the `Request` and no-argument handler shapes are unchanged. Not published.

## [0.5.27] - 2026-10-31

- HANDOFF rewritten to current state (0.5.23 to 0.5.26 merged, roadmap has no `next` rows, watch rows named). Docs only, no code change. Not published.

## [0.5.26] - 2026-10-31

- **Benchmark harness (ROADMAP row 5):** new unpublished crate `httpant-bench` runs each adapter against the bare web library it wraps (hyper, axum, actix-web, poem, salvo, warp) on the same constant route, machine and load (64 keep-alive connections), alternating sides between rounds, checking every response. First measurement in `docs/BENCHMARKS.md`: HTTPAnt served 0.91 (hyper), 0.93 (axum), 0.81 (actix-web), 0.93 (poem), 0.90 (salvo) and 0.69 (warp) of the bare library's requests per second; p99 latency 10 to 60 percent higher. The doc says what that supports and what it does not (the load generator may be the limit; spread not kept). A short run of all twelve servers is a test, so the harness is compiled and run in CI. No change to any published crate.

## [0.5.25] - 2026-10-31

- **actix-web now terminates TLS (`tls: yes`; HTTP/1.1 only, `http2: no`)**, the optional "clean route" `docs/TLS-ADAPTER-PROBES.md` described. `HttpServer` cannot take a stream already through the handshake, so for TLS the adapter skips it: HTTPAnt's own acceptor (the one hyper, axum, poem and salvo use) accepts and handshakes in tasks on the caller's runtime, and hands each finished stream to `actix_http::HttpService` over the same `actix_web::App` default service on an actix thread, as HTTP/1.1. So a slow certificate source costs only its own handshake (the spike had blocked an actix worker: 2035 ms for a fast handshake behind six slow ones), and the audit events come from HTTPAnt's acceptor. Cleartext still uses `HttpServer` with two workers; **the TLS listener is one actix thread**, so a TLS actix-web server is not multi-core. Cleartext h2c stays `no`. **HTTP/2 is deliberately not offered:** `actix_http`'s HTTP/2 is `h2` 0.3, and with it on `cargo deny` fails on RUSTSEC-2026-0258 (empty DATA frames queued without limit, low severity, patched only in `h2` 0.4.16, no 0.3 fix); I turned it on, saw `cargo deny check advisories` fail, and left the `actix-http/http2` feature off rather than touch `deny.toml`. No new crates (actix-http and actix-service were in the graph already); the workspace names them as dependencies of the `actix-web` feature.
- **Evidence:** conformance case 17 runs on actix-web (its mutants still fail) and case 18 on its `tls: yes`, `http2: no` branch (the first adapter on it); `tests/tls.rs` includes it; new `tests/tls_isolation.rs` (a fast handshake while six wait 1500 ms on the certificate source must take under half of that, on every TLS adapter, with a control that the slow ones were slow) and `tests/tls_shutdown.rs` (graceful shutdown over TLS on every TLS adapter: the request in flight completes, an idle keep-alive connection is closed, `shutdown` returns after; removing the wait for requests in flight fails it on actix-web only). The capability matrix, README and probe notes say actix-web terminates TLS. Not published.

## [0.5.24] - 2026-10-31

- Documentation only: `docs/PATENT-SWEEP.md` gains the **re-sweep** its first scope line called for, for HTTP/2, the ACME handlers, client https and the private-address guard, at `6d564a0` (0.5.22): twelve queries, nine patents read in full (all independent claims), every one `outside as read` with the missing element named, none needing review. It also records, in the order it happened, that 0.5.19, 0.5.21 and 0.5.22 were published before this was done. Not legal advice; the absence of hits is weak evidence and the file says so. No code changed. Not published.

## [0.5.23] - 2026-10-31

- Documentation only: ROADMAP row 12 (the once-seen failure of `the_whole_suite_passes_on_every_adapter`) now records the measurement asked for: 40 consecutive runs of that test under 32 saturating CPU-bound processes on a 32-core Windows machine at `6d564a0`, all passed, 33 s to 45 s each. Not reproduced, and not claimed fixed; the row stays on watch, and the test's failure message already names the adapter and the failing cases. No code changed. Not published.

## [0.5.22] - 2026-10-31

- **The client's private-address guard (slice 2 of the client-https design; WebHostAnt need 10).** `Client::builder().refuse_private_addresses(true)` (off by default) makes the client refuse, with the new `Error::PrivateAddress`, to connect to loopback, private-network, link-local (the cloud metadata address 169.254.169.254 among them), carrier-grade-NAT, multicast, documentation and reserved addresses. It is checked on the address actually connected to, in three places: the name lookup keeps only public answers (so a name that resolves to a private address, or to a public and a private one, gets no private address to try and there is no second lookup to change), an IP literal in the URL is refused before connecting (hyper-util never sends a literal to the resolver), and the connection is looked at once more after it is made, before TLS and before any request byte. An IPv4 address inside an IPv6 one (`::ffff:10.0.0.1`, NAT64 `64:ff9b::/96`, 6to4 `2002::/16`) is judged as the IPv4 address. With the guard off both wrappers pass everything through, so there is one connector path, not a guarded and an unguarded one. A followed redirect (not built yet) would go through the same connector. Through a proxy (not built yet) the address reached would be the proxy's, which the proxy slice must say.
- **Conformance: client case 9 `client-private-guard`.** A plain TCP peer on loopback counts connections; a guarded client must refuse an IPv4, an IPv6 and an IPv4-in-IPv6 loopback literal **and the name `localhost`**, each with `Error::PrivateAddress` and no connection made, while the same peer answers a client that was not told to refuse. New mutant `client-guard-checks-name-only` (turns away a private IP written in the URL and nothing else) fails it on the name. Unit tests cover the classification (a table of private addresses and of public ones next to them, including the range edges), the resolver on a mixed public and private answer (the shape a rebinding answer takes), the literal check (no connection made), and the post-connect look on its own with an unfiltered resolver (the backstop, with a positive control that the unguarded connector does connect). Not tested: a real DNS server changing its answer between two lookups (not reproducible in CI); the argument is that the filtered answer is the only list the connector tries.
- Not published. Slice 3 (OS root store, proxy, opt-in redirects) is not started.

## [0.5.21] - 2026-10-31

- **The client calls `https://` (slice 1 of the client-https design, `docs/HTTPS-CLIENT-DESIGN.md`).** New cargo feature **`https`** on `httpant-client` and on `httpant` (off by default; without it an `https://` URL is the error ``scheme `https` is not compiled in: enable the `https` feature``): hyper-rustls on rustls 0.23 with the `ring` provider (as the server), TLS 1.2 and 1.3, the bundled Mozilla roots (`webpki-roots`), the certificate chain and the host name verified. There is **no switch to turn verification off** in the API. rustls **does not check revocation** (the README says so). `Client::builder()` is new: `connect_timeout` (default 10 s), `timeout` (response head, default 30 s), `read_timeout` (a body quiet between chunks, default 30 s), `add_root_pem` (trust a private CA, an empty or key-only PEM is an error); `Client::new()` has the same finite defaults. New `RequestBuilder::bearer` and `Client::patch`; `Authorization`, `Proxy-Authorization`, `Cookie` and `bearer` values are marked sensitive, so `Debug` never prints them, and `RequestBuilder` has a `Debug` that shows header names only.
- **Fixed: errors leaked the query string.** `Error::InvalidUrl` printed the URL as given, a `?token=...` included; it now prints scheme, host and path only (no user name, password, query or fragment). New errors name which step failed: `ConnectTimeout`, `ReadTimeout` (and the existing `Timeout` is the response head), `TlsHandshake` (a certificate that is untrusted, expired or for another name, ending with what to do), `SchemeNotCompiledIn`. Transport errors now carry their whole cause chain. `Error` is `#[non_exhaustive]`, so these are additions.
- **Conformance: client case 8 `client-https`** against a TLS server written in the suite (self-signed certificates): a trusted certificate for the right name is fetched over TLS; an untrusted one, one for another host name, and a self-signed one with no extra root are each refused with a TLS handshake error before any request is sent. Two new mutants that must fail it: `client-accept-any-certificate` and `client-ignore-host-name` (a verifier that forgives a name mismatch). The capability matrix's client TLS cell is now **measured** (`yes`). The mutants are built inside the unpublished conformance crate with its own rustls connector; the published client has **no** way to be handed a rustls configuration, so no consumer can supply a verifier.
- Not in this slice: the private-address guard (slice 2), the OS root store, proxies, opt-in redirects (slice 3). Cost of the feature, measured (Windows, `CARGO_BUILD_JOBS=2`, cold `cargo build -p httpant-client`): 39 to 51 crates (cfg-if, getrandom 0.2, hyper-rustls, ring, rustls, rustls-pki-types, rustls-webpki, subtle, tokio-rustls, untrusted, webpki-roots, zeroize) and 47 s to 63 s. Licences: ring is `Apache-2.0 AND ISC`, webpki-roots `CDLA-Permissive-2.0`, the rest ISC or MIT/Apache-2.0; `cargo deny check` passes unchanged. Minimum Rust 1.88 still holds with the feature on. Not published.

## [0.5.20] - 2026-10-31

- Documentation only: `docs/HTTPS-CLIENT-DESIGN.md`, the design note for https in the client (options and a recommendation, nothing built), and ROADMAP row 14. HANDOFF rewritten. It records, against WebHostAnt's 13 needs: rustls with the `ring` provider (as the server), bundled roots by default with the OS store as an explicit choice, a cargo feature `https`, and a private-address guard that works in three places (a filtering resolver, a check of IP-literal hosts that never reach the resolver, a post-connect check), plus one defect found while reading: `Error::InvalidUrl` prints the whole URL, query string included. Not published.

## [0.5.19] - 2026-10-31

- **Fix: the Windows leg of CI was red on `main` after the restructure (#37).** Two examples were both called `hello` (the root `httpant` one a stranger copies, and `httpant-server`'s), so both wrote `target/debug/examples/hello.exe` (cargo warned `output filename collision`) and on Windows, linking at the same time, one failed with `LNK1104: cannot open file ... hello.exe`. The server crate's example is now `server_hello` (`cargo run --example server_hello -p httpant-server`). The root example, the README and the hub card are untouched. Not published.

## [0.5.18] - 2026-10-31

- **The package, not the checkout** (first-release gate, EXPECTATIONS 6.24). Nothing is published and no adapter behaviour changed.
- **README.** The repository README was the old crate's known-issues page; it is now HTTPAnt's front page: the ten-line hello-world first (the one in `examples/hello.rs`, compiled and run by rustdoc as a doctest and by CI as an example), install, choosing the web library by id, what you get, **what it does not do yet**, status, and a pointer from `web-server-abstraction`. The old crate's known-issues text moved, with its wording unchanged, to `docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md`. `crates/httpant-conformance/tests/readme_example.rs` keeps the README block, `examples/hello.rs` and the hub card (`easyants-card.toml`, `example_lines = [2, 11]`) the same program; changing one fails the test (checked by editing the example).
- **Metadata.** `readme`, `keywords`, `categories` and `rust-version` on all four publishable crates; `rust-version = "1.88"` is the default build (measured, kept by a new CI job; the optional adapters can need 1.89 to 1.95). The root `httpant` ships exactly 10 files (`include`: it was the whole repository). Each member crate gets a short README that points at `httpant`, and copies of both licence files, which CI compares with the repository's.
- **Hub card.** `easyants-card.toml` (status `ready`, install `cargo add httpant`, example `examples/hello.rs` lines 2 to 11). The hub registry entry still has no `repo`, so nothing reads the card until the first release.
- **Fresh-eyes trial, passed** (`docs/FRESH-EYES-TRIAL.md`): a fresh Sonnet agent given only the packaged README, offline, behind a directory source holding the four crates exactly as packaged, reached the example's result and the other two tasks first time with no error or warning. It found a **wrong claim in the README**: it said an unknown adapter id gets an error that lists the ids and the feature, but there are two errors (`unknown adapter ...: use one of [...]` for an id that does not exist; `adapter ... is not compiled in: enable the ... feature` for one whose feature is off). The README now quotes both. Also fixed from the trial: the first example's silence (exit status 0 is success), the second example (now the first program with one call changed, complete and a doctest), how to print an error, what `httpant::Result` is. A second run (PM: re-run on the revised package), a new agent on the re-packaged crates, **also passed** (all three tasks first attempt, no error or warning, both error messages matching the README character for character); it asked for an error-printing snippet (added, a third README doctest) and an explanation of `Origin` (added). The ROADMAP gains the first-release gate table.
- **CI.** `cargo publish --workspace --dry-run --locked` packages the four crates in dependency order (core, client, server, httpant) and builds each from its tarball, resolving the others from the tarballs just made; the licence-copy check; and an `msrv` job on Rust 1.88.0. Publish order, from that dry run: `httpant-core`, `httpant-client`, `httpant-server`, `httpant`; `httpant-conformance` is `publish = false`. Not published.

## [0.5.17] - 2026-10-31

- **Restructure only: `httpant` becomes the root package** of the repository. A stranger writes `use httpant::{Client, Server}` (the design's hello-world, which had no crate behind it: the name was claimed on crates.io at 0.0.0 but nothing in the workspace was called `httpant`). The root package is a thin facade: `httpant::Client`, `Server`, `ServerAdapter`, `ClientAdapter`, `RunningServer`, `ServeOptions`, everything `httpant-core` exports (`Result`, `Error`, `IntoResponse`, `Handler`, `Router`, the `http` types), and `httpant::server` / `httpant::client` for the rest of each half. Features forward to `httpant-server`: `axum`, `actix-web`, `poem`, `salvo`, `warp`, `tls`, `http2`; the hyper adapter is always there. `examples/hello.rs` is the ten-line hello-world (plus the licence header), compiled and **run** in CI (`cargo run --example hello -p httpant`, and `test = true`).
- **What moves: nothing.** `httpant-core`, `httpant-server`, `httpant-client` and `httpant-conformance` keep their names and paths under `crates/`; the workspace gains one member (the root package) and `default-members` lists all five, so `cargo build` / `cargo test` at the root cover what they covered before. Why the root, not `crates/httpant`: the EasyAnts hub card reads its example from a path matching `^(examples|tests)/` at the repository root (`easyants/src/lib/card.ts`), which a virtual workspace cannot offer, and a root package lets the README, the licences and `examples/` be the repository's own with nothing copied. No README, metadata or packaging change is in this PR (they follow). Not published.

## [0.5.16] - 2026-10-31

- Conformance: case 10 (`connection-handling`) now has mutants for all three of its parts (ROADMAP row 8); it had one, for keep-alive. `Connection: close` and `Expect: 100-continue` are decided by the HTTP/1.1 layer below the service, so no wrapper around the service can break them; the mutants are subjects. `mutants::serve_stripping` is a copy of the reference accept loop whose stream drops one whole header line (matched without case) from what the HTTP/1.1 layer reads, so that layer never sees it, with the real application running on top. `ignore-connection-close` strips `Connection: close` (the server keeps the connection open after it); `skip-expect-continue` strips `Expect: 100-continue` (no `100 Continue` is sent, so a client that waits for it before the body never gets it). Both fail case 10, and a new test checks that each of the three case-10 mutants fails with the message that names its own defect, so none can pass by failing somewhere unrelated. The capability-matrix doc's sentence saying these two parts could not be shown to fail is replaced. Conformance and docs only; no adapter changed. Not published.

## [0.5.15] - 2026-10-31

- Documentation only: rocket's refusal of extension methods is now **measured** (ROADMAP row 6; PM: "read, not run" is not acceptable for the release matrix). A throwaway probe ran a real rocket 0.5.2 with one route and a raw TCP client: `GET /` `200` (control), `PROPFIND /` and `FOO /` **`400 Bad Request`**, and `PATCH`, `CONNECT`, `TRACE` (known methods, no route) `404`. The `404` rows are the control for the `400`: the same route table answers a known method without a route with `404`, so the `400` is the method being refused. This confirms the source reading behind CHANGELOG 0.1.9 (an extension method never reaches a route, so conformance case 1 cannot pass); rocket stays out. The design note's row says "measured" and points at `docs/ROCKET-PROBE.md`, which holds the result and the whole probe so it can be re-run. Not covered: a rocket fairing or request guard that might see the request first, and later rocket releases. Not published.

## [0.5.14] - 2026-10-31

- **salvo serves HTTP/2 over TLS**, the last HTTP/2 item for the adapters that terminate TLS: every one of them (hyper, axum, poem, salvo) now offers `h2`. With the `http2` feature of `httpant-server` it enables salvo's own `http2`; salvo's connection builder reads the first bytes and serves HTTP/2 or HTTP/1.1, so HTTPAnt only has to offer `h2` on the listener it already owns (`salvo_tls.rs`). Declared `http2: Yes`. `tests/http2.rs` now runs on salvo as well: h2 negotiated and served, 8 concurrent streams, an HTTP/1.1 client on the same listener, and the 2 MiB limit answering `413` over h2. Salvo's builder also detects the h2 preface on cleartext, so its cleartext h2c answer **changed from `no` to `yes`** with the feature on (re-measured: the matrix now has axum, poem, salvo and warp accepting h2c, hyper and actix-web not).
- **The case-18 mutant needed a server of its own.** `offers-h2-without-serving-it` used salvo as the HTTP/1.1-only server behind an acceptor that offers `h2`; with salvo serving HTTP/2 no real adapter is HTTP/1.1-only on TLS any more. `mutants::serve_http1_only_behind_h2_offer` is a small hyper HTTP/1.1 server behind a TLS acceptor that offers `h2`; case 18 still fails on it and passes on the reference. The read-back test's ALPN expectation no longer has a salvo exception. Not published.

## [0.5.13] - 2026-10-31

- The cleartext HTTP/2 (h2c) difference between adapters is now **measured and shown** in the capability matrix (PM ruling: record it as measured, do not paper over it). A probe in the conformance suite (`h2_case::h2c_support`, field `Measured::h2c`) sends the HTTP/2 connection preface and an empty SETTINGS frame to a cleartext listener and reads the reply: an HTTP/2 SETTINGS frame is `yes`; HTTP/1.1 text, a closed connection or silence is `no`. It is a probe, not a case: nothing in the need list asks for h2c, so no adapter is failed for either answer. Result with the `http2` feature on: **axum, poem and warp accept h2c; hyper, actix-web and salvo do not.** Warp was not known to accept it before this. The matrix gets a *cleartext h2c (measured)* column and a note saying what `yes` and `no` mean and that the column does not say which is right. Documentation and a probe only; no adapter behaviour changed. Not published.

## [0.5.12] - 2026-10-30

- HTTP/2 over TLS (WebHostAnt need 12: offer `h2` and `http/1.1`, with the chosen protocol visible), first slice, behind the new `http2` feature of `httpant-server` (hyper's `http2`, hyper-util's `server-auto`; pulls the `h2` crate). New capability `Capabilities::http2`. `TlsAcceptor::alpn_protocols(&[..])` sets what the listener offers (default `http/1.1`); **an adapter sets it to what it really serves**, `h2` first only if it speaks HTTP/2. The hyper adapter uses hyper-util's auto connection builder on TLS connections (HTTP/2 when negotiated, HTTP/1.1 otherwise) and keeps cleartext listeners on HTTP/1.1. axum and poem hand their own connection builders the TLS stream and offer `h2` too. Declared `http2: Yes` (with `tls` and `http2` on): hyper, axum, poem. Declared `No`: salvo (HTTP/1.1 on TLS), actix-web and warp (no TLS). `read_back` now offers `h2` and `http/1.1`, so the ALPN it reports is the one the listener would choose.
- Conformance case 18 `http2-alpn` on every adapter, client = hyper's HTTP/2 client and tokio-rustls: `Yes` negotiates `h2` with a client offering `h2` and `http/1.1` and answers a request over HTTP/2 with the request reported `https`; `No` with TLS `Yes` negotiates `http/1.1` and answers over it, never an `h2` it cannot serve; TLS `No` has nothing to negotiate (case 17); `Unknown` fails. Mutant `offers-h2-without-serving-it` (salvo, which speaks HTTP/1.1 on TLS, behind an acceptor that offers `h2`) must fail it. Trying the same mutant on axum and on poem showed they *do* serve h2 when offered (the case passed on them), which is what made it safe to declare them `Yes`. `tests/http2.rs` over real sockets on hyper, axum and poem: h2 negotiated and served, 8 concurrent streams, an HTTP/1.1 client on the same listener, and the 2 MiB body limit answering `413` over h2. The capability matrix gains an *HTTP/2 (over TLS)* column.
- **Found, not decided:** cleartext HTTP/2 (h2c by prior knowledge) is not uniform with the `http2` feature on: axum's and poem's builders accept the preface on a cleartext listener, hyper's (here `http1_only`) refuses it. Nothing in the need list asks for h2c, so it is neither required nor forbidden by conformance; ROADMAP has the row. salvo, actix-web and warp are not HTTP/2 yet. Not published.

## [0.5.11] - 2026-10-30

- TLS-ALPN-01 validation on the same port (WebHostAnt need 13), feature `tls`, on every adapter that terminates TLS (hyper, axum, poem, salvo), done once in `TlsAcceptor` below the adapters. `CertSource::acme_tls_cert(server_name)` (default: `None`, nothing pending) returns the challenge certificate for a name while a validation is pending; the source builds it (with its `acmeIdentifier` extension) and HTTPAnt only routes the handshake. The acceptor asks only when the client hello offers the ALPN protocol `acme-tls/1`; if the source has a challenge pending it answers with that certificate alone, negotiates `acme-tls/1`, completes the handshake, closes the connection, and the application never sees it. New audit event `TlsEvent::AcmeChallengeServed { server_name }` (not a refusal). If nothing is pending, a validator that offers only `acme-tls/1` is refused like any client with no protocol in common (`HandshakeRefused`, `Protocol`) and is never given the normal certificate under a protocol it did not agree on. An ordinary client is unaffected and the source is not asked about challenges for it. A client that offers `acme-tls/1` together with other protocols is treated as a validator while a challenge is pending. `tests/tls.rs` on all four adapters; offering the challenge to every client as a mutation fails the test. `docs/TLS-ADAPTER-PROBES.md` question 3 answered.
- Fixed: `httpant-server` used tokio's `AsyncWriteExt` without enabling tokio's `io-util` feature; the workspace build hid it because another crate enables it, and `cargo clippy -p httpant-server --features tls` (the crate alone) failed to compile. The feature is now declared. Not published.

## [0.5.10] - 2026-10-30

- Documentation only: the actix-web row of `docs/TLS-ADAPTER-PROBES.md` and the capability matrix note are now a **run**, not a reading (PM ruling: "read, not run" is not acceptable for the release matrix). A spike (`spike/actix-tls` at e799dc8, not merged) enabled `actix-web/rustls-0_23` and bridged HTTPAnt's async `CertSource` into actix's synchronous certificate resolver by blocking the worker thread. Result: case 17 and refusal of unknown names and failing sources work; a renewed certificate first failed to reach resuming clients (fixed by turning the session cache and tickets off); but a slow certificate source freezes the worker (a fast handshake took 2035 ms behind six 700 ms ones, against 7 ms on hyper and axum) and HTTPAnt's audit events for timeouts, protocol failures and certificate changes cannot be sent because actix-tls runs the handshake. actix-web stays `tls: No`, now with the run as evidence. Not published.

## [0.5.9] - 2026-10-30

- **poem and salvo now terminate TLS** (`Capabilities::tls` is `Yes` with the `tls` feature), the result of probing the four libraries that declared `tls: No` from memory (`docs/TLS-ADAPTER-PROBES.md`). poem takes any `poem::listener::Acceptor`: `poem_tls.rs` accepts TCP, runs HTTPAnt's handshake in a task of its own and hands over the finished stream. salvo takes any `salvo_core::conn::Acceptor`: `salvo_tls.rs` wraps the existing one so each connection's handshake is HTTPAnt's, inside salvo's own `HandshakeStream` (so its lazy handshake and timeout apply). Both report the request as `https` from the connection. `TlsAcceptor::accept` is now generic over the IO type (it was `TcpStream`), which salvo's wrapped stream needs. Proof is conformance case 17 (it ran the full HTTPS scenario on them the moment they declared `Yes`) and `tests/tls.rs`, which now runs on hyper, axum, poem and salvo: refusal on unknown name and source failure, renewal, handshake timeout, read-back, audit events.
- **warp and actix-web stay edge mode only, now with evidence.** warp 0.4.3: its `Accept` trait is in a private module, and implementing it from outside fails to compile with `E0603: module server is private`, "trait Accept is not publicly re-exported" (run); its own `Server::tls` is an empty stub. actix-web 4.18.1: `HttpServer` has no acceptor or stream entry point, only `listen_rustls_0_23(lst, rustls::ServerConfig)` whose certificate resolver is synchronous, so an async `CertSource` that may refuse a handshake cannot be honoured (read, not run). Case 17 still holds both to a named refusal. The capability matrix is re-blessed (terminates TLS: hyper, axum, poem, salvo). Not published.

## [0.5.8] - 2026-10-30

- Conformance case 17 `tls-terminate` on every adapter, and the rule behind it in one place. `httpant_server::require_tls(&dyn ServerAdapter)` (feature `tls`) is "TLS is only asked of an adapter that declares `tls: Yes`, else `Error::TlsNotSupported`"; `Server::serve` and the conformance `AdapterSubject` both call it, so the suite tests the rule `Server` applies and not a copy. The case holds each adapter to what it declares: `Yes` (hyper, axum): HTTPS on TLS 1.3 and on TLS 1.2 with the certificate it was given, the request reported as `https` by the connection (`127.0.0.1 https direct`), and a cleartext client gets no `200`; `No` (actix-web, poem, salvo, warp): refuses to start with a TLS acceptor, with the named error, so TLS asked for is never served in clear; `Unknown`: fails. Mutant `serve-tls-in-clear` (asked for TLS, serves cleartext) must fail it. Beyond the mutant, an adapter wrongly declaring `Yes` (warp, tried by hand and reverted) fails the whole-suite test.
- `Setup` gains `tls`; the conformance crate gains `rustls`, `tokio-rustls` and `rcgen` (it is `publish = false`). The server capability matrix (`docs/CAPABILITY-MATRIX.md`, blessed) gains a *terminates TLS* column: declared, held to its declaration by case 17; a `no` is not a measurement of what the web library could do, only that HTTPAnt does not terminate TLS on it yet (probing them is still open, ROADMAP row 4). Not published.

## [0.5.7] - 2026-10-30

- TLS audit events (WebHostAnt need 23, for AuditAnt), feature `tls`. `TlsAcceptor::audit(sink)` takes a `TlsAudit` (`fn event(&self, &TlsEvent)`, called on the connection's task, so it must not block). Two events: `HandshakeRefused { server_name, reason }` with `RefusalReason` `UnknownName`, `SourceFailed`, `Timeout` or `Protocol` (a plaintext client, a malformed hello, a failed handshake), and `CertificateChanged { server_name, serial }` when the certificate completed-handshake-served for a name differs from the one served for it before (a renewal or replacement; never for the first certificate a name gets, and only after the handshake completed). Events carry no key material, no certificate bytes, and not the text of a source's error (that text is the source's own business and could hold anything); a name in an event is client-supplied, so it is cut to 253 bytes and anything not printable ASCII becomes `?`, which makes it safe to write into a log line as it is. The memory that notices a change is bounded (1024 names; beyond that, further names are served as before but their changes are not reported), so a client inventing server names cannot grow it. **Not done: "listener rebound"**: HTTPAnt has no operation that rebinds a listener (a supervisor hands it a bound socket), so there is no such event to send; if rebinding is added it needs its event then.
- Tests over real sockets on hyper and axum (`tests/tls.rs`): nothing reported for a first or repeated certificate, exactly one `CertificateChanged` after a swap, unknown name and failing source each with their own reason and the name, the source's message and the key absent from the event, a silent client reported as `Timeout` with no name, a plaintext client as `Protocol`. Unit tests for the name cleaning and the bounded change memory. Treating the first certificate as a change made the test fail, as it should. Not published.

## [0.5.6] - 2026-10-30

- Port-80 redirect to HTTPS (WebHostAnt need 18), on every adapter. `Server::redirect_to_https(HttpsRedirect::new(["example.com", ..]).port(8443))` / `ServeOptions::redirect_to_https`: a cleartext request for a **listed** host gets `308 Permanent Redirect` (method and body kept) to `https://<host><path and query>`, with `:<port>` only when the port is not 443. Anti-abuse rules: the `Host` must be in the list (compared without case, port or trailing dot; IPv6 literals keep their brackets), any other host is `421 Misdirected Request` and a missing one `400`, so a forged `Host` cannot turn this server into an open redirect; text that is not a plausible host (spaces, `/`, `@`, `#`, CR/LF, `_`, bad ports) is never matched; an empty list refuses everything. A request that already arrived over HTTPS (this server's own TLS, or a trusted proxy saying `X-Forwarded-Proto: https`) passes through untouched, so there is no loop behind an edge, and the same header from an untrusted peer is ignored. The ACME challenge prefix is answered **before** the redirect, so HTTP-01 keeps working on the port that redirects everything else. A Tower wrapper like the others, inside the `Origin` wrapper (it reads the scheme from it).
- Tests: unit tests for host normalisation and `Location` building, and `tests/redirect.rs` over real sockets on every adapter feature (GET and POST redirected with the target kept, custom port, forged and missing hosts, challenge path answered, trusted-proxy https passes and untrusted forgery does not, no redirect configured). The allow-list check was removed as a mutation and the forged-host test failed, as it should. Not published.

## [0.5.5] - 2026-10-30

- ACME HTTP-01 challenge prefix (WebHostAnt need 14), on every adapter and on cleartext and TLS listeners alike (HTTP-01 is validated on port 80). `Server::acme_challenge(responder)` / `ServeOptions::acme_challenge` take a `ChallengeResponder` (`key_authorization(token) -> Option<String>`, `Debug`); HTTPAnt answers `/.well-known/acme-challenge/<token>` **before the application's routes**, so an app route on that path never shadows a pending challenge, and does nothing else of ACME (no account keys, no storage, no renewal). `200` with the key authorization as `application/octet-stream` and `Cache-Control: no-store` (HEAD: same head, no body); `404` for a token the responder does not know; `404` without asking the responder for a token that is not base64url (`A-Za-z0-9-_`, 1 to 256 characters: no `.`, `/`, `%`), so a crafted path cannot reach it; `405` with `Allow: GET, HEAD` for other methods. With no responder registered the prefix is the application's. It is a Tower wrapper inside the limits (so the limits apply) and outside the application, the same code for all six adapters. Tests over real sockets on every adapter feature (`tests/challenge.rs`, run with `--all-features`): answered ahead of an app route, HEAD, unknown and malformed tokens, wrong method, other paths pass, no-responder passthrough.
- Fixed: an unused `mut` warning in `edge.rs` that appeared when `httpant-server` was built without the `tls` feature (the workspace build hides it by enabling `tls`). Not published.

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
