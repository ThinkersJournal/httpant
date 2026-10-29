// SPDX-License-Identifier: MIT OR Apache-2.0
/// Default limit applied when buffering a body, in bytes (2 MiB). Visible in [`Capabilities`] so it is never a hidden number.
pub const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// Whether an adapter does a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Passes the conformance case for it.
    Yes,
    /// Cannot do it; using it must return a named error, never partly work.
    No,
    /// Not yet established by a conformance run. The starting value for every adapter.
    Unknown,
}

/// What an adapter can and cannot do (design section 8). Filled by the conformance run, not by assertion.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// Request bodies reach the handler incrementally.
    pub streaming_request_bodies: Support,
    /// Response bodies are sent as produced.
    pub streaming_response_bodies: Support,
    /// An in-flight request completes after shutdown is requested.
    pub graceful_shutdown: Support,
    /// Accepts handlers that are not `Send` (actix-web).
    pub local_handlers: Support,
    /// The body limit the adapter applies unless told otherwise, in bytes.
    pub default_body_limit: usize,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            streaming_request_bodies: Support::Unknown,
            streaming_response_bodies: Support::Unknown,
            graceful_shutdown: Support::Unknown,
            local_handlers: Support::Unknown,
            default_body_limit: DEFAULT_BODY_LIMIT,
        }
    }
}
