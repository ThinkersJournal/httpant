//! Core traits and types for the web server abstraction.

use crate::error::Result;
use crate::types::{HttpMethod, Request, Response};
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed future that returns a `Result<Response>`
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A handler function type that takes a request and returns a response
pub type HandlerFn = Arc<dyn Fn(Request) -> BoxFuture<Result<Response>> + Send + Sync>;

/// Trait for middleware components
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Process a request through the middleware
    async fn call(&self, req: Request, next: Next) -> Result<Response>;
}

/// Represents the next middleware in the chain
pub struct Next {
    handler: HandlerFn,
}

impl Next {
    pub fn new(handler: HandlerFn) -> Self {
        Self { handler }
    }

    pub async fn run(self, req: Request) -> Result<Response> {
        (self.handler)(req).await
    }
}

/// A handler that can be converted to a HandlerFn
pub trait Handler<T>: Clone + Send + Sync + 'static {
    fn into_handler(self) -> HandlerFn;
}

/// Implementation for async functions that take a Request and return `Result<Response>`
impl<F, Fut> Handler<()> for F
where
    F: Fn(Request) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Response>> + Send + 'static,
{
    fn into_handler(self) -> HandlerFn {
        Arc::new(move |req| {
            let handler = self.clone();
            Box::pin(async move { handler(req).await })
        })
    }
}

/// Route definition with advanced routing features
pub struct Route {
    pub path: String,
    pub method: HttpMethod,
    pub handler: HandlerFn,
    /// Indicates if this route uses wildcards or path parameters
    pub is_dynamic: bool,
}

impl Route {
    pub fn new<H, T>(path: impl Into<String>, method: HttpMethod, handler: H) -> Self
    where
        H: Handler<T>,
    {
        let path_str = path.into();
        let is_dynamic = path_str.contains(':') || path_str.contains('*');

        Self {
            path: path_str,
            method,
            handler: handler.into_handler(),
            is_dynamic,
        }
    }

    /// Check if this route matches a given path
    pub fn matches(&self, path: &str) -> bool {
        if !self.is_dynamic {
            return self.path == path;
        }

        self.match_dynamic_path(path).is_some()
    }

    /// Extract path parameters from a matching path
    pub fn extract_params(&self, path: &str) -> std::collections::HashMap<String, String> {
        self.match_dynamic_path(path).unwrap_or_default()
    }

    /// Match dynamic path patterns
    fn match_dynamic_path(&self, path: &str) -> Option<std::collections::HashMap<String, String>> {
        let route_parts: Vec<&str> = self.path.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if route_parts.len() != path_parts.len() {
            // Handle wildcard at the end
            if let Some(last_part) = route_parts.last()
                && last_part.starts_with('*')
                && route_parts.len() <= path_parts.len()
            {
                // Wildcard matches remaining path
                let mut params = std::collections::HashMap::new();
                let param_name = last_part.trim_start_matches('*');
                if !param_name.is_empty() {
                    let remaining_path = path_parts[route_parts.len() - 1..].join("/");
                    params.insert(param_name.to_string(), remaining_path);
                }
                return Some(params);
            }
            return None;
        }

        let mut params = std::collections::HashMap::new();

        for (route_part, path_part) in route_parts.iter().zip(path_parts.iter()) {
            if route_part.starts_with(':') {
                // Path parameter
                let param_name = route_part.trim_start_matches(':');
                params.insert(param_name.to_string(), path_part.to_string());
            } else if route_part.starts_with('*') {
                // Wildcard
                let param_name = route_part.trim_start_matches('*');
                if !param_name.is_empty() {
                    params.insert(param_name.to_string(), path_part.to_string());
                }
            } else if route_part != path_part {
                // Exact match required
                return None;
            }
        }

        Some(params)
    }
}

