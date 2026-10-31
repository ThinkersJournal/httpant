// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extractors: typed access to the path captures and the query string, as handler arguments.
//!
//! ```
//! # use httpant_core::{Path, Query, Router};
//! #[derive(serde::Deserialize)]
//! struct Id { id: u32 }
//! #[derive(serde::Deserialize)]
//! struct Page { page: Option<u32> }
//!
//! let router = Router::new().get("/items/:id", |Path(Id { id }): Path<Id>, Query(p): Query<Page>| async move {
//!     format!("item {id}, page {}", p.page.unwrap_or(1))
//! });
//! # let _ = router;
//! ```
//!
//! A value that does not fit its type is answered `400` before the handler runs; the response says so in general terms and never repeats what the
//! client sent. There are two typed extractors, [`PathParams`] and the trait to write your own, not a zoo: the request itself is still a handler
//! argument.
use std::marker::PhantomData;

use http::StatusCode;
use serde::de::DeserializeOwned;

use crate::{IntoResponse, PathParams, Request, Response};

/// Something a handler can take as an argument, built from the request without consuming its body. On failure the `Err` is the response sent
/// instead of running the handler.
pub trait FromRequest: Sized + Send + 'static {
    /// Build the value from the request, or the response to answer with.
    fn from_request(req: &Request) -> Result<Self, Response>;
}

/// The path captures as a value of type `T` (a struct whose field names are the route's `:name` / `*rest` names, or a map).
///
/// Values are the percent-decoded captures (see [`PathParams`]); numbers and other types that parse from text are parsed. A capture that does
/// not parse (`/items/abc` for `id: u32`) is answered `400`. A field the route does not capture is a mistake in the handler, not in the request,
/// and is answered `500` with a message that names no field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<T>(pub T);

/// The query string as a value of type `T` (a struct or map; `Option` fields are the optional parameters).
///
/// `+` and `%XX` are decoded as in HTML forms. A missing required field, a value that does not parse, a repeated field (for a non-sequence
/// field) or bytes that are not UTF-8 are answered `400`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query<T>(pub T);

fn plain(status: StatusCode, text: &'static str) -> Response {
    let mut res = text.into_response();
    *res.status_mut() = status;
    res
}

impl FromRequest for PathParams {
    fn from_request(req: &Request) -> Result<Self, Response> {
        // The router puts the captures in the extensions; a route with none has none to give.
        Ok(req
            .extensions()
            .get::<PathParams>()
            .cloned()
            .unwrap_or_default())
    }
}

impl<T: DeserializeOwned + Send + 'static> FromRequest for Path<T> {
    fn from_request(req: &Request) -> Result<Self, Response> {
        let params = req
            .extensions()
            .get::<PathParams>()
            .cloned()
            .unwrap_or_default();
        let pairs: Vec<(&str, &str)> = params.iter().collect();
        // Through the form encoding and back, so "42" becomes a `u32` the way serde's own text formats do it, without a second parser.
        let encoded = serde_urlencoded::to_string(&pairs).map_err(|_| {
            plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server error: the route's path captures could not be read; report this to the site's operator",
            )
        })?;
        match serde_urlencoded::from_str::<T>(&encoded) {
            Ok(v) => Ok(Path(v)),
            // serde's own wording for a field the type wants and the route never captured: the handler's mistake, not the client's.
            Err(e) if e.to_string().starts_with("missing field") => Err(plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server error: the handler asks for a path parameter its route does not capture; report this to the site's operator",
            )),
            Err(_) => Err(plain(
                StatusCode::BAD_REQUEST,
                "bad request: a value in the path is not of the kind this route expects; check the URL",
            )),
        }
    }
}

impl<T: DeserializeOwned + Send + 'static> FromRequest for Query<T> {
    fn from_request(req: &Request) -> Result<Self, Response> {
        let raw = req.uri().query().unwrap_or("");
        let bad = || {
            plain(
                StatusCode::BAD_REQUEST,
                "bad request: the query string is missing a parameter or has one of the wrong kind; check the URL",
            )
        };
        // The form decoder below replaces bytes that are not UTF-8 with U+FFFD; a handler should not get a value the client did not send.
        if !decodes_to_utf8(raw) {
            return Err(bad());
        }
        serde_urlencoded::from_str::<T>(raw)
            .map(Query)
            .map_err(|_| bad())
    }
}

