// SPDX-License-Identifier: MIT OR Apache-2.0
//! The drain of an unread request body (`drain.rs`) is an amplification surface: the client chooses the body and how fast it arrives. These
//! tests hold its two bounds over real sockets, on every adapter compiled in: a client that trickles its body cannot keep a refused request
//! open longer than `drain_timeout`, and a client that floods cannot make the server read more than `max_drain_bytes` (the server stops
//! reading, so the flood backs up in the socket buffers and the client's writes fail long before it has offered everything).
//! Raw HTTP/1.1, so nothing of HTTPAnt's is on the client side.

use std::time::{Duration, Instant};

use httpant_core::http::StatusCode;
use httpant_server::{Limits, RunningServer, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "actix-web")]
    "actix-web",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
    #[cfg(feature = "warp")]
    "warp",
];

async fn serve(id: &str, limits: Limits) -> RunningServer {
    Server::with_adapter(id)
        .unwrap()
        .limits(limits)
        .get("/refuse", || async { StatusCode::FORBIDDEN })
        .bind("127.0.0.1:0")
        .await
        .unwrap()
}

/// What the client saw first from the server, and how long after it connected.
struct First {
    after: Duration,
    reply: String,
}

/// Read until the server says something (a head), closes, or resets: the moment the refused request stopped being held.
async fn first_word(read: &mut tokio::net::tcp::OwnedReadHalf, start: Instant) -> First {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let got = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match read.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
    })
    .await;
    assert!(
        got.is_ok(),
        "the server neither answered nor closed in 10 s"
    );
    First {
        after: start.elapsed(),
        reply: String::from_utf8_lossy(&buf).into_owned(),
    }
}

/// A body that arrives a byte at a time, for much longer than the drain timeout, is cut off at the timeout: the refusal is sent (with
/// `Connection: close`) or the connection ends, within the timeout and a little more, never when the client is done.
#[tokio::test]
async fn a_trickled_body_cannot_hold_a_refusal_longer_than_the_drain_timeout() {
    let timeout = Duration::from_millis(500);
    for id in ADAPTERS {
        let srv = serve(id, Limits::default().drain_timeout(timeout)).await;
        let tcp = TcpStream::connect(srv.addr()).await.unwrap();
        let (mut read, mut write) = tcp.into_split();
        let start = Instant::now();
        write
            .write_all(b"GET /refuse HTTP/1.1\r\nHost: t\r\nContent-Length: 1000000\r\n\r\n")
            .await
            .unwrap();
        // 60 bytes, one every 100 ms: six seconds of body, twelve times the timeout.
        let trickle = tokio::spawn(async move {
            for _ in 0..60 {
                if write.write_all(b"x").await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            write
        });
        let first = first_word(&mut read, start).await;
        trickle.abort();
        assert!(
            first.after >= timeout - Duration::from_millis(100),
            "{id}: answered after {:?}, before the body was given its {timeout:?}: {:?}",
            first.after,
            first.reply
        );
        assert!(
            first.after < timeout + Duration::from_millis(2500),
            "{id}: held the refused request for {:?} with a drain timeout of {timeout:?}",
            first.after
        );
        if !first.reply.is_empty() {
            assert!(
                first.reply.starts_with("HTTP/1.1 403"),
                "{id}: {:?}",
                first.reply
            );
            assert!(
                first
                    .reply
                    .to_ascii_lowercase()
                    .contains("connection: close"),
                "{id}: an unfinished body must close the connection: {:?}",
                first.reply
            );
        }
        srv.shutdown().await.unwrap();
    }
}

/// A flood is read only up to the cap: the server stops reading and closes, so the client's writes fail after what the socket buffers hold,
/// far short of what it offered. A server that read it all would take every byte. Two floods: a chunked body (no declared length) that a
/// handler refuses, and a declared 256 MiB that the body limit refuses with `413` on its head. On actix-web both used to be read whole: it
/// lingers 5 s after answering and reads without limit, unless told not to (`actix_adapter.rs`, httpant#72).
#[tokio::test]
async fn a_flooded_body_is_not_read_past_the_cap() {
    const OFFERED: usize = 256 * 1024 * 1024;
    const CHUNK: usize = 64 * 1024;
    let cap = 256 * 1024u64;
    for id in ADAPTERS {
        for chunked in [true, false] {
            let srv = serve(id, Limits::default().max_drain_bytes(cap)).await;
            let tcp = TcpStream::connect(srv.addr()).await.unwrap();
            let (mut read, mut write) = tcp.into_split();
            let start = Instant::now();
            let head = if chunked {
                "GET /refuse HTTP/1.1
Host: t
Transfer-Encoding: chunked

"
                .to_owned()
            } else {
                format!(
                    "GET /refuse HTTP/1.1
Host: t
Content-Length: {OFFERED}

"
                )
            };
            write.write_all(head.as_bytes()).await.unwrap();
            let flood = tokio::spawn(async move {
                let mut piece = Vec::new();
                if chunked {
                    piece.extend_from_slice(
                        format!(
                            "{CHUNK:x}
"
                        )
                        .as_bytes(),
                    );
                }
                piece.extend(std::iter::repeat_n(b'x', CHUNK));
                if chunked {
                    piece.extend_from_slice(
                        b"
",
                    );
                }
                let mut offered = 0usize;
                while offered < OFFERED {
                    if write.write_all(&piece).await.is_err() {
                        break;
                    }
                    offered += CHUNK;
                }
                offered
            });
            let first = first_word(&mut read, start).await;
            let offered = tokio::time::timeout(Duration::from_secs(20), flood)
                .await
                .unwrap_or_else(|_| panic!("{id}: the client was still writing after 20 s"))
                .unwrap();
            assert!(
                offered < OFFERED,
                "{id} (chunked: {chunked}): the server took all {OFFERED} bytes of a flood with a drain cap of {cap}"
            );
            assert!(
                first.after < Duration::from_secs(5),
                "{id} (chunked: {chunked}): {:?}",
                first.after
            );
            if !first.reply.is_empty() {
                let want = if chunked {
                    "HTTP/1.1 403"
                } else {
                    "HTTP/1.1 413"
                };
                assert!(first.reply.starts_with(want), "{id}: {:?}", first.reply);
            }
            srv.shutdown().await.unwrap();
        }
    }
}
