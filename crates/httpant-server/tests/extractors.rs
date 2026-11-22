// SPDX-License-Identifier: MIT OR Apache-2.0
//! `Path` and `Query` as handler arguments, on every adapter: the extractors are built below the adapters, so the answers must be the same
//! everywhere, including the refusals.
use httpant_client::Client;
use httpant_server::Server;
use httpant_server::core::{Path, Query};
use serde::Deserialize;

const ADAPTERS: &[&str] = &[
    "hyper",
    #[cfg(feature = "axum")]
    "axum",
    #[cfg(feature = "actix-web")]
    "actix-web",
    #[cfg(feature = "poem")]
    "poem",
    #[cfg(feature = "salvo")]
    "salvo",
    #[cfg(feature = "warp")]
    "warp",
];

#[derive(Deserialize)]
struct Item {
    id: u32,
}

#[derive(Deserialize)]
struct Page {
    page: Option<u32>,
    q: String,
}

#[tokio::test]
async fn path_and_query_arguments_answer_the_same_on_every_adapter() {
    for id in ADAPTERS {
        let server = Server::with_adapter(id)
            .unwrap()
            .get(
                "/items/:id",
                |Path(i): Path<Item>, Query(p): Query<Page>| async move {
                    format!("{}|{:?}|{}", i.id, p.page, p.q)
                },
            )
            .bind("127.0.0.1:0")
            .await
            .unwrap();
        let base = format!("http://{}", server.addr());
        let client = Client::new();
        let ok = client
            .get(format!("{base}/items/42?q=a+b%21&page=2"))
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), 200, "{id}");
        assert_eq!(ok.text().await.unwrap(), "42|Some(2)|a b!", "{id}");
        for (target, status) in [
            ("/items/abc?q=x", 400),
            ("/items/1", 400),
            ("/items/1?q=%FF", 400),
        ] {
            let res = client.get(format!("{base}{target}")).send().await.unwrap();
            assert_eq!(res.status(), status, "{id}: {target}");
        }
        server.shutdown().await.unwrap();
    }
}

/// A route that is not `Send` (`Server::local_route`) takes the same extractors as one that is, and answers the same: the control is the same
/// handler body on an ordinary route of the same server, and a failing extractor stops the handler before it runs.
#[cfg(feature = "actix-web")]
#[tokio::test]
async fn local_routes_take_path_and_query_and_refuse_like_ordinary_ones() {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use httpant_server::core::http::Method;

    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    let server = Server::with_adapter("actix-web")
        .unwrap()
        .get(
            "/send/:id",
            |Path(i): Path<Item>, Query(p): Query<Page>| async move {
                format!("{}|{:?}|{}", i.id, p.page, p.q)
            },
        )
        .local_route(Method::GET, "/local/:id", || {
            // State that is not `Send`, held across an await.
            let calls = Rc::new(Cell::new(0u32));
            move |Path(i): Path<Item>, Query(p): Query<Page>| {
                let calls = Rc::clone(&calls);
                async move {
                    calls.set(calls.get() + 1);
                    tokio::task::yield_now().await;
                    format!("{}|{:?}|{}", i.id, p.page, p.q)
                }
            }
        })
        .local_route(Method::GET, "/local-none", || || async { "none" })
        .local_route(Method::GET, "/local-guarded/:id", move || {
            let flag = Arc::clone(&flag);
            move |Path(_i): Path<Item>| {
                let flag = Arc::clone(&flag);
                async move {
                    flag.store(true, Ordering::SeqCst);
                    "ran"
                }
            }
        })
        .bind("127.0.0.1:0")
        .await
        .unwrap();
    let base = format!("http://{}", server.addr());
    let client = Client::new();
    let get = |target: &'static str| {
        let (client, base) = (client.clone(), base.clone());
        async move {
            let res = client.get(format!("{base}{target}")).send().await.unwrap();
            (res.status().as_u16(), res.text().await.unwrap())
        }
    };
    // Same answer as the ordinary route, success and refusals alike.
    for (send, local) in [
        ("/send/42?q=a+b%21&page=2", "/local/42?q=a+b%21&page=2"),
        ("/send/abc?q=x", "/local/abc?q=x"),
        ("/send/1", "/local/1"),
        ("/send/1?q=%FF", "/local/1?q=%FF"),
    ] {
        let (a, b) = (get(send).await, get(local).await);
        assert_eq!(a, b, "{send} vs {local}");
    }
    assert_eq!(get("/local/42?q=a+b%21&page=2").await.1, "42|Some(2)|a b!");
    assert_eq!(get("/local/abc?q=x").await.0, 400);
    assert_eq!(get("/local-none").await, (200, "none".to_owned()));
    assert_eq!(get("/local-guarded/zz").await.0, 400);
    assert!(
        !ran.load(Ordering::SeqCst),
        "the local handler ran after its extractor failed"
    );
    assert_eq!(get("/local-guarded/5").await, (200, "ran".to_owned()));
    server.shutdown().await.unwrap();
}
