// SPDX-License-Identifier: MIT OR Apache-2.0
use std::convert::Infallible;
use std::sync::Arc;

use http::header::{ALLOW, HeaderValue};
use http::{Method, StatusCode};

use crate::{BoxFuture, BoxService, Handler, IntoResponse, Request, Response};

type ErasedHandler = Arc<dyn Fn(Request) -> BoxFuture<'static, Response> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Literal(String),
    /// `:name`, one non-empty path segment.
    Param(String),
    /// `*name`, the rest of the path (possibly empty); must be last.
    Rest(String),
}

#[derive(Clone)]
struct Route {
    method: Method,
    segments: Vec<Segment>,
    handler: ErasedHandler,
}

/// Parameters captured from the path (`:id`, `*rest`), placed in the request's extensions before the handler runs.
///
/// [`get`](PathParams::get) returns the **percent-decoded** value; [`get_raw`](PathParams::get_raw) the text as it was on the wire. The router
/// refuses (with `400`, before any handler runs) a request whose captured value cannot be decoded safely; see [`Router`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathParams {
    decoded: Vec<(String, String)>,
    raw: Vec<(String, String)>,
}

impl PathParams {
    /// The percent-decoded value captured under `name`. For a `*rest` capture, segments are decoded one by one and joined with `/`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.decoded
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The value captured under `name` exactly as it appeared in the request target, still percent-encoded.
    pub fn get_raw(&self, name: &str) -> Option<&str> {
        self.raw
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every capture as `(name, decoded value)`, in route order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.decoded.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Why a captured path value was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    /// `%` not followed by two hex digits.
    BadEscape,
    /// The decoded bytes are not UTF-8.
    NotUtf8,
    /// The decoded value holds a NUL, `/` or `\`: an encoded separator would let one segment pose as several.
    Separator,
    /// The decoded segment is `.` or `..`.
    DotSegment,
}

impl Refused {
    fn message(self) -> &'static str {
        match self {
            Refused::BadEscape => {
                "bad request: a `%` in the path is not followed by two hex digits"
            }
            Refused::NotUtf8 => "bad request: the percent-decoded path is not valid UTF-8",
            Refused::Separator => {
                "bad request: the path holds an encoded `/`, backslash or NUL, which is refused"
            }
            Refused::DotSegment => {
                "bad request: the path holds an encoded `.` or `..` segment, which is refused"
            }
        }
    }
}

/// Percent-decode one path segment. `+` is a plus (that is a form rule, not a path rule). Refuses what could make a segment mean something
/// other than the one name the client wrote: encoded `/` and `\`, NUL, and `.` / `..` once decoded.
fn decode_segment(raw: &str) -> Result<String, Refused> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = |b: Option<&u8>| b.and_then(|b| (*b as char).to_digit(16));
            let (Some(hi), Some(lo)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) else {
                return Err(Refused::BadEscape);
            };
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| Refused::NotUtf8)?;
    if decoded
        .chars()
        .any(|c| matches!(c, '\u{0}' | '/' | '\u{5c}'))
    {
        return Err(Refused::Separator);
    }
    if decoded == "." || decoded == ".." {
        return Err(Refused::DotSegment);
    }
    Ok(decoded)
}

/// Decode every raw capture, or say why not.
fn decode_params(captures: Vec<(String, String)>) -> Result<PathParams, Refused> {
    let mut decoded = Vec::with_capacity(captures.len());
    for (name, raw) in &captures {
        let value = raw
            .split('/')
            .map(decode_segment)
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        decoded.push((name.clone(), value));
    }
    Ok(PathParams {
        decoded,
        raw: captures,
    })
}

