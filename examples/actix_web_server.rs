#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a web server with the Actix-Web adapter
    #[cfg(feature = "actix-web")]
    {
        use web_server_abstraction::{
            HttpMethod, Response, StatusCode, WebServer,
            middleware::{CorsMiddleware, LoggingMiddleware},
        };
        let server = WebServer::with_actix_web_adapter()
            // Add middleware
            .middleware(LoggingMiddleware::new())
            .middleware(CorsMiddleware::new().allow_origin("*"))
            // Add routes
            .route("/", HttpMethod::GET, |_req| async {
                Ok(Response::ok().body("Hello from Actix-Web!"))
            })
            .route("/health", HttpMethod::GET, |_req| async {
                Ok(Response::ok().body("Actix-Web server is healthy"))
            })
            .route("/users", HttpMethod::POST, |_req| async {
                Ok(Response::new(StatusCode::CREATED).body("User created via Actix-Web"))
            });

        // Bind to an address
        let bound_server = server.bind("127.0.0.1:3002").await?;

        println!("Actix-Web server starting on http://127.0.0.1:3002");

        // Run the server
        bound_server.run().await?;
    }

    #[cfg(not(feature = "actix-web"))]
    {
        println!(
            "Actix-Web feature not enabled. Run with: cargo run --example actix_web_server --features actix-web"
        );
        println!("Note: Actix-Web adapter is currently in simplified implementation mode.");
    }

    Ok(())
}
