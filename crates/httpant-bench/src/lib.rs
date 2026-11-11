// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark harness: each HTTPAnt adapter against the bare web library it wraps.
//!
//! One question: what does going through HTTPAnt cost on top of the library itself? So both sides serve the same route, on the same machine,
//! under the same load (keep-alive connections, one request in flight on each), and runs alternate between the two so a slow minute on the
//! machine lands on both. The load generator checks every response (status 200 and the exact body); a run with a wrong answer is reported as
//! an error count, never as speed. It is not a framework league table. Two routes: the constant route does nothing, so only the conversion
//! layer is measured; the `item` route parses a path capture and computes from it, so the router, the `Path` extractor and a handler that
//! works are in the number too. A third "side", the ceiling, is a bare TCP server that answers every read with the right bytes: what the load
//! generator can do at most on this machine, so a reader can tell whether the generator or the server was the limit. See
//! `docs/BENCHMARKS.md` for what the numbers can and cannot support.
pub mod bare;

use std::net::TcpListener;
use std::time::{Duration, Instant};

use httpant_core::Path;
use httpant_server::Server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The response body of the constant route.
pub const BODY: &str = "Hello, World!";

/// The id the `item` route is asked for, and the body it must answer: `item ` and twice the id. A handler that parses the capture and does
/// arithmetic on it cannot be answered by a constant, and a wrong parse is a wrong body, which the checker counts as an error.
pub const ITEM_ID: u32 = 12345;
pub const ITEM_BODY: &str = "item 24690";

/// What the load asks for and what a right answer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// `GET /` answering [`BODY`]: nothing but the conversion layer.
    Constant,
    /// `GET /items/12345` answering [`ITEM_BODY`]: the router's capture, the `Path` extractor and a handler that computes.
    Item,
}

impl Workload {
    pub const ALL: [Workload; 2] = [Workload::Constant, Workload::Item];

    pub fn name(self) -> &'static str {
        match self {
            Workload::Constant => "constant",
            Workload::Item => "item",
        }
    }

    pub fn from_name(name: &str) -> Option<Workload> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }

    fn request(self) -> &'static [u8] {
        match self {
            Workload::Constant => b"GET / HTTP/1.1\r\nHost: bench\r\n\r\n",
            Workload::Item => b"GET /items/12345 HTTP/1.1\r\nHost: bench\r\n\r\n",
        }
    }

    /// The body a right answer has.
    pub fn body(self) -> &'static str {
        match self {
            Workload::Constant => BODY,
            Workload::Item => ITEM_BODY,
        }
    }
}

/// Every adapter the harness can run, in the order they are reported.
pub const ADAPTERS: &[&str] = &["hyper", "axum", "actix-web", "poem", "salvo", "warp"];

/// Which server answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Bare,
    Httpant,
    /// Not a web library: a bare TCP server answering every read with the right bytes, the most the load generator can ask of anything.
    Ceiling,
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

/// The `content-length` of a response head (0 when absent), found without allocating: this runs once per request, and allocating here is
/// load-generator cost that a faster server shows up as.
fn content_length(head: &[u8]) -> usize {
    const NAME: &[u8] = b"content-length:";
    for line in head.split(|b| *b == b'\n') {
        if line.len() > NAME.len() && line[..NAME.len()].eq_ignore_ascii_case(NAME) {
            let v = &line[NAME.len()..];
            let v = v.strip_suffix(b"\r").unwrap_or(v);
            return std::str::from_utf8(v)
                .ok()
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0);
        }
    }
    0
}

