// SPDX-License-Identifier: MIT OR Apache-2.0
//! actix-web serving connections whose TLS handshake HTTPAnt ran itself.
//!
//! `HttpServer` has no entry point for an already-handshaken stream (`docs/TLS-ADAPTER-PROBES.md`), and the certificate resolver in actix's own
//! rustls support is synchronous, so an async, refusing `CertSource` would block an actix worker. This module does not use `HttpServer` for TLS:
//! the accept loop and every handshake run as tasks on the caller's runtime (so a slow certificate source costs only its own handshake, and the
//! audit events come from HTTPAnt's acceptor, as on hyper, axum, poem and salvo), and each finished stream is handed to `actix_http::HttpService`
//! (the same service `HttpServer` builds, over `actix_web::App`) on an actix thread. HTTP/1.1 only: actix-http's HTTP/2 is `h2` 0.3, which has an
//! advisory with no fix in that line (RUSTSEC-2026-0258).

use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use actix_http::{HttpService, Protocol};
use actix_service::{Service, ServiceFactory, map_config};
use actix_web::dev::AppConfig;
use actix_web::{App, HttpRequest, web};
use httpant_core::{BoxService, Error, Result};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::server::TlsStream;

use crate::{Grace, RunningServer, TlsAcceptor};

/// Counts requests in flight: held from the head to the last byte of the response body, so shutdown can wait for exactly those.
#[derive(Clone, Default)]
pub(crate) struct InFlight(Arc<AtomicUsize>);

pub(crate) struct InFlightGuard(Arc<AtomicUsize>);

impl InFlight {
    pub(crate) fn enter(&self) -> InFlightGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        InFlightGuard(Arc::clone(&self.0))
    }

    fn current(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A finished TLS stream, as actix-http reads and writes it.
struct Io(TlsStream<TcpStream>);

impl AsyncRead for Io {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Io {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

type Ready = (Io, Protocol, SocketAddr);

pub(crate) async fn serve(
    svc: BoxService,
    listener: std::net::TcpListener,
    tls: TlsAcceptor,
    grace: Grace,
) -> Result<RunningServer> {
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let tcp = TcpListener::from_std(listener)?;
    let (ready_tx, mut ready_rx) = mpsc::channel::<Ready>(128);
    let accepting = tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = tcp.accept().await else {
                // Out of descriptors or a reset on accept: back off briefly rather than spin.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            };
            let (tls, ready_tx) = (tls.clone(), ready_tx.clone());
            tokio::spawn(async move {
                if let Ok(stream) = tls.accept(stream).await {
                    // `h2` is never offered on this listener (see `ActixAdapter::capabilities`), so every connection is HTTP/1.1.
                    let _ = ready_tx.send((Io(stream), Protocol::Http1, peer)).await;
                }
            });
        }
    });

    let in_flight = InFlight::default();
    let counter = in_flight.clone();
    let (actix_stop, actix_stopped) = oneshot::channel::<()>();
    let (done_tx, done_rx) = oneshot::channel::<()>();
    let (started_tx, started_rx) = oneshot::channel::<std::io::Result<()>>();
    std::thread::Builder::new()
        .name("httpant-actix-tls".into())
        .spawn(move || {
            actix_web::rt::System::new().block_on(async move {
                let app = move || {
                    let (svc, counter) = (svc.clone(), counter.clone());
                    App::new().default_service(web::to(
                        move |req: HttpRequest, payload: web::Payload| {
                            let guard = counter.enter();
                            crate::actix_adapter::handle(svc.clone(), req, payload, Some(guard))
                        },
                    ))
                };
                let http =
                    HttpService::build().finish(map_config(app(), |_: ()| AppConfig::default()));
                let handler = match <HttpService<Io, _, _, _, _> as ServiceFactory<(
                    Io,
                    Protocol,
                    Option<SocketAddr>,
                )>>::new_service(&http, ())
                .await
                {
                    Ok(handler) => Rc::new(handler),
                    Err(_) => {
                        let _ = started_tx.send(Err(std::io::Error::other(
                            "actix could not build its HTTP service",
                        )));
                        return;
                    }
                };
                let _ = started_tx.send(Ok(()));
                let mut stopped = actix_stopped;
                loop {
                    tokio::select! {
                        biased;
                        _ = &mut stopped => break,
                        next = ready_rx.recv() => {
                            let Some((io, protocol, peer)) = next else { break };
                            let handler = Rc::clone(&handler);
                            actix_web::rt::spawn(async move {
                                let _ = handler.call((io, protocol, Some(peer))).await;
                            });
                        }
                    }
                }
                // Stop accepting, then wait for the requests that are running (the grace wrapper ends them at the deadline). A keep-alive
                // connection with nothing running is closed when this returns.
                drop(ready_rx);
                while in_flight.current() > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            });
            let _ = done_tx.send(());
        })?;
    started_rx
        .await
        .map_err(|_| Error::Io(std::io::Error::other("actix thread ended early")))??;

    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        // A dropped `RunningServer` stops the server too, as in the other adapters.
        let _ = stopped.await;
        accepting.abort();
        let _ = actix_stop.send(());
        let _ = done_rx.await;
    });
    Ok(RunningServer::new(addr, stop, task, grace))
}
