//! Serve "hello" and fetch it. CI runs this (`cargo run --example hello -p httpant-server`), so it cannot rot.
//! The client half is slice 3; until then the fetch is a plain std socket, and the server side is the part that is final.
use std::io::{Read, Write};

use httpant_server::Server;

#[tokio::main]
async fn main() -> httpant_server::core::Result<()> {
    let server = Server::new()
        .get("/", || async { "hello" })
        .bind("127.0.0.1:0")
        .await?;
    let mut tcp = std::net::TcpStream::connect(server.addr())?;
    tcp.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?;
    let mut reply = String::new();
    tcp.read_to_string(&mut reply)?;
    assert!(
        reply.starts_with("HTTP/1.1 200") && reply.ends_with("hello"),
        "{reply}"
    );
    println!(
        "served and fetched: {}",
        reply.lines().last().unwrap_or_default()
    );
    server.shutdown().await
}
