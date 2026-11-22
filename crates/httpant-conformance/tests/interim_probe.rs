// SPDX-License-Identifier: MIT OR Apache-2.0
//! ROADMAP row 20 probe: what each adapter writes when a handler returns a 1xx status. Prints, asserts nothing yet.

use std::net::SocketAddr;
use std::time::Duration;

use httpant_core::http::StatusCode;
use httpant_core::{PathParams, Request};
use httpant_server::Server;

const IDS: &[&str] = &["hyper", "axum", "actix-web", "poem", "salvo", "warp"];

async fn status(req: Request) -> StatusCode {
    req.extensions()
        .get::<PathParams>()
        .and_then(|p| p.get("code"))
        .and_then(|c| c.parse::<u16>().ok())
        .and_then(|c| StatusCode::from_u16(c).ok())
        .unwrap_or(StatusCode::BAD_REQUEST)
}

fn raw_blocking(addr: SocketAddr, request: &[u8]) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(request).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => {
                out.extend_from_slice(b"<read timed out>");
                break;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn probe_interim_statuses() {
    for id in IDS {
        let server = Server::with_adapter(id)
            .unwrap()
            .get("/status/:code", status)
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        for code in [100u16, 101, 102, 103, 199] {
            let addr = server.addr();
            let req = format!(
                "GET /status/{code} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
            )
            .into_bytes();
            let got = tokio::task::spawn_blocking(move || raw_blocking(addr, &req))
                .await
                .unwrap();
            println!("PROBE {id} {code}: {got:?}");
        }
        let _ = server.shutdown().await;
    }
}
