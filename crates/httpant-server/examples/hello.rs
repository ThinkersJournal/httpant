// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serve "hello" and fetch it. CI runs this (`cargo run --example hello -p httpant-server`), so it cannot rot.
//! The client half is slice 3; until then the fetch is a plain std socket, and the server side is the part that is final.
//!
//! Anything that came off the network is escaped before it is printed: copy that habit, a reply can contain CR/LF and fake log lines.
use std::io::{Read, Write};

use httpant_server::Server;

/// `text` with every control character escaped (CR becomes `\r`, LF `\n`, ESC `\u{1b}`), so it can sit inside one log line.
fn loggable(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

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
    let last_line = reply.lines().last().unwrap_or_default();
    assert!(
        reply.starts_with("HTTP/1.1 200") && last_line == "hello",
        "unexpected reply: {}",
        loggable(&reply)
    );
    println!("served and fetched: {}", loggable(last_line));
    server.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::loggable;

    #[test]
    fn control_characters_cannot_start_a_new_log_line() {
        let hostile = "hello\r\nINFO forged entry\u{1b}[31m\ttab";
        let safe = loggable(hostile);
        assert_eq!(safe, r"hello\r\nINFO forged entry\u{1b}[31m\ttab");
        assert!(!safe.contains(['\r', '\n', '\u{1b}', '\t']));
    }

    #[test]
    fn ordinary_text_is_unchanged() {
        assert_eq!(loggable("hello, wörld"), "hello, wörld");
    }
}
