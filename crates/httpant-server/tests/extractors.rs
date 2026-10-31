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
