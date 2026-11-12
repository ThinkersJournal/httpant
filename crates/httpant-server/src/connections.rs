// SPDX-License-Identifier: MIT OR Apache-2.0
//! The connection limit: [`Limits::max_connections`](crate::Limits::max_connections).
//!
//! The limit is a count of slots that an adapter's accept loop passes through: it takes a slot **before** it accepts, so at the limit it does
//! not accept at all and a new client waits in the operating system's queue (nothing is refused or dropped), and the slot goes back when the
//! connection ends, because the [`ConnectionPermit`] travels with the connection. Every adapter whose accept loop HTTPAnt can reach uses the
//! same gate, so the limit means the same thing on each; warp's accept loop is private to warp (`Capabilities::connection_limit` says so).
use std::sync::Arc;

#[cfg(unix)]
use httpant_core::Error;
use httpant_core::Result;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[cfg(unix)]
/// Open files kept back for what is not a client connection: the listener, log files, the resolver, TLS material and the like.
const FILE_RESERVE: usize = 256;

/// The slots a server's connections take: a count that an adapter's accept loop passes through before it accepts.
#[derive(Clone, Debug)]
pub struct ConnectionGate {
    slots: Option<Arc<Semaphore>>,
}

impl ConnectionGate {
    /// A gate with `max` slots; `0` is no limit. On Unix the process's soft open-file limit is raised, if it is too low, to what the limit needs
    /// (never past the hard limit); a limit the hard limit cannot hold fails here, at start, with
    /// [`httpant_core::Error::ConnectionLimitTooHigh`]: the file limit is checked once, not argued with under load.
    pub fn new(max: usize) -> Result<Self> {
        if max == 0 {
            return Ok(Self::off());
        }
        check_open_files(max)?;
        Ok(Self {
            slots: Some(Arc::new(Semaphore::new(max))),
        })
    }

    /// A gate that never waits.
    pub fn off() -> Self {
        Self { slots: None }
    }

    /// Wait for a slot. The permit is held by the connection and gives the slot back when it is dropped.
    pub async fn admit(&self) -> ConnectionPermit {
        match &self.slots {
            Some(slots) => ConnectionPermit(Some(
                Arc::clone(slots)
                    .acquire_owned()
                    .await
                    .expect("the semaphore is never closed"),
            )),
            None => ConnectionPermit(None),
        }
    }

    /// Slots free now (`None`: no limit).
    pub fn available(&self) -> Option<usize> {
        self.slots.as_ref().map(|s| s.available_permits())
    }
}

