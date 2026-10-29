use std::net::SocketAddr;

use httpant_core::{BoxFuture, BoxService, Capabilities, Error, Result};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// What an adapter is given to serve on.
#[derive(Debug)]
#[non_exhaustive]
pub struct ServeOptions {
    /// An already-bound listening socket, so a supervisor (systemd, WebHostAnt) can hand one over. Binding to port 0 and reading the real
    /// address back from [`RunningServer::addr`] is part of the contract.
    pub listener: std::net::TcpListener,
}

impl ServeOptions {
    /// Serve on this socket.
    pub fn new(listener: std::net::TcpListener) -> Self {
        Self { listener }
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

/// A server that is accepting connections.
#[derive(Debug)]
pub struct RunningServer {
    addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl RunningServer {
    /// Built by adapters: the bound address, a signal that makes the accept loop stop, and the accept loop's task.
    pub fn new(addr: SocketAddr, stop: oneshot::Sender<()>, task: JoinHandle<()>) -> Self {
        Self {
            addr,
            stop: Some(stop),
            task,
        }
    }

    /// The address the server is really listening on (the port is real even when 0 was asked for).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting new connections and wait for the accept loop to end.
    ///
    /// This is **not** a graceful shutdown: whether an in-flight request completes is conformance case 9 (not yet run) and shows in
    /// [`Capabilities::graceful_shutdown`] as `Unknown`.
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.join().await
    }

    /// Wait until the server stops by itself (it does not, unless it fails).
    pub async fn join(self) -> Result<()> {
        self.task
            .await
            .map_err(|e| Error::Io(std::io::Error::other(e)))
    }
}