// Since we can't use trait objects easily, we'll use an enum for different adapters
pub enum AdapterType {
    Mock(crate::adapters::mock::MockAdapter),
    #[cfg(feature = "axum")]
    Axum(crate::adapters::axum::AxumAdapter),
    #[cfg(feature = "actix-web")]
    ActixWeb(crate::adapters::actix_web::ActixWebAdapter),
    #[cfg(feature = "warp")]
    Warp(crate::adapters::warp::WarpAdapter),
    #[cfg(feature = "rocket")]
    Rocket(crate::adapters::rocket::RocketAdapter),
    #[cfg(feature = "salvo")]
    Salvo(crate::adapters::salvo::SalvoAdapter),
    #[cfg(feature = "poem")]
    Poem(crate::adapters::poem::PoemAdapter),
}

/// Main web server struct that uses an adapter
pub struct WebServer {
    adapter: AdapterType,
    routes: Vec<Route>,
    middleware: Vec<Box<dyn Middleware>>,
}

impl Default for WebServer {
    fn default() -> Self {
        Self::new()
    }
}

impl WebServer {
    /// Create a new web server with the mock adapter (for testing)
    pub fn new() -> Self {
        Self::with_mock_adapter()
    }

    /// Create a new web server with the mock adapter
    pub fn with_mock_adapter() -> Self {
        Self {
            adapter: AdapterType::Mock(crate::adapters::mock::MockAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Create a new web server with the Axum adapter
    #[cfg(feature = "axum")]
    pub fn with_axum_adapter() -> Self {
        Self {
            adapter: AdapterType::Axum(crate::adapters::axum::AxumAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Create a new web server with the Actix-Web adapter
    #[cfg(feature = "actix-web")]
    pub fn with_actix_web_adapter() -> Self {
        Self {
            adapter: AdapterType::ActixWeb(crate::adapters::actix_web::ActixWebAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Create a new web server with the Warp adapter
    #[cfg(feature = "warp")]
    pub fn with_warp_adapter() -> Self {
        Self {
            adapter: AdapterType::Warp(crate::adapters::warp::WarpAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    // Note: The following adapter constructors are work-in-progress:

    /// Create a new web server with the Rocket adapter
    #[cfg(feature = "rocket")]
    pub fn with_rocket_adapter() -> Self {
        Self {
            adapter: AdapterType::Rocket(crate::adapters::rocket::RocketAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Create a new web server with the Salvo adapter
    #[cfg(feature = "salvo")]
    pub fn with_salvo_adapter() -> Self {
        Self {
            adapter: AdapterType::Salvo(crate::adapters::salvo::SalvoAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Create a new web server with the Poem adapter
    #[cfg(feature = "poem")]
    pub fn with_poem_adapter() -> Self {
        Self {
            adapter: AdapterType::Poem(crate::adapters::poem::PoemAdapter::new()),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Add a route to the server
    pub fn route<H, T>(mut self, path: impl Into<String>, method: HttpMethod, handler: H) -> Self
    where
        H: Handler<T>,
    {
        let route = Route::new(path, method, handler);
        self.routes.push(route);
        self
    }

    /// Add middleware to the server
    pub fn middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware + 'static,
    {
        self.middleware.push(Box::new(middleware));
        self
    }

    /// Enable automatic path parameter extraction
    /// This adds middleware that automatically extracts path parameters
    /// from routes and makes them available via req.param()
    pub fn with_path_params(mut self) -> Self {
        // Extract route patterns for parameter matching
        let route_patterns: Vec<(String, HttpMethod)> = self
            .routes
            .iter()
            .map(|r| (r.path.clone(), r.method))
            .collect();

        let param_middleware = crate::middleware::PathParameterMiddleware::new(route_patterns);
        self.middleware.push(Box::new(param_middleware));
        self
    }

    /// Add a WebSocket route to the server
    /// Note: This is a basic implementation. Full WebSocket support requires
    /// framework-specific handling in each adapter.
    pub fn websocket(mut self, path: impl Into<String>) -> Self {
        // Add a placeholder route that indicates WebSocket upgrade capability
        let websocket_handler = |req: crate::types::Request| async move {
            // Check if this is a WebSocket upgrade request
            if req
                .headers
                .get("Upgrade")
                .is_some_and(|v| v.to_lowercase() == "websocket")
            {
                // Create proper WebSocket upgrade with correct accept key
                match crate::types::WebSocketUpgrade::from_request(req) {
                    Ok(upgrade) => {
                        let accept_key = upgrade.generate_accept_key();
                        Ok(crate::types::Response::new(
                            crate::types::StatusCode::SWITCHING_PROTOCOLS,
                        )
                        .header("Upgrade", "websocket")
                        .header("Connection", "Upgrade")
                        .header("Sec-WebSocket-Accept", accept_key))
                    }
                    Err(e) => Err(e),
                }
            } else {
                Err(crate::error::WebServerError::custom(
                    "Not a WebSocket upgrade request",
                ))
            }
        };

        self.routes.push(Route::new(
            path,
            crate::types::HttpMethod::GET,
            websocket_handler,
        ));
        self
    }

    /// Convenience method for GET routes
    pub fn get<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::GET, handler)
    }

    /// Convenience method for POST routes
    pub fn post<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::POST, handler)
    }

    /// Convenience method for PUT routes
    pub fn put<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::PUT, handler)
    }

    /// Convenience method for DELETE routes
    pub fn delete<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::DELETE, handler)
    }

    /// Convenience method for PATCH routes
    pub fn patch<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::PATCH, handler)
    }

    /// Convenience method for HEAD routes
    pub fn head<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::HEAD, handler)
    }

    /// Convenience method for OPTIONS routes
    pub fn options<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::OPTIONS, handler)
    }

    /// Convenience method for TRACE routes
    pub fn trace<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::TRACE, handler)
    }

    /// Convenience method for CONNECT routes
    pub fn connect<H, T>(self, path: impl Into<String>, handler: H) -> Self
    where
        H: Handler<T>,
    {
        self.route(path, crate::types::HttpMethod::CONNECT, handler)
    }

    /// Add a route with path parameters (e.g., "/users/:id")
    pub fn param_route<H, T>(self, path: impl Into<String>, method: HttpMethod, handler: H) -> Self
    where
        H: Handler<T>,
    {
        // This is the same as route() but with clear intention for parameterized paths
        self.route(path, method, handler)
    }

    /// Add a wildcard route (e.g., "/static/*file")
    pub fn wildcard_route<H, T>(
        self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: H,
    ) -> Self
    where
        H: Handler<T>,
    {
        // This is the same as route() but with clear intention for wildcard paths
        self.route(path, method, handler)
    }

    /// Bind the server to an address
    pub async fn bind(mut self, addr: &str) -> Result<BoundServer> {
        // Apply all routes and middleware to the adapter
        match &mut self.adapter {
            AdapterType::Mock(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "axum")]
            AdapterType::Axum(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "actix-web")]
            AdapterType::ActixWeb(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "warp")]
            AdapterType::Warp(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "rocket")]
            AdapterType::Rocket(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "salvo")]
            AdapterType::Salvo(adapter) => {
                for route in self.routes {
                    let _ = adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    let _ = adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
            #[cfg(feature = "poem")]
            AdapterType::Poem(adapter) => {
                for route in self.routes {
                    adapter.route(&route.path, route.method, route.handler);
                }
                for middleware in self.middleware {
                    adapter.middleware(middleware);
                }
                adapter.bind(addr).await?;
            }
        }

        Ok(BoundServer {
            adapter: self.adapter,
        })
    }
}

/// A bound server ready to run
pub struct BoundServer {
    adapter: AdapterType,
}

impl BoundServer {
    /// Run the server
    pub async fn run(self) -> Result<()> {
        match self.adapter {
            AdapterType::Mock(adapter) => adapter.run().await,
            #[cfg(feature = "axum")]
            AdapterType::Axum(adapter) => adapter.run().await,
            #[cfg(feature = "actix-web")]
            AdapterType::ActixWeb(adapter) => adapter.run().await,
            #[cfg(feature = "warp")]
            AdapterType::Warp(adapter) => adapter.run().await,
            #[cfg(feature = "rocket")]
            AdapterType::Rocket(adapter) => adapter.run().await,
            #[cfg(feature = "salvo")]
            AdapterType::Salvo(adapter) => adapter.run().await,
            #[cfg(feature = "poem")]
            AdapterType::Poem(adapter) => adapter.run().await,
        }
    }
}

// Temporarily disabled until tests are updated
// #[cfg(test)]
// mod tests;
