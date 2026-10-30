// SPDX-License-Identifier: MIT OR Apache-2.0
//! An axum [`Listener`](axum::serve::Listener) that hands over connections whose TLS handshake has finished. Handshakes run in their own tasks,
//! so a client that stalls in one holds nothing but its own slot (and the handshake timeout ends that).

use std::io;
use std::net::SocketAddr;

use axum::serve::{IncomingStream, Listener};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::server::TlsStream;

use crate::TlsAcceptor;

/// Connection info of a TLS connection: the peer address.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TlsPeer(pub SocketAddr);

impl axum::extract::connect_info::Connected<IncomingStream<'_, TlsListener>> for TlsPeer {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

pub(crate) struct TlsListener {
    local: SocketAddr,
    ready: mpsc::Receiver<(TlsStream<tokio::net::TcpStream>, SocketAddr)>,
    accepting: JoinHandle<()>,
}

impl TlsListener {
    pub(crate) fn new(tcp: TcpListener, acceptor: TlsAcceptor) -> Self {
        let local = tcp
            .local_addr()
            .unwrap_or_else(|_| ([0, 0, 0, 0], 0).into());
        let (tx, ready) = mpsc::channel(128);
        let accepting = tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = tcp.accept().await else {
                    // Out of descriptors or a reset on accept: back off briefly rather than spin.
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                };
                let (acceptor, tx) = (acceptor.clone(), tx.clone());
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(stream).await {
                        let _ = tx.send((tls, peer)).await;
                    }
                });
            }
        });
        Self {
            local,
            ready,
            accepting,
        }
    }
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        // Stops accepting (the socket closes with the task).
        self.accepting.abort();
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(conn) => conn,
            // The accept task only ends by being aborted in `drop`, which cannot happen while `self` is alive.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}
