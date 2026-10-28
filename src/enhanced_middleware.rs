//! Enhanced Middleware Integration
//!
//! This module provides a comprehensive middleware system with built-in middleware
//! for common web server needs including CORS, compression, rate limiting, and more.

use crate::{
    config::{CompressionConfig, CorsConfig, SecurityConfig},
    core::{Middleware, Next},
    error::{Result, WebServerError},
    types::{Request, Response},
};
use async_trait::async_trait;
use bytes::Bytes;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Enhanced middleware trait with configuration support
#[async_trait]
pub trait EnhancedMiddleware: Send + Sync {
    /// Process a request before it reaches the handler
    async fn before_request(&self, request: &mut Request) -> Result<Option<Response>>;

    /// Process a response after the handler
    async fn after_response(&self, response: &mut Response) -> Result<()>;

    /// Get middleware name for debugging/logging
    fn name(&self) -> &'static str;

    /// Check if middleware is enabled
    fn is_enabled(&self) -> bool {
        true
    }
}

/// CORS middleware with configurable options
pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    /// Check if origin is allowed
    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.config.allowed_origins.contains(&"*".to_string())
            || self.config.allowed_origins.contains(&origin.to_string())
    }

    /// Get allowed methods as a string
    fn get_allowed_methods(&self) -> String {
        self.config.allowed_methods.join(", ")
    }

    /// Get allowed headers as a string
    fn get_allowed_headers(&self) -> String {
        self.config.allowed_headers.join(", ")
    }
}

#[async_trait]
impl EnhancedMiddleware for CorsMiddleware {
    async fn before_request(&self, request: &mut Request) -> Result<Option<Response>> {
        if !self.config.enabled {
            return Ok(None);
        }

        // Handle preflight requests
        if request.method == crate::types::HttpMethod::OPTIONS {
            let origin = request.headers.get("origin").cloned().unwrap_or_default();

            if self.is_origin_allowed(&origin) {
                let mut response = Response::new(crate::types::StatusCode::OK);

                // Add CORS headers
                response
                    .headers
                    .insert("Access-Control-Allow-Origin".to_string(), origin);
                response.headers.insert(
                    "Access-Control-Allow-Methods".to_string(),
                    self.get_allowed_methods(),
                );
                response.headers.insert(
                    "Access-Control-Allow-Headers".to_string(),
                    self.get_allowed_headers(),
                );

                if self.config.max_age > 0 {
                    response.headers.insert(
                        "Access-Control-Max-Age".to_string(),
                        self.config.max_age.to_string(),
                    );
                }

                return Ok(Some(response));
            }
        }

        Ok(None)
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Add CORS headers to all responses
        if let Some(origin) = response.headers.get("origin") {
            if self.is_origin_allowed(origin) {
                response
                    .headers
                    .insert("Access-Control-Allow-Origin".to_string(), origin.clone());
            }
        } else if self.config.allowed_origins.contains(&"*".to_string()) {
            response
                .headers
                .insert("Access-Control-Allow-Origin".to_string(), "*".to_string());
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "CORS"
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Compression middleware with configurable compression levels
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Check if request accepts gzip compression
    #[allow(dead_code)]
    fn accepts_gzip(&self, request: &Request) -> bool {
        if let Some(accept_encoding) = request.headers.get("accept-encoding") {
            accept_encoding.contains("gzip")
        } else {
            false
        }
    }

    /// Compress data using gzip
    fn compress_gzip(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::new(6)); // Default compression level
        encoder
            .write_all(data)
            .map_err(|e| WebServerError::custom(format!("Compression failed: {}", e)))?;
        encoder
            .finish()
            .map_err(|e| WebServerError::custom(format!("Compression finish failed: {}", e)))
    }
}

#[async_trait]
impl EnhancedMiddleware for CompressionMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<Option<Response>> {
        // Compression is applied in after_response
        Ok(None)
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Get response body
        let body_bytes = response.body.bytes().await?;

        // Only compress if body is large enough
        if body_bytes.len() < self.config.min_size {
            return Ok(());
        }

        // Check if content is already compressed
        if response.headers.get("content-encoding").is_some() {
            return Ok(());
        }

        // Compress the body
        let compressed = self.compress_gzip(&body_bytes)?;

