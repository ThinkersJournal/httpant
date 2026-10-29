//! Known-bad variants of the reference adapter. Each reproduces a defect the survey measured in the old crate
//! (`docs/OLD-CRATE-SURVEY.md`), implemented as a wrapper around the service the adapter runs.

use std::convert::Infallible;

use httpant_core::http::{HeaderMap, Method, StatusCode, Uri};
use httpant_core::{Body, BoxFuture, BoxService, Request, Response};
use tower::ServiceExt;

use crate::app::BODY_LIMIT;

/// One way of being wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutant {
    /// Unknown request methods are served as GET (old: axum, actix, warp, salvo, poem).
    FoldMethodToGet,
    /// Statuses outside a short list are sent as 200 (old: axum, warp).
    ClampStatus,
    /// Layers are stored and never run (old: all six).
    NoMiddleware,
    /// Headers held in a `HashMap<String, String>`: repeats collapse, non-UTF8 values vanish (old: the core `Headers` type).
    HashMapHeaders,
    /// The handler's request body is always empty (old: salvo, poem).
    EmptyRequestBody,
    /// Response bodies pass through `String::from_utf8_lossy` (old: warp).
    LossyResponseBody,
    /// An oversize request body is silently cut at the limit and the handler runs (old: rocket's 2 MB).
    TruncateRequestBody,
    /// Every handler sees `GET /` (old: warp).
    HardCodeGetRoot,
    /// A trailing slash is trimmed before routing (old: poem).
    TrimTrailingSlash,
}

impl Mutant {
    /// Stable short id.
    pub fn id(self) -> &'static str {
        match self {
            Mutant::FoldMethodToGet => "fold-method-to-get",
            Mutant::ClampStatus => "clamp-status",
            Mutant::NoMiddleware => "no-middleware",
            Mutant::HashMapHeaders => "hashmap-headers",
            Mutant::EmptyRequestBody => "empty-request-body",
            Mutant::LossyResponseBody => "lossy-response-body",
            Mutant::TruncateRequestBody => "truncate-request-body",
            Mutant::HardCodeGetRoot => "hard-code-get-root",
            Mutant::TrimTrailingSlash => "trim-trailing-slash",
        }
    }
}

type Step = fn(BoxService, Request) -> BoxFuture<'static, Response>;

/// Wrap `svc` so every request goes through `mutant`'s defect.
pub fn apply(mutant: Mutant, svc: BoxService) -> BoxService {
    let step: Step = match mutant {
        Mutant::NoMiddleware => return svc, // the defect is in how the subject builds the service
        Mutant::FoldMethodToGet => fold_method_to_get,
        Mutant::ClampStatus => clamp_status,
        Mutant::HashMapHeaders => hashmap_headers,
        Mutant::EmptyRequestBody => empty_request_body,
        Mutant::LossyResponseBody => lossy_response_body,
        Mutant::TruncateRequestBody => truncate_request_body,
        Mutant::HardCodeGetRoot => hard_code_get_root,
        Mutant::TrimTrailingSlash => trim_trailing_slash,
    };
    BoxService::new(tower::service_fn(move |req: Request| {
        let svc = svc.clone();
        async move { Ok::<_, Infallible>(step(svc, req).await) }
    }))
}

async fn call(svc: BoxService, req: Request) -> Response {
    svc.oneshot(req).await.unwrap()
}

fn fold_method_to_get(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let standard = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::HEAD,
            Method::OPTIONS,
            Method::PATCH,
        ];
        if !standard.contains(req.method()) {
            *req.method_mut() = Method::GET;
        }
        call(svc, req).await
    })
}

fn clamp_status(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let mut res = call(svc, req).await;
        if !matches!(res.status().as_u16(), 200 | 201 | 400 | 404 | 500) {
            *res.status_mut() = StatusCode::OK;
        }
        res
    })
}

fn collapse(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if value.to_str().is_ok() {
            out.insert(name.clone(), value.clone()); // insert, not append: last value wins
        }
    }
    out
}

fn hashmap_headers(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        *req.headers_mut() = collapse(req.headers());
        let mut res = call(svc, req).await;
        *res.headers_mut() = collapse(res.headers());
        res
    })
}

fn empty_request_body(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        *req.body_mut() = Body::empty();
        call(svc, req).await
    })
}

fn lossy_response_body(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let (parts, body) = call(svc, req).await.into_parts();
        let bytes = body.bytes(64 * 1024 * 1024).await.unwrap_or_default();
        Response::from_parts(
            parts,
            Body::from_bytes(String::from_utf8_lossy(&bytes).into_owned()),
        )
    })
}

fn truncate_request_body(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let declared = req
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if declared > BODY_LIMIT {
            // Cut at the limit and carry on, as if the body had fit.
            *req.body_mut() = Body::from_bytes(vec![b'x'; BODY_LIMIT]);
        }
        call(svc, req).await
    })
}

fn hard_code_get_root(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        *req.method_mut() = Method::GET;
        *req.uri_mut() = Uri::from_static("/");
        call(svc, req).await
    })
}

fn trim_trailing_slash(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let path = req.uri().path();
        let trimmed = if path.len() > 1 {
            path.trim_end_matches('/')
        } else {
            path
        };
        let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
        let rebuilt = match req.uri().query() {
            Some(q) => format!("{trimmed}?{q}"),
            None => trimmed.to_owned(),
        };
        if let Ok(uri) = rebuilt.parse::<Uri>() {
            *req.uri_mut() = uri;
        }
        call(svc, req).await
    })
}
