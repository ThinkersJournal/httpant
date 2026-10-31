# HTTPAnt

**Serve HTTP and call HTTP once, on the web library you choose.**

```rust
use httpant::{Client, Server};
#[tokio::main]
async fn main() -> httpant::Result<()> {
    let app = Server::new().get("/", || async { "hello" });
    let server = app.bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", server.addr());
    let text = Client::new().get(url).send().await?.text().await?;
    assert_eq!(text, "hello");
    Ok(())
}
```

That is the whole program. It prints nothing: it serves `hello`, fetches it and checks it with `assert_eq!`, so a run that exits with status 0 worked.
It needs the two `cargo add` commands under Install, and it is compiled and run in this repository's CI, so it cannot rot.
A handler returns anything that is `IntoResponse` (a `&str` here). `httpant::Result<T>` is `Result<T, httpant::Error>`, and every `httpant::Error`
prints (with `{}`) as a sentence that ends with what to do next.

## Install

```sh
cargo add httpant
cargo add tokio --features macros,rt-multi-thread    # for #[tokio::main]
```

## Choose the web library

The default is hyper. Any other library is a cargo feature of the same name and one id in the code. This is the first program with one call
changed (`Server::new()` became `Server::with_adapter("hyper")?`); nothing else moves:

```rust
use httpant::{Client, Server};
#[tokio::main]
async fn main() -> httpant::Result<()> {
    // "hyper" is always there; "axum", "actix-web", "poem", "salvo" and "warp" after `cargo add httpant --features axum` (and so on).
    let app = Server::with_adapter("hyper")?.get("/", || async { "hello" });
    let server = app.bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", server.addr());
    let text = Client::new().get(url).send().await?.text().await?;
    assert_eq!(text, "hello");
    Ok(())
}
```

Two different mistakes get two different messages, each ending with what to do. An id that does not exist: ``unknown adapter `nonexistent`: use one of ["hyper"]``
(it lists the ids this build has). An id that exists but whose feature is off: ``adapter `axum` is not compiled in: enable the `axum` feature``.
The id can come from your settings instead of your code.

## What you get

Every web library sits behind the same **conformance suite** (18 cases over real sockets, each with deliberately broken adapters it must catch),
so the same program behaves the same on each. What each one does, as measured and not as claimed:
[docs/CAPABILITY-MATRIX.md](https://github.com/ThinkersJournal/httpant/blob/main/docs/CAPABILITY-MATRIX.md).

- **Safe defaults, with an escape hatch.** Requests are limited before a handler runs: target 8 KiB, 64 header fields, 16 KiB of headers, body 2 MiB
  (`Limits`); `shutdown()` waits 30 s for requests in flight, then ends them. Raise or remove a limit with one call, in code that says so.
- **Real HTTP.** Unknown methods are not turned into `GET`, every status code reaches the client as sent, repeated headers stay repeated, and
  bodies stream. (The crate this replaces did none of these: [what it got wrong](https://github.com/ThinkersJournal/httpant/blob/main/docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md).)
- **Behind a proxy or CDN.** The client address and scheme are believed from `X-Forwarded-For` / `X-Forwarded-Proto` only from peers you list; nobody
  is trusted by default. Read them from `Origin`.
- **TLS in the server** (feature `tls`, rustls): certificates come from a `CertSource` you provide, asked once per handshake, so a renewed certificate
  serves the next connection with no restart. HTTP/2 over TLS (feature `http2`), the ACME HTTP-01 and TLS-ALPN-01 hooks, a port-80 redirect, audit
  events and a read-back of what a listener really serves are there too. HTTPAnt does not do ACME, storage or renewal: that is the source's job.
- **A client half** with the same shape: `Client::new().get(url).send().await?`.

## What it does not do (yet)

- It is not a web framework: no ORM, auth, sessions or templating.
- The client speaks plain `http://`; an `https://` URL is an error that says so.
- TLS termination is in hyper, axum, poem and salvo; actix-web and warp serve behind an edge that terminates it, and refuse a TLS acceptor by name
  instead of serving in clear. rocket is not supported: it answers an unknown method such as `PROPFIND` with `400` before a route is tried
  ([measured](https://github.com/ThinkersJournal/httpant/blob/main/docs/ROCKET-PROBE.md)).
- Cleartext HTTP/2 is not uniform across the libraries (the matrix shows which accept it).

## Status

Pre-1.0: a breaking change bumps the second number (0.5 to 0.6). Minimum supported Rust version: see `rust-version` in `Cargo.toml`; the
`actix-web`, `salvo` and `warp` features can need a newer compiler than the default build (cargo says which).
[CHANGELOG](https://github.com/ThinkersJournal/httpant/blob/main/CHANGELOG.md), [design](https://github.com/ThinkersJournal/httpant/blob/main/docs/HTTPANT-DESIGN.md),
[security policy](https://github.com/ThinkersJournal/httpant/blob/main/SECURITY.md).

## Coming from `web-server-abstraction`

That crate (1.0.x) is end of life, and security fixes go here only. Its known defects, with the evidence:
[docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md](https://github.com/ThinkersJournal/httpant/blob/main/docs/WEB-SERVER-ABSTRACTION-KNOWN-ISSUES.md).

## Licence

MIT or Apache-2.0, at your option.
