# Rocket and extension methods: measured

ROADMAP row 6. The claim that rocket cannot be an HTTPAnt adapter rested on a reading of its source ("rocket 0.5.2 has nine request methods and
answers 400 to any other before a route is tried", `Request::from_hyp`, `BadMethod`); the design note and CHANGELOG 0.1.9 both said so. This
runs it. Conformance case 1 needs `PROPFIND` (an extension method, RFC 4918) to reach the application.

**Run 2026-10-31**, rocket 0.5.2 and rocket_http 0.5.2 (from the probe's `Cargo.lock`), default features off, one route (`GET /`), a raw TCP
client sending HTTP/1.1 with `Connection: close`. Windows, release build.

| request line | rocket 0.5.2 answered | what it shows |
|---|---|---|
| `GET /` | `200 OK` | positive control: the server is serving the route |
| `PROPFIND /` | **`400 Bad Request`** | an extension method never reaches a route |
| `FOO /` | **`400 Bad Request`** | the same for a made-up method |
| `PATCH /` | `404 Not Found` | a method rocket knows, with no route for it: a routing answer, not a method refusal |
| `CONNECT /` | `404 Not Found` | likewise |
| `TRACE /` | `404 Not Found` | likewise |

The `PATCH`, `CONNECT` and `TRACE` rows are the control for the `400`: the same server, with the same route table, answers a known method
without a route with `404`, so the `400` on `PROPFIND` and `FOO` is the method being refused, not the route being missing.

**Result:** the source reading is confirmed. A request with a method outside rocket's nine never reaches a route, so conformance case 1
(`PROPFIND`) cannot pass on a rocket adapter, and rocket stays out of HTTPAnt. The design note's row now says "measured".

Not covered: whether a rocket fairing or request guard could see the request before the `400` (the answer here is for a plain route table);
rocket 0.5.2 is the version that compiles today, and a later release may change the method set. Re-run this when it does.

## The probe (re-runnable, not part of the workspace)

`Cargo.toml`:

```toml
[package]
name = "rocket-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
rocket = { version = "0.5", default-features = false }
tokio = { version = "1", features = ["net", "io-util", "time", "rt", "macros"] }
```

`src/main.rs`:

```rust
#[macro_use]
extern crate rocket;

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[get("/")]
fn index() -> &'static str {
    "ok"
}

async fn ask(port: u16, request_line: &str) -> String {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!("{request_line} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut reply)).await;
    let text = String::from_utf8_lossy(&reply).into_owned();
    text.lines().next().unwrap_or("<no response>").to_owned()
}

#[rocket::main]
async fn main() {
    let port = 18765u16;
    let config = rocket::Config {
        port,
        address: "127.0.0.1".parse().unwrap(),
        log_level: rocket::config::LogLevel::Off,
        cli_colors: false,
        ..rocket::Config::debug_default()
    };
    let server = rocket::custom(config).mount("/", routes![index]);
    tokio::spawn(async move {
        let _ = server.launch().await;
    });
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for line in ["GET /", "PROPFIND /", "FOO /", "PATCH /", "CONNECT /", "TRACE /"] {
        println!("{line:<12} -> {}", ask(port, line).await);
    }
}
```

Build time on this box: 3 m 31 s release, `CARGO_BUILD_JOBS=2`.
