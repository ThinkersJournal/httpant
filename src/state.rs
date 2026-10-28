//! Application state management for sharing data across requests.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Application state container that can store any type
#[derive(Clone, Debug)]
pub struct AppState {
    data: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl AppState {
    /// Create a new empty state container
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a value into the state
    pub fn insert<T: Send + Sync + 'static>(&self, value: T) {
        let mut data = self.data.write().unwrap();
        data.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Get a value from the state
    pub fn get<T: Send + Sync + Clone + 'static>(&self) -> Option<T> {
        let data = self.data.read().unwrap();
        let value = data.get(&TypeId::of::<T>())?;
        let value_ref = value.downcast_ref::<T>()?;
        // Clone the value to return it safely
        Some(value_ref.clone())
    }

    /// Check if a type exists in the state
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        let data = self.data.read().unwrap();
        data.contains_key(&TypeId::of::<T>())
    }

    /// Remove a value from the state
    pub fn remove<T: Send + Sync + 'static>(&self) -> Option<T> {
        let mut data = self.data.write().unwrap();
        let value = data.remove(&TypeId::of::<T>())?;
        value.downcast().ok().map(|boxed| *boxed)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// A better approach for shared state using `Arc<T>` directly
#[derive(Clone, Debug)]
pub struct SharedState<T> {
    inner: Arc<RwLock<T>>,
}

impl<T> SharedState<T> {
    /// Create new shared state
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(value)),
        }
    }

    /// Get a read lock on the value
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, T> {
        self.inner.read().unwrap()
    }

    /// Get a write lock on the value
    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
        self.inner.write().unwrap()
    }

    /// Get the inner Arc for sharing
    pub fn inner(&self) -> Arc<RwLock<T>> {
        self.inner.clone()
    }
}

/// Configuration for the application
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub environment: Environment,
    pub database_url: Option<String>,
    pub redis_url: Option<String>,
    pub secret_key: Option<String>,
    pub cors_origins: Vec<String>,
    pub max_request_size: usize,
    pub request_timeout: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Environment {
    Development,
    Staging,
    Production,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            environment: Environment::Development,
            database_url: None,
            redis_url: None,
            secret_key: None,
            cors_origins: vec!["http://localhost:3000".to_string()],
            max_request_size: 1024 * 1024 * 10, // 10MB
            request_timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(host) = std::env::var("HOST") {
            config.host = host;
        }

        if let Ok(port_str) = std::env::var("PORT")
            && let Ok(port) = port_str.parse()
        {
            config.port = port;
        }

        if let Ok(env_str) = std::env::var("ENVIRONMENT") {
            config.environment = match env_str.to_lowercase().as_str() {
                "production" | "prod" => Environment::Production,
                "staging" | "stage" => Environment::Staging,
                _ => Environment::Development,
            };
        }

        config.database_url = std::env::var("DATABASE_URL").ok();
        config.redis_url = std::env::var("REDIS_URL").ok();
        config.secret_key = std::env::var("SECRET_KEY").ok();

        if let Ok(origins) = std::env::var("CORS_ORIGINS") {
            config.cors_origins = origins.split(',').map(|s| s.trim().to_string()).collect();
        }

        if let Ok(timeout_str) = std::env::var("REQUEST_TIMEOUT")
            && let Ok(timeout_secs) = timeout_str.parse::<u64>()
        {
            config.request_timeout = std::time::Duration::from_secs(timeout_secs);
        }

        config
    }

    /// Check if running in production
    pub fn is_production(&self) -> bool {
        self.environment == Environment::Production
    }

    /// Check if running in development
    pub fn is_development(&self) -> bool {
        self.environment == Environment::Development
    }

    /// Get the server bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_state() {
        let state = SharedState::new(42);

        // Test read
        assert_eq!(*state.read(), 42);

        // Test write
        *state.write() = 100;
        assert_eq!(*state.read(), 100);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.environment, Environment::Development);
        assert!(config.is_development());
        assert!(!config.is_production());
    }

    #[test]
    fn test_config_bind_address() {
        let config = Config {
            host: "0.0.0.0".to_string(),
            port: 3000,
            ..Default::default()
        };
        assert_eq!(config.bind_address(), "0.0.0.0:3000");
    }
}
