// SPDX-License-Identifier: MIT OR Apache-2.0
//! Case 24: handlers that are not `Send`. A route registered with `LocalHandlers::route` is served only by an adapter that declares
//! `local_handlers: Yes`; any other refuses to start (`LocalHandlersNotSupported`). Where it is served, it is served *inside* everything
//! HTTPAnt wraps around a route: the body limit, the shutdown grace, the trusted-proxy `Origin`, the ACME challenge prefix and the HTTPS
//! redirect. Each of those is checked here on a local route, and each has a mutant that serves with that one thing off.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use httpant_core::http::Method;
use httpant_core::{BoxFuture, Bytes, Error, Request, Router, Support};
use httpant_server::{ChallengeResponder, HttpsRedirect, LocalHandlers, TrustedProxies};
use tokio::sync::Notify;

use crate::raw::{Conn, RawRequest, RawResponse, send};
use crate::subject::{App, Setup, Subject};

type Outcome = Result<(), String>;

const PROMPT: Duration = Duration::from_secs(5);

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

#[derive(Debug)]
struct Pending;

impl ChallengeResponder for Pending {
    fn key_authorization<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move { (token == "tok1").then(|| "tok1.thumbprint".to_owned()) })
    }
}

/// Every route here is local: each factory builds state with an `Rc`, which a `Send` handler could not hold.
fn local_app(hang_started: Arc<Notify>) -> App {
    let mut local = LocalHandlers::new();
    let mut router = Router::new();
    router = local.route(router, Method::GET, "/local/hello", || {
        let seen = Rc::new(Cell::new(0u32));
        move |_req: Request| {
            let seen = Rc::clone(&seen);
            async move {
                seen.set(seen.get() + 1);
                // Held across an await: the future is not `Send`.
                tokio::task::yield_now().await;
                format!("local {}", seen.get())
            }
        }
    });
    router = local.route(router, Method::POST, "/local/echo", || {
        |req: Request| async move { req.into_body().bytes(1 << 20).await }
    });
    // Reads the whole body, asking for far more than any server limit, so only the server's limit can stop it.
    router = local.route(router, Method::POST, "/local/sink", || {
        |req: Request| async move {
            let bytes: Result<Bytes, Error> = req.into_body().bytes(1 << 40).await;
            bytes.map(|b| format!("got {}", b.len()))
        }
    });
    router = local.route(router, Method::GET, "/local/origin", || {
        |req: Request| async move {
            match req.extensions().get::<httpant_server::Origin>() {
                Some(o) => format!(
                    "{} {} {}",
                    o.client,
                    match o.scheme {
                        httpant_server::Scheme::Http => "http",
                        httpant_server::Scheme::Https => "https",
                    },
                    if o.via_trusted_proxy {
                        "proxy"
                    } else {
                        "direct"
                    }
                ),
                None => "none".to_owned(),
            }
        }
    });
    router = local.route(router, Method::GET, "/local/hang", move || {
        let started = Arc::clone(&hang_started);
        move |_req: Request| {
            let started = Arc::clone(&started);
            async move {
                started.notify_one();
                std::future::pending::<String>().await
            }
        }
    });
    // A route of the application's own on the ACME prefix: the prefix is answered before any route, local ones included.
    router = local.route(
        router,
        Method::GET,
        "/.well-known/acme-challenge/:token",
        || |_req: Request| async { "from the local app" },
    );
    App {
        router,
        layers: Vec::new(),
        local,
    }
}

async fn start(
    s: &dyn Subject,
    setup: Setup,
) -> Result<(httpant_server::RunningServer, Arc<Notify>), Error> {
    let started = Arc::new(Notify::new());
    let server = s.start_with(local_app(Arc::clone(&started)), setup).await?;
    Ok((server, started))
}

async fn ask(addr: std::net::SocketAddr, req: RawRequest) -> Result<RawResponse, String> {
    send(addr, &req)
        .await
        .map_err(|e| format!("request failed: {e}"))
}

fn text(res: &RawResponse) -> String {
    String::from_utf8_lossy(&res.body).into_owned()
}

