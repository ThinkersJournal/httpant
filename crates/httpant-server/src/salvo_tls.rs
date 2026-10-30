// SPDX-License-Identifier: MIT OR Apache-2.0
//! A salvo [`Acceptor`] that puts each accepted TCP connection through HTTPAnt's TLS handshake. salvo handshakes lazily inside the stream
//! (`HandshakeStream`), so a client that stalls in the handshake holds nothing but its own connection, and the handshake timeout ends that.

use std::io::Result as IoResult;

use salvo_core::conn::tcp::TcpCoupler;
use salvo_core::conn::{Accepted, Acceptor, HandshakeStream, Holding};
use salvo_core::fuse::ArcFusePolicy;
use salvo_core::http::uri::Scheme;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;

use crate::TlsAcceptor;

pub(crate) struct TlsSalvoAcceptor<A> {
    inner: A,
    tls: TlsAcceptor,
    holdings: Vec<Holding>,
}

impl<A: Acceptor> TlsSalvoAcceptor<A> {
    pub(crate) fn new(inner: A, tls: TlsAcceptor) -> Self {
        let holdings = inner
            .holdings()
            .iter()
            .map(|h| Holding {
                local_addr: h.local_addr.clone(),
                http_versions: h.http_versions.clone(),
                http_scheme: Scheme::HTTPS,
            })
            .collect();
        Self {
            inner,
            tls,
            holdings,
        }
    }
}

impl<A> Acceptor for TlsSalvoAcceptor<A>
where
    A: Acceptor + Send + 'static,
    A::Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Coupler = TcpCoupler<Self::Stream>;
    type Stream = HandshakeStream<TlsStream<A::Stream>>;

    fn holdings(&self) -> &[Holding] {
        &self.holdings
    }

    async fn accept(
        &mut self,
        fuse_policy: Option<ArcFusePolicy>,
    ) -> IoResult<Accepted<Self::Coupler, Self::Stream>> {
        let Accepted {
            stream,
            fuse_config,
            conn_ctrl,
            local_addr,
            remote_addr,
            ..
        } = self.inner.accept(fuse_policy).await?;
        let tls = self.tls.clone();
        Ok(Accepted {
            coupler: TcpCoupler::new(),
            stream: HandshakeStream::new(async move { tls.accept(stream).await }, fuse_config),
            fuse_config,
            conn_ctrl,
            local_addr,
            remote_addr,
            http_scheme: Scheme::HTTPS,
        })
    }
}
