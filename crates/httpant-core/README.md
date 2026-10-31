# httpant-core

The shared types of HTTPAnt: the `http` crate's request, response, method, status, header and URI types, a streaming `Body` with explicit size
limits, `Error`, `IntoResponse`, `Handler`, `Router`, Tower layer glue, and `Capabilities`.

**Most programs want [`httpant`](https://crates.io/crates/httpant)**, which re-exports this crate's useful parts behind one `use httpant::...`. Depend on this crate directly only to write an adapter or to avoid the other half.

Part of [HTTPAnt](https://github.com/ThinkersJournal/httpant): one interface to many web libraries, behind a conformance suite every adapter must pass.

## Licence

MIT or Apache-2.0, at your option.
