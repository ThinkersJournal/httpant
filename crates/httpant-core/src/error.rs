// SPDX-License-Identifier: MIT OR Apache-2.0
use http::StatusCode;

use crate::BoxError;

/// `Result` with HTTPAnt's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// HTTPAnt's error. Every message ends with what to do next.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A body was larger than the limit it was read with.
    #[error("body is larger than the limit of {limit} bytes: raise the limit or stream the body")]
    BodyTooLarge {
        /// The limit that was exceeded, in bytes.
        limit: usize,
    },
    /// Reading a body failed part-way.
    #[error("body could not be read: {0}; treat the request as failed")]
    Body(BoxError),
    /// A body was read as text but is not UTF-8.
    #[error("body is not valid UTF-8: read it as bytes instead")]
    NotUtf8,
    /// Binding or serving failed at the operating-system level.
    #[error("i/o error: {0}; check that the address is free and permitted")]
    Io(#[from] std::io::Error),
    /// An adapter id that no adapter has.
    #[error("unknown adapter `{id}`: use one of {available:?}")]
    UnknownAdapter {
        /// The id that was requested.
        id: String,
        /// The ids that exist in this build.
        available: Vec<&'static str>,
    },
    /// An adapter was asked for by id but is not compiled in.
    #[error("adapter `{id}` is not compiled in: enable the `{id}` feature")]
    AdapterNotCompiledIn {
        /// The adapter id that was requested.
        id: String,
    },
    /// A client request URL that is not usable.
    #[error("invalid URL `{url}`: {reason}; use an absolute http:// or https:// URL")]
    InvalidUrl {
        /// The URL as given, without any user name, password, query or fragment (those can carry secrets and never appear in an error).
        url: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A header name or value a client request cannot carry.
    #[error(
        "invalid header `{name}`: names are HTTP tokens and values may not contain CR, LF or NUL"
    )]
    InvalidHeader {
        /// The header name as given (lossy for non-UTF8 bytes).
        name: String,
    },
    /// A client request for a scheme the chosen client adapter does not do.
    #[error(
        "scheme `{scheme}` is not supported by the `{adapter}` client adapter: use http:// or https://"
    )]
    UnsupportedScheme {
        /// The scheme asked for.
        scheme: String,
        /// The client adapter id.
        adapter: &'static str,
    },
    /// TLS was asked for on an adapter that does not terminate it; the server does not start, it never serves in clear instead.
    #[error(
        "the `{adapter}` adapter cannot terminate TLS: use the hyper or axum adapter, or terminate TLS at an edge and trust its proxy"
    )]
    TlsNotSupported {
        /// The adapter id.
        adapter: &'static str,
    },
    /// A client request for a scheme that exists but whose cargo feature is off.
    #[error("scheme `{scheme}` is not compiled in: enable the `{feature}` feature")]
    SchemeNotCompiledIn {
        /// The scheme asked for.
        scheme: String,
        /// The cargo feature that provides it.
        feature: &'static str,
    },
    /// A client's TLS handshake failed (an untrusted or expired certificate, a name that does not match, a peer that does not speak TLS).
    #[error(
        "TLS handshake failed: {0}; check the host name and that its certificate is trusted and not expired (add_root_pem trusts a private CA)"
    )]
    TlsHandshake(String),
    /// TLS is configured wrongly (bad certificate or key, or the `tls` feature is off).
    #[error("TLS configuration: {0}")]
    Tls(String),
    /// A client request did not get a response head before its timeout.
    #[error("request timed out after {0:?}: raise the timeout or check the peer")]
    Timeout(std::time::Duration),
    /// A client could not connect before the connect timeout.
    #[error(
        "connecting timed out after {0:?}: raise the connect timeout or check the address and the network"
    )]
    ConnectTimeout(std::time::Duration),
    /// A response body went quiet for longer than the read timeout.
    #[error("the response went quiet for {0:?}: raise the read timeout or check the peer")]
    ReadTimeout(std::time::Duration),
    /// A client request failed in the transport (connect, send, receive).
    #[error("request failed: {0}; check the address and that the peer is up")]
    Request(BoxError),
}

impl Error {
    /// The status the default mapper answers with.
    pub fn status(&self) -> StatusCode {
        match self {
            Error::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Error::Body(_) | Error::NotUtf8 => StatusCode::BAD_REQUEST,
            Error::AdapterNotCompiledIn { .. }
            | Error::Io(_)
            | Error::UnknownAdapter { .. }
            | Error::InvalidUrl { .. }
            | Error::InvalidHeader { .. }
            | Error::UnsupportedScheme { .. }
            | Error::SchemeNotCompiledIn { .. }
            | Error::TlsNotSupported { .. }
            | Error::Tls(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::Timeout(_) | Error::ConnectTimeout(_) | Error::ReadTimeout(_) => {
                StatusCode::GATEWAY_TIMEOUT
            }
            Error::Request(_) | Error::TlsHandshake(_) => StatusCode::BAD_GATEWAY,
        }
    }
}
