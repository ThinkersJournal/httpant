// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]

/// The server half in full (adapters, limits, edge mode, TLS), for what the top level does not re-export.
pub use httpant_server as server;

/// The client half in full.
pub use httpant_client as client;

pub use httpant_client::{Client, ClientAdapter};
pub use httpant_server::{RunningServer, ServeOptions, Server, ServerAdapter};

// Everything the two halves share: the `http` crate's types, `Body`, `Error` and `Result`, `IntoResponse`, `Handler`, `Router`, `Capabilities`.
pub use httpant_core::*;
