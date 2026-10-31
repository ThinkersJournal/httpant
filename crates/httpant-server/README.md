# httpant-server

The server half of HTTPAnt: the `Server` builder, the `ServerAdapter` trait, the hyper reference adapter and, behind cargo features, the axum,
actix-web, poem, salvo and warp adapters; request limits, bounded graceful shutdown, edge mode (trusted proxies), and TLS termination with HTTP/2.

**Most programs want [`httpant`](https://crates.io/crates/httpant)**, which re-exports this crate's useful parts behind one `use httpant::...`. Depend on this crate directly only to write an adapter or to avoid the other half.

Part of [HTTPAnt](https://github.com/ThinkersJournal/httpant): one interface to many web libraries, behind a conformance suite every adapter must pass.

## Licence

MIT or Apache-2.0, at your option.
