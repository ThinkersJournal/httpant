// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant_core::{
    Body, BoxFuture, ClientCapabilities, Error, Request, Response, Result, Support,
};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use crate::ClientAdapter;

/// The reference client adapter: hyper-util's pooled client over HTTP/1.1, `http://` only. The conversion is the whole adapter.
#[derive(Clone)]
pub struct HyperClientAdapter {
    client: Client<HttpConnector, Body>,
}

impl HyperClientAdapter {
    /// A client with hyper-util's default connection pool.
    pub fn new() -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }
}

impl Default for HyperClientAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAdapter for HyperClientAdapter {
    fn id(&self) -> &'static str {
        "hyper"
    }

    fn capabilities(&self) -> ClientCapabilities {
        let mut c = ClientCapabilities::default();
        // Measured by the client conformance cases 5-7 and the redirect probe (`declared_client_capabilities_match_the_measured_ones`).
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.connection_reuse = Support::Yes;
        c.follows_redirects = Support::No;
        // Not measurable against a plain-TCP peer: this adapter is built without a TLS connector.
        c.tls = Support::No;
        c
    }

    fn send(&self, req: Request) -> BoxFuture<'_, Result<Response>> {
        Box::pin(async move {
            match req.uri().scheme_str() {
                Some("http") => {}
                Some(other) => {
                    return Err(Error::UnsupportedScheme {
                        scheme: other.to_owned(),
                        adapter: "hyper",
                    });
                }
                None => {
                    return Err(Error::InvalidUrl {
                        url: req.uri().to_string(),
                        reason: "no scheme",
                    });
                }
            }
            let res = self
                .client
                .request(req)
                .await
                .map_err(|e| Error::Request(Box::new(e)))?;
            Ok(res.map(Body::new))
        })
    }
}
