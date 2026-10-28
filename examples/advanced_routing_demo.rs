//! Advanced routing features demo
//!
//! This example demonstrates the advanced routing capabilities including:
//! - Path parameters (e.g., /users/:id)
//! - Wildcard routes (e.g., /static/*file)
//! - WebSocket support
//! - Convenience HTTP method functions

use web_server_abstraction::{
    Result, WebServer,
    types::{Request, Response, StatusCode},
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Advanced routing demo with automatic path parameter extraction!");
    println!("Features demonstrated:");
    println!("  • Automatic path parameter extraction (req.param() and req.params())");
    println!("  • Complete HTTP method support (GET, POST, PUT, DELETE, HEAD, OPTIONS)");
    println!("  • Nested routing patterns");
    println!("  • WebSocket support");
    println!();

    let server = WebServer::with_mock_adapter()
        .with_path_params() // Enable automatic path parameter extraction!
        // Basic routes
        .get("/", home_handler)
        .post("/users", create_user_handler)
        // Path parameter routes with multiple HTTP methods
        .get("/users/:id", get_user_handler)
        .put("/users/:id", update_user_handler)
        .delete("/users/:id", delete_user_handler)
        .head("/users/:id", head_user_handler) // New: HEAD support
        .options("/users/:id", options_user_handler) // New: OPTIONS support
        // Multiple path parameters
        .get("/users/:user_id/posts/:post_id", get_user_post_handler)
        // Wildcard routes for serving static files
        .get("/static/*file", serve_static_handler)
        .get("/assets/*path", serve_assets_handler)
        // WebSocket endpoint
        .websocket("/ws")
        // Mixed parameter and literal segments
        .get("/api/v1/users/:id/profile", get_user_profile_handler);

    println!("🎯 Route configuration complete!");
    println!("Available endpoints:");
    println!("  • GET    /users/:id           - Get user details");
    println!("  • PUT    /users/:id           - Update user");
    println!("  • DELETE /users/:id           - Delete user");
    println!("  • HEAD   /users/:id           - User headers only");
    println!("  • OPTIONS /users/:id          - User options");
    println!("  • GET    /users/:user_id/posts/:post_id - Get user post");
    println!("  • WS     /ws                  - WebSocket connection");
    println!();

    // For demonstration, show how route matching works
    demonstrate_route_matching();

    let bound_server = server.bind("127.0.0.1:3000").await?;
    println!("🎯 Server bound to 127.0.0.1:3000");

    bound_server.run().await
}

// Route handlers

