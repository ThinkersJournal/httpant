// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTTPAnt: serve HTTP and call HTTP once, on the web library you choose.
//!
//! This crate is a thin facade over `httpant-core`, `httpant-server` and `httpant-client`. See `examples/hello.rs` for the smallest program.

/// The server half in full (adapters, limits, edge mode, TLS), for what the top level does not re-export.
pub use httpant_server as server;

/// The client half in full.
pub use httpant_client as client;

pub use httpant_client::{Client, ClientAdapter};
pub use httpant_server::{RunningServer, ServeOptions, Server, ServerAdapter};

// Everything the two halves share: the `http` crate's types, `Body`, `Error` and `Result`, `IntoResponse`, `Handler`, `Router`, `Capabilities`.
pub use httpant_core::*;
