// SPDX-License-Identifier: MIT OR Apache-2.0
use httpant::{Client, Server};
#[tokio::main]
async fn main() -> httpant::Result<()> {
    let app = Server::new().get("/", || async { "hello" });
    let server = app.bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", server.addr());
    let text = Client::new().get(url).send().await?.text().await?;
    assert_eq!(text, "hello");
    Ok(())
}
