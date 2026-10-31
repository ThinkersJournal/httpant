# Benchmarks note: TechEmpower round 23 (source for adapter speed claims)

Purpose: any HTTPAnt statement about how fast an adapter is must cite this file or a measurement of our own, with ref.

## The round

- **Round R23**, GitHub release `TechEmpower/FrameworkBenchmarks` tag `R23`, published 2025-03-03 (release body: "First round with new
  hardware"; environment "Citrine", `hw=ph`). A blog post "Framework Benchmarks Round 23" dated 2025-03-17 appeared in a web search result;
  the page was not opened.
- Raw data read: `https://www.techempower.com/benchmarks/results/round23/ph.json` (5.0 MB), fetched 2026-10-28. Run name "Continuous
  Benchmarking Run 2025-01-30 18:47:41", uuid `91a66052-9d86-446c-b31a-eadbd669ed08`, ran 2025-01-30 to 2025-02-06, repo commit
  `523534bb61450e3522d775a749ad060753e26e3a`, 15 s per level, concurrency levels 16..512, 841 frameworks listed.
- **Is it the latest?** Unverified beyond: `R23` is the newest release tag, a web search found no Round 24, and the GitHub repository is
  **archived** (`archived: true`, last push 2026-03-24). A later round published elsewhere would not have been found.
- `tfb-status.techempower.com` refused the connection on 2026-10-28, so no newer continuous run was read.

## Numbers (requests per second, peak over the concurrency levels)

Method (ours, not TechEmpower's published table): per framework, `totalRequests / (endTime - startTime)` per concurrency level from
`rawData[test][framework]`, then the maximum. The website's table may select differently; **treat these as a reading of the raw file, not a
quote of the site**. Rust entries only (`language == Rust`: 57 in this file).

| Entry (class) | JSON | Fortunes |
|---|---|---|
| may-minihttp (micro) | 3,102,063 | 1,327,379 |
| hyper (micro) | 2,885,610 | no plain entry; `hyper-db`: 1,066,645 |
| xitca-web (micro) | 2,720,330 | 1,075,043 |
| axum (platform) | 2,709,795 | no plain entry; `axum-pg` (fullstack): 1,114,266; `axum-pg-pool`: 67,449; `axum-sqlx`: 38,712 |
| salvo (micro) | 2,089,153 | `salvo-pg`: 969,747; `salvo-sqlx`: 32,289 |
| rocket (fullstack) | 540,665 | 220,494 |
| actix, actix-web-*, actix-http, actix-server | **no data in this file** | **no data in this file** |
| warp-rust | **no data in this file** | **no data in this file** |
| poem | not looked up | not looked up |

Control: the same script returns the figures above for entries that exist. Its "absent" result for `actix` is `name in rawData[test]`
being false; the run's `failed` map is empty for those names, so it is not a recorded failure and the reason is unknown.

## What these numbers can and cannot support

- They compare whole programs (framework + database driver + test code) on one machine, not adapters. Fortunes entries differ by database
  driver by a factor of ~30 (`axum-pg` vs `axum-sqlx`), more than any framework-to-framework difference, so a Fortunes figure says little
  about the web framework. JSON is the closer reading of transport cost.
- R23 was the first round on new hardware; rankings can move between rounds with environment changes.
- Actix-web has no figure here, so HTTPAnt must not claim anything about actix speed from R23. Measure it in our own bench harness.
- Nothing in the TechEmpower figures says what HTTPAnt's own overhead is. That is the next section, measured by our own harness.

## HTTPAnt's own overhead: adapter vs the bare library (our measurement)

Harness: `crates/httpant-bench` (unpublished; `cargo run --release -p httpant-bench`). Both sides serve `GET /` with the same 13-byte body
(`Hello, World!`) on a loopback listener, on the same machine, under the same load: 64 keep-alive connections, one request in flight on each.
The server and the load generator run on separate 4-thread runtimes; actix-web runs two workers on both sides (what HTTPAnt's actix adapter
uses). Each adapter is measured 5 times per side for 3 s after a 0.5 s warm-up, and the order of the two sides alternates between rounds so a
slow minute on the machine lands on both. Every response is checked (status 200, exact body); a wrong answer is counted, never counted as
speed (a unit test feeds the checker a wrong body and requires 0 requests and some errors). A short run of all twelve servers is a test in CI,
so the harness cannot rot; the numbers below are not produced in CI.

Measured at `5bbeeda` plus the harness (0.5.26 placeholder), 2026-10-31, release build, rustc 1.99.1, Windows 11, AMD Ryzen 9 7940HX
(32 logical processors), other lanes' processes possibly running on the same machine (not controlled). The route in the HTTPAnt run goes
through the router, the boxed service and the adapter's request and response conversion; the bare run goes through the library's own route.

| Adapter | bare req/s (median of 5) | HTTPAnt req/s (median of 5) | HTTPAnt / bare | bare p99 us | HTTPAnt p99 us | wrong answers |
|---|---|---|---|---|---|---|
| hyper | 44,992 | 40,741 | 0.91 | 2,585 | 2,906 | 0 |
| axum | 42,894 | 39,709 | 0.93 | 2,719 | 3,056 | 0 |
| actix-web | 41,848 | 33,961 | 0.81 | 3,002 | 3,733 | 0 |
| poem | 40,898 | 38,159 | 0.93 | 2,898 | 3,210 | 0 |
| salvo | 40,675 | 36,567 | 0.90 | 2,995 | 3,453 | 0 |
| warp | 42,666 | 29,483 | 0.69 | 2,715 | 4,410 | 0 |

What this supports, and what it does not:

- **A cost per adapter, on a route that does nothing.** On this machine HTTPAnt served 0.69 to 0.93 of the bare library's requests per second.
  The adapters differ: hyper, axum, poem and salvo lose 7 to 10 percent, actix-web 19, and warp 31. p99 latency rose by 10 to 60 percent.
- **It is an upper bound on relative cost, not an absolute speed.** A route that returns a constant spends nothing on application work, so the
  conversion layer is the largest share it will ever be; a real handler dilutes it.
- **The load generator may be the limit.** About 40,000 requests per second from 64 connections on a 32-thread machine is far below what these
  libraries do (TechEmpower JSON above, on other hardware). I did not measure whether the generator or the server saturated; if the generator
  did, the ratios are compressed toward 1 and the real gaps are larger. The harness gives the generator 4 threads and parses each response
  with allocation; both are the first things to change.
- **Spread was not recorded.** The table shows medians of five runs; the per-round figures were not kept, so how stable the ratios are is
  not shown. The ordering of warp, actix-web and the other four is large next to the run-to-run noise I saw, but that is an impression, not a
  measurement. Re-run with more rounds before quoting a single ratio.
- **One route, one body size, HTTP/1.1 cleartext, keep-alive.** Nothing about TLS, HTTP/2, request bodies, large responses, connection
  setup, or many routes. Nothing is claimed about actix-web or warp in absolute terms (TechEmpower R23 has no figure for either).
- Do not compare rows with each other as a framework ranking: the harness measures each library against itself.
