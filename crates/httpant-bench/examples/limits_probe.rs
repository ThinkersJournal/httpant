// SPDX-License-Identifier: MIT OR Apache-2.0
//! What does each adapter do about a stalled request head, an idle keep-alive connection and many connections?
//!
//! `cargo run -p httpant-bench --example limits_probe -- [--window SECONDS] [--header-timeout SECONDS] [--connections N]`
//!
//! Defaults: a 40 s window, no `Limits::header_read_timeout` set by HTTPAnt's caller (the default of 30 s applies where HTTPAnt enforces it, so
//! pass `--header-timeout 0` to see what each library does on its own), 3000 connections. The numbers in `docs/LIMITS-BY-ADAPTER.md` came
//! from this, once with `--header-timeout 0` and once with `--window 12 --header-timeout 3`.
use std::time::{Duration, Instant};

use httpant_server::{Limits, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Send `send`, then see how long the server leaves the connection open (up to `limit`).
async fn closes_after(addr: std::net::SocketAddr, send: &[u8], limit: Duration) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(send).await.unwrap();
    let start = Instant::now();
    let mut buf = [0u8; 4096];
    loop {
        let left = limit.saturating_sub(start.elapsed());
        if left.is_zero() {
            return format!("open after {}s", limit.as_secs());
        }
        match tokio::time::timeout(left, s.read(&mut buf)).await {
            Err(_) => return format!("open after {}s", limit.as_secs()),
            Ok(Ok(0)) | Ok(Err(_)) => {
                return format!("closed at {:.1}s", start.elapsed().as_secs_f32());
            }
            Ok(Ok(_)) => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let window = Duration::from_secs(arg("--window").map_or(40, |v| v.parse().expect("--window")));
    let header: u64 = arg("--header-timeout").map_or(0, |v| v.parse().expect("--header-timeout"));
    let connections: usize =
        arg("--connections").map_or(3000, |v| v.parse().expect("--connections"));
    let ids = ["hyper", "axum", "actix-web", "poem", "salvo", "warp"];
    let mut tasks = Vec::new();
    for id in ids {
        tasks.push(tokio::spawn(async move {
            let srv = Server::with_adapter(id)
                .unwrap()
                .limits(Limits::default().header_read_timeout(Duration::from_secs(header)))
                .get("/", || async { "hi" })
                .bind("127.0.0.1:0")
                .await
                .unwrap();
            let addr = srv.addr();
            let stalled = closes_after(addr, b"GET / HTTP/1.1\r\nHost: x\r\nX-A: ", window).await;
            let idle = closes_after(addr, b"GET / HTTP/1.1\r\nHost: x\r\n\r\n", window).await;
            let mut held = Vec::new();
            let mut failed = 0;
            for _ in 0..connections {
                match TcpStream::connect(addr).await {
                    Ok(c) => held.push(c),
                    Err(_) => failed += 1,
                }
            }
            let mut probe = TcpStream::connect(addr).await.unwrap();
            probe
                .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut out = Vec::new();
            let served = tokio::time::timeout(Duration::from_secs(5), probe.read_to_end(&mut out))
                .await
                .is_ok()
                && out.starts_with(b"HTTP/1.1 200");
            drop(held);
            srv.shutdown().await.ok();
            format!(
                "{id:10} | stalled head: {stalled:16} | idle keep-alive after one request: {idle:16} | {connections} idle connections, then a request: {} (connect failures {failed})",
                if served { "served" } else { "NOT served" }
            )
        }));
    }
    for t in tasks {
        println!("{}", t.await.unwrap());
    }
}
