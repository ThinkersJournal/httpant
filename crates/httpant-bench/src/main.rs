// SPDX-License-Identifier: MIT OR Apache-2.0
//! `cargo run --release -p httpant-bench -- [--rounds N] [--seconds S] [--connections C] [--adapters a,b] [--routes constant,item] [--load-threads T]`
//!
//! Prints, for each route, the Markdown table `docs/BENCHMARKS.md` records, preceded by the load generator's ceiling on that route (a bare
//! TCP server answering every read with the right bytes): an adapter near the ceiling was limited by the generator, not by itself. Build with
//! `--release`: a debug build measures the compiler's settings.
use std::time::Duration;

use httpant_bench::{ADAPTERS, Workload, ceiling, compare, table};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds: usize = arg(&args, "--rounds").map_or(11, |v| v.parse().expect("--rounds N"));
    let seconds: f64 = arg(&args, "--seconds").map_or(3.0, |v| v.parse().expect("--seconds S"));
    let connections: usize =
        arg(&args, "--connections").map_or(64, |v| v.parse().expect("--connections C"));
    let load_threads: usize =
        arg(&args, "--load-threads").map_or(4, |v| v.parse().expect("--load-threads T"));
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
    let workloads: Vec<Workload> = match arg(&args, "--routes") {
        None => Workload::ALL.to_vec(),
        Some(list) => list
            .split(',')
            .map(|w| Workload::from_name(w).unwrap_or_else(|| panic!("unknown route {w}")))
            .collect(),
    };
    // The servers get four threads and the load generator `--load-threads` (default four), so neither can starve the other on a big machine
    // and the numbers do not depend on how many cores it has.
    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime");
    let load_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(load_threads)
        .enable_all()
        .build()
        .expect("runtime");
    let srv = server_rt.handle().clone();
    let warmup = Duration::from_millis(500);
    let duration = Duration::from_secs_f64(seconds);
    for workload in workloads {
        let (ceil, rows) = load_rt.block_on(async {
            let ceil = ceiling(&srv, workload, rounds, connections, warmup, duration).await;
            let rows = compare(&srv, &ids, workload, rounds, connections, warmup, duration).await;
            (ceil, rows)
        });
        println!("### route: {}\n", workload.name());
        println!(
            "load generator ceiling (bare TCP server, right bytes): median {:.0} req/s, best {:.0}, wrong answers {}\n",
            ceil.median(),
            ceil.max(),
            ceil.errors
        );
        println!("{}", table(&rows));
    }
    println!(
        "rounds={rounds} seconds={seconds} connections={connections} server_threads=4 load_threads={load_threads} (actix: 2 workers on both sides)"
    );
}
