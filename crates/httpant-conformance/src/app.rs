// SPDX-License-Identifier: MIT OR Apache-2.0
//! The application every case runs against, so a case differs from the next only in what it sends.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use httpant_core::http::header::{HeaderValue, SET_COOKIE};
use httpant_core::http::{Method, StatusCode};
use httpant_core::{
    Body, BoxService, Bytes, Error, IntoResponse, PathParams, Request, Response, Router, layer_fn,
};
use tokio::sync::Notify;
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

/// `/param/:x` and `/file/*rest`: the percent-decoded capture, then the raw one.
async fn param(req: Request) -> String {
    let p = req
        .extensions()
        .get::<PathParams>()
        .cloned()
        .unwrap_or_default();
    let (name, _) = p.iter().next().unwrap_or(("", ""));
    format!(
        "{}|{}",
        p.get(name).unwrap_or_default(),
        p.get_raw(name).unwrap_or_default()
    )
}

/// `/sink`: reads the whole request body (the handler asks for far more than any server limit, so only the server's limit can stop it).
async fn sink(req: Request) -> Result<String, Error> {
    let bytes = req.into_body().bytes(1 << 40).await?;
    Ok(format!("got {}", bytes.len()))
}

async fn who(req: Request) -> String {
    format!(
        "{} {} {}",
        req.method(),
        req.uri().path(),
        req.uri().query().unwrap_or("-")
    )
}

/// How long a handler waits for the test to release it before giving up, so a broken test cannot hang the suite.
const GATE_WAIT: Duration = Duration::from_secs(10);

/// Signals between a case and the handlers it talks to. A case that must see "the handler is running" or "the handler may go on" uses
/// these instead of sleeping, so it never depends on timing.
#[derive(Clone, Default)]
pub struct Gates {
    /// `/stream` sends its second chunk only after this.
    pub stream_release: Arc<Notify>,
    /// `/upload` signals this when its first request chunk arrives.
    pub upload_first_chunk: Arc<Notify>,
    /// `/slow` signals this when it starts.
    pub slow_started: Arc<Notify>,
    /// `/slow` answers only after this.
    pub slow_release: Arc<Notify>,
}

async fn wait(gate: &Notify) {
    let _ = tokio::time::timeout(GATE_WAIT, gate.notified()).await;
}

/// `/stream`: "one", then nothing until the test releases the gate, then "two". An adapter that buffers the response never delivers "one".
fn stream_response(gate: Arc<Notify>) -> Response {
    let frames = futures_util::stream::unfold(0u8, move |state| {
        let gate = Arc::clone(&gate);
        async move {
            let data: &'static [u8] = match state {
                0 => b"one",
                1 => {
                    wait(&gate).await;
                    b"two"
                }
                _ => return None,
            };
            Some((
                Ok::<_, Infallible>(Frame::data(Bytes::from_static(data))),
                state + 1,
            ))
        }
    });
    Response::new(Body::new(StreamBody::new(frames)))
}

/// `/upload`: reads the request body frame by frame, signals on the first, answers with the byte count.
async fn upload(req: Request, first_chunk: Arc<Notify>) -> String {
    let mut body = req.into_body();
    let mut seen_first = false;
    let mut total = 0usize;
    while let Some(Ok(frame)) = body.frame().await {
        if let Ok(data) = frame.into_data() {
            if !seen_first {
                seen_first = true;
                first_chunk.notify_one();
            }
            total += data.len();
        }
    }
    format!("got {total}")
}

/// The routes and layers.
pub fn app() -> App {
    app_with_gates().0
}

/// The routes and layers, and the [`Gates`] their handlers wait on.
pub fn app_with_gates() -> (App, Gates) {
    let gates = Gates::default();
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
        .get("/param/:x", param)
        .get("/file/*rest", param)
        .post("/sink", sink)
        .get("/who/:x", who)
        .post("/who/:x", who)
        .get("/a", || async { "a" })
        .get("/b/", || async { "b" })
        .get("/stream", {
            let gate = Arc::clone(&gates.stream_release);
            move || {
                let gate = Arc::clone(&gate);
                async move { stream_response(gate) }
            }
        })
        .post("/upload", {
            let gate = Arc::clone(&gates.upload_first_chunk);
            move |req: Request| upload(req, Arc::clone(&gate))
        })
        .get("/slow", {
            let (started, release) = (
                Arc::clone(&gates.slow_started),
                Arc::clone(&gates.slow_release),
            );
            move || {
                let (started, release) = (Arc::clone(&started), Arc::clone(&release));
                async move {
                    started.notify_one();
                    wait(&release).await;
                    "finished"
                }
            }
        })
        .get("/delay/:x", delayed)
        .get("/hello", hello);

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
    (
        App {
            router,
            layers: vec![block, header],
        },
        gates,
    )
}

/// `/delay/:x`: the parameter, after a short pause, so concurrent requests overlap.
async fn delayed(req: Request) -> String {
    tokio::time::sleep(Duration::from_millis(20)).await;
    req.extensions()
        .get::<PathParams>()
        .and_then(|p| p.get("x"))
        .unwrap_or_default()
        .to_owned()
}

/// `/hello`: a body with a known length and two headers, for the HEAD case to compare.
async fn hello() -> Response {
    let mut res = "hello".into_response();
    res.headers_mut()
        .insert("x-hello", HeaderValue::from_static("yes"));
    res
}