/// Read one response off the connection: true if it was `200` with exactly `body`.
async fn one_response(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    body: &[u8],
) -> std::io::Result<bool> {
    buf.clear();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = &buf[..end];
            let len = content_length(head);
            if buf.len() >= end + 4 + len {
                let ok = head.len() >= 12
                    && head[..12].eq_ignore_ascii_case(b"http/1.1 200")
                    && &buf[end + 4..end + 4 + len] == body;
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
    workload: Workload,
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
                let request = workload.request();
                let body = workload.body().as_bytes();
                while Instant::now() < end {
                    let t = Instant::now();
                    if stream.write_all(request).await.is_err() {
                        errors += 1;
                        break;
                    }
                    match one_response(&mut stream, &mut buf, body).await {
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

#[derive(serde::Deserialize)]
struct Item {
    id: u32,
}

/// Measure one server: bind, serve, load, stop.
///
/// The server is started, and stopped, on `srv`'s runtime and the load runs on the caller's, so the two do not share worker threads.
pub async fn measure(
    srv: &tokio::runtime::Handle,
    id: &'static str,
    side: Side,
    workload: Workload,
    connections: usize,
    warmup: Duration,
    duration: Duration,
) -> Run {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let run = match side {
        Side::Bare => {
            let bare = srv
                .spawn(async move { bare::serve(id, workload, listener) })
                .await
                .expect("start");
            let run = load(addr, workload, connections, warmup, duration).await;
            bare.stop();
            run
        }
        Side::Ceiling => {
            let ceiling = srv
                .spawn(async move { bare::ceiling(workload, listener) })
                .await
                .expect("start");
            let run = load(addr, workload, connections, warmup, duration).await;
            ceiling.stop();
            run
        }
        Side::Httpant => {
            drop(listener);
            let server = srv
                .spawn(async move {
                    let server = Server::with_adapter(id).expect("adapter compiled in");
                    let server = match workload {
                        Workload::Constant => server.get("/", || async { BODY }),
                        Workload::Item => server
                            .get("/items/:id", |Path(item): Path<Item>| async move {
                                format!("item {}", item.id * 2)
                            }),
                    };
                    server.bind(addr).await.expect("bind")
                })
                .await
                .expect("start");
            let run = load(server.addr(), workload, connections, warmup, duration).await;
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

fn median_u64(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// One adapter's result over all rounds. Index `i` of each vector is round `i`, so the two sides of a round can be paired.
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
    /// HTTPAnt over bare, round by round (the two sides of one round ran back to back, so a slow minute is in both).
    pub fn round_ratios(&self) -> Vec<f64> {
        self.httpant_rps
            .iter()
            .zip(&self.bare_rps)
            .map(|(h, b)| h / b)
            .collect()
    }
    /// The smallest and the largest per-round ratio: how far one round can be from the median.
    pub fn ratio_range(&self) -> (f64, f64) {
        let r = self.round_ratios();
        (
            r.iter().copied().fold(f64::INFINITY, f64::min),
            r.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
}

/// Run every named adapter `rounds` times on each side, alternating which side goes first from round to round.
pub async fn compare(
    srv: &tokio::runtime::Handle,
    ids: &[&'static str],
    workload: Workload,
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
                let run = measure(srv, row.id, side, workload, connections, warmup, duration).await;
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
                    Side::Ceiling => unreachable!("the ceiling is measured by `ceiling`"),
                }
            }
        }
    }
    rows
}

/// What the load generator itself can do: requests per second (per run, `rounds` runs) against the ceiling server, and the wrong answers.
#[derive(Clone, Debug)]
pub struct CeilingResult {
    pub rps: Vec<f64>,
    pub errors: u64,
}

impl CeilingResult {
    pub fn median(&self) -> f64 {
        median(self.rps.clone())
    }
    pub fn max(&self) -> f64 {
        self.rps.iter().copied().fold(0.0, f64::max)
    }
}

/// Run the load against the ceiling server `rounds` times.
pub async fn ceiling(
    srv: &tokio::runtime::Handle,
    workload: Workload,
    rounds: usize,
    connections: usize,
    warmup: Duration,
    duration: Duration,
) -> CeilingResult {
    let mut out = CeilingResult {
        rps: Vec::new(),
        errors: 0,
    };
    for _ in 0..rounds {
        let run = measure(
            srv,
            "ceiling",
            Side::Ceiling,
            workload,
            connections,
            warmup,
            duration,
        )
        .await;
        out.errors += run.errors;
        out.rps.push(run.rps());
    }
    out
}

/// The table as Markdown.
pub fn table(rows: &[Row]) -> String {
    let mut out = String::from(
        "| Adapter | bare req/s (median) | HTTPAnt req/s (median) | HTTPAnt / bare (median) | HTTPAnt / bare (worst..best round) | bare p99 us | HTTPAnt p99 us | wrong answers |\n|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let (lo, hi) = r.ratio_range();
        out.push_str(&format!(
            "| {} | {:.0} | {:.0} | {:.2} | {:.2}..{:.2} | {} | {} | {} |\n",
            r.id,
            r.bare_median(),
            r.httpant_median(),
            r.ratio(),
            lo,
            hi,
            median_u64(r.bare_p99_us.clone()),
            median_u64(r.httpant_p99_us.clone()),
            r.errors
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every adapter, both sides, both routes, a short run: the harness cannot rot, and every response was the right one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn every_adapter_answers_right_on_both_sides_on_both_routes() {
        for workload in Workload::ALL {
            let rows = compare(
                &tokio::runtime::Handle::current(),
                ADAPTERS,
                workload,
                2,
                4,
                Duration::from_millis(50),
                Duration::from_millis(300),
            )
            .await;
            for r in &rows {
                assert_eq!(
                    r.errors,
                    0,
                    "{} {}: wrong or missing answers",
                    workload.name(),
                    r.id
                );
                assert!(r.bare_median() > 0.0, "{}: bare served nothing", r.id);
                assert!(r.httpant_median() > 0.0, "{}: HTTPAnt served nothing", r.id);
                let (lo, hi) = r.ratio_range();
                assert!(0.0 < lo && lo <= r.ratio() && r.ratio() <= hi);
                assert_eq!(r.round_ratios().len(), 2);
            }
        }
    }

    /// The ceiling server answers the checker's way on both routes, so its number is a number for the same work.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_ceiling_answers_right() {
        for workload in Workload::ALL {
            let c = ceiling(
                &tokio::runtime::Handle::current(),
                workload,
                1,
                4,
                Duration::from_millis(50),
                Duration::from_millis(300),
            )
            .await;
            assert_eq!(c.errors, 0, "{}", workload.name());
            assert!(c.median() > 0.0);
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
        for workload in Workload::ALL {
            let run = load(
                addr,
                workload,
                2,
                Duration::ZERO,
                Duration::from_millis(200),
            )
            .await;
            assert_eq!(run.requests, 0);
            assert!(run.errors > 0);
        }
    }

    /// The `item` route's answer is the one the checker wants only if the handler parsed the capture: a constant `item 0` is an error.
    #[test]
    fn the_item_body_is_twice_the_id() {
        assert_eq!(ITEM_BODY, format!("item {}", ITEM_ID * 2));
    }

    #[test]
    fn content_length_is_found_in_any_case_and_absent_is_zero() {
        assert_eq!(
            content_length(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nx: y"),
            13
        );
        assert_eq!(
            content_length(b"HTTP/1.1 200 OK\r\ncontent-length:7\r\n"),
            7
        );
        assert_eq!(content_length(b"HTTP/1.1 200 OK\r\nx: y"), 0);
    }
}
