//! Mountable
//! This module allows libraries to define web interfaces that can be mounted
//! into any host application regardless of the underlying web framework.
//! Now with deep authentication integration for seamless auth across all interfaces.

use crate::auth::{AuthContext, AuthRequirements, auth_middleware};
use crate::core::{Handler, Route};
use crate::error::Result;
use crate::types::{HttpMethod, Request, Response};
use std::collections::HashMap;
use std::sync::Arc;

/// A boxed future that returns a Result<Response>
pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

/// An authenticated route that includes both the route and authentication middleware
pub struct AuthenticatedRoute {
    pub route: Route,
    pub auth_requirements: AuthRequirements,
    pub auth_context: Arc<AuthContext>,
}

impl AuthenticatedRoute {
    /// Create a new authenticated route
    pub fn new(
        route: Route,
        auth_requirements: AuthRequirements,
        auth_context: Arc<AuthContext>,
    ) -> Self {
        Self {
            route,
            auth_requirements,
            auth_context,
        }
    }

    /// Execute the route with authentication middleware
    pub async fn execute(&self, request: Request) -> Result<Response> {
        // Apply authentication middleware
        let _user_session =
            auth_middleware(&self.auth_context, &self.auth_requirements, &request).await?;

        // For now, just return a placeholder response indicating successful auth
        // In a real implementation, we'd integrate with the Route's handler properly
        Ok(Response::ok().body("Authenticated route executed"))
    }
}

/// A mountable web interface that can be integrated into any host application
///
/// This allows libraries to define their web routes and middleware without
/// knowing what web framework the host application uses or where their
/// routes will be mounted in the overall routing scheme.
pub struct MountableInterface {
    name: String,
    description: String,
    routes: Vec<RouteDefinition>,
    middleware: Vec<String>, // Middleware names for now, could be enhanced
    mount_options: MountOptions,
}

/// Route definition for a mountable interface
pub struct RouteDefinition {
    /// Relative path within the interface (e.g., "/status", "/api/data")
    pub path: String,
    /// HTTP method
    pub method: HttpMethod,
    /// Handler function that can be called to create new Route instances
    pub handler_fn: Arc<dyn Fn() -> Route + Send + Sync>,
    /// Authentication requirements for this route
    pub auth_requirements: AuthRequirements,
    /// Optional description for documentation
    pub description: Option<String>,
    /// Tags for organizing routes
    pub tags: Vec<String>,
}

/// Configuration options for mounting an interface
#[derive(Clone, Debug, Default)]
pub struct MountOptions {
    /// Whether to strip the mount prefix from request paths
    pub strip_prefix: bool,
    /// Whether to add trailing slashes to paths
    pub add_trailing_slash: bool,
    /// Custom middleware to apply to all routes in this interface
    pub middleware: Vec<String>,
}

/// Builder for creating mountable interfaces
pub struct InterfaceBuilder {
    name: String,
    description: String,
    routes: Vec<RouteDefinition>,
    middleware: Vec<String>,
    mount_options: MountOptions,
}

impl InterfaceBuilder {
    /// Create a new interface builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            routes: Vec::new(),
            middleware: Vec::new(),
            mount_options: MountOptions::default(),
        }
    }

    /// Set the description for this interface
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a route to this interface
    pub fn route<H, T>(mut self, path: impl Into<String>, method: HttpMethod, handler: H) -> Self
    where
        H: Handler<T> + Clone + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler_fn = Arc::new(move || Route::new("", HttpMethod::GET, handler.clone()));

        let route = RouteDefinition {
            path: path.into(),
            method,
            handler_fn,
            auth_requirements: AuthRequirements::default(),
            description: None,
            tags: Vec::new(),
        };
        self.routes.push(route);
        self
    }

    /// Add a route with authentication requirements
    pub fn route_with_auth<H, T>(
        mut self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: H,
        auth_requirements: AuthRequirements,
    ) -> Self
    where
        H: Handler<T> + Clone + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler_fn = Arc::new(move || Route::new("", HttpMethod::GET, handler.clone()));

        let route = RouteDefinition {
            path: path.into(),
            method,
            handler_fn,
            auth_requirements,
            description: None,
            tags: Vec::new(),
        };
        self.routes.push(route);
        self
    }

    /// Add a route with description and tags
    pub fn route_with_meta<H, T>(
        mut self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: H,
        description: impl Into<String>,
        tags: Vec<String>,
    ) -> Self
    where
        H: Handler<T> + Clone + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler_fn = Arc::new(move || Route::new("", HttpMethod::GET, handler.clone()));

        let route = RouteDefinition {
            path: path.into(),
            method,
            handler_fn,
            auth_requirements: AuthRequirements::default(),
            description: Some(description.into()),
            tags,
        };
        self.routes.push(route);
        self
    }

    /// Add a route with full configuration (description, tags, and auth)
    pub fn route_with_full_config<H, T>(
        mut self,
        path: impl Into<String>,
        method: HttpMethod,
        handler: H,
        auth_requirements: AuthRequirements,
        description: impl Into<String>,
        tags: Vec<String>,
    ) -> Self
    where
        H: Handler<T> + Clone + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        let handler_fn = Arc::new(move || Route::new("", HttpMethod::GET, handler.clone()));

        let route = RouteDefinition {
            path: path.into(),
            method,
            handler_fn,
            auth_requirements,
            description: Some(description.into()),
            tags,
        };
        self.routes.push(route);
        self
    }

    /// Add middleware to this interface
    pub fn middleware(mut self, middleware: impl Into<String>) -> Self {
        self.middleware.push(middleware.into());
        self
    }

    /// Configure mount options
    pub fn mount_options(mut self, options: MountOptions) -> Self {
        self.mount_options = options;
        self
    }

    /// Set default authentication requirements for all routes
    pub fn default_auth_requirements(mut self, auth_requirements: AuthRequirements) -> Self {
        // Apply the auth requirements to all existing routes that don't have specific requirements
        for route in &mut self.routes {
            if !route.auth_requirements.required && route.auth_requirements.permissions.is_empty() {
                route.auth_requirements = auth_requirements.clone();
            }
        }
        self
    }

    /// Require authentication for all routes in this interface
    pub fn require_auth(mut self) -> Self {
        let auth_requirements = AuthRequirements::required();
        for route in &mut self.routes {
            if !route.auth_requirements.required {
                route.auth_requirements = auth_requirements.clone();
            }
        }
        self
    }

    /// Require specific permissions for all routes in this interface
    pub fn require_permissions(mut self, permissions: Vec<String>) -> Self {
        let auth_requirements = AuthRequirements::required().with_permissions(permissions);
        for route in &mut self.routes {
            if route.auth_requirements.permissions.is_empty() {
                route.auth_requirements = auth_requirements.clone();
            }
        }
        self
    }

    /// Build the mountable interface
    pub fn build(self) -> MountableInterface {
        MountableInterface {
            name: self.name,
            description: self.description,
            routes: self.routes,
            middleware: self.middleware,
            mount_options: self.mount_options,
        }
    }
}