/// Routes with `:name` and `*rest` path syntax and exact-method matching on [`http::Method`] (extension methods work).
///
/// Captured values are percent-decoded (see [`PathParams`]); literals are matched raw. A capture that cannot be decoded safely (bad escape,
/// not UTF-8, an encoded `/`, backslash or NUL, an encoded `.` or `..` segment) is answered `400` before the handler runs.
///
/// A known path with the wrong method answers `405` with an `Allow` header; an unknown path `404`. **No path normalisation**: `/a` and `/a/`
/// are different paths, the same on every adapter (design section 3).
///
/// `HEAD` on a path with a `GET` route and no `HEAD` route runs the `GET` handler (and `Allow` lists `HEAD`); the server drops the body.
#[derive(Clone, Default)]
pub struct Router {
    routes: Vec<Route>,
}

impl Router {
    /// An empty router (answers 404 to everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a route for any method.
    ///
    /// # Panics
    /// If `path` does not start with `/` or has a `*rest` segment that is not last: a route table is written once, at start-up.
    pub fn route<M>(mut self, method: Method, path: &str, handler: impl Handler<M>) -> Self {
        assert!(
            path.starts_with('/'),
            "route path `{path}` must start with `/`"
        );
        let mut segments = Vec::new();
        let parts: Vec<&str> = path[1..].split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            segments.push(if let Some(name) = part.strip_prefix(':') {
                Segment::Param(name.to_owned())
            } else if let Some(name) = part.strip_prefix('*') {
                assert!(
                    i == parts.len() - 1,
                    "route path `{path}`: a `*` segment must be last"
                );
                Segment::Rest(name.to_owned())
            } else {
                Segment::Literal((*part).to_owned())
            });
        }
        let handler: ErasedHandler = Arc::new(move |req| handler.clone().call(req));
        self.routes.push(Route {
            method,
            segments,
            handler,
        });
        self
    }

    /// `GET`.
    pub fn get<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::GET, path, handler)
    }

    /// `POST`.
    pub fn post<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::POST, path, handler)
    }

    /// `PUT`.
    pub fn put<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::PUT, path, handler)
    }

    /// `DELETE`.
    pub fn delete<M>(self, path: &str, handler: impl Handler<M>) -> Self {
        self.route(Method::DELETE, path, handler)
    }

    /// The whole router as the one Tower service adapters run.
    pub fn into_service(self) -> BoxService {
        let routes = Arc::new(self.routes);
        BoxService::new(tower::service_fn(move |req: Request| {
            let routes = Arc::clone(&routes);
            async move { Ok::<_, Infallible>(dispatch(&routes, req).await) }
        }))
    }
}

/// Match `path` against `segments`. Literals are compared **raw** (an encoded `%65` does not match `e`): decoding before matching would let an
/// encoded spelling slip past a layer that looks at the raw path. Captures are returned raw, in route order.
fn matches(segments: &[Segment], path: &str) -> Option<Vec<(String, String)>> {
    let path = path.strip_prefix('/')?;
    let parts: Vec<&str> = path.split('/').collect();
    let mut params = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            Segment::Literal(l) => {
                if parts.get(i) != Some(&l.as_str()) {
                    return None;
                }
            }
            Segment::Param(name) => match parts.get(i) {
                Some(p) if !p.is_empty() => params.push((name.clone(), (*p).to_owned())),
                _ => return None,
            },
            Segment::Rest(name) => {
                if parts.len() < i + 1 {
                    return None;
                }
                params.push((name.clone(), parts[i..].join("/")));
                return Some(params);
            }
        }
    }
    (parts.len() == segments.len()).then_some(params)
}

/// Decode the captures and run the handler, or refuse the request with `400` (the handler never sees an undecodable value).
async fn run(route: &Route, captures: Vec<(String, String)>, mut req: Request) -> Response {
    match decode_params(captures) {
        Ok(params) => {
            req.extensions_mut().insert(params);
            (route.handler)(req).await
        }
        Err(why) => (StatusCode::BAD_REQUEST, why.message()).into_response(),
    }
}

