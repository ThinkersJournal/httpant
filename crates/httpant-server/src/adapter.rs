// SPDX-License-Identifier: MIT OR Apache-2.0
use std::net::SocketAddr;
use std::time::Duration;

use httpant_core::{BoxFuture, BoxService, Capabilities, Error, Result};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::{DEFAULT_SHUTDOWN_GRACE, Grace, Limits};

/// What an adapter is given to serve on.
#[derive(Debug)]
#[non_exhaustive]
pub struct ServeOptions {
    /// An already-bound listening socket, so a supervisor (systemd, WebHostAnt) can hand one over. Binding to port 0 and reading the real
    /// address back from [`RunningServer::addr`] is part of the contract.
    pub listener: std::net::TcpListener,
    /// What the server accepts before a handler runs.
    pub limits: Limits,
    grace: Grace,
}

impl ServeOptions {
    /// Serve on this socket.
    pub fn new(listener: std::net::TcpListener) -> Self {
        Self {
            listener,
            limits: Limits::default(),
            grace: Grace::new(Some(DEFAULT_SHUTDOWN_GRACE)),
        }
    }

    /// Serve with these limits instead of the defaults.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// How long [`RunningServer::shutdown`] waits for requests in flight before ending them (default 30 s).
    pub fn shutdown_grace(mut self, period: Duration) -> Self {
        self.grace = Grace::new(Some(period));
        self
    }

    /// Wait for requests in flight for ever: one request that never finishes then holds `shutdown()` for ever. Say so in the code that asks.
    pub fn no_shutdown_grace(mut self) -> Self {
        self.grace = Grace::new(None);
        self
    }

    /// The grace period and its switch; an adapter hands it to [`RunningServer::new`].
    pub fn grace(&self) -> Grace {
        self.grace.clone()
    }

    /// The service an adapter must serve: `svc` behind this server's limits and its shutdown switch. Every adapter calls this on the service
    /// it is given (conformance cases 14 and 15).
    pub fn wrap(&self, svc: BoxService) -> BoxService {
        self.grace.wrap(self.limits.wrap(svc))
    }
}

/// One web library behind HTTPAnt. An adapter's only job is to run the one boxed service and feed it requests: it never sees routes.
pub trait ServerAdapter: Send + Sync + 'static {
    /// The id the adapter is chosen by: `"hyper"`, `"axum"`, ...
    fn id(&self) -> &'static str;

    /// What the adapter can and cannot do, as established by the conformance run.
    fn capabilities(&self) -> Capabilities;

    /// Start serving `svc` on `opts.listener`. Returns once the socket is accepting.
    fn serve(&self, svc: BoxService, opts: ServeOptions) -> BoxFuture<'_, Result<RunningServer>>;
}

/// How long after a forced end of the requests in flight the web library gets to stop by itself before its task is aborted.
pub const FORCE_WAIT: Duration = Duration::from_secs(5);

/// A server that is accepting connections.
#[derive(Debug)]
pub struct RunningServer {
    addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    grace: Grace,
}

impl RunningServer {
    /// Built by adapters: the bound address, a signal that makes the accept loop stop, the accept loop's task, and the [`Grace`] from
    /// [`ServeOptions::grace`].
    pub fn new(
        addr: SocketAddr,
        stop: oneshot::Sender<()>,
        task: JoinHandle<()>,
        grace: Grace,
    ) -> Self {
        Self {
            addr,
            stop: Some(stop),
            task,
            grace,
        }
    }

    /// The address the server is really listening on (the port is real even when 0 was asked for).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting new connections, let every request in flight finish, close idle keep-alive connections, and return once they have
    /// (conformance case 9).
    ///
    /// Requests still running when the grace period ends (see [`ServeOptions::shutdown_grace`], 30 s by default) are ended: the handler is
    /// dropped, the client gets `503` with `Connection: close` (a response already streaming is cut), and `shutdown` returns (conformance
    /// case 15). If the web library still has not stopped [`FORCE_WAIT`] after that, its task is aborted and `shutdown` returns anyway.
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(period) = self.grace.period() else {
            return self.join().await;
        };
        if let Ok(done) = tokio::time::timeout(period, &mut self.task).await {
            return done.map_err(|e| Error::Io(std::io::Error::other(e)));
        }
        self.grace.force();
        match tokio::time::timeout(FORCE_WAIT, &mut self.task).await {
            Ok(done) => done.map_err(|e| Error::Io(std::io::Error::other(e))),
            Err(_) => {
                self.task.abort();
                Ok(())
            }
        }
    }

    /// Wait until the server stops by itself (it does not, unless it fails).
    pub async fn join(self) -> Result<()> {
        self.task
            .await
            .map_err(|e| Error::Io(std::io::Error::other(e)))
    }
}
