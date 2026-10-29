// SPDX-License-Identifier: MIT OR Apache-2.0
//! A minimal HTTP/1.1 client over a bare TCP socket, for the conformance suite only.
//!
//! It writes exactly the bytes it is given and reports exactly the bytes it receives (header names and values are byte strings, repeated
//! headers stay repeated), which is what lets the suite see defects a typed client would hide: a folded method, a dropped `Set-Cookie`.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long [`send`] waits for a whole response before failing, so a hung server fails a test instead of hanging it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// A request written to the socket byte for byte.
#[derive(Clone, Debug)]
pub struct RawRequest {
    method: Vec<u8>,
    target: Vec<u8>,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    body: Vec<u8>,
}

impl RawRequest {
    /// A request line with any method token, including ones `http::Method` would also accept (`PROPFIND`) and ones it would not.
    pub fn new(method: impl AsRef<[u8]>, target: impl AsRef<[u8]>) -> Self {
        Self {
            method: method.as_ref().to_vec(),
            target: target.as_ref().to_vec(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Add a header; name and value are raw bytes and may repeat.
    pub fn header(mut self, name: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Self {
        self.headers
            .push((name.as_ref().to_vec(), value.as_ref().to_vec()));
        self
    }

    /// Set the body and a matching `Content-Length`.
    pub fn body(mut self, body: impl AsRef<[u8]>) -> Self {
        self.body = body.as_ref().to_vec();
        self
    }

    /// The exact bytes sent. `Host` and `Connection: close` are added unless already present, so one request is one connection.
    pub fn to_bytes(&self) -> Vec<u8> {
        let has = |n: &str| {
            self.headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case(n.as_bytes()))
        };
        let mut out = Vec::new();
        out.extend_from_slice(&self.method);
        out.push(b' ');
        out.extend_from_slice(&self.target);
        out.extend_from_slice(b" HTTP/1.1\r\n");
        if !has("host") {
            out.extend_from_slice(b"Host: localhost\r\n");
        }
        if !has("connection") {
            out.extend_from_slice(b"Connection: close\r\n");
        }
        if !self.body.is_empty() && !has("content-length") {
            out.extend_from_slice(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());
        }
        for (k, v) in &self.headers {
            out.extend_from_slice(k);
            out.extend_from_slice(b": ");
            out.extend_from_slice(v);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

/// A response as received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawResponse {
    /// The status code from the status line.
    pub status: u16,
    /// The reason phrase from the status line, as sent.
    pub reason: Vec<u8>,
    /// Header lines in order, names and values as sent (surrounding whitespace on the value removed).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// The body after chunked decoding, if any.
    pub body: Vec<u8>,
}

impl RawResponse {
    /// Every value of a header, in order, matched case-insensitively. Repeats are separate entries.
    pub fn header_values(&self, name: &str) -> Vec<&[u8]> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name.as_bytes()))
            .map(|(_, v)| v.as_slice())
            .collect()
    }
}

/// Send one request on a new connection and read one response, failing after [`DEFAULT_TIMEOUT`].
pub async fn send(addr: SocketAddr, request: &RawRequest) -> io::Result<RawResponse> {
    send_with_timeout(addr, request, DEFAULT_TIMEOUT).await
}

/// [`send`] with an explicit timeout.
pub async fn send_with_timeout(
    addr: SocketAddr,
    request: &RawRequest,
    timeout: Duration,
) -> io::Result<RawResponse> {
    let head_only = request.method.eq_ignore_ascii_case(b"HEAD");
    tokio::time::timeout(timeout, async {
        let (mut read, mut write) = TcpStream::connect(addr).await?.into_split();
        // Written concurrently with the read, and a write error ignored: a server may answer (413) and close before it has read a large
        // body, and the answer is what the test is about.
        let bytes = request.to_bytes();
        let writer = tokio::spawn(async move {
            let _ = write.write_all(&bytes).await;
            write
        });
        let res = read_response(&mut read, head_only).await;
        writer.abort();
        res
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "no complete response before the timeout",
        )
    })?
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}

struct Reader<S> {
    stream: S,
    buf: Vec<u8>,
    eof: bool,
}

impl<S: AsyncRead + Unpin> Reader<S> {
    async fn fill(&mut self) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        let n = self.stream.read(&mut chunk).await?;
        if n == 0 {
            self.eof = true;
        }
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(())
    }

