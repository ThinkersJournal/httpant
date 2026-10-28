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
- Nothing here says what HTTPAnt's own overhead is. That is a separate measurement (adapter vs the bare framework) with its own row in the design.
