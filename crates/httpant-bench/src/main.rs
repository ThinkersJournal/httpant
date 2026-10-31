// SPDX-License-Identifier: MIT OR Apache-2.0
//! `cargo run --release -p httpant-bench -- [--rounds N] [--seconds S] [--connections C] [--adapters a,b]`
//!
//! Prints the Markdown table `docs/BENCHMARKS.md` records. Build with `--release`: a debug build measures the compiler's settings.
use std::time::Duration;

use httpant_bench::{ADAPTERS, compare, table};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = arg(&args, "--rounds").map_or(5, |v| v.parse().expect("--rounds N"));
    let seconds: f64 = arg(&args, "--seconds").map_or(3.0, |v| v.parse().expect("--seconds S"));
    let connections: usize =
        arg(&args, "--connections").map_or(64, |v| v.parse().expect("--connections C"));
    let ids: Vec<&'static str> = match arg(&args, "--adapters") {
        None => ADAPTERS.to_vec(),
        Some(list) => list
            .split(',')
            .map(|w| {
                *ADAPTERS
                    .iter()
                    .find(|a| **a == w)
                    .unwrap_or_else(|| panic!("unknown adapter {w}"))
            })
            .collect(),
    };
    // The servers and the load generator get four threads each, so neither can starve the other on a big machine and the numbers do not
    // depend on how many cores it has.
    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let load_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let srv = server_rt.handle().clone();
    let rows = load_rt.block_on(async {
        compare(
            &srv,
            &ids,
            rounds,
            connections,
            Duration::from_millis(500),
            Duration::from_secs_f64(seconds),
        )
        .await
    });
    println!("{}", table(&rows));
    println!(
        "rounds={rounds} seconds={seconds} connections={connections} server_threads=4 (actix: 2 workers on both sides)"
    );
}
