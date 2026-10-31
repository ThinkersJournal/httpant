// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark harness: each HTTPAnt adapter against the bare web library it wraps.
//!
//! One question: what does going through HTTPAnt cost on top of the library itself? So both sides serve the same route (`GET /`, a fixed
//! body), on the same machine, under the same load (keep-alive connections, one request in flight on each), and runs alternate between the two
//! so a slow minute on the machine lands on both. The load generator checks every response (status 200 and the exact body); a run with a
//! wrong answer is reported as an error count, never as speed. It is not a framework league table: the route does nothing, so a framework's
//! routing, extractors and middleware are not measured. See `docs/BENCHMARKS.md` for what the numbers can and cannot support.
pub mod bare;

use std::net::TcpListener;
use std::time::{Duration, Instant};

use httpant_server::Server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The response body both sides send.
pub const BODY: &str = "Hello, World!";

/// Every adapter the harness can run, in the order they are reported.
pub const ADAPTERS: &[&str] = &["hyper", "axum", "actix-web", "poem", "salvo", "warp"];

const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: bench\r\n\r\n";

/// Which server answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Bare,
    Httpant,
}

/// One measured run of one server.
#[derive(Clone, Debug)]
pub struct Run {
    pub requests: u64,
    pub errors: u64,
    pub seconds: f64,
    /// Latency percentiles in microseconds.
    pub p50_us: u64,
    pub p99_us: u64,
}

impl Run {
    pub fn rps(&self) -> f64 {
        self.requests as f64 / self.seconds
    }
}