pub fn local_handlers(s: &dyn Subject) -> BoxFuture<'_, Outcome> {
    Box::pin(async move {
        // A subject that declares nothing (a mutant) is one that serves local routes: they are built on the adapter that does.
        let serves = s
            .declared()
            .is_none_or(|c| c.local_handlers == Support::Yes);
        if !serves {
            return match start(s, Setup::default()).await {
                Err(Error::LocalHandlersNotSupported { .. }) => Ok(()),
                Err(e) => Err(format!(
                    "an adapter that declares `local_handlers: No` must refuse a local route with `LocalHandlersNotSupported`, got: {e}"
                )),
                Ok((server, _)) => {
                    let _ = server.shutdown().await;
                    Err("an adapter that declares `local_handlers: No` started with a local route (it must refuse, never run it elsewhere)".to_owned())
                }
            };
        }
        let begin = |setup: Setup| async move {
            start(s, setup)
                .await
                .map_err(|e| format!("could not start with local routes: {e}"))
        };

        // The route works, is routed like any other (404, 405 with Allow), and a body reaches it.
        {
            let (server, _) = begin(Setup::default()).await?;
            let addr = server.addr();
            for _ in 0..4 {
                let res = ask(addr, RawRequest::new("GET", "/local/hello")).await?;
                ensure!(
                    res.status == 200 && text(&res).starts_with("local "),
                    "GET /local/hello: got {} `{}`",
                    res.status,
                    text(&res)
                );
            }
            let res = ask(addr, RawRequest::new("POST", "/local/echo").body("hi")).await?;
            ensure!(
                res.status == 200 && res.body == b"hi",
                "POST /local/echo: got {} `{}`",
                res.status,
                text(&res)
            );
            let res = ask(addr, RawRequest::new("DELETE", "/local/hello")).await?;
            let allow = res
                .header_values("allow")
                .iter()
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .collect::<Vec<_>>()
                .join(",");
            ensure!(
                res.status == 405 && allow.contains("GET"),
                "DELETE on a local GET route: expected 405 with Allow: GET, got {} Allow `{allow}`",
                res.status
            );
            let res = ask(addr, RawRequest::new("GET", "/local/nope")).await?;
            ensure!(res.status == 404, "unknown path: got {}", res.status);
            let _ = server.shutdown().await;
        }

        // The body limit holds on a local route: declared 3 MiB over the 2 MiB default is refused on the head.
        {
            let (server, _) = begin(Setup::default()).await?;
            let res = ask(
                server.addr(),
                RawRequest::new("POST", "/local/sink")
                    .header("Content-Length", (3 * 1024 * 1024).to_string())
                    .body(vec![b'x'; 16 * 1024]),
            )
            .await?;
            let _ = server.shutdown().await;
            ensure!(
                res.status == 413,
                "a declared 3 MiB body on a local route (limit 2 MiB): expected 413, got {}",
                res.status
            );
        }

        // The `Origin` is there on a local route, and believed only from a trusted proxy.
        {
            let setup = Setup {
                trusted_proxies: TrustedProxies::none()
                    .parse("127.0.0.0/8")
                    .map_err(|e| format!("bad test network: {e}"))?,
                ..Setup::default()
            };
            let (server, _) = begin(setup).await?;
            let res = ask(
                server.addr(),
                RawRequest::new("GET", "/local/origin")
                    .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
                    .header("x-forwarded-proto", "https"),
            )
            .await?;
            let _ = server.shutdown().await;
            ensure!(
                res.status == 200 && text(&res) == "203.0.113.9 https proxy",
                "a trusted proxy on a local route: expected `203.0.113.9 https proxy`, got {} `{}`",
                res.status,
                text(&res)
            );
        }

        // The ACME prefix is answered before any route, a local one on that very path included.
        {
            let setup = Setup {
                challenge: Some(Arc::new(Pending)),
                ..Setup::default()
            };
            let (server, _) = begin(setup).await?;
            let res = ask(
                server.addr(),
                RawRequest::new("GET", "/.well-known/acme-challenge/tok1"),
            )
            .await?;
            let _ = server.shutdown().await;
            ensure!(
                res.status == 200 && text(&res) == "tok1.thumbprint",
                "the ACME challenge over a local route on its path: expected `tok1.thumbprint`, got {} `{}`",
                res.status,
                text(&res)
            );
        }

        // The HTTPS redirect covers a local route: a listed host is sent to https, any other host is 421.
        {
            let setup = Setup {
                https_redirect: Some(HttpsRedirect::new(["example.test"])),
                ..Setup::default()
            };
            let (server, _) = begin(setup).await?;
            let addr = server.addr();
            let listed = ask(
                addr,
                RawRequest::new("GET", "/local/hello").header("host", "example.test"),
            )
            .await?;
            let other = ask(
                addr,
                RawRequest::new("GET", "/local/hello").header("host", "other.test"),
            )
            .await?;
            let _ = server.shutdown().await;
            let location = listed
                .header_values("location")
                .first()
                .map(|v| String::from_utf8_lossy(v).into_owned());
            ensure!(
                listed.status == 308
                    && location.as_deref() == Some("https://example.test/local/hello"),
                "a listed host on a local route: expected 308 to https://example.test/local/hello, got {} {location:?}",
                listed.status
            );
            ensure!(
                other.status == 421,
                "an unlisted host on a local route: expected 421, got {}",
                other.status
            );
        }

        // The shutdown grace ends a local handler that never finishes, and does not do it early.
        {
            const GRACE: Duration = Duration::from_millis(1500);
            let setup = Setup {
                grace: Some(GRACE),
                ..Setup::default()
            };
            let (server, hang_started) = begin(setup).await?;
            let mut stuck = Conn::open(server.addr())
                .await
                .map_err(|e| format!("could not connect: {e}"))?
                .with_timeout(PROMPT);
            stuck
                .send(&RawRequest::new("GET", "/local/hang"))
                .await
                .map_err(|e| format!("send /local/hang: {e}"))?;
            ensure!(
                tokio::time::timeout(PROMPT, hang_started.notified())
                    .await
                    .is_ok(),
                "the local /local/hang handler never started"
            );
            let started = tokio::time::Instant::now();
            let stopping = tokio::spawn(server.shutdown());
            tokio::time::sleep(GRACE / 3).await;
            ensure!(
                !stopping.is_finished(),
                "shutdown returned after {:?}, before the {GRACE:?} grace period: a local request in flight was cut early",
                started.elapsed()
            );
            match tokio::time::timeout(GRACE + Duration::from_secs(3), stopping).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => return Err(format!("shutdown failed: {e}")),
                Ok(Err(e)) => return Err(format!("shutdown task: {e}")),
                Err(_) => {
                    return Err(format!(
                        "shutdown did not return {:?} after it was requested, with a grace period of {GRACE:?}: a local request that never finishes holds it",
                        started.elapsed()
                    ));
                }
            }
            if let Ok(res) = stuck.response("GET").await {
                ensure!(
                    res.status == 503,
                    "the abandoned local request was answered {} (expected 503 or a closed connection)",
                    res.status
                );
            }
        }
        Ok(())
    })
}
