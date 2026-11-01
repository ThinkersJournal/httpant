// SPDX-License-Identifier: MIT OR Apache-2.0
//! Refusing cleartext HTTP/2 by prior knowledge (h2c) below a web library that would otherwise accept it.
//!
//! Several libraries (axum, poem, salvo) hand every cleartext connection to a hyper-util `auto::Builder` that serves HTTP/2 when the connection
//! starts with the HTTP/2 preface, and none offers a setting to stop that (`docs/H2C-POLICY.md`, step 1). [`GuardedIo`] wraps the connection's
//! I/O instead: it passes every byte through unchanged and unbuffered, and when the first bytes of the connection spell `PRI * HTTP/2.0` (the
//! start of the preface; as an HTTP/1.1 request line it is not a request any server answers) it fails the read, so the connection ends before
//! the library has decided anything. Once one byte of the first bytes differs, the guard is finished for that connection and costs one
//! comparison per read of the first segment only.
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The start of the HTTP/2 connection preface that no HTTP/1.1 request line can be: method `PRI`, target `*`, version `HTTP/2.0`.
const PREFACE_START: &[u8] = b"PRI * HTTP/2.0";

/// `T` with the cleartext HTTP/2 preface refused, see the [module documentation](self).
pub(crate) struct GuardedIo<T> {
    inner: T,
    /// How many leading bytes of the connection have matched [`PREFACE_START`]; `None` once one differed.
    matched: Option<usize>,
}

impl<T> GuardedIo<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner,
            matched: Some(0),
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for GuardedIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        std::task::ready!(Pin::new(&mut self.inner).poll_read(cx, buf))?;
        if let Some(mut matched) = self.matched {
            for &byte in &buf.filled()[before..] {
                if byte != PREFACE_START[matched] {
                    self.matched = None;
                    return Poll::Ready(Ok(()));
                }
                matched += 1;
                if matched == PREFACE_START.len() {
                    // Keep what was read out of the library's hands, then end the connection.
                    buf.set_filled(before);
                    self.matched = None;
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "cleartext HTTP/2 is not enabled",
                    )));
                }
            }
            self.matched = Some(matched);
        }
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for GuardedIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// axum: a listener whose connections are guarded. axum's `ConnectInfo<SocketAddr>` is implemented for its own `TcpListener` only, so the peer
/// address travels as [`GuardedPeer`] (as it does as `TlsPeer` on the TLS listener).
#[cfg(feature = "axum")]
pub(crate) mod axum_listener {
    use std::io;
    use std::net::SocketAddr;

    use axum::serve::{IncomingStream, Listener};
    use tokio::net::{TcpListener, TcpStream};

    use super::GuardedIo;

    /// Connection info of a guarded cleartext connection: the peer address.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct GuardedPeer(pub SocketAddr);

    impl axum::extract::connect_info::Connected<IncomingStream<'_, GuardedListener>> for GuardedPeer {
        fn connect_info(stream: IncomingStream<'_, GuardedListener>) -> Self {
            Self(*stream.remote_addr())
        }
    }

    pub(crate) struct GuardedListener(pub TcpListener);

    impl Listener for GuardedListener {
        type Io = GuardedIo<TcpStream>;
        type Addr = SocketAddr;

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            let (io, addr) = Listener::accept(&mut self.0).await;
            (GuardedIo::new(io), addr)
        }

        fn local_addr(&self) -> io::Result<Self::Addr> {
            Listener::local_addr(&self.0)
        }
    }
}

/// poem: an acceptor whose connections are guarded.
#[cfg(feature = "poem")]
pub(crate) mod poem_acceptor {
    use std::io::Result as IoResult;

    use poem::http::uri::Scheme;
    use poem::listener::Acceptor;
    use poem::web::{LocalAddr, RemoteAddr};

    use super::GuardedIo;

    pub(crate) struct GuardedPoemAcceptor<A>(pub A);

    impl<A: Acceptor> Acceptor for GuardedPoemAcceptor<A> {
        type Io = GuardedIo<A::Io>;

        fn local_addr(&self) -> Vec<LocalAddr> {
            self.0.local_addr()
        }

        async fn accept(&mut self) -> IoResult<(Self::Io, LocalAddr, RemoteAddr, Scheme)> {
            let (io, local, remote, scheme) = self.0.accept().await?;
            Ok((GuardedIo::new(io), local, remote, scheme))
        }
    }
}

/// salvo: an acceptor whose connections are guarded.
#[cfg(feature = "salvo")]
pub(crate) mod salvo_acceptor {
    use std::io::Result as IoResult;

