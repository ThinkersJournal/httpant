// SPDX-License-Identifier: MIT OR Apache-2.0
//! The ACME HTTP-01 challenge prefix, served before the application's routes. HTTPAnt does not do ACME: whoever does (WebServAnt, an edge
//! adapter) registers a [`ChallengeResponder`] that knows which tokens are pending, and HTTPAnt only answers the one path prefix with
//! what it returns (WebHostAnt need 14). This works on any adapter and on cleartext as well as TLS listeners, because HTTP-01 is validated on
//! port 80.

use std::convert::Infallible;
use std::sync::Arc;

use httpant_core::http::{HeaderValue, Method, StatusCode, header};
use httpant_core::{Body, BoxFuture, BoxService, IntoResponse, Request, Response};
use tower::ServiceExt;

/// The path prefix HTTP-01 validation requests arrive on (RFC 8555 section 8.3).
pub const CHALLENGE_PREFIX: &str = "/.well-known/acme-challenge/";

/// Longest token HTTPAnt will pass on; a real token is 43 characters. A longer one is answered `404` without asking.
const MAX_TOKEN_LEN: usize = 256;

/// Answers HTTP-01 challenge requests. Registered with [`Server::acme_challenge`](crate::Server::acme_challenge).
pub trait ChallengeResponder: std::fmt::Debug + Send + Sync + 'static {
    /// The key authorization for a pending `token`, or `None` when no challenge for it is pending (answered `404`). `token` has already been
    /// checked to be non-empty base64url text (`A-Z a-z 0-9 - _`), so it cannot carry a path separator or a dot segment.
    fn key_authorization<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Option<String>>;
}

/// The token in a request path under [`CHALLENGE_PREFIX`], if the path is a challenge path at all. `Some(None)` is a challenge path whose
/// token is not acceptable.
fn token_of(path: &str) -> Option<Option<&str>> {
    let rest = path.strip_prefix(CHALLENGE_PREFIX)?;
    let ok = !rest.is_empty()
        && rest.len() <= MAX_TOKEN_LEN
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    Some(ok.then_some(rest))
}

/// Wrap `svc` so the challenge prefix is answered by `responder` and never reaches `svc`. Every request under the prefix is answered here,
/// whatever the application has routed there.
pub(crate) fn wrap(responder: Option<Arc<dyn ChallengeResponder>>, svc: BoxService) -> BoxService {
    let Some(responder) = responder else {
        return svc;
    };
    BoxService::new(tower::service_fn(move |req: Request| {
        let (svc, responder) = (svc.clone(), Arc::clone(&responder));
        let step: BoxFuture<'static, Response> = Box::pin(async move {
            let Some(token) = token_of(req.uri().path()) else {
                return svc
                    .oneshot(req)
                    .await
                    .unwrap_or_else(|e: Infallible| match e {});
            };
            if req.method() != Method::GET && req.method() != Method::HEAD {
                let mut res = StatusCode::METHOD_NOT_ALLOWED.into_response();
                res.headers_mut()
                    .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
                return res;
            }
            let found = match token {
                Some(token) => responder.key_authorization(token).await,
                None => None,
            };
            match found {
                None => StatusCode::NOT_FOUND.into_response(),
                Some(text) => {
                    let mut res = Response::new(if req.method() == Method::HEAD {
                        Body::empty()
                    } else {
                        Body::from_bytes(text.clone())
                    });
                    let h = res.headers_mut();
                    h.insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/octet-stream"),
                    );
                    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                    h.insert(header::CONTENT_LENGTH, HeaderValue::from(text.len()));
                    res
                }
            }
        });
        async move { Ok::<_, Infallible>(step.await) }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_prefix_is_a_challenge_path_and_only_base64url_tokens_pass() {
        assert_eq!(token_of("/"), None);
        assert_eq!(token_of("/.well-known/acme-challenge"), None);
        assert_eq!(token_of("/.well-known/other/x"), None);
        assert_eq!(token_of("/x/.well-known/acme-challenge/t"), None);
        assert_eq!(
            token_of("/.well-known/acme-challenge/aB3-_9"),
            Some(Some("aB3-_9"))
        );
        for bad in ["", "a/b", "..", "a.b", "a%2Fb", "a b", "a\\b"] {
            let path = format!("{CHALLENGE_PREFIX}{bad}");
            assert_eq!(token_of(&path), Some(None), "{bad:?}");
        }
        let long = format!("{CHALLENGE_PREFIX}{}", "a".repeat(MAX_TOKEN_LEN + 1));
        assert_eq!(token_of(&long), Some(None));
        let longest = format!("{CHALLENGE_PREFIX}{}", "a".repeat(MAX_TOKEN_LEN));
        assert!(matches!(token_of(&longest), Some(Some(_))));
    }
}
