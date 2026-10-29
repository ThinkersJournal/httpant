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
}

impl Error {
    /// The status the default mapper answers with.
    pub fn status(&self) -> StatusCode {
        match self {
            Error::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Error::Body(_) | Error::NotUtf8 => StatusCode::BAD_REQUEST,
            Error::AdapterNotCompiledIn { .. } | Error::Io(_) | Error::UnknownAdapter { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}
