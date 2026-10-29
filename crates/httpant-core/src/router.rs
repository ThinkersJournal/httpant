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
/// Values are the raw path text: **not** percent-decoded (UNVERIFIED which rule users want; decide at review).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathParams(pub Vec<(String, String)>);

impl PathParams {
    /// The value captured under `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Routes with `:name` and `*rest` path syntax and exact-method matching on [`http::Method`] (extension methods work).
///
/// A known path with the wrong method answers `405` with an `Allow` header; an unknown path `404`. **No path normalisation**: `/a` and `/a/`
/// are different paths, the same on every adapter (design section 3).
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

fn matches(segments: &[Segment], path: &str) -> Option<PathParams> {
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
                return Some(PathParams(params));
            }
        }
    }
    (parts.len() == segments.len()).then_some(PathParams(params))
}

async fn dispatch(routes: &[Route], mut req: Request) -> Response {
    let path = req.uri().path().to_owned();
    let mut allowed: Vec<&Method> = Vec::new();
    for route in routes {
        if let Some(params) = matches(&route.segments, &path) {
            if route.method == *req.method() {
                req.extensions_mut().insert(params);
                return (route.handler)(req).await;
            }
            if !allowed.contains(&&route.method) {
                allowed.push(&route.method);
            }
        }
    }
    if allowed.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut res = StatusCode::METHOD_NOT_ALLOWED.into_response();
    let list = allowed
        .iter()
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(", ");
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
        assert_eq!(res.headers()[ALLOW], "GET, PROPFIND");
        assert_eq!(text(call(r, "PROPFIND", "/m").await).await, "pf");
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

    #[test]
    #[should_panic(expected = "must be last")]
    fn rest_segment_must_be_last() {
        let _ = Router::new().get("/a/*r/b", || async { "x" });
    }
}
