# httpant-client

The client half of HTTPAnt: the `Client` builder, the `ClientAdapter` trait and the hyper-util reference adapter (`http://`
always; `https://` with the `https` feature: rustls, the bundled Mozilla roots, certificate and host name verified, no revocation check).

**Most programs want [`httpant`](https://crates.io/crates/httpant)**, which re-exports this crate's useful parts behind one `use httpant::...`. Depend on this crate directly only to write an adapter or to avoid the other half.

Part of [HTTPAnt](https://github.com/ThinkersJournal/httpant): one interface to many web libraries, behind a conformance suite every adapter must pass.

## Licence

MIT or Apache-2.0, at your option.