async fn dispatch(routes: &[Route], req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let mut allowed: Vec<&Method> = Vec::new();
    // HEAD is answered by the GET handler unless a route says otherwise; the server, not the router, drops the body.
    let mut head_fallback = None;
    for route in routes {
        if let Some(params) = matches(&route.segments, &path) {
            if route.method == *req.method() {
                return run(route, params, req).await;
            }
            if *req.method() == Method::HEAD
                && route.method == Method::GET
                && head_fallback.is_none()
            {
                head_fallback = Some((route, params));
            }
            if !allowed.contains(&&route.method) {
                allowed.push(&route.method);
            }
        }
    }
    if let Some((route, params)) = head_fallback {
        return run(route, params, req).await;
    }
    if allowed.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut res = StatusCode::METHOD_NOT_ALLOWED.into_response();
    let mut names: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
    if allowed.contains(&&Method::GET) && !allowed.contains(&&Method::HEAD) {
        let at = names.iter().position(|n| *n == "GET").map_or(0, |i| i + 1);
        names.insert(at, "HEAD");
    }
    let list = names.join(", ");
    if let Ok(v) = HeaderValue::from_str(&list) {
        res.headers_mut().insert(ALLOW, v);
    }
    res
}

#[cfg(test)]
mod tests {
    use tower::ServiceExt;

    use super::*;
    use crate::Body;

    async fn call(r: Router, method: &str, uri: &str) -> Response {
        let req = http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        r.into_service().oneshot(req).await.unwrap()
    }

    async fn text(res: Response) -> String {
        res.into_body().text(1024).await.unwrap()
    }