/// A slot of the [`ConnectionGate`], held for as long as its connection is open.
#[derive(Debug)]
pub struct ConnectionPermit(#[allow(dead_code)] Option<OwnedSemaphorePermit>);

/// A connection's I/O with its slot in the [`ConnectionGate`] riding along: the slot goes back when the I/O is dropped, whichever library
/// owns it by then.
#[cfg(any(feature = "axum", feature = "poem", feature = "salvo", feature = "tls"))]
pub(crate) struct Held<T> {
    inner: T,
    _permit: ConnectionPermit,
}

#[cfg(any(feature = "axum", feature = "poem", feature = "salvo", feature = "tls"))]
impl<T> Held<T> {
    pub(crate) fn new(inner: T, permit: ConnectionPermit) -> Self {
        Self {
            inner,
            _permit: permit,
        }
    }
}

#[cfg(any(feature = "axum", feature = "poem", feature = "salvo", feature = "tls"))]
mod held_io {
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use super::Held;

    impl<T: AsyncRead + Unpin> AsyncRead for Held<T> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl<T: AsyncWrite + Unpin> AsyncWrite for Held<T> {
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
}

/// axum: a listener that takes a slot before it accepts and hands the slot to the connection.
#[cfg(feature = "axum")]
pub(crate) struct GatedListener<L> {
    pub(crate) inner: L,
    pub(crate) gate: ConnectionGate,
}

#[cfg(feature = "axum")]
impl<L: axum::serve::Listener> axum::serve::Listener for GatedListener<L> {
    type Io = Held<L::Io>;
    type Addr = L::Addr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let permit = self.gate.admit().await;
        let (io, addr) = self.inner.accept().await;
        (Held::new(io, permit), addr)
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// poem: an acceptor that takes a slot before it accepts and hands the slot to the connection.
#[cfg(feature = "poem")]
pub(crate) struct GatedPoemAcceptor<A> {
    pub(crate) inner: A,
    pub(crate) gate: ConnectionGate,
}

#[cfg(feature = "poem")]
impl<A: poem::listener::Acceptor> poem::listener::Acceptor for GatedPoemAcceptor<A> {
    type Io = Held<A::Io>;

    fn local_addr(&self) -> Vec<poem::web::LocalAddr> {
        self.inner.local_addr()
    }

    async fn accept(
        &mut self,
    ) -> std::io::Result<(
        Self::Io,
        poem::web::LocalAddr,
        poem::web::RemoteAddr,
        poem::http::uri::Scheme,
    )> {
        let permit = self.gate.admit().await;
        let (io, local, remote, scheme) = self.inner.accept().await?;
        Ok((Held::new(io, permit), local, remote, scheme))
    }
}

/// salvo: an acceptor that takes a slot before it accepts and hands the slot to the connection.
#[cfg(feature = "salvo")]
pub(crate) struct GatedSalvoAcceptor<A> {
    pub(crate) inner: A,
    pub(crate) gate: ConnectionGate,
}

#[cfg(feature = "salvo")]
impl<A> salvo_core::conn::Acceptor for GatedSalvoAcceptor<A>
where
    A: salvo_core::conn::Acceptor + Send + 'static,
    A::Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    type Coupler = salvo_core::conn::tcp::TcpCoupler<Self::Stream>;
    type Stream = Held<A::Stream>;

    fn holdings(&self) -> &[salvo_core::conn::Holding] {
        self.inner.holdings()
    }

    async fn accept(
        &mut self,
        fuse_policy: Option<salvo_core::fuse::ArcFusePolicy>,
    ) -> std::io::Result<salvo_core::conn::Accepted<Self::Coupler, Self::Stream>> {
        let permit = self.gate.admit().await;
        let salvo_core::conn::Accepted {
            stream,
            fuse_config,
            conn_ctrl,
            local_addr,
            remote_addr,
            http_scheme,
            ..
        } = self.inner.accept(fuse_policy).await?;
        Ok(salvo_core::conn::Accepted {
            coupler: salvo_core::conn::tcp::TcpCoupler::new(),
            stream: Held::new(stream, permit),
            fuse_config,
            conn_ctrl,
            local_addr,
            remote_addr,
            http_scheme,
        })
    }
}

#[cfg(unix)]
fn check_open_files(max: usize) -> Result<()> {
    let needed = max.saturating_add(FILE_RESERVE);
    let Ok((soft, hard)) = rlimit::Resource::NOFILE.get() else {
        // Not being able to read the limit is not a reason to refuse to start.
        return Ok(());
    };
    let as_usize = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
    if as_usize(soft) >= needed {
        return Ok(());
    }
    // The soft limit is often 1024 by default while the hard limit is far higher; a server that wants more connections than that raises the
    // soft limit to what it needs, as other servers do (up to the hard limit, never past it). Only a hard limit that is too low fails.
    if as_usize(hard) >= needed && rlimit::Resource::NOFILE.set(needed as u64, hard).is_ok() {
        return Ok(());
    }
    Err(Error::ConnectionLimitTooHigh {
        wanted: max,
        needed,
        available: as_usize(soft.max(rlimit::Resource::NOFILE.get().map_or(soft, |(s, _)| s))),
    })
}

/// Windows has no per-process limit on open files that a socket count meets: nothing to check.
#[cfg(not(unix))]
fn check_open_files(_max: usize) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[cfg(unix)]
    use httpant_core::Error;

    use super::*;

    #[tokio::test]
    async fn slots_are_taken_and_given_back() {
        let gate = ConnectionGate::new(2).expect("gate");
        let a = gate.admit().await;
        let _b = gate.admit().await;
        assert_eq!(gate.available(), Some(0));
        // The third waits for a slot.
        let third = tokio::time::timeout(Duration::from_millis(100), gate.admit()).await;
        assert!(third.is_err(), "a third connection must wait");
        drop(a);
        let third = tokio::time::timeout(Duration::from_secs(2), gate.admit()).await;
        assert!(third.is_ok(), "a slot came back when a connection closed");
    }

    #[tokio::test]
    async fn zero_is_no_limit() {
        let gate = ConnectionGate::new(0).expect("gate");
        assert_eq!(gate.available(), None);
        let mut held = Vec::new();
        for _ in 0..1000 {
            held.push(gate.admit().await);
        }
    }

    /// A limit above the process's open-file limit is refused at start, with the numbers in the message.
    #[cfg(unix)]
    #[test]
    fn a_limit_the_process_cannot_hold_fails_at_start() {
        let err = ConnectionGate::new(usize::MAX / 2).expect_err("cannot be held");
        // (The numbers are the process's own: the soft limit that was read, and the reserve added to the wish.)
        assert!(
            matches!(err, Error::ConnectionLimitTooHigh { .. }),
            "{err:?}"
        );
        assert!(err.to_string().contains("ulimit -n"), "{err}");
    }
}
