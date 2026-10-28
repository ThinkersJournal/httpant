//! Axum framework adapter.

use crate::core::{HandlerFn, Middleware};
use crate::error::{Result, WebServerError};
use crate::types::{Body, Headers, HttpMethod, Request, Response, StatusCode};
use axum::{
    Router,
    http::{self, HeaderMap, Method, StatusCode as AxumStatusCode},
    response::IntoResponse,
    routing::{MethodRouter, delete, get, head, options, patch, post, put},
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

/// Axum framework adapter
pub struct AxumAdapter {
    router: Router,
    middleware: Vec<Box<dyn Middleware>>,
    addr: Option<SocketAddr>,
}

impl Default for AxumAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AxumAdapter {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            middleware: Vec::new(),
            addr: None,
        }
    }

    /// Bind the server to an address
    pub async fn bind(&mut self, addr: &str) -> Result<()> {
        let socket_addr: SocketAddr = addr
            .parse()
            .map_err(|e| WebServerError::bind_error(format!("Invalid address {}: {}", addr, e)))?;
        self.addr = Some(socket_addr);
        Ok(())
    }

    /// Run the server
    pub async fn run(self) -> Result<()> {
        let addr = self
            .addr
            .ok_or_else(|| WebServerError::bind_error("No address bound"))?;

        // Apply middleware and tracing to the router
        let app = self
            .router
            .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()));

        println!("Axum server starting on {}", addr);

        let listener = TcpListener::bind(addr).await.map_err(|e| {
            WebServerError::adapter_error(format!("Failed to bind listener: {}", e))
        })?;

        axum::serve(listener, app)
            .await
            .map_err(|e| WebServerError::adapter_error(format!("Axum server error: {}", e)))?;

        Ok(())
    }

    /// Add a route to the server
    pub fn route(&mut self, path: &str, method: HttpMethod, handler: HandlerFn) -> &mut Self {
        // Create an adapter that converts between Axum and our types
        let handler = Arc::new(handler);
        let adapter_handler = {
            // Clone the Arc for the closure
            let handler_clone = Arc::clone(&handler);
            move |req: axum::extract::Request| {
                let handler = Arc::clone(&handler_clone);
                async move {
                    // Convert Axum request to our Request type
                    let our_request = match convert_axum_request_to_ours(req).await {
                        Ok(req) => req,
                        Err(e) => {
                            return (
                                AxumStatusCode::BAD_REQUEST,
                                format!("Request conversion error: {}", e),
                            )
                                .into_response();
                        }
                    };

                    // Call handler directly (middleware would be applied at server level)
                    let result = handler(our_request).await;

                    // Convert our Response to Axum response
                    match result {
                        Ok(response) => {
                            convert_our_response_to_axum(response).await.into_response()
                        }
                        Err(e) => (
                            AxumStatusCode::INTERNAL_SERVER_ERROR,
                            format!("Handler error: {}", e),
                        )
                            .into_response(),
                    }
                }
            }
        };

        let method_router: MethodRouter = match method {
            HttpMethod::GET => get(adapter_handler),
            HttpMethod::POST => post(adapter_handler),
            HttpMethod::PUT => put(adapter_handler),
            HttpMethod::DELETE => delete(adapter_handler),
            HttpMethod::PATCH => patch(adapter_handler),
            HttpMethod::HEAD => head(adapter_handler),
            HttpMethod::OPTIONS => options(adapter_handler),
            _ => get(adapter_handler),
        };

        self.router = self.router.clone().route(path, method_router);
        println!("Added Axum route: {:?} {}", method, path);
        self
    }

    /// Add middleware to the server
    pub fn middleware(&mut self, middleware: Box<dyn Middleware>) -> &mut Self {
        self.middleware.push(middleware);
        println!("Added middleware to Axum adapter");
        self
    }
}

/// Convert Axum request to our Request type
async fn convert_axum_request_to_ours(req: axum::extract::Request) -> Result<Request> {
    let (parts, body) = req.into_parts();

    // Convert method
    let method = match parts.method {
        Method::GET => HttpMethod::GET,
        Method::POST => HttpMethod::POST,
        Method::PUT => HttpMethod::PUT,
        Method::DELETE => HttpMethod::DELETE,
        Method::PATCH => HttpMethod::PATCH,
        Method::HEAD => HttpMethod::HEAD,
        Method::OPTIONS => HttpMethod::OPTIONS,
        _ => HttpMethod::GET, // Default fallback
    };

    // Convert URI directly from the Axum request
    let uri = parts.uri;

    // Convert headers
    let mut headers = Headers::new();
    for (name, value) in parts.headers.iter() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Convert body
    let body_bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| WebServerError::adapter_error(format!("Failed to read body: {}", e)))?;
    let body = Body::from(body_bytes.to_vec());

    // Initialize extensions and other fields
    let extensions = HashMap::new();
    let path_params = HashMap::new();
    let cookies = HashMap::new();
    let form_data = None;
    let multipart = None;

    Ok(Request {
        method,
        uri,
        version: parts.version, // Use the actual version from the request
        headers,
        body,
        extensions,
        path_params,
        cookies,
        form_data,
        multipart,
    })
}

/// Convert our Response to Axum response format
async fn convert_our_response_to_axum(response: Response) -> impl IntoResponse {
    let mut header_map = HeaderMap::new();

    // Convert headers
    for (name, value) in response.headers.iter() {
        if let (Ok(header_name), Ok(header_value)) = (
            name.parse::<http::HeaderName>(),
            value.parse::<http::HeaderValue>(),
        ) {
            header_map.insert(header_name, header_value);
        }
    }

    // Convert status code
    let axum_status = match response.status {
        StatusCode::OK => AxumStatusCode::OK,
        StatusCode::CREATED => AxumStatusCode::CREATED,
        StatusCode::NOT_FOUND => AxumStatusCode::NOT_FOUND,
        StatusCode::INTERNAL_SERVER_ERROR => AxumStatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_REQUEST => AxumStatusCode::BAD_REQUEST,
        StatusCode::UNAUTHORIZED => AxumStatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN => AxumStatusCode::FORBIDDEN,
        StatusCode::NO_CONTENT => AxumStatusCode::NO_CONTENT,
        _ => AxumStatusCode::OK, // Default fallback
    };

    // Convert body to bytes
    let body_bytes = response.body.bytes().await.unwrap_or_default();

    (axum_status, header_map, body_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Body, Headers, StatusCode};

    #[tokio::test]
    async fn test_axum_adapter_creation() {
        let adapter = AxumAdapter::new();
        assert!(adapter.middleware.is_empty());
        assert!(adapter.addr.is_none());
    }

    #[tokio::test]
    async fn test_axum_adapter_bind() {
        let mut adapter = AxumAdapter::new();
        let result = adapter.bind("127.0.0.1:0").await;
        assert!(result.is_ok());
        assert!(adapter.addr.is_some());
    }

    #[tokio::test]
    async fn test_response_conversion() {
        let response = Response {
            status: StatusCode::OK,
            headers: {
                let mut h = Headers::new();
                h.insert("content-type".to_string(), "application/json".to_string());
                h
            },
            body: Body::from("test response"),
        };

        let _axum_response = convert_our_response_to_axum(response).await;
        // The conversion should succeed without panicking
        // More detailed testing would require integration with Axum's test framework
    }
}
