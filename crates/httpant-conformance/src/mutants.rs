// SPDX-License-Identifier: MIT OR Apache-2.0
//! Known-bad variants of the reference adapter. Each reproduces a defect the survey measured in the old crate
//! (`docs/OLD-CRATE-SURVEY.md`), implemented as a wrapper around the service the adapter runs.

use std::convert::Infallible;

use httpant_core::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use httpant_core::{Body, BoxFuture, BoxService, Request, Response, Result};
use httpant_server::RunningServer;
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
    /// The whole response is collected before the first byte is sent (frameworks that buffer).
    BufferResponseBody,
    /// The whole request body is collected before the handler runs (frameworks that buffer).
    BufferRequestBody,
    /// Shutdown cuts every connection at once, in-flight requests included.
    AbortOnShutdown,
    /// Every response says `Connection: close`, so the connection cannot be reused.
    ForceConnectionClose,
    /// One request at a time: a global lock held across the handler.
    SerialiseRequests,
    /// A HEAD response loses the headers the GET response has.
    HeadStripsHeaders,
    /// The default limits, except that request targets are not limited.
    NoUriLimit,
    /// The default limits, except that header fields are not counted.
    NoHeaderCountLimit,
    /// The default limits, except that header bytes are not limited.
    NoHeaderBytesLimit,
    /// The default limits, except that request bodies are not limited.
    NoBodyLimit,
    /// The path is percent-decoded before routing, so an encoded `/` or `..` becomes a real separator or dot segment.
    DecodeBeforeRouting,
}

impl Mutant {
    /// The limits the subject serves a mutant with: the defaults, minus the one the mutant is about.
    pub fn limits(self) -> httpant_server::Limits {
        let mut l = httpant_server::Limits::default();
        match self {
            Mutant::NoUriLimit => l.max_uri_len = None,
            Mutant::NoHeaderCountLimit => l.max_headers = None,
            Mutant::NoHeaderBytesLimit => l.max_header_bytes = None,
            Mutant::NoBodyLimit => l.max_body_bytes = None,
            _ => {}
        }
        l
    }

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
            Mutant::BufferResponseBody => "buffer-response-body",
            Mutant::BufferRequestBody => "buffer-request-body",
            Mutant::AbortOnShutdown => "abort-on-shutdown",
            Mutant::ForceConnectionClose => "force-connection-close",
            Mutant::SerialiseRequests => "serialise-requests",
            Mutant::HeadStripsHeaders => "head-strips-headers",
            Mutant::DecodeBeforeRouting => "decode-before-routing",
            Mutant::NoUriLimit => "no-uri-limit",
            Mutant::NoHeaderCountLimit => "no-header-count-limit",
            Mutant::NoHeaderBytesLimit => "no-header-bytes-limit",
            Mutant::NoBodyLimit => "no-body-limit",
        }
    }
}

type Step = fn(BoxService, Request) -> BoxFuture<'static, Response>;

/// Wrap `svc` so every request goes through `mutant`'s defect.
pub fn apply(mutant: Mutant, svc: BoxService) -> BoxService {
    let step: Step = match mutant {
        // The defect is in how the subject builds or runs the service, not in the service.
        Mutant::NoMiddleware
        | Mutant::AbortOnShutdown
        | Mutant::NoUriLimit
        | Mutant::NoHeaderCountLimit
        | Mutant::NoHeaderBytesLimit
        | Mutant::NoBodyLimit => return svc,
        Mutant::FoldMethodToGet => fold_method_to_get,
        Mutant::ClampStatus => clamp_status,
        Mutant::HashMapHeaders => hashmap_headers,
        Mutant::EmptyRequestBody => empty_request_body,
        Mutant::LossyResponseBody => lossy_response_body,
        Mutant::TruncateRequestBody => truncate_request_body,
        Mutant::HardCodeGetRoot => hard_code_get_root,
        Mutant::TrimTrailingSlash => trim_trailing_slash,
        Mutant::BufferResponseBody => buffer_response_body,
        Mutant::BufferRequestBody => buffer_request_body,
        Mutant::ForceConnectionClose => force_connection_close,
        Mutant::SerialiseRequests => serialise_requests,
        Mutant::HeadStripsHeaders => head_strips_headers,
        Mutant::DecodeBeforeRouting => decode_before_routing,
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

fn buffer_response_body(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let (parts, body) = call(svc, req).await.into_parts();
        let bytes = body.bytes(64 * 1024 * 1024).await.unwrap_or_default();
        Response::from_parts(parts, Body::from_bytes(bytes))
    })
}

fn buffer_request_body(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let (parts, body) = req.into_parts();
        let bytes = body.bytes(64 * 1024 * 1024).await.unwrap_or_default();
        call(svc, Request::from_parts(parts, Body::from_bytes(bytes))).await
    })
}

fn force_connection_close(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let mut res = call(svc, req).await;
        res.headers_mut()
            .insert("connection", HeaderValue::from_static("close"));
        res
    })
}

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn serialise_requests(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let _turn = ONE_AT_A_TIME.lock().await;
        call(svc, req).await
    })
}

fn head_strips_headers(svc: BoxService, req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let head = req.method() == Method::HEAD;
        let mut res = call(svc, req).await;
        if head {
            res.headers_mut().clear();
            *res.body_mut() = Body::empty();
        }
        res
    })
}

/// A copy of the reference accept loop whose shutdown drops every connection immediately: the defect [`Mutant::AbortOnShutdown`] stands for.
pub fn serve_aborting(svc: BoxService, listener: std::net::TcpListener) -> Result<RunningServer> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use tokio::sync::oneshot;
    use tokio::task::JoinSet;

    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let (stop, mut stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let mut conns = JoinSet::new();
        loop {
            tokio::select! {
                _ = &mut stopped => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let svc = svc.clone();
                    conns.spawn(async move {
                        let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                            svc.clone().oneshot(req.map(Body::new))
                        });
                        let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                    });
                }
            }
        }
        // Dropping the set aborts every connection task: nothing in flight is allowed to finish.
        drop(conns);
    });
    Ok(RunningServer::new(addr, stop, task))
}

fn decode_before_routing(svc: BoxService, mut req: Request) -> BoxFuture<'static, Response> {
    Box::pin(async move {
        let path = req.uri().path();
        let decoded = path
            .replace("%2F", "/")
            .replace("%2f", "/")
            .replace("%2e", ".")
            .replace("%2E", ".");
        let rebuilt = match req.uri().query() {
            Some(q) => format!("{decoded}?{q}"),
            None => decoded,
        };
        if let Ok(uri) = rebuilt.parse::<Uri>() {
            *req.uri_mut() = uri;
        }
        call(svc, req).await
    })
}
