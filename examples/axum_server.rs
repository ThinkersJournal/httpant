use web_server_abstraction::{
    HttpMethod, Request, Response, StatusCode, WebServer,
    middleware::{CorsMiddleware, LoggingMiddleware},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a web server with the Axum adapter
    let server = WebServer::with_axum_adapter()
        // Add middleware
        .middleware(LoggingMiddleware::new())
        .middleware(CorsMiddleware::new().allow_origin("*"))
        // Add routes
        .route("/", HttpMethod::GET, |_req: Request| async {
            Ok(Response::ok().body("Hello from Axum!"))
        })
        .route("/health", HttpMethod::GET, |_req: Request| async {
            Ok(Response::ok().body("Axum server is healthy"))
        })
        .route("/users", HttpMethod::POST, |_req: Request| async {
            Ok(Response::new(StatusCode::CREATED).body("User created via Axum"))
        })
        .route("/users/:id", HttpMethod::GET, |_req: Request| async {
            Ok(Response::ok().body("User details from Axum"))
        });

    // Bind to an address
    let bound_server = server.bind("127.0.0.1:3001").await?;

    println!("Axum server starting on http://127.0.0.1:3001");

    // Run the server
    bound_server.run().await?;

    Ok(())
}