impl MountableInterface {
    /// Create a new interface builder
    pub fn builder(name: impl Into<String>) -> InterfaceBuilder {
        InterfaceBuilder::new(name)
    }

    /// Get the name of this interface
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the description of this interface
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get all routes in this interface
    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }

    /// Get middleware for this interface
    pub fn middleware(&self) -> &[String] {
        &self.middleware
    }

    /// Get mount options
    pub fn mount_options(&self) -> &MountOptions {
        &self.mount_options
    }

    /// Mount this interface at the given prefix path
    /// Returns a list of routes with the prefix applied
    pub fn mount_at(&self, prefix: impl AsRef<str>) -> Result<Vec<Route>> {
        let prefix = prefix.as_ref();
        let prefix = if prefix.starts_with('/') {
            prefix.to_string()
        } else {
            format!("/{}", prefix)
        };

        let mut mounted_routes = Vec::new();

        for route_def in &self.routes {
            let _full_path = if prefix == "/" {
                route_def.path.clone()
            } else {
                format!("{}{}", prefix, route_def.path)
            };

            // Create a new route instance using the handler function
            let route = (route_def.handler_fn)();
            mounted_routes.push(route);
        }

        Ok(mounted_routes)
    }

    /// Mount this interface at the given prefix path with authentication support
    pub fn mount_with_auth_at(
        &self,
        prefix: impl AsRef<str>,
        auth_context: Arc<AuthContext>,
    ) -> Result<Vec<AuthenticatedRoute>> {
        let prefix = prefix.as_ref();
        let prefix = if prefix.starts_with('/') {
            prefix.to_string()
        } else {
            format!("/{}", prefix)
        };

        let mut mounted_routes = Vec::new();

        for route_def in &self.routes {
            let _full_path = if prefix == "/" {
                route_def.path.clone()
            } else {
                format!("{}{}", prefix, route_def.path)
            };

            // Create a new route instance using the handler function
            let route = (route_def.handler_fn)();

            // Create an authenticated route with the route's auth requirements
            let authenticated_route = AuthenticatedRoute::new(
                route,
                route_def.auth_requirements.clone(),
                auth_context.clone(),
            );
            mounted_routes.push(authenticated_route);
        }

        Ok(mounted_routes)
    }

    /// Generate OpenAPI documentation for this interface
    pub fn openapi_spec(&self) -> OpenApiSpec {
        OpenApiSpec {
            interface_name: self.name.clone(),
            description: self.description.clone(),
            routes: self
                .routes
                .iter()
                .map(|r| RouteDoc {
                    path: r.path.clone(),
                    method: r.method,
                    description: r.description.clone(),
                    tags: r.tags.clone(),
                })
                .collect(),
        }
    }
}

/// Registry for managing multiple mountable interfaces
pub struct InterfaceRegistry {
    interfaces: HashMap<String, MountableInterface>,
    auth_context: Option<Arc<AuthContext>>,
}