async fn home_handler(_req: Request) -> Result<Response> {
    Ok(Response::new(StatusCode::OK)
        .header("Content-Type", "text/html")
        .body("
            <h1>🚀 Advanced Routing Demo - Enhanced Features</h1>
            <p>This demo showcases the enhanced web server abstraction with automatic path parameter extraction!</p>

            <h2>✨ New Features</h2>
            <ul>
                <li><strong>Automatic Path Parameter Extraction</strong> - Use req.param() and req.params()</li>
                <li><strong>Complete HTTP Method Support</strong> - GET, POST, PUT, DELETE, HEAD, OPTIONS, TRACE, CONNECT</li>
                <li><strong>Proper WebSocket Support</strong> - RFC 6455 compliant key generation</li>
                <li><strong>Enhanced Middleware System</strong> - Path parameter middleware and transformation middleware</li>
            </ul>

            <h2>🧪 Try these endpoints:</h2>
            <ul>
                <li><a href=\"/users/123\">/users/123</a> - User details (GET)</li>
                <li><strong>curl -X HEAD /users/123</strong> - User headers only (HEAD)</li>
                <li><strong>curl -X OPTIONS /users/123</strong> - User options (OPTIONS)</li>
                <li><a href=\"/users/456/posts/789\">/users/456/posts/789</a> - User post (Multiple params)</li>
                <li><a href=\"/static/logo.png\">/static/logo.png</a> - Static file (Wildcard param)</li>
                <li><a href=\"/assets/css/style.css\">/assets/css/style.css</a> - Asset file (Wildcard param)</li>
                <li><a href=\"/api/v1/users/123/profile\">/api/v1/users/123/profile</a> - User profile (Nested params)</li>
            </ul>

            <h2>🔧 Technical Details</h2>
            <p>All path parameters are now automatically extracted using the enhanced middleware system.
            Handlers can simply call <code>req.param(\"name\")</code> or <code>req.params()</code> to access extracted values.</p>
        "))
}

async fn create_user_handler(req: Request) -> Result<Response> {
    println!("Creating user with body: {:?}", req.body);
    Response::json(&serde_json::json!({
        "message": "User created",
        "id": 42
    }))
}

async fn get_user_handler(req: Request) -> Result<Response> {
    // Path parameters are now automatically extracted!
    let user_id = req.param("id").unwrap_or("unknown");

    Response::json(&serde_json::json!({
        "id": user_id,
        "name": "John Doe",
        "email": "john@example.com",
        "extracted_automatically": true
    }))
}

async fn update_user_handler(req: Request) -> Result<Response> {
    let user_id = req.param("id").unwrap_or("unknown");

    Response::json(&serde_json::json!({
        "message": format!("User {} updated", user_id),
        "auto_extracted": true
    }))
}

async fn delete_user_handler(req: Request) -> Result<Response> {
    let user_id = req.param("id").unwrap_or("unknown");

    Response::json(&serde_json::json!({
        "message": format!("User {} deleted", user_id),
        "params": req.params()
    }))
}

async fn get_user_post_handler(req: Request) -> Result<Response> {
    let user_id = req.param("user_id").unwrap_or("unknown");
    let post_id = req.param("post_id").unwrap_or("unknown");

    Response::json(&serde_json::json!({
        "user_id": user_id,
        "post_id": post_id,
        "title": "Sample Post",
        "content": "This is a sample post content",
        "all_params": req.params()
    }))
}

async fn head_user_handler(req: Request) -> Result<Response> {
    let user_id = req.param("id").unwrap_or("unknown");

    // HEAD should return same headers as GET but no body
    Ok(Response::new(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("X-User-ID", user_id)
        .header("Content-Length", "85")) // Approximate length
}

async fn options_user_handler(_req: Request) -> Result<Response> {
    Ok(Response::new(StatusCode::OK)
        .header("Allow", "GET, PUT, DELETE, HEAD, OPTIONS")
        .header(
            "Access-Control-Allow-Methods",
            "GET, PUT, DELETE, HEAD, OPTIONS",
        )
        .header(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization",
        ))
}
async fn serve_static_handler(req: Request) -> Result<Response> {
    let file_path = req.param("file").unwrap_or("index.html");

    Ok(Response::new(StatusCode::OK)
        .header("Content-Type", guess_content_type(file_path))
        .body(format!(
            "Static file content for: {} (auto-extracted)",
            file_path
        )))
}

async fn serve_assets_handler(req: Request) -> Result<Response> {
    let asset_path = req.param("path").unwrap_or("");

    Ok(Response::new(StatusCode::OK)
        .header("Content-Type", guess_content_type(asset_path))
        .body(format!(
            "Asset content for: {} (auto-extracted)",
            asset_path
        )))
}

async fn get_user_profile_handler(req: Request) -> Result<Response> {
    let user_id = req.param("id").unwrap_or("unknown");

    Response::json(&serde_json::json!({
        "user_id": user_id,
        "profile": {
            "bio": "Software developer",
            "location": "San Francisco",
            "website": "https://example.com"
        },
        "auto_extracted": true
    }))
}

// Helper functions

fn guess_content_type(file_path: &str) -> &'static str {
    if file_path.ends_with(".html") {
        "text/html"
    } else if file_path.ends_with(".css") {
        "text/css"
    } else if file_path.ends_with(".js") {
        "application/javascript"
    } else if file_path.ends_with(".png") {
        "image/png"
    } else if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

fn demonstrate_route_matching() {
    use web_server_abstraction::core::Route;
    use web_server_abstraction::types::HttpMethod;

    println!("\n🔍 Route Matching Demonstration:");

    // Create some test routes
    let user_route = Route::new("/users/:id", HttpMethod::GET, |_| async {
        Ok(Response::new(StatusCode::OK))
    });
    let static_route = Route::new("/static/*file", HttpMethod::GET, |_| async {
        Ok(Response::new(StatusCode::OK))
    });
    let complex_route = Route::new(
        "/api/v1/users/:user_id/posts/:post_id",
        HttpMethod::GET,
        |_| async { Ok(Response::new(StatusCode::OK)) },
    );

    // Test route matching
    let test_cases = vec![
        ("/users/123", &user_route),
        ("/static/css/style.css", &static_route),
        ("/api/v1/users/456/posts/789", &complex_route),
    ];

    for (path, route) in test_cases {
        let matches = route.matches(path);
        let params = route.extract_params(path);
        println!(
            "  Path: {} -> Route: {} (matches: {}, params: {:?})",
            path, route.path, matches, params
        );
    }
    println!();
}