/// Whether the `%XX` escapes in `s` produce valid UTF-8 (a `%` not followed by two hex digits stays as it is, as the form decoder leaves it).
fn decodes_to_utf8(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: Option<&u8>| b.and_then(|b| (*b as char).to_digit(16));
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2)))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    std::str::from_utf8(&out).is_ok()
}

#[doc(hidden)]
pub struct Extracted<T>(PhantomData<fn() -> T>);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::Deserialize;
    use tower::ServiceExt;

    use crate::{Body, Path, PathParams, Query, Router};

    async fn call(r: &Router, uri: &str) -> (u16, String) {
        let req = http::Request::builder()
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let res = r.clone().into_service().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        (status, res.into_body().text(1024).await.unwrap())
    }

    #[derive(Deserialize)]
    struct Item {
        id: u32,
        name: String,
    }

    #[derive(Deserialize)]
    struct Page {
        page: Option<u32>,
        q: String,
    }

    fn router() -> Router {
        Router::new()
            .get("/items/:id/:name", |Path(i): Path<Item>| async move {
                format!("{}|{}", i.id, i.name)
            })
            .get("/search", |Query(p): Query<Page>| async move {
                format!("{:?}|{}", p.page, p.q)
            })
            .get(
                "/both/:id",
                |Path(m): Path<HashMap<String, String>>,
                 Query(q): Query<HashMap<String, String>>| async move {
                    format!("{}|{}", m["id"], q.get("x").map_or("-", |v| v.as_str()))
                },
            )
            .get("/raw/:id", |p: PathParams| async move {
                p.get("id").unwrap().to_owned()
            })
            .get("/wrong/:id", |Path(_i): Path<Item>| async move { "never" })
    }

    #[tokio::test]
    async fn path_fields_are_parsed_and_percent_decoded() {
        let r = router();
        assert_eq!(
            call(&r, "/items/42/a%20b%2Bc+d").await,
            (200, "42|a b+c+d".to_owned())
        );
    }

    #[tokio::test]
    async fn a_path_value_of_the_wrong_kind_is_400_and_does_not_echo_it() {
        let (status, body) = call(&router(), "/items/abc/x").await;
        assert_eq!(status, 400);
        assert!(!body.contains("abc"), "the response repeated client input");
    }

    #[tokio::test]
    async fn a_field_the_route_does_not_capture_is_500_not_400() {
        // `Item` wants `name`; "/wrong/:id" captures only `id`: the handler's mistake.
        let (status, body) = call(&router(), "/wrong/7").await;
        assert_eq!(status, 500);
        assert!(!body.contains("name"), "the response named the field");
    }

    #[tokio::test]
    async fn query_fields_are_parsed_optional_ones_may_be_absent() {
        let r = router();
        assert_eq!(
            call(&r, "/search?q=hello+world%21&page=3").await,
            (200, "Some(3)|hello world!".to_owned())
        );
        assert_eq!(call(&r, "/search?q=x").await, (200, "None|x".to_owned()));
    }

    #[tokio::test]
    async fn a_missing_or_wrong_query_value_is_400_and_does_not_echo_it() {
        let r = router();
        assert_eq!(call(&r, "/search").await.0, 400);
        let (status, body) = call(&r, "/search?q=x&page=SECRETVALUE").await;
        assert_eq!(status, 400);
        assert!(!body.contains("SECRETVALUE"));
        // Bytes that are not UTF-8 once decoded.
        assert_eq!(call(&r, "/search?q=%FF").await.0, 400);
    }

    #[tokio::test]
    async fn two_extractors_and_the_raw_params_work_together() {
        let r = router();
        assert_eq!(call(&r, "/both/9?x=1").await, (200, "9|1".to_owned()));
        assert_eq!(call(&r, "/raw/zz").await, (200, "zz".to_owned()));
    }

    #[tokio::test]
    async fn a_failing_extractor_stops_the_handler() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let r = Router::new().get("/n/:id", move |Path(i): Path<Item>| {
            let flag = Arc::clone(&flag);
            async move {
                flag.store(true, Ordering::SeqCst);
                i.id.to_string()
            }
        });
        assert_eq!(call(&r, "/n/5").await.0, 500);
        assert!(
            !ran.load(Ordering::SeqCst),
            "the handler ran after its extractor failed"
        );
    }
}