    /// One line without its terminator (CRLF, or a bare LF).
    async fn line(&mut self) -> io::Result<Vec<u8>> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(line);
            }
            if self.eof {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed inside a response head",
                ));
            }
            self.fill().await?;
        }
    }

    async fn exact(&mut self, n: usize) -> io::Result<Vec<u8>> {
        while self.buf.len() < n {
            if self.eof {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed inside a response body",
                ));
            }
            self.fill().await?;
        }
        Ok(self.buf.drain(..n).collect())
    }

    /// The status line and header lines, with an empty body.
    async fn head(&mut self) -> io::Result<RawResponse> {
        let status_line = self.line().await?;
        let mut parts = status_line.splitn(3, |&b| b == b' ');
        let version = parts.next().unwrap_or_default();
        if !version.starts_with(b"HTTP/1.") {
            return Err(bad("status line does not start with HTTP/1."));
        }
        let status: u16 = std::str::from_utf8(parts.next().unwrap_or_default())
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("status code is not a number"))?;
        let reason = parts.next().unwrap_or_default().to_vec();

        let mut headers = Vec::new();
        loop {
            let line = self.line().await?;
            if line.is_empty() {
                break;
            }
            let colon = line
                .iter()
                .position(|&b| b == b':')
                .ok_or_else(|| bad("header line without a colon"))?;
            headers.push((line[..colon].to_vec(), trim(&line[colon + 1..]).to_vec()));
        }
        Ok(RawResponse {
            status,
            reason,
            headers,
            body: Vec::new(),
        })
    }

    /// One chunk of a chunked body; `None` after the last chunk (its trailers are consumed).
    async fn chunk(&mut self) -> io::Result<Option<Vec<u8>>> {
        let size_line = self.line().await?;
        let size_text = size_line.split(|&b| b == b';').next().unwrap_or_default();
        let size = std::str::from_utf8(trim(size_text))
            .ok()
            .and_then(|s| usize::from_str_radix(s, 16).ok())
            .ok_or_else(|| bad("chunk size is not hex"))?;
        if size == 0 {
            while !self.line().await?.is_empty() {} // trailers
            return Ok(None);
        }
        let data = self.exact(size).await?;
        if !self.line().await?.is_empty() {
            return Err(bad("chunk not followed by CRLF"));
        }
        Ok(Some(data))
    }

    async fn read_to_close(&mut self) -> io::Result<Vec<u8>> {
        while !self.eof {
            self.fill().await?;
        }
        Ok(std::mem::take(&mut self.buf))
    }
}

fn trim(mut v: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = v {
        if *first == b' ' || *first == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = v {
        if *last == b' ' || *last == b'\t' {
            v = rest;
        } else {
            break;
        }
    }
    v
}

fn is_chunked(res: &RawResponse) -> bool {
    res.header_values("transfer-encoding")
        .iter()
        .any(|v| v.to_ascii_lowercase().windows(7).any(|w| w == b"chunked"))
}

fn has_no_body(res: &RawResponse, head_request: bool) -> bool {
    head_request || (100..200).contains(&res.status) || res.status == 204 || res.status == 304
}

impl<S: AsyncRead + Unpin> Reader<S> {
    /// The rest of a response whose head has been read.
    async fn finish(
        &mut self,
        mut res: RawResponse,
        head_request: bool,
    ) -> io::Result<RawResponse> {
        if has_no_body(&res, head_request) {
            return Ok(res);
        }
        if is_chunked(&res) {
            while let Some(data) = self.chunk().await? {
                res.body.extend(data);
            }
        } else if let Some(len) = res.header_values("content-length").first() {
            let n: usize = std::str::from_utf8(len)
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad("Content-Length is not a number"))?;
            res.body = self.exact(n).await?;
        } else {
            res.body = self.read_to_close().await?;
        }
        Ok(res)
    }
}

