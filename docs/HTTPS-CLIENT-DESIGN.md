# HTTPS in the HTTPAnt client: options and a recommendation

Written 2026-10-31 against `origin/main` `1e926bd` (0.5.19), for the PM and for WebHostAnt, whose needs are in
[WebHostAnt `docs/HTTPS-CLIENT-NEEDS.md`](https://github.com/ThinkersJournal/WebHostAnt/pull/20) (13 needs, quoted by number below). **A design
note: nothing here is built.** What is measured is marked *measured*; what was read is marked *read*; the rest is an estimate and says so.

## The problem, in one example

```rust
let r = Client::new().get("https://api.cloudflare.com/client/v4/...").bearer(token).send().await?;
```

Today that line is `Error::UnsupportedScheme`: `scheme `https` is not supported by the `hyper` client adapter` (*read*,
`crates/httpant-client/src/hyper_adapter.rs`; the capability matrix says client TLS `no`). WebHostAnt cannot call Cloudflare's API or probe a
deployed app's health without it.

## What the client is today (read at 1e926bd)

- `Client::new()` / `with_adapter("hyper")`; `get/post/put/delete/head`; **no builder** (`Client::builder()` does not exist), **no `bearer`**, **no
  `patch`**, one per-request `timeout` that covers only "until the response head arrives".
- Redirects are **not followed** (`follows_redirects = No`, measured by the redirect probe). That already satisfies the default half of need 8.
- Bodies are limited on read (`bytes()` defaults to 2 MiB). Headers are kept with repeats. Both satisfy need 6.
- `Error::InvalidUrl` prints **the whole URL as given** (`invalid URL `{url}`...`, *read*, `error.rs`). A URL with `?token=...` in it would be printed.
  That is a hole against need 12 and is fixed in the first slice, not left to the caller.
- One client adapter, hyper-util, built with `build_http()` (an `HttpConnector`, no TLS).

## Decision 1: which TLS library

| option | for | against |
|---|---|---|
| **rustls** (`hyper-rustls` 0.27, `tokio-rustls` 0.26) | The server side already uses rustls 0.23 with the `ring` provider (workspace `Cargo.toml`), so one TLS stack to audit, one crypto provider, no system library. Pure Rust plus ring's C/asm. Same behaviour on Windows, macOS, Linux and in a minimal container. Licences are all in `deny.toml`'s allow list already (`ISC`, `MIT`, `Apache-2.0`, `CDLA-Permissive-2.0`; the comment there says the TLS stack was already approved). | We carry the root store ourselves (decision 2). A crypto provider choice is ours (ring, as the server). |
| native-tls (`hyper-tls` 0.6, `native-tls` 0.2) | Uses the OS verifier, so a corporate or school root installed in the OS "just works". | OpenSSL is a system dependency on Linux (build failures in slim images, version skew); three different TLS implementations means three behaviours to describe and test; it contradicts the rustls-only decision recorded for the server (`HANDOFF`/ROADMAP row 4). `hyper-tls` declares no MSRV. |

**Recommend rustls**, with the `ring` provider, `tls12` on (TLS 1.2 and 1.3, nothing older: need 1). Do not offer native-tls. If a school's inspecting
proxy needs the OS store, that is a root-store option (below), not a second TLS library.

Versions at the date of writing (*read*, `cargo info`): hyper-rustls 0.27.10 (Apache-2.0 OR ISC OR MIT, rust-version 1.85), tokio-rustls 0.26.6,
rustls 0.23.46 (rust-version 1.71), rustls-webpki 0.103.16 (ISC), webpki-roots 1.0.11 (CDLA-Permissive-2.0, 1.71), rustls-native-certs 0.8.5
(Apache-2.0 OR ISC OR MIT, 1.71), rustls-platform-verifier 0.7.1 (MIT OR Apache-2.0, 1.85). rustls 0.24 and webpki 0.104 exist only as
pre-releases; stay on 0.23 (the server is on it and the dependencies rule wants the latest **stable**).

## Decision 2: which roots to trust (WebHostAnt question 1; needs 1, 3, 4)

| option | what it is | trade |
|---|---|---|
| **Bundled** (`webpki-roots`, Mozilla's set compiled in) | Same trust everywhere, works in a scratch container, nothing to find on disk. | Stale until the next release of the crate; does not know a corporate or school root. |
| **OS store** (`rustls-native-certs`) | Reads the platform's roots (Windows, macOS, Linux). | Depends on the machine; absent in minimal containers; load cost at startup; an empty store must be an error, not "trust nothing silently". |
| OS verifier (`rustls-platform-verifier`) | Asks the platform to verify the chain (Windows CryptoAPI and so on), so revocation and enterprise policy apply. | Heavier, platform-specific code paths, not measured here. A later option, not the first slice. |
| Extra root(s) | Add PEM roots on top of either set. | Needed by need 3 so WebHostAnt's tests can run https against a local fake server with its own CA. |

**Recommend:** the **default is the bundled set** (need 1: verified by default, host name checked, same answer on every machine), `RootStore::Os`
and `RootStore::Both` are explicit choices (need 4: "off by default, HTTPAnt's call"; off, because an unexpected OS store changes who is trusted
without anyone asking), and `add_root_pem(bytes)` works with any of them (need 3). An empty or unparsable root set is an error that says so; it never
falls back to trusting everything. Turning verification off (need 2) is **not** in the first slices; if it is wanted later it is a separately named
type (`dangerous_skip_verification`), never a boolean, and WebHostAnt has said it will not use it.

## Decision 3: where it lives, and what it is called

The hyper adapter gets a connector: `hyper_rustls::HttpsConnectorBuilder`, wrapping the same `HttpConnector`, behind a cargo feature. Proposal:
feature **`https`** on `httpant-client`, forwarded by the `httpant` facade (`cargo add httpant --features https`). Why not the existing `tls`: `tls` means
*terminate* TLS in the server; turning on a server feature to make outbound calls, or the reverse, would surprise. The two features share rustls and
the crypto provider, so enabling both costs one copy.

Default **off** in the first release that has it, like every other optional capability, with the existing style of error: `scheme `https` is not
compiled in: enable the `https` feature`. Whether it should become a default feature is a later decision with the measured compile cost in hand
(**not measured here**; the implementation PR measures the added crates and build time and puts the numbers in the CHANGELOG).

Other client adapters: `ClientAdapter::capabilities().tls` already exists, and a conformance case decides, so an adapter that cannot do https says
`No` and is refused by name. A reqwest-style adapter would bring its own TLS and roots and would be held to the same case; it is not needed for this
task and is not proposed.

## Decision 4: the private-address guard (WebHostAnt question 2; need 10)

**Yes, it is possible below the hyper connector, and it must be in two places, because one is not enough.** *Read* in hyper-util 0.1.21
(`client/legacy/connect/http.rs`):

- `HttpConnector` takes a custom resolver (`HttpConnector::new_with_resolver`, line 223). A resolver that **filters the addresses it returns** means
  the connector can only connect to what passed the check. That is the address actually connected to, so a DNS answer that changes between "check"
  and "connect" (rebinding) cannot slip past: there is no second lookup.
- But **an IP literal in the URL never reaches the resolver**: `dns::SocketAddrs::try_parse(host, port)` runs first (line 559). `https://169.254.169.254/`
  or `http://[::1]:8080/` would go straight through. So the host must also be checked as a literal, in the connector wrapper, before connecting.
- Backstop: the connected stream reports its peer (`HttpInfo::remote_addr`, line 628). After connect (and before any byte is written, for http; before
  the TLS handshake, for https) compare it against the same rule and drop the connection if it fails.

What counts as private is a function with its own tests: loopback, unspecified, private (RFC 1918), link-local including the cloud metadata address,
carrier-grade NAT (100.64.0.0/10), unique-local and link-local IPv6, and **IPv4-mapped IPv6** (`::ffff:10.0.0.1` must be judged as the IPv4 address it is).

**After a redirect** the guard applies again for free, because a followed redirect is a new request through the same connector; this is also why
opt-in redirects are implemented above the adapter, in one place, and not left to each adapter.

**Caveat, stated now so nobody is surprised:** through a proxy (need 9) the connected address is the proxy's, so the guard then judges the proxy, not
the target. The proxy slice must say so in its documentation and refuse "guard on" with "proxy on" unless the caller picks which one is meant.

## The needs, one by one

| # | need | plan | slice |
|---|---|---|---|
| 1 | https verified by default | hyper-rustls, bundled roots, TLS 1.2/1.3, host name checked | 1 |
| 2 | verification off, loud | not built; separate named type if ever wanted | later |
| 3 | extra trusted root | `add_root_pem` | 1 |
| 4 | OS store | `RootStore::Os` / `Both`, opt-in | 3 |
| 5 | methods, headers, body, `bearer` redacted | add `Client::builder()`, `patch`, `bearer(token)`: the header value is built with `HeaderValue::set_sensitive(true)`, so `Debug` prints it as sensitive; no `Debug` for the builder prints values | 1 |
| 6 | status, headers, bounded body, 429 unchanged | already true; add a test that `Retry-After` and a `429` arrive unchanged | 1 |
| 7 | finite timeouts: connect, whole request, idle | builder defaults (proposal: connect 10 s, request 30 s, idle read 30 s), each settable; the error names which fired (a new `Error::Timeout` kind, not three strings) | 1 |
| 8 | redirects off by default; opt-in follow with a cap | off already (measured); opt-in follow above the adapter: cap, never https to http, `Authorization` dropped when the host changes | 3 (default-off part is already true) |
| 9 | explicit proxy | `proxy(url)` with CONNECT for https; env variables only if asked | 3 |
| 10 | private-address guard | decision 4 | 2 |
| 11 | no retry of non-idempotent requests | the client never retries; say so in the docs and test it | 1 |
| 12 | no secrets in errors | the token never reaches an error because only the sensitive header holds it; **fix `InvalidUrl`** to print scheme, host and path, not the query; `Request` errors carry hyper's text, which is checked by a test with a token in the query | 1 |
| 13 | client `tls` is measured | a client conformance case against a local https peer with a known CA (rcgen, already a dev-dependency), plus mutants (client that skips verification must fail; client that ignores the host name must fail) | 1 |

## Slices and what WebHostAnt can rely on (question 3)

The must rows are 1, 3, 5, 6, 7, 8, 10, 12. Two implementation PRs deliver them; versions are the PM's to allocate.

1. **HTTPS, verified.** `Client::builder()`, feature `https`, bundled roots, `add_root_pem`, `bearer`, timeouts, error hygiene, conformance case with mutants,
   capability matrix client column `tls` becomes measured. Needs 1, 3, 5, 6, 7, 11, 12, 13 (and 8's default).
2. **Private-address guard.** Needs 10: resolver filter, literal check, post-connect backstop, the classification function with its tests,
   a conformance case against a local peer on a loopback address with the guard on (must be refused) and off (must connect), and a mutant that
   checks the name only.
3. **Later (the should rows):** OS store, proxy, opt-in redirect following.

WebHostAnt's own tests need slices 1 and 2 only.

## Cost and risk

- **MSRV:** every proposed crate declares 1.85 or lower (*read* above); the workspace is at 1.88 and the `msrv` job checks the default build. The
  `https` feature is added to that job when it exists, so the claim is measured, not read.
- **Licences / cargo deny:** all licences are already in the allow list. `webpki-roots` is `CDLA-Permissive-2.0`, which is listed. The first slice's
  PR runs `cargo deny` and an advisory against the new graph is the PM's call, never a config edit. Expect `multiple-versions = "warn"` noise
  (hyper-rustls pulling a second version of something); to be read, not hidden.
- **Supply chain / size:** more dependencies in the `https` build only. Number and build time to be measured in slice 1.
- **Windows:** ring needs the MSVC build tools it already needs for the server's `tls` feature; nothing new.
- **Certificate revocation:** rustls does not check revocation by default. The README must not claim it does.
- **A wrong default is the real risk.** A client that accepts anything, or that trusts the OS silently, would be worse than no https. Hence: bundled
  default, mutants that make verification fail loudly, and no "skip verification" switch in the first slices.

## What I am asking the PM to decide

1. Feature name `https` (recommended) versus reusing `tls`.
2. Bundled roots as the default, OS store opt-in (recommended).
3. Slice order: HTTPS first, then the guard (recommended; WebHostAnt's tests need both, in that order).
4. Whether `https` should one day be a default feature (not now; decide with the measured cost).
