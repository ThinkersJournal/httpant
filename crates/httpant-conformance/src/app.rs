//! The application every case runs against, so a case differs from the next only in what it sends.

use std::convert::Infallible;

use httpant_core::http::header::{HeaderValue, SET_COOKIE};
use httpant_core::http::{Method, StatusCode};
use httpant_core::{
    BoxService, Bytes, Error, IntoResponse, PathParams, Request, Response, Router, layer_fn,
};
use tower::ServiceExt;

use crate::subject::App;

/// The body limit the `/echo` route reads with (1 MiB).
pub const BODY_LIMIT: usize = 1024 * 1024;

/// The bytes `/bin` serves: every byte value, four times.
pub fn binary_body() -> Vec<u8> {
    (0..=255u8).cycle().take(1024).collect()
}

async fn echo_method(req: Request) -> String {
    req.method().to_string()
}

async fn status(req: Request) -> StatusCode {
    let code = req
        .extensions()
        .get::<PathParams>()
        .and_then(|p| p.get("code"))
        .and_then(|c| c.parse::<u16>().ok());
    code.and_then(|c| StatusCode::from_u16(c).ok())
        .unwrap_or(StatusCode::BAD_REQUEST)
}

async fn cookies() -> Response {
    let mut res = "cookies".into_response();
    res.headers_mut()
        .append(SET_COOKIE, HeaderValue::from_static("a=1"));
    res.headers_mut()
        .append(SET_COOKIE, HeaderValue::from_static("b=2"));
    res
}

async fn raw_header(req: Request) -> String {
    match req.headers().get("x-raw") {
        Some(v) => v.as_bytes().iter().map(|b| format!("{b:02x}")).collect(),
        None => "absent".to_owned(),
    }
}

async fn echo_body(req: Request) -> Result<Bytes, Error> {
    req.into_body().bytes(BODY_LIMIT).await
}

async fn who(req: Request) -> String {
    format!(
        "{} {} {}",
        req.method(),
        req.uri().path(),
        req.uri().query().unwrap_or("-")
    )
}

/// The routes and layers.
pub fn app() -> App {
    let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid method token");
    let router = Router::new()
        .get("/m", echo_method)
        .route(propfind, "/m", echo_method)
        .route(Method::TRACE, "/m", echo_method)
        .get("/status/:code", status)
        .get("/cookies", cookies)
        .get("/hdr", raw_header)
        .post("/echo", echo_body)
        .get("/bin", || async { binary_body() })
        .get("/who/:x", who)
        .post("/who/:x", who)
        .get("/a", || async { "a" })
        .get("/b/", || async { "b" });

    // Added in this order, so the header layer is outermost and its header is on the blocked response too.
    let block = layer_fn(tower::layer::layer_fn(|inner: BoxService| {
        tower::service_fn(move |req: Request| {
            let inner = inner.clone();
            async move {
                if req.headers().contains_key("x-block") {
                    let mut res = (StatusCode::FORBIDDEN, "blocked").into_response();
                    res.headers_mut()
                        .insert("x-blocked-by", HeaderValue::from_static("layer"));
                    return Ok::<_, Infallible>(res);
                }
                inner.oneshot(req).await
            }
        })
    }));
    let header = layer_fn(tower::layer::layer_fn(|inner: BoxService| {
        tower::service_fn(move |req: Request| {
            let inner = inner.clone();
            async move {
                let mut res = inner.oneshot(req).await?;
                res.headers_mut()
                    .insert("x-layer", HeaderValue::from_static("a"));
                Ok::<_, Infallible>(res)
            }
        })
    }));
    App {
        router,
        layers: vec![block, header],
    }
}