async fn read_response<S: AsyncRead + Unpin>(
    stream: &mut S,
    head_request: bool,
) -> io::Result<RawResponse> {
    let mut r = Reader {
        stream,
        buf: Vec::new(),
        eof: false,
    };
    let head = r.head().await?;
    r.finish(head, head_request).await
}

/// One connection that stays open across several exchanges, for what a one-shot [`send`] cannot show: responses arriving piece by piece,
/// keep-alive, an interim `100 Continue`, who closes the connection and when. Reads time out after [`DEFAULT_TIMEOUT`] unless told otherwise.
pub struct Conn {
    reader: Reader<tokio::net::tcp::OwnedReadHalf>,
    write: tokio::net::tcp::OwnedWriteHalf,
    timeout: Duration,
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "no data before the timeout")
}

impl Conn {
    /// Connect to `addr`.
    pub async fn open(addr: SocketAddr) -> io::Result<Self> {
        let (read, write) = TcpStream::connect(addr).await?.into_split();
        Ok(Self {
            reader: Reader {
                stream: read,
                buf: Vec::new(),
                eof: false,
            },
            write,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Fail a read that takes longer than `timeout` (a case that expects data to arrive promptly uses a short one, so a broken adapter
    /// fails quickly).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Write bytes as given.
    pub async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write.write_all(bytes).await
    }

    /// Write a whole request (its body, if any, too).
    pub async fn send(&mut self, request: &RawRequest) -> io::Result<()> {
        self.write(&request.to_bytes()).await
    }

    /// Read one response head (an interim `1xx` is a response too).
    pub async fn head(&mut self) -> io::Result<RawResponse> {
        tokio::time::timeout(self.timeout, self.reader.head())
            .await
            .map_err(|_| timed_out())?
    }

    /// Read the next chunk of a chunked body, as soon as it arrives; `None` after the last one.
    pub async fn chunk(&mut self) -> io::Result<Option<Vec<u8>>> {
        tokio::time::timeout(self.timeout, self.reader.chunk())
            .await
            .map_err(|_| timed_out())?
    }

    /// Read one whole response to a request of `method`.
    pub async fn response(&mut self, method: &str) -> io::Result<RawResponse> {
        let head_request = method.eq_ignore_ascii_case("HEAD");
        tokio::time::timeout(self.timeout, async {
            let head = self.reader.head().await?;
            self.reader.finish(head, head_request).await
        })
        .await
        .map_err(|_| timed_out())?
    }

    /// Send a request and read its response, on this connection. Unlike [`send`], a request without a `Connection` header asks to keep
    /// the connection alive, since reuse is the point.
    pub async fn exchange(&mut self, request: &RawRequest) -> io::Result<RawResponse> {
        let has_connection = request
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(b"connection"));
        if has_connection {
            self.send(request).await?;
        } else {
            self.send(&request.clone().header("Connection", "keep-alive"))
                .await?;
        }
        let method = String::from_utf8_lossy(&request.method).into_owned();
        self.response(&method).await
    }

    /// `true` if the server closes the connection (end of stream or a reset) within `within`; `false` if it stays open and silent, or
    /// sends something.
    pub async fn closed_by_peer(&mut self, within: Duration) -> bool {
        if self.reader.eof {
            return true;
        }
        match tokio::time::timeout(within, self.reader.fill()).await {
            Ok(Err(_)) => true,
            Ok(Ok(())) => self.reader.eof,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    /// A hand-written server: reads the request head (and `body_len` more bytes), replies with `reply`, returns the request bytes it saw.
    async fn serve_once(
        reply: &'static [u8],
        body_len: usize,
    ) -> (SocketAddr, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 1024];
            let head_end = loop {
                let n = s.read(&mut chunk).await.unwrap();
                assert!(n > 0, "client closed before finishing the request head");
                seen.extend_from_slice(&chunk[..n]);
                if let Some(i) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            while seen.len() < head_end + body_len {
                let n = s.read(&mut chunk).await.unwrap();
                seen.extend_from_slice(&chunk[..n]);
            }
            s.write_all(reply).await.unwrap();
            s.shutdown().await.unwrap();
            seen
        });
        (addr, task)
    }

    #[tokio::test]
    async fn sends_an_unusual_method_and_non_utf8_header_bytes_untouched() {
        let (addr, server) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", 0).await;
        let req = RawRequest::new("PROPFIND", "/a?b=1").header("X-Raw", [0xffu8, 0xfe, b'x']);
        send(addr, &req).await.unwrap();
        let seen = server.await.unwrap();
        assert!(
            seen.starts_with(b"PROPFIND /a?b=1 HTTP/1.1\r\n"),
            "{:?}",
            String::from_utf8_lossy(&seen)
        );
        assert!(seen.windows(7).any(|w| w == *b"X-Raw: "));
        assert!(
            seen.windows(5)
                .any(|w| w == [b' ', 0xff, 0xfe, b'x', b'\r'])
        );
    }

    #[tokio::test]
    async fn sends_a_body_with_a_content_length() {
        let (addr, server) = serve_once(b"HTTP/1.1 204 No Content\r\n\r\n", 5).await;
        send(addr, &RawRequest::new("POST", "/").body("hello"))
            .await
            .unwrap();
        let seen = server.await.unwrap();
        assert!(
            seen.windows(17).any(|w| w == b"Content-Length: 5"),
            "{:?}",
            String::from_utf8_lossy(&seen)
        );
        assert!(seen.ends_with(b"\r\n\r\nhello"));
    }

    #[tokio::test]
    async fn reads_status_reason_and_repeated_headers_as_separate_entries() {
        let (addr, _s) = serve_once(
            b"HTTP/1.1 418 I'm a teapot\r\nSet-Cookie: a=1\r\nset-cookie: b=2\r\nContent-Length: 2\r\n\r\nhi",
            0,
        )
        .await;
        let res = send(addr, &RawRequest::new("GET", "/")).await.unwrap();
        assert_eq!(res.status, 418);
        assert_eq!(res.reason, b"I'm a teapot");
        assert_eq!(
            res.header_values("SET-COOKIE"),
            [b"a=1".as_slice(), b"b=2".as_slice()]
        );
        assert_eq!(res.body, b"hi");
    }

    #[tokio::test]
    async fn decodes_chunked_bodies_and_keeps_binary_bytes() {
        let (addr, _s) = serve_once(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n\xff\x00\xfe\r\n2;ext=1\r\nab\r\n0\r\nX-Trailer: t\r\n\r\n",
            0,
        )
        .await;
        let res = send(addr, &RawRequest::new("GET", "/")).await.unwrap();
        assert_eq!(res.body, [0xff, 0x00, 0xfe, b'a', b'b']);
    }

    #[tokio::test]
    async fn without_framing_the_body_runs_to_connection_close() {
        let (addr, _s) = serve_once(b"HTTP/1.1 200 OK\r\n\r\nuntil close", 0).await;
        assert_eq!(
            send(addr, &RawRequest::new("GET", "/")).await.unwrap().body,
            b"until close"
        );
    }

    #[tokio::test]
    async fn a_head_response_has_no_body_even_with_a_content_length() {
        let (addr, _s) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n", 0).await;
        let res = send(addr, &RawRequest::new("HEAD", "/")).await.unwrap();
        assert_eq!(res.header_values("content-length"), [b"99".as_slice()]);
        assert!(res.body.is_empty());
    }

    #[tokio::test]
    async fn a_truncated_body_is_an_error_not_a_short_success() {
        let (addr, _s) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort", 0).await;
        let err = send(addr, &RawRequest::new("GET", "/")).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn a_silent_server_times_out_instead_of_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _hold = tokio::spawn(async move {
            let (_s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let err = send_with_timeout(
            addr,
            &RawRequest::new("GET", "/"),
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