        // Update response
        response.body = crate::types::Body::from_bytes(Bytes::from(compressed));
        response
            .headers
            .insert("Content-Encoding".to_string(), "gzip".to_string());
        response.headers.insert(
            "Content-Length".to_string(),
            response.body.bytes().await?.len().to_string(),
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Compression"
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[async_trait]
impl Middleware for CompressionMiddleware {
    async fn call(&self, req: Request, next: Next) -> Result<Response> {
        // Call the next middleware/handler
        let mut response = next.run(req).await?;

        // Apply compression to the response
        if self.is_enabled() {
            self.after_response(&mut response).await?;
        }

        Ok(response)
    }
}

/// Rate limiting middleware
pub struct RateLimitMiddleware {
    config: SecurityConfig,
    requests: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl RateLimitMiddleware {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            config,
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if request is rate limited
    fn is_rate_limited(&self, client_ip: &str) -> bool {
        let rate_limit = match self.config.rate_limit_per_minute {
            Some(limit) => limit,
            None => return false, // No rate limiting
        };

        let mut requests = self.requests.lock().unwrap();
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        // Get or create request history for this IP
        let client_requests = requests.entry(client_ip.to_string()).or_default();

        // Remove requests older than 1 minute
        client_requests.retain(|&request_time| request_time > one_minute_ago);

        // Check if rate limit exceeded
        if client_requests.len() >= rate_limit as usize {
            return true;
        }

        // Add current request
        client_requests.push(now);
        false
    }

    /// Get client IP from request
    fn get_client_ip(&self, request: &Request) -> String {
        // Try various headers for client IP
        if let Some(forwarded) = request.headers.get("x-forwarded-for")
            && let Some(ip) = forwarded.split(',').next()
        {
            return ip.trim().to_string();
        }

        if let Some(real_ip) = request.headers.get("x-real-ip") {
            return real_ip.clone();
        }

        // Fallback to "unknown" (in production you'd get this from the connection)
        "unknown".to_string()
    }
}

#[async_trait]
impl EnhancedMiddleware for RateLimitMiddleware {
    async fn before_request(&self, request: &mut Request) -> Result<Option<Response>> {
        if self.config.rate_limit_per_minute.is_none() {
            return Ok(None);
        }

        let client_ip = self.get_client_ip(request);

        if self.is_rate_limited(&client_ip) {
            let mut response = Response::new(crate::types::StatusCode::TOO_MANY_REQUESTS);
            response
                .headers
                .insert("Retry-After".to_string(), "60".to_string());
            response.body = crate::types::Body::from_string("Rate limit exceeded");
            return Ok(Some(response));
        }

        Ok(None)
    }

    async fn after_response(&self, _response: &mut Response) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "RateLimit"
    }

    fn is_enabled(&self) -> bool {
        self.config.rate_limit_per_minute.is_some()
    }
}

/// Security headers middleware
pub struct SecurityHeadersMiddleware {
    config: SecurityConfig,
}

impl SecurityHeadersMiddleware {
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl EnhancedMiddleware for SecurityHeadersMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<Option<Response>> {
        Ok(None)
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        // Add security headers
        response
            .headers
            .insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
        response
            .headers
            .insert("X-Frame-Options".to_string(), "DENY".to_string());
        response
            .headers
            .insert("X-XSS-Protection".to_string(), "1; mode=block".to_string());
        response.headers.insert(
            "Referrer-Policy".to_string(),
            "strict-origin-when-cross-origin".to_string(),
        );

        // Add HSTS header if TLS is enabled
        if self.config.tls.enabled {
            response.headers.insert(
                "Strict-Transport-Security".to_string(),
                "max-age=31536000; includeSubDomains".to_string(),
            );
        }

        // Add CSP header if CSRF protection is enabled
        if self.config.enable_csrf_protection {
            response.headers.insert(
                "Content-Security-Policy".to_string(),
                "default-src 'self'".to_string(),
            );
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "SecurityHeaders"
    }
}

/// Middleware stack manager
pub struct MiddlewareStack {
    middlewares: Vec<Box<dyn EnhancedMiddleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add middleware to the stack
    pub fn add_middleware(&mut self, middleware: Box<dyn EnhancedMiddleware>) {
        self.middlewares.push(middleware);
    }

    /// Create a default middleware stack from configuration
    pub fn from_config(
        cors_config: CorsConfig,
        compression_config: CompressionConfig,
        security_config: SecurityConfig,
    ) -> Self {
        let mut stack = Self::new();

        // Add middlewares in order of execution
        stack.add_middleware(Box::new(SecurityHeadersMiddleware::new(
            security_config.clone(),
        )));
        stack.add_middleware(Box::new(RateLimitMiddleware::new(security_config)));
        stack.add_middleware(Box::new(CorsMiddleware::new(cors_config)));
        stack.add_middleware(Box::new(CompressionMiddleware::new(compression_config)));

        stack
    }

    /// Process request through all middleware
    pub async fn process_request(&self, request: &mut Request) -> Result<Option<Response>> {
        for middleware in &self.middlewares {
            if !middleware.is_enabled() {
                continue;
            }

            if let Some(response) = middleware.before_request(request).await? {
                return Ok(Some(response));
            }
        }
        Ok(None)
    }

    /// Process response through all middleware (in reverse order)
    pub async fn process_response(&self, response: &mut Response) -> Result<()> {
        for middleware in self.middlewares.iter().rev() {
            if !middleware.is_enabled() {
                continue;
            }

            middleware.after_response(response).await?;
        }
        Ok(())
    }

    /// Get list of enabled middlewares
    pub fn get_enabled_middlewares(&self) -> Vec<&'static str> {
        self.middlewares
            .iter()
            .filter(|m| m.is_enabled())
            .map(|m| m.name())
            .collect()
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Headers, HttpMethod, StatusCode};

    #[tokio::test]
    async fn test_cors_middleware() {
        let config = CorsConfig {
            enabled: true,
            allowed_origins: vec!["https://example.com".to_string()],
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            allowed_headers: vec!["content-type".to_string()],
            credentials: false,
            max_age: 3600,
        };

        let middleware = CorsMiddleware::new(config);

        // Test preflight request
        let mut request = Request {
            method: HttpMethod::OPTIONS,
            uri: http::Uri::from_static("https://example.com/test"),
            version: http::Version::HTTP_11,
            headers: {
                let mut headers = Headers::new();
                headers.insert("origin".to_string(), "https://example.com".to_string());
                headers
            },
            body: crate::types::Body::empty(),
            extensions: std::collections::HashMap::new(),
            path_params: std::collections::HashMap::new(),
            cookies: std::collections::HashMap::new(),
            form_data: None,
            multipart: None,
        };

        let response = middleware.before_request(&mut request).await.unwrap();
        assert!(response.is_some());

        let response = response.unwrap();
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            response.headers.get("Access-Control-Allow-Origin"),
            Some(&"https://example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_rate_limit_middleware() {
        let config = SecurityConfig {
            rate_limit_per_minute: Some(2),
            ..Default::default()
        };

        let middleware = RateLimitMiddleware::new(config);

        let mut request = Request {
            method: HttpMethod::GET,
            uri: http::Uri::from_static("https://example.com/test"),
            version: http::Version::HTTP_11,
            headers: {
                let mut headers = Headers::new();
                headers.insert("x-forwarded-for".to_string(), "192.168.1.1".to_string());
                headers
            },
            body: crate::types::Body::empty(),
            extensions: std::collections::HashMap::new(),
            path_params: std::collections::HashMap::new(),
            cookies: std::collections::HashMap::new(),
            form_data: None,
            multipart: None,
        };

        // First request should pass
        let response1 = middleware.before_request(&mut request).await.unwrap();
        assert!(response1.is_none());

        // Second request should pass
        let response2 = middleware.before_request(&mut request).await.unwrap();
        assert!(response2.is_none());

        // Third request should be rate limited
        let response3 = middleware.before_request(&mut request).await.unwrap();
        assert!(response3.is_some());

        let response = response3.unwrap();
        assert_eq!(response.status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_middleware_stack() {
        let cors_config = CorsConfig::default();
        let compression_config = CompressionConfig::default();
        let security_config = SecurityConfig::default();

        let stack = MiddlewareStack::from_config(cors_config, compression_config, security_config);

        let enabled = stack.get_enabled_middlewares();
        assert!(enabled.contains(&"SecurityHeaders"));
        assert!(enabled.contains(&"CORS"));
        assert!(enabled.contains(&"Compression"));
    }
}