    #[tokio::test]
    async fn literal_param_and_rest_routes_capture_values() {
        let r = Router::new()
            .get("/u/:id", |req: Request| async move {
                req.extensions()
                    .get::<PathParams>()
                    .unwrap()
                    .get("id")
                    .unwrap()
                    .to_owned()
            })
            .get("/f/*rest", |req: Request| async move {
                req.extensions()
                    .get::<PathParams>()
                    .unwrap()
                    .get("rest")
                    .unwrap()
                    .to_owned()
            });
        assert_eq!(text(call(r.clone(), "GET", "/u/42").await).await, "42");
        assert_eq!(
            text(call(r.clone(), "GET", "/f/a/b/c").await).await,
            "a/b/c"
        );
        assert_eq!(call(r, "GET", "/u/").await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn trailing_slash_is_a_different_path() {
        let r = Router::new()
            .get("/a", || async { "a" })
            .get("/b/", || async { "b" });
        assert_eq!(call(r.clone(), "GET", "/a").await.status(), StatusCode::OK);
        assert_eq!(
            call(r.clone(), "GET", "/a/").await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(r.clone(), "GET", "/b").await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(call(r, "GET", "/b/").await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn wrong_method_is_405_with_allow_and_never_the_get_handler() {
        let r = Router::new().get("/m", || async { "get" }).route(
            Method::from_bytes(b"PROPFIND").unwrap(),
            "/m",
            || async { "pf" },
        );
        let res = call(r.clone(), "MKCOL", "/m").await;
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(res.headers()[ALLOW], "GET, HEAD, PROPFIND");
        assert_eq!(text(call(r, "PROPFIND", "/m").await).await, "pf");
    }

    #[tokio::test]
    async fn head_runs_the_get_handler_unless_a_head_route_exists() {
        let r = Router::new()
            .get("/g", || async { "get" })
            .route(Method::HEAD, "/h", || async { "explicit" })
            .get("/h", || async { "get" });
        assert_eq!(text(call(r.clone(), "HEAD", "/g").await).await, "get");
        assert_eq!(text(call(r.clone(), "HEAD", "/h").await).await, "explicit");
        let res = call(r, "HEAD", "/nope").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_path_is_404_and_query_does_not_affect_matching() {
        let r = Router::new().get("/a", || async { "a" });
        assert_eq!(
            call(r.clone(), "GET", "/zzz").await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(call(r, "GET", "/a?x=1").await.status(), StatusCode::OK);
    }

    async fn param(r: &Router, target: &str) -> (StatusCode, String) {
        let res = call(r.clone(), "GET", target).await;
        let status = res.status();
        (status, text(res).await)
    }

    fn decoding_router() -> Router {
        let get = |name: &'static str| {
            move |req: Request| async move {
                let p = req.extensions().get::<PathParams>().unwrap();
                format!("{}|{}", p.get(name).unwrap(), p.get_raw(name).unwrap())
            }
        };
        Router::new()
            .get("/p/:x", get("x"))
            .get("/f/*rest", get("rest"))
            .get("/hello", || async { "literal" })
    }

    #[tokio::test]
    async fn a_param_is_percent_decoded_and_the_raw_text_is_kept() {
        let r = decoding_router();
        assert_eq!(param(&r, "/p/a%20b").await.1, "a b|a%20b");
        assert_eq!(param(&r, "/p/caf%C3%A9").await.1, "caf\u{e9}|caf%C3%A9");
        assert_eq!(param(&r, "/p/%41%62").await.1, "Ab|%41%62");
        // `+` is a plus in a path (a space only in form encoding); a lower-case escape works.
        assert_eq!(param(&r, "/p/a+b%2d").await.1, "a+b-|a+b%2d");
    }

    #[tokio::test]
    async fn a_rest_capture_is_decoded_segment_by_segment() {
        let r = decoding_router();
        assert_eq!(param(&r, "/f/a%20b/c").await.1, "a b/c|a%20b/c");
        assert_eq!(param(&r, "/f/").await.1, "|");
    }

    #[tokio::test]
    async fn a_malformed_escape_is_a_400_and_no_handler_runs() {
        let r = decoding_router();
        for target in ["/p/%zz", "/p/%4", "/p/%", "/p/a%2", "/f/ok/%g0"] {
            let (status, body) = param(&r, target).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{target}");
            assert!(body.contains("hex digits"), "{target}: {body}");
        }
    }

    #[tokio::test]
    async fn decoded_bytes_that_are_not_utf8_are_a_400() {
        let r = decoding_router();
        let (status, body) = param(&r, "/p/%ff%fe").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("UTF-8"), "{body}");
    }

    #[tokio::test]
    async fn an_encoded_separator_or_nul_is_a_400() {
        let r = decoding_router();
        for target in ["/p/a%2Fb", "/p/a%2fb", "/p/a%5Cb", "/p/%00", "/f/a/b%2Fc"] {
            let (status, body) = param(&r, target).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{target}");
            assert!(body.contains("encoded"), "{target}: {body}");
        }
    }

    #[tokio::test]
    async fn an_encoded_dot_segment_is_a_400() {
        let r = decoding_router();
        for target in ["/p/%2e%2e", "/p/%2E", "/f/a/%2e%2e/b", "/f/%2e%2E"] {
            let (status, body) = param(&r, target).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{target}");
            assert!(body.contains("segment"), "{target}: {body}");
        }
        // A name that merely contains dots is a name.
        assert_eq!(param(&r, "/p/..a").await.0, StatusCode::OK);
        assert_eq!(param(&r, "/p/a.b").await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn literals_are_matched_raw_so_an_encoded_spelling_is_a_different_path() {
        let r = decoding_router();
        assert_eq!(param(&r, "/hello").await.1, "literal");
        assert_eq!(param(&r, "/h%65llo").await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_undecodable_value_is_refused_on_head_too_and_not_on_a_wrong_method() {
        let r = decoding_router();
        assert_eq!(
            call(r.clone(), "HEAD", "/p/%zz").await.status(),
            StatusCode::BAD_REQUEST
        );
        // The path has no POST route: the answer is 405, not a decoding error.
        assert_eq!(
            call(r, "POST", "/p/%zz").await.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    #[should_panic(expected = "must be last")]
    fn rest_segment_must_be_last() {
        let _ = Router::new().get("/a/*r/b", || async { "x" });
    }
}