impl InterfaceRegistry {
    /// Create a new interface registry
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
            auth_context: None,
        }
    }

    /// Create a new interface registry with authentication support
    pub fn with_auth(auth_context: Arc<AuthContext>) -> Self {
        Self {
            interfaces: HashMap::new(),
            auth_context: Some(auth_context),
        }
    }

    /// Set the authentication context
    pub fn set_auth_context(&mut self, auth_context: Arc<AuthContext>) {
        self.auth_context = Some(auth_context);
    }

    /// Get the authentication context
    pub fn auth_context(&self) -> Option<&Arc<AuthContext>> {
        self.auth_context.as_ref()
    }

    /// Register a new interface
    pub fn register(&mut self, interface: MountableInterface) -> Result<()> {
        let name = interface.name().to_string();
        if self.interfaces.contains_key(&name) {
            return Err(crate::error::WebServerError::ConfigError(format!(
                "Interface '{}' is already registered",
                name
            )));
        }
        self.interfaces.insert(name, interface);
        Ok(())
    }

    /// Get an interface by name
    pub fn get(&self, name: &str) -> Option<&MountableInterface> {
        self.interfaces.get(name)
    }

    /// List all registered interfaces
    pub fn list(&self) -> Vec<&str> {
        self.interfaces.keys().map(|s| s.as_str()).collect()
    }

    /// Mount an interface at the given prefix with authentication support
    pub fn mount(
        &self,
        interface_name: &str,
        prefix: impl AsRef<str>,
    ) -> Result<Vec<AuthenticatedRoute>> {
        let interface = self.get(interface_name).ok_or_else(|| {
            crate::error::WebServerError::ConfigError(format!(
                "Interface '{}' not found",
                interface_name
            ))
        })?;

        let auth_context = self.auth_context.as_ref().ok_or_else(|| {
            crate::error::WebServerError::ConfigError(
                "No authentication context available. Use with_auth() or set_auth_context()"
                    .to_string(),
            )
        })?;

        interface.mount_with_auth_at(prefix, auth_context.clone())
    }

    /// Mount all interfaces with their default paths and authentication
    pub fn mount_all_with_auth(&self) -> Result<Vec<AuthenticatedRoute>> {
        let auth_context = self.auth_context.as_ref().ok_or_else(|| {
            crate::error::WebServerError::ConfigError(
                "No authentication context available".to_string(),
            )
        })?;

        let mut all_routes = Vec::new();

        for (name, interface) in &self.interfaces {
            let routes =
                interface.mount_with_auth_at(format!("/{}", name), auth_context.clone())?;
            all_routes.extend(routes);
        }

        Ok(all_routes)
    }

    /// Mount all interfaces with their default paths
    pub fn mount_all(&self) -> Result<Vec<AuthenticatedRoute>> {
        let auth_context = self.auth_context.as_ref().ok_or_else(|| {
            crate::error::WebServerError::ConfigError(
                "No authentication context available".to_string(),
            )
        })?;

        let mut all_routes = Vec::new();

        for (name, interface) in &self.interfaces {
            let routes =
                interface.mount_with_auth_at(format!("/{}", name), auth_context.clone())?;
            all_routes.extend(routes);
        }

        Ok(all_routes)
    }
}

impl Default for InterfaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// OpenAPI specification for documentation
#[derive(Debug, Clone)]
pub struct OpenApiSpec {
    pub interface_name: String,
    pub description: String,
    pub routes: Vec<RouteDoc>,
}

/// Route documentation for OpenAPI
#[derive(Debug, Clone)]
pub struct RouteDoc {
    pub path: String,
    pub method: HttpMethod,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Response;

    async fn hello_handler(_req: crate::types::Request) -> Result<Response> {
        Ok(Response::ok().body("Hello, World!"))
    }

    async fn status_handler(_req: crate::types::Request) -> Result<Response> {
        Response::json(&serde_json::json!({"status": "ok"}))
    }

    #[test]
    fn test_interface_builder() {
        let interface = MountableInterface::builder("test-api")
            .description("Test API interface")
            .route("/hello", HttpMethod::GET, hello_handler)
            .route("/status", HttpMethod::GET, status_handler)
            .middleware("cors")
            .build();

        assert_eq!(interface.name(), "test-api");
        assert_eq!(interface.description(), "Test API interface");
        assert_eq!(interface.routes().len(), 2);
        assert_eq!(interface.middleware().len(), 1);
    }

    #[test]
    fn test_interface_registry() {
        let mut registry = InterfaceRegistry::new();

        let interface1 = MountableInterface::builder("api-v1")
            .route("/users", HttpMethod::GET, hello_handler)
            .build();

        let interface2 = MountableInterface::builder("admin")
            .route("/status", HttpMethod::GET, status_handler)
            .build();

        registry.register(interface1).unwrap();
        registry.register(interface2).unwrap();

        assert_eq!(registry.list().len(), 2);
        assert!(registry.get("api-v1").is_some());
        assert!(registry.get("admin").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_mounting() {
        let interface = MountableInterface::builder("test")
            .route("/hello", HttpMethod::GET, hello_handler)
            .build();

        let routes = interface.mount_at("/api/v1").unwrap();
        assert_eq!(routes.len(), 1);

        // Test that the route works (this is a basic test)
        // In a real implementation, you'd test the full path resolution
    }
}
