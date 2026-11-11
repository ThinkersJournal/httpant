// SPDX-License-Identifier: MIT OR Apache-2.0
//! Opt-in redirect following (ROADMAP row 14 slice 3c), measured against a scripted raw TCP server so that what each hop actually sent
//! (method, headers, body) can be read back.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use httpant_client::Client;
use httpant_core::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the server saw it.
#[derive(Clone, Debug)]
struct Seen {
    request_line: String,
    headers: String,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_owned())
        })
    }
}

struct Server {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// Answers with `script(request_line)`: a full raw HTTP response. Reads the head and any `Content-Length` body.
async fn serve(script: impl Fn(&str, SocketAddr) -> String + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let script = Arc::new(script);
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            let script = Arc::clone(&script);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i;
                    }
                    match conn.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let (request_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
                let want = headers
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = buf[head_end + 4..].to_vec();
                while body.len() < want {
                    match conn.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => body.extend_from_slice(&chunk[..n]),
                    }
                }
                log.lock().unwrap().push(Seen {
                    request_line: request_line.to_owned(),
                    headers: headers.to_owned(),
                    body,
                });
                let local = conn.local_addr().unwrap();
                let reply = script(request_line, local);
                let _ = conn.write_all(reply.as_bytes()).await;
                let _ = conn.shutdown().await;
            });
        }
    });
    Server { addr, seen }
}

