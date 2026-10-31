// SPDX-License-Identifier: MIT OR Apache-2.0
//! The bare web libraries: each serves `GET /` with the same fixed body and does nothing else, on a listener it is given. These are the
//! baseline an HTTPAnt adapter is compared with, so each is written the way that library's own documentation serves a route.
use std::convert::Infallible;
use std::net::TcpListener as StdListener;

use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;

use crate::BODY;

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
pub fn serve(id: &str, listener: StdListener) -> Bare {
    listener.set_nonblocking(true).expect("non-blocking");
    match id {
        "hyper" => Bare::task(tokio::spawn(hyper_bare(listener))),
        "axum" => Bare::task(tokio::spawn(axum_bare(listener))),
        "actix-web" => actix_bare(listener),
        "poem" => Bare::task(tokio::spawn(poem_bare(listener))),
        "salvo" => Bare::task(tokio::spawn(salvo_bare(listener))),
        "warp" => Bare::task(tokio::spawn(warp_bare(listener))),
        other => panic!("no bare server for {other}"),
    }
}

async fn hyper_bare(listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            let svc =
                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
                    Ok::<_, Infallible>(hyper::Response::new(Full::new(Bytes::from_static(
                        BODY.as_bytes(),
                    ))))
                });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

async fn axum_bare(listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let app = axum::Router::new().route("/", axum::routing::get(|| async { BODY }));
    let _ = axum::serve(listener, app).await;
}

/// actix runs on threads of its own; two workers, the same as the HTTPAnt actix adapter's cleartext listener.
fn actix_bare(listener: StdListener) -> Bare {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            let server = actix_web::HttpServer::new(|| {
                actix_web::App::new().route(
                    "/",
                    actix_web::web::get().to(|| async { actix_web::HttpResponse::Ok().body(BODY) }),
                )
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

async fn poem_bare(listener: StdListener) {
    #[poem::handler]
    fn hello() -> &'static str {
        BODY
    }
    let acceptor = poem::listener::TcpAcceptor::from_std(listener).expect("acceptor");
    let app = poem::Route::new().at("/", poem::get(hello));
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

async fn salvo_bare(listener: StdListener) {
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let acceptor = salvo_core::conn::tcp::TcpAcceptor::try_from(listener).expect("acceptor");
    let router = salvo_core::Router::with_path("/").get(SalvoHello);
    salvo_core::Server::new(acceptor)
        .serve(salvo_core::Service::new(router))
        .await;
}

async fn warp_bare(listener: StdListener) {
    use warp::Filter;
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let route =
        warp::path::end().map(|| warp::http::Response::new(Bytes::from_static(BODY.as_bytes())));
    warp::serve(route).incoming(listener).run().await;
}
