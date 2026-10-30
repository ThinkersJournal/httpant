// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant_core::{Body, BoxFuture, BoxService, Capabilities, Request, Result, Support};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;
use tower::ServiceExt;

use crate::{PeerAddr, RunningServer, ServeOptions, ServerAdapter};

/// The reference adapter: hyper 1.x over HTTP/1.1, no framework on top. The conversion is the whole adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct HyperAdapter;

impl ServerAdapter for HyperAdapter {
    fn id(&self) -> &'static str {
        "hyper"
    }

    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        // Measured by conformance cases 8 and 9 (the test `declared_capabilities_match_the_measured_ones`), not asserted.
        c.streaming_request_bodies = Support::Yes;
        c.streaming_response_bodies = Support::Yes;
        c.graceful_shutdown = Support::Yes;
        // Not measurable by this suite: a handler that is not `Send` cannot be written against the `Send` service contract.
        c.local_handlers = Support::No;
        // Measured by the TLS test in `tls_tests` and conformance case 17, not asserted.
        c.tls = if cfg!(feature = "tls") {
            Support::Yes
        } else {
            Support::No
        };
        c
    }

    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>> {
        Box::pin(async move {
            let grace = opts.grace();
            let svc = opts.wrap(svc);
            opts.listener.set_nonblocking(true)?;
            let addr = opts.listener.local_addr()?;
            let listener = TcpListener::from_std(opts.listener)?;
            #[cfg(feature = "tls")]
            let tls = opts.tls.clone();
            let (stop, mut stopped) = oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                let (drain, draining) = watch::channel(false);
                let mut conns = JoinSet::new();
                loop {
                    tokio::select! {
                        _ = &mut stopped => break,
                        accepted = listener.accept() => {
                            let Ok((stream, peer)) = accepted else { continue };
                            let svc = svc.clone();
                            let draining = draining.clone();
                            #[cfg(feature = "tls")]
                            let tls = tls.clone();
                            conns.spawn(async move {
                                #[cfg(feature = "tls")]
                                if let Some(acceptor) = tls {
                                    // A failed handshake (unknown name, bad client, timeout) ends only this connection.
                                    let Ok(stream) = acceptor.accept(stream).await else { return };
                                    return serve_conn(stream, peer, svc, draining, true).await;
                                }
                                serve_conn(stream, peer, svc, draining, false).await;
                            });
                        }
                        Some(_) = conns.join_next(), if !conns.is_empty() => {}
                    }
                }
                // Graceful: stop accepting (the socket closes), tell every connection to finish, and return only when they have.
                drop(listener);
                let _ = drain.send(true);
                while conns.join_next().await.is_some() {}
            });
            Ok(RunningServer::new(addr, stop, task, grace))
        })
    }
}

/// Serve one connection (cleartext or already through the TLS handshake) until it ends or the server drains.
async fn serve_conn<I>(
    io: I,
    peer: std::net::SocketAddr,
    svc: BoxService,
    mut draining: watch::Receiver<bool>,
    over_tls: bool,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
        let mut req: Request = req.map(Body::new);
        req.extensions_mut().insert(PeerAddr(peer));
        #[cfg(feature = "tls")]
        if over_tls {
            req.extensions_mut().insert(crate::TlsConnection);
        }
        let _ = over_tls;
        svc.clone().oneshot(req)
    });
    // A connection that errors (peer reset, malformed request) ends only that connection.
    let conn = http1::Builder::new().serve_connection(TokioIo::new(io), service);
    tokio::pin!(conn);
    tokio::select! {
        _ = conn.as_mut() => {}
        _ = async { let _ = draining.wait_for(|d| *d).await; } => {
            // Finish the request in flight, then close; an idle keep-alive connection closes at once.
            conn.as_mut().graceful_shutdown();
            let _ = conn.await;
        }
    }
}
