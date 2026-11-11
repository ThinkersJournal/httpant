// SPDX-License-Identifier: MIT OR Apache-2.0
//! The bare web libraries: each serves one route (`GET /` with a fixed body, or `GET /items/{id}` answering `item ` and twice the id) and
//! does nothing else, on a listener it is given. These are the baseline an HTTPAnt adapter is compared with, so each is written the way that
//! library's own documentation serves a route. Also the ceiling: not a library, a TCP server that answers every read with the right bytes.
use std::convert::Infallible;
use std::net::TcpListener as StdListener;

use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;

use crate::{BODY, Workload};

/// A running bare server: stopping it ends the task (or, for actix, the thread's server).
pub struct Bare {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl Bare {
    fn task(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            stop: Some(Box::new(move || handle.abort())),
        }
    }

    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// Serve `id` bare on `listener`. `id` is one of `crate::ADAPTERS`.
pub fn serve(id: &str, workload: Workload, listener: StdListener) -> Bare {
    listener.set_nonblocking(true).expect("non-blocking");
    match id {
        "hyper" => Bare::task(tokio::spawn(hyper_bare(workload, listener))),
        "axum" => Bare::task(tokio::spawn(axum_bare(workload, listener))),
        "actix-web" => actix_bare(workload, listener),
        "poem" => Bare::task(tokio::spawn(poem_bare(workload, listener))),
        "salvo" => Bare::task(tokio::spawn(salvo_bare(workload, listener))),
        "warp" => Bare::task(tokio::spawn(warp_bare(workload, listener))),
        other => panic!("no bare server for {other}"),
    }
}

/// The load generator's ceiling: a TCP server that answers every read with the right response for `workload`, parsing nothing.
pub fn ceiling(workload: Workload, listener: StdListener) -> Bare {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    listener.set_nonblocking(true).expect("non-blocking");
    let body = workload.body();
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    Bare::task(tokio::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let _ = stream.set_nodelay(true);
            let reply = reply.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                // One request is in flight on a connection, so one read is one request.
                while stream.read(&mut buf).await.unwrap_or(0) > 0 {
                    if stream.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    }))
}

/// The number in `/items/<number>`, or 0 when the path is anything else (a wrong body, which the checker counts as an error).
fn item_id(path: &str) -> u32 {
    path.strip_prefix("/items/")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

async fn hyper_bare(workload: Workload, listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            let svc = hyper::service::service_fn(
                move |req: hyper::Request<hyper::body::Incoming>| async move {
                    let body = match workload {
                        Workload::Constant => Bytes::from_static(BODY.as_bytes()),
                        Workload::Item => {
                            Bytes::from(format!("item {}", item_id(req.uri().path()) * 2))
                        }
                    };
                    Ok::<_, Infallible>(hyper::Response::new(Full::new(body)))
                },
            );
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

async fn axum_bare(workload: Workload, listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let app = match workload {
        Workload::Constant => axum::Router::new().route("/", axum::routing::get(|| async { BODY })),
        Workload::Item => axum::Router::new().route(
            "/items/{id}",
            axum::routing::get(
                |axum::extract::Path(id): axum::extract::Path<u32>| async move {
                    format!("item {}", id * 2)
                },
            ),
        ),
    };
    let _ = axum::serve(listener, app).await;
}

/// actix runs on threads of its own; two workers, the same as the HTTPAnt actix adapter's cleartext listener.
fn actix_bare(workload: Workload, listener: StdListener) -> Bare {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            let server = actix_web::HttpServer::new(move || {
                actix_web::App::new().configure(move |cfg| match workload {
                    Workload::Constant => {
                        cfg.route(
                            "/",
                            actix_web::web::get()
                                .to(|| async { actix_web::HttpResponse::Ok().body(BODY) }),
                        );
                    }
                    Workload::Item => {
                        cfg.route(
                            "/items/{id}",
                            actix_web::web::get().to(|id: actix_web::web::Path<u32>| async move {
                                actix_web::HttpResponse::Ok().body(format!("item {}", *id * 2))
                            }),
                        );
                    }
                })
            })
            .workers(2)
            .disable_signals()
            .listen(listener)
            .expect("listen")
            .run();
            let _ = tx.send(server.handle());
            let _ = server.await;
        });
    });
    let handle = rx.recv().expect("actix handle");
    Bare {
        stop: Some(Box::new(move || {
            // The server stops on its own thread; do not wait for it here.
            std::thread::spawn(move || {
                actix_web::rt::System::new().block_on(handle.stop(false));
            });
        })),
    }
}

async fn poem_bare(workload: Workload, listener: StdListener) {
    #[poem::handler]
    fn hello() -> &'static str {
        BODY
    }
    #[poem::handler]
    fn item(poem::web::Path(id): poem::web::Path<u32>) -> String {
        format!("item {}", id * 2)
    }
    let acceptor = poem::listener::TcpAcceptor::from_std(listener).expect("acceptor");
    let app = match workload {
        Workload::Constant => poem::Route::new().at("/", poem::get(hello)),
        Workload::Item => poem::Route::new().at("/items/:id", poem::get(item)),
    };
    let _ = poem::Server::new_with_acceptor(acceptor).run(app).await;
}

struct SalvoHello;

#[salvo_core::async_trait]
impl salvo_core::Handler for SalvoHello {
    async fn handle(
        &self,
        _req: &mut salvo_core::Request,
        _depot: &mut salvo_core::Depot,
        res: &mut salvo_core::Response,
        _ctrl: &mut salvo_core::FlowCtrl,
    ) {
        res.render(BODY);
    }
}

struct SalvoItem;

#[salvo_core::async_trait]
impl salvo_core::Handler for SalvoItem {
    async fn handle(
        &self,
        req: &mut salvo_core::Request,
        _depot: &mut salvo_core::Depot,
        res: &mut salvo_core::Response,
        _ctrl: &mut salvo_core::FlowCtrl,
    ) {
        let id: u32 = req.param("id").unwrap_or(0);
        res.render(format!("item {}", id * 2));
    }
}

async fn salvo_bare(workload: Workload, listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let acceptor = salvo_core::conn::tcp::TcpAcceptor::try_from(listener).expect("acceptor");
    let router = match workload {
        Workload::Constant => salvo_core::Router::with_path("/").get(SalvoHello),
        Workload::Item => salvo_core::Router::with_path("items/{id}").get(SalvoItem),
    };
    salvo_core::Server::new(acceptor)
        .serve(salvo_core::Service::new(router))
        .await;
}

async fn warp_bare(workload: Workload, listener: StdListener) {
    use warp::Filter;
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    match workload {
        Workload::Constant => {
            let route = warp::path::end()
                .map(|| warp::http::Response::new(Bytes::from_static(BODY.as_bytes())));
            warp::serve(route).incoming(listener).run().await;
        }
        Workload::Item => {
            let route = warp::path!("items" / u32)
                .map(|id: u32| warp::http::Response::new(Bytes::from(format!("item {}", id * 2))));
            warp::serve(route).incoming(listener).run().await;
        }
    }
}