fn redirect(status: &str, location: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn the_default_follows_nothing_and_hands_back_the_redirect() {
    let s = serve(|line, _| {
        if line.contains("/start") {
            redirect("302 Found", "/end")
        } else {
            ok("end")
        }
    })
    .await;
    let res = Client::new()
        .get(format!("http://{}/start", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 302);
    assert_eq!(s.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_opted_in_client_follows_a_chain_and_a_relative_location() {
    let s = serve(|line, _| match line.split(' ').nth(1).unwrap() {
        "/a" => redirect("301 Moved Permanently", "/b"),
        "/b" => redirect("307 Temporary Redirect", "c"),
        "/c" => ok("arrived"),
        other => ok(other),
    })
    .await;
    let client = Client::builder().follow_redirects(5).build().unwrap();
    let text = client
        .get(format!("http://{}/a", s.addr))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "arrived");
    let seen = s.seen.lock().unwrap();
    let lines: Vec<&str> = seen.iter().map(|r| r.request_line.as_str()).collect();
    assert_eq!(
        lines,
        ["GET /a HTTP/1.1", "GET /b HTTP/1.1", "GET /c HTTP/1.1"]
    );
}

#[tokio::test]
async fn more_redirects_than_the_cap_is_an_error_not_a_loop() {
    let s = serve(|_, _| redirect("302 Found", "/again")).await;
    let client = Client::builder().follow_redirects(3).build().unwrap();
    let Err(err) = client.get(format!("http://{}/", s.addr)).send().await else {
        panic!("a redirect loop must end");
    };
    assert!(matches!(err, Error::Redirect(_)), "{err}");
    assert_eq!(
        s.seen.lock().unwrap().len(),
        4,
        "the first request and exactly 3 follows"
    );
}

#[tokio::test]
async fn a_post_becomes_a_get_without_its_body_on_303_and_302_but_keeps_both_on_307() {
    for (status, kept) in [
        ("303 See Other", false),
        ("302 Found", false),
        ("307 Temporary Redirect", true),
        ("308 Permanent Redirect", true),
    ] {
        let s = serve(move |line, _| {
            if line.contains("/first") {
                redirect(status, "/second")
            } else {
                ok("done")
            }
        })
        .await;
        let client = Client::builder().follow_redirects(2).build().unwrap();
        client
            .post(format!("http://{}/first", s.addr))
            .header("content-type", "text/plain")
            .body("payload")
            .send()
            .await
            .unwrap();
        let seen = s.seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "{status}");
        let second = &seen[1];
        if kept {
            assert!(second.request_line.starts_with("POST /second"), "{status}");
            assert_eq!(second.body, b"payload", "{status}");
            assert_eq!(second.header("content-type").as_deref(), Some("text/plain"));
        } else {
            assert!(second.request_line.starts_with("GET /second"), "{status}");
            assert!(second.body.is_empty(), "{status}");
            assert!(second.header("content-type").is_none(), "{status}");
            assert!(
                second.header("content-length").is_none_or(|v| v == "0"),
                "{status}"
            );
        }
    }
}

#[tokio::test]
async fn a_streamed_body_cannot_follow_a_307_so_the_redirect_is_handed_back() {
    let s = serve(|_, _| redirect("307 Temporary Redirect", "/second")).await;
    let client = Client::builder().follow_redirects(2).build().unwrap();
    let res = client
        .post(format!("http://{}/first", s.addr))
        .body_stream(httpant_core::Body::from_bytes("streamed"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 307);
    assert_eq!(s.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn credentials_go_with_a_redirect_to_the_same_place_and_not_to_another_host_or_port() {
    // `localhost` and `127.0.0.1` are the same machine but different hosts to a client: that is exactly the case to hold.
    let other = serve(|_, _| ok("other")).await;
    let other_addr = other.addr;
    let origin = serve(move |line, local| match line.split(' ').nth(1).unwrap() {
        "/same" => redirect("302 Found", "/landed"),
        "/cross" => redirect(
            "302 Found",
            &format!("http://localhost:{}/x", other_addr.port()),
        ),
        "/port" => redirect(
            "302 Found",
            &format!("http://{}:{}/x", local.ip(), other_addr.port()),
        ),
        _ => ok("landed"),
    })
    .await;
    let client = Client::builder().follow_redirects(3).build().unwrap();
    let send = |path: &'static str| {
        let client = client.clone();
        let url = format!("http://{}{path}", origin.addr);
        async move {
            client
                .get(url)
                .bearer("secret-token")
                .header("cookie", "session=1")
                .header("x-trace", "keep")
                .send()
                .await
                .unwrap()
        }
    };

    send("/same").await;
    {
        let seen = origin.seen.lock().unwrap();
        let landed = seen.last().unwrap();
        assert!(landed.request_line.starts_with("GET /landed"));
        assert_eq!(
            landed.header("authorization").as_deref(),
            Some("Bearer secret-token")
        );
        assert_eq!(landed.header("cookie").as_deref(), Some("session=1"));
    }

    send("/cross").await;
    send("/port").await;
    let seen = other.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for r in seen.iter() {
        assert!(
            r.header("authorization").is_none(),
            "Authorization followed a redirect to another place: {}",
            r.headers
        );
        assert!(
            r.header("cookie").is_none(),
            "Cookie followed a redirect to another place"
        );
        assert_eq!(
            r.header("x-trace").as_deref(),
            Some("keep"),
            "an ordinary header is kept"
        );
    }
}

#[tokio::test]
async fn a_redirect_from_https_to_http_is_refused_before_anything_is_sent_in_clear() {
    // The https origin is a TLS server written here; its only job is to answer with a redirect to a plain-http listener we can watch.
    let watched = serve(|_, _| ok("clear")).await;
    let target = format!("http://{}/", watched.addr);
    let (addr, cert_pem) = tls_redirector(target).await;
    let client = Client::builder()
        .add_root_pem(cert_pem.as_bytes())
        .follow_redirects(3)
        .build()
        .unwrap();
    let Err(err) = client.get(format!("https://{addr}/")).send().await else {
        panic!("a downgrade must be refused");
    };
    assert!(matches!(err, Error::Redirect(_)), "{err}");
    assert!(err.to_string().contains("https to http"), "{err}");
    assert!(watched.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_redirect_without_a_usable_location_comes_back_as_the_response() {
    let s = serve(|_, _| {
        "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
    })
    .await;
    let client = Client::builder().follow_redirects(3).build().unwrap();
    let res = client
        .get(format!("http://{}/", s.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 302);
}

#[tokio::test]
async fn a_request_can_choose_its_own_cap() {
    let s = serve(|line, _| {
        if line.contains("/end") {
            ok("end")
        } else {
            redirect("302 Found", "/end")
        }
    })
    .await;
    let followed = Client::new()
        .get(format!("http://{}/", s.addr))
        .follow_redirects(2)
        .send()
        .await
        .unwrap();
    assert_eq!(followed.status().as_u16(), 200);
    let not_followed = Client::builder()
        .follow_redirects(2)
        .build()
        .unwrap()
        .get(format!("http://{}/", s.addr))
        .follow_redirects(0)
        .send()
        .await
        .unwrap();
    assert_eq!(not_followed.status().as_u16(), 302);
}

/// A TLS server (tokio-rustls) with a self-signed certificate for 127.0.0.1 that answers every request with a redirect to `to`.
async fn tls_redirector(to: String) -> (SocketAddr, String) {
    use rustls::pki_types::PrivateKeyDer;
    let made = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let key = PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![made.cert.der().clone()], key)
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let to = to.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = tls.write_all(redirect("302 Found", &to).as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    (addr, made.cert.pem())
}
