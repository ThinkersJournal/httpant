// SPDX-License-Identifier: MIT OR Apache-2.0
//! A poem [`Acceptor`] that hands over connections whose TLS handshake has finished. Handshakes run in their own tasks, so a client that
//! stalls in one holds nothing but its own slot (and the handshake timeout ends that).

use std::io::Result as IoResult;

use poem::http::uri::Scheme;
use poem::listener::{Acceptor, TcpAcceptor};
use poem::web::{LocalAddr, RemoteAddr};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::server::TlsStream;

use crate::TlsAcceptor;

type Ready = (TlsStream<TcpStream>, LocalAddr, RemoteAddr);

pub(crate) struct TlsPoemAcceptor {
    local: Vec<LocalAddr>,
    ready: mpsc::Receiver<Ready>,
    accepting: JoinHandle<()>,
}

impl TlsPoemAcceptor {
    pub(crate) fn new(mut tcp: TcpAcceptor, tls: TlsAcceptor) -> Self {
        let local = tcp.local_addr();
        let (tx, ready) = mpsc::channel(128);
        let accepting = tokio::spawn(async move {
            loop {
                let Ok((stream, local, remote, _scheme)) = tcp.accept().await else {
                    // Out of descriptors or a reset on accept: back off briefly rather than spin.
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                };
                let (tls, tx) = (tls.clone(), tx.clone());
                tokio::spawn(async move {
                    if let Ok(stream) = tls.accept(stream).await {
                        let _ = tx.send((stream, local, remote)).await;
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

impl Drop for TlsPoemAcceptor {
    fn drop(&mut self) {
        // Stops accepting (the socket closes with the task).
        self.accepting.abort();
    }
}

impl Acceptor for TlsPoemAcceptor {
    type Io = TlsStream<TcpStream>;

    fn local_addr(&self) -> Vec<LocalAddr> {
        self.local.clone()
    }

    async fn accept(&mut self) -> IoResult<(Self::Io, LocalAddr, RemoteAddr, Scheme)> {
        match self.ready.recv().await {
            Some((io, local, remote)) => Ok((io, local, remote, Scheme::HTTPS)),
            // The accept task only ends by being aborted in `drop`, which cannot happen while `self` is alive.
            None => std::future::pending().await,
        }
    }
}
