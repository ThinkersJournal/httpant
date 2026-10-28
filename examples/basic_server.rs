use web_server_abstraction::{
    HttpMethod, Response, StatusCode, WebServer,
    middleware::{CorsMiddleware, LoggingMiddleware},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a web server with the mock adapter (for demonstration)
    let server = WebServer::new()
        // Add middleware
        .middleware(LoggingMiddleware::new())
        .middleware(CorsMiddleware::new().allow_origin("*"))
        // Add routes
        .route("/", HttpMethod::GET, |_req| async {
            Ok(Response::ok().body("Hello, World!"))
        })
        .route("/health", HttpMethod::GET, |_req| async {
            Ok(Response::ok().body("OK"))
        })
        .route("/users", HttpMethod::POST, |_req| async {
            Ok(Response::new(StatusCode::CREATED).body("User created"))
        })
        .route("/users/:id", HttpMethod::GET, |_req| async {
            Ok(Response::ok().body("User details"))
        });

    // Bind to an address
    let bound_server = server.bind("127.0.0.1:3000").await?;

    println!("Server starting on http://127.0.0.1:3000");

    // Run the server
    bound_server.run().await?;

    Ok(())
}