    use salvo_core::conn::tcp::TcpCoupler;
    use salvo_core::conn::{Accepted, Acceptor, Holding};
    use salvo_core::fuse::ArcFusePolicy;
    use tokio::io::{AsyncRead, AsyncWrite};

    use super::GuardedIo;

    pub(crate) struct GuardedSalvoAcceptor<A>(pub A);

    impl<A> Acceptor for GuardedSalvoAcceptor<A>
    where
        A: Acceptor + Send + 'static,
        A::Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        type Coupler = TcpCoupler<Self::Stream>;
        type Stream = GuardedIo<A::Stream>;

        fn holdings(&self) -> &[Holding] {
            self.0.holdings()
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
                http_scheme,
                ..
            } = self.0.accept(fuse_policy).await?;
            Ok(Accepted {
                coupler: TcpCoupler::new(),
                stream: GuardedIo::new(stream),
                fuse_config,
                conn_ctrl,
                local_addr,
                remote_addr,
                http_scheme,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    const FULL_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

    async fn read_all(client_sends: &[&[u8]]) -> (Vec<u8>, io::Result<usize>) {
        let (mut client, server) = duplex(1024);
        let mut guarded = GuardedIo::new(server);
        for part in client_sends {
            client.write_all(part).await.unwrap();
            client.flush().await.unwrap();
        }
        drop(client);
        let mut got = Vec::new();
        let mut chunk = [0u8; 64];
        loop {
            match guarded.read(&mut chunk).await {
                Ok(0) => return (got, Ok(0)),
                Ok(n) => got.extend_from_slice(&chunk[..n]),
                Err(e) => return (got, Err(e)),
            }
        }
    }

    #[tokio::test]
    async fn the_whole_preface_in_one_segment_is_refused_and_none_of_it_is_passed_on() {
        let (got, end) = read_all(&[FULL_PREFACE]).await;
        assert_eq!(end.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
        assert!(got.is_empty(), "{} bytes reached the library", got.len());
    }

    #[tokio::test]
    async fn a_preface_arriving_one_byte_at_a_time_is_refused_before_it_is_complete() {
        let (mut client, server) = duplex(1024);
        let mut guarded = GuardedIo::new(server);
        let mut got = Vec::new();
        let mut end = None;
        for byte in FULL_PREFACE {
            client.write_all(&[*byte]).await.unwrap();
            client.flush().await.unwrap();
            let mut one = [0u8; 8];
            match guarded.read(&mut one).await {
                Ok(n) => got.extend_from_slice(&one[..n]),
                Err(e) => {
                    end = Some(e);
                    break;
                }
            }
        }
        assert_eq!(
            end.expect("refused").kind(),
            io::ErrorKind::ConnectionAborted
        );
        // The library saw fewer bytes than it needs to recognise HTTP/2 (24), and not the byte that completed the match.
        assert_eq!(got.len(), PREFACE_START.len() - 1);
    }

    #[tokio::test]
    async fn an_http1_request_passes_through_unchanged() {
        let request: &[u8] = b"GET / HTTP/1.1\r\nHost: a\r\n\r\n";
        let (got, end) = read_all(&[request]).await;
        assert!(end.is_ok());
        assert_eq!(got, request);
    }

    #[tokio::test]
    async fn a_request_that_starts_like_the_preface_but_is_not_passes_through() {
        // `PUT`, `POST` and `PATCH` share the first byte with `PRI`; the second byte ends the doubt.
        for request in [
            &b"POST /a HTTP/1.1\r\nHost: a\r\nContent-Length: 0\r\n\r\n"[..],
            b"PUT /a HTTP/1.1\r\nHost: a\r\n\r\n",
            // Right method and target, wrong version: not the preface either.
            b"PRI * HTTP/1.1\r\nHost: a\r\n\r\n",
        ] {
            let (got, end) = read_all(&[request]).await;
            assert!(end.is_ok(), "{:?}", String::from_utf8_lossy(request));
            assert_eq!(got, request);
        }
    }

    #[tokio::test]
    async fn a_connection_that_ends_inside_the_preface_start_is_not_an_error() {
        let (got, end) = read_all(&[b"PRI * HT"]).await;
        assert!(end.is_ok());
        assert_eq!(got, b"PRI * HT");
    }

    #[tokio::test]
    async fn bytes_written_back_are_passed_through() {
        let (mut client, server) = duplex(64);
        let mut guarded = GuardedIo::new(server);
        guarded.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
        guarded.flush().await.unwrap();
        let mut got = [0u8; 19];
        client.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"HTTP/1.1 200 OK\r\n\r\n");
    }
}