/// Read one response off the connection: true if it was `200` with exactly [`BODY`].
async fn one_response(stream: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<bool> {
    buf.clear();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            if buf.len() >= end + 4 + len {
                let ok = head.starts_with("http/1.1 200")
                    && &buf[end + 4..end + 4 + len] == BODY.as_bytes();
                return Ok(ok);
            }
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// `connections` keep-alive connections, each sending its next request when the last answer is in, for `duration` (after `warmup`).
pub async fn load(
    addr: std::net::SocketAddr,
    connections: usize,
    warmup: Duration,
    duration: Duration,
) -> Run {
    let started = Instant::now();
    let warm_end = started + warmup;
    let end = warm_end + duration;
    let tasks: Vec<_> = (0..connections)
        .map(|_| {
            tokio::spawn(async move {
                let mut lat: Vec<u32> = Vec::new();
                let mut errors = 0u64;
                let Ok(mut stream) = TcpStream::connect(addr).await else {
                    return (lat, 1);
                };
                let _ = stream.set_nodelay(true);
                let mut buf = Vec::with_capacity(512);
                while Instant::now() < end {
                    let t = Instant::now();
                    if stream.write_all(REQUEST).await.is_err() {
                        errors += 1;
                        break;
                    }
                    match one_response(&mut stream, &mut buf).await {
                        Ok(true) => {
                            if t >= warm_end {
                                lat.push(t.elapsed().as_micros().min(u32::MAX as u128) as u32);
                            }
                        }
                        _ => {
                            if t >= warm_end {
                                errors += 1;
                            }
                            break;
                        }
                    }
                }
                (lat, errors)
            })
        })
        .collect();
    let mut all: Vec<u32> = Vec::new();
    let mut errors = 0;
    for t in tasks {
        let (lat, e) = t.await.expect("load task");
        all.extend(lat);
        errors += e;
    }
    all.sort_unstable();
    let pick = |q: f64| -> u64 {
        if all.is_empty() {
            0
        } else {
            u64::from(all[((all.len() - 1) as f64 * q) as usize])
        }
    };
    Run {
        requests: all.len() as u64,
        errors,
        seconds: duration.as_secs_f64(),
        p50_us: pick(0.50),
        p99_us: pick(0.99),
    }
}

/// Measure one server: bind, serve, load, stop.
///
/// The server is started, and stopped, on `srv`'s runtime and the load runs on the caller's, so the two do not share worker threads.
pub async fn measure(
    srv: &tokio::runtime::Handle,
    id: &'static str,
    side: Side,
    connections: usize,
    warmup: Duration,
    duration: Duration,
) -> Run {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let run = match side {
        Side::Bare => {
            let bare = srv
                .spawn(async move { bare::serve(id, listener) })
                .await
                .expect("start");
            let run = load(addr, connections, warmup, duration).await;
            bare.stop();
            run
        }
        Side::Httpant => {
            drop(listener);
            let server = srv
                .spawn(async move {
                    Server::with_adapter(id)
                        .expect("adapter compiled in")
                        .get("/", || async { BODY })
                        .bind(addr)
                        .await
                        .expect("bind")
                })
                .await
                .expect("start");
            let run = load(server.addr(), connections, warmup, duration).await;
            srv.spawn(server.shutdown())
                .await
                .expect("stop")
                .expect("shutdown");
            run
        }
    };
    // Let the listener's port and the stopped server's tasks go before the next run.
    tokio::time::sleep(Duration::from_millis(100)).await;
    run
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    v[v.len() / 2]
}

/// One adapter's result over all rounds.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: &'static str,
    pub bare_rps: Vec<f64>,
    pub httpant_rps: Vec<f64>,
    pub bare_p99_us: Vec<u64>,
    pub httpant_p99_us: Vec<u64>,
    pub errors: u64,
}

impl Row {
    pub fn bare_median(&self) -> f64 {
        median(self.bare_rps.clone())
    }
    pub fn httpant_median(&self) -> f64 {
        median(self.httpant_rps.clone())
    }
    /// HTTPAnt's median requests per second as a share of the bare library's.
    pub fn ratio(&self) -> f64 {
        self.httpant_median() / self.bare_median()
    }
}

/// Run every named adapter `rounds` times on each side, alternating which side goes first from round to round.
pub async fn compare(
    srv: &tokio::runtime::Handle,
    ids: &[&'static str],
    rounds: usize,
    connections: usize,
    warmup: Duration,
    duration: Duration,
) -> Vec<Row> {
    let mut rows: Vec<Row> = ids
        .iter()
        .map(|id| Row {
            id,
            bare_rps: Vec::new(),
            httpant_rps: Vec::new(),
            bare_p99_us: Vec::new(),
            httpant_p99_us: Vec::new(),
            errors: 0,
        })
        .collect();
    for round in 0..rounds {
        for row in &mut rows {
            let order = if round % 2 == 0 {
                [Side::Bare, Side::Httpant]
            } else {
                [Side::Httpant, Side::Bare]
            };
            for side in order {
                let run = measure(srv, row.id, side, connections, warmup, duration).await;
                row.errors += run.errors;
                match side {
                    Side::Bare => {
                        row.bare_rps.push(run.rps());
                        row.bare_p99_us.push(run.p99_us);
                    }
                    Side::Httpant => {
                        row.httpant_rps.push(run.rps());
                        row.httpant_p99_us.push(run.p99_us);
                    }
                }
            }
        }
    }
    rows
}

/// The table as Markdown.
pub fn table(rows: &[Row]) -> String {
    let mut out = String::from(
        "| Adapter | bare req/s (median) | HTTPAnt req/s (median) | HTTPAnt / bare | bare p99 us | HTTPAnt p99 us | wrong answers |\n|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let p99 = |v: &[u64]| {
            let mut v = v.to_vec();
            v.sort_unstable();
            v[v.len() / 2]
        };
        out.push_str(&format!(
            "| {} | {:.0} | {:.0} | {:.2} | {} | {} | {} |\n",
            r.id,
            r.bare_median(),
            r.httpant_median(),
            r.ratio(),
            p99(&r.bare_p99_us),
            p99(&r.httpant_p99_us),
            r.errors
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every adapter, both sides, a short run: the harness cannot rot, and every response was the right one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn every_adapter_answers_right_on_both_sides() {
        let rows = compare(
            &tokio::runtime::Handle::current(),
            ADAPTERS,
            1,
            4,
            Duration::from_millis(50),
            Duration::from_millis(300),
        )
        .await;
        for r in &rows {
            assert_eq!(r.errors, 0, "{}: wrong or missing answers", r.id);
            assert!(r.bare_median() > 0.0, "{}: bare served nothing", r.id);
            assert!(r.httpant_median() > 0.0, "{}: HTTPAnt served nothing", r.id);
        }
    }

    /// Positive control for the checker: a server that answers with the wrong body is counted as errors, not as speed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wrong_body_is_an_error_not_a_request() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut b = [0u8; 256];
                    while s.read(&mut b).await.unwrap_or(0) > 0 {
                        let _ = s
                            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nWRONG")
                            .await;
                    }
                });
            }
        });
        let run = load(addr, 2, Duration::ZERO, Duration::from_millis(200)).await;
        assert_eq!(run.requests, 0);
        assert!(run.errors > 0);
    }
}
