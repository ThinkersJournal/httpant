// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant_core::{BoxFuture, ClientCapabilities, Request, Response, Result};

/// One HTTP client library behind HTTPAnt. An adapter's only job is to send one `http::Request` and hand back one `http::Response`;
/// the builder, timeouts and body limits sit above it so they are the same everywhere.
pub trait ClientAdapter: Send + Sync + 'static {
    /// The id the adapter is chosen by: `"hyper"`, `"reqwest"`, ...
    fn id(&self) -> &'static str;

    /// What the adapter can and cannot do, as established by the client conformance run.
    fn capabilities(&self) -> ClientCapabilities;

    /// Send `req` (an absolute URI) and return the response as soon as its head has arrived. A `3xx` is returned as it is unless
    /// [`ClientCapabilities::follows_redirects`] says the adapter follows.
    fn send(&self, req: Request) -> BoxFuture<'_, Result<Response>>;
}
