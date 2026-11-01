// SPDX-License-Identifier: MIT OR Apache-2.0
//! Settings a program reads from a file, the environment or a command line, and hands to [`Server::apply`](crate::Server::apply).
//!
//! [`ServerSettings`] is a plain `serde` struct with no dependency on any configuration library: any loader that can fill a `Deserialize`
//! struct (ConfigurAnt's `Config::deserialize::<T>()`, `toml`, `serde_json`, ...) can fill it. Every field is optional and an absent field
//! leaves HTTPAnt's own default in place, so an empty configuration changes nothing.
//!
//! ```text
//! adapter             = "hyper"      # the web library, by id (see Server::with_adapter)
//! addr                = "0.0.0.0:8080"
//! shutdown_grace_secs = 30           # seconds shutdown() waits for requests in flight
//! limits.max_uri_len      = 8192
//! limits.max_headers      = 64
//! limits.max_header_bytes = 16384
//! limits.max_body_bytes   = 2097152
//! ```
//!
//! What a setting cannot do, on purpose: remove a limit or the shutdown grace (`Limits::unlimited()` and `no_shutdown_grace()` stay in code,
//! where the reason can be written next to them), and carry a TLS certificate (a certificate source is code, not a value). An unknown key is
//! an error, not ignored, so a typo in a configuration file is reported instead of leaving a default in force.
use serde::Deserialize;

use crate::Limits;

/// Settings for a [`Server`](crate::Server). Every field is optional; an absent one leaves HTTPAnt's default (or what the builder already has).
///
/// ```text
/// adapter             = "hyper"      # the web library, by id (see Server::with_adapter)
/// addr                = "0.0.0.0:8080"
/// shutdown_grace_secs = 30           # seconds shutdown() waits for requests in flight
/// limits.max_uri_len      = 8192
/// limits.max_headers      = 64
/// limits.max_header_bytes = 16384
/// limits.max_body_bytes   = 2097152
/// ```
///
/// An unknown key is an error, so a typo is reported. A setting cannot remove a limit or the grace (`Limits::unlimited()` and
/// `no_shutdown_grace()` stay in code) or carry a certificate.
///
/// `addr` is carried here so one configuration describes the whole server, but [`Server::apply`](crate::Server::apply) does not bind: read
/// the field and pass it to `bind`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct ServerSettings {
    /// The web library, by id (`hyper`, `axum`, `actix-web`, `poem`, `salvo`, `warp`); an id that is unknown or not compiled in is an error.
    pub adapter: Option<String>,
    /// The address to bind, such as `0.0.0.0:8080`. Not used by `apply`.
    pub addr: Option<String>,
    /// Seconds `shutdown()` waits for requests in flight before ending them.
    pub shutdown_grace_secs: Option<u64>,
    /// Request limits, see [`LimitsSettings`].
    pub limits: LimitsSettings,
}

/// The limits of [`Limits`] as settings. An absent field keeps the default, see [`Limits`] for what each one answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct LimitsSettings {
    /// Longest request target (path and query), in bytes.
    pub max_uri_len: Option<usize>,
    /// Most header fields.
    pub max_headers: Option<usize>,
    /// Largest total of header name and value bytes.
    pub max_header_bytes: Option<usize>,
    /// Largest request body, in bytes.
    pub max_body_bytes: Option<u64>,
}

impl LimitsSettings {
    /// `base` with every field that is set replaced.
    pub(crate) fn over(&self, mut base: Limits) -> Limits {
        if let Some(v) = self.max_uri_len {
            base = base.max_uri_len(v);
        }
        if let Some(v) = self.max_headers {
            base = base.max_headers(v);
        }
        if let Some(v) = self.max_header_bytes {
            base = base.max_header_bytes(v);
        }
        if let Some(v) = self.max_body_bytes {
            base = base.max_body_bytes(v);
        }
        base
    }
}
