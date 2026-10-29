# Web Server Abstraction

## Known issues in web-server-abstraction 1.0.2 (superseded by HTTPAnt, in progress)

This crate is being replaced by [HTTPAnt](https://github.com/ThinkersJournal/httpant). 1.0.2 is **not yanked**. A source read of the
crate found the defects below; each line gives its basis. Full evidence, with line numbers: [docs/OLD-CRATE-SURVEY.md](docs/OLD-CRATE-SURVEY.md).

- **Unknown request methods are served as GET** (axum, actix-web, warp, salvo, poem). Basis: a `_ => HttpMethod::GET` arm in each adapter's request conversion.
- **warp: every handler sees a GET to `/`**, whatever was sent. Basis: `convert_warp_request_to_ours` hard-codes both. Warp also never sets response headers and corrupts binary bodies (`from_utf8_lossy`).
- **axum and warp send status codes outside a short list as 200** (a 418 or 503 reaches the client as 200). Basis: a `_ => OK` arm in the status match. actix-web, salvo and poem convert status correctly.
- **salvo and poem: request bodies are always empty.** Basis: `Body::from_bytes(vec![].into()), // Empty body for now` in both adapters.
- **Middleware is stored and never applied** (all six adapters). Basis: no code path reads the stored middleware; rocket and poem have wrapper types marked `#[allow(dead_code)]`, salvo and poem print "not yet fully integrated".
- **Bodies are fully buffered**, with no streaming; axum reads with no size limit, rocket caps at 2 MB and turns a read error into an empty body.
- **No test sends a request to a running server.** Basis: `tests/integration_tests.rs` only builds servers and calls `bind`; a search of `src/`, `tests/` and `examples/` for HTTP clients found none (the unit tests in some files were not read).

Affected adapters: all six (axum, actix-web, warp, rocket, salvo, poem); the per-adapter list is in the survey. The "production-ready" wording further down this
page and in `Cargo.toml` predates this survey and is not supported by it. Nothing here is a fix: 1.0.2 on crates.io is unchanged.

An ergonomic abstraction layer over popular Rust web frameworks# Use specific framework adapters

# [cfg(feature = "axum")]

let server = WebServer::with_axum_adapter();

# [cfg(feature = "actix-web")]

let server = WebServer::with_actix_adapter();

# [cfg(feature = "warp")]

let server = WebServer::with_warp_adapter();

# [cfg(feature = "rocket")]

let server = WebServer::with_rocket_adapter();

# [cfg(feature = "salvo")]

let server = WebServer::with_salvo_adapter();

# [cfg(feature = "poem")]

let server = WebServer::with_poem_adapter();

// Or use the mock adapter for testing
let server = WebServer::with_mock_adapter();ou to write web applications once and run them on any supported framework.

## Features

- **Framework Agnostic**: Write once, run on any supported framework
- **Type Safe**: Leverages Rust's type system for compile-time guarantees
- **Async First**: Built for modern async Rust with native async/await support
- **Middleware Support**: Composable middleware system for cross-cutting concerns
- **Tower Integration**: Built on the Tower ecosystem for compatibility
- **Ergonomic API**: Clean, intuitive API that's easy to learn and use
- **Advanced Routing**: Path parameters (`:id`) and wildcards (`*file`) support
- **WebSocket Ready**: Built-in WebSocket upgrade handling and message types
- **HTTP Method Shortcuts**: Convenient `.get()`, `.post()`, `.put()`, etc. methods
- **Rich Middleware**: 9+ built-in middleware types for common web patterns
- **Performance Optimized**: Comprehensive benchmarking and profiling infrastructure

## Supported Frameworks

| Framework | Feature Flag | Status |
|-----------|-------------|--------|
| Mock (Testing) | Default | ✅ Complete |
| Axum | `axum` | ✅ Complete |
| Actix-Web | `actix-web` | ✅ Complete |
| Warp | `warp` | ✅ Complete |
| Rocket | `rocket` | ✅ Complete |
| Salvo | `salvo` | ✅ Complete |
| Poem | `poem` | ✅ Complete |

> **Note**: All framework adapters are production-ready and fully tested with their latest versions.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
web-server-abstraction = "1.0.2"  # Includes Axum support by default

# Or explicitly enable specific framework features
web-server-abstraction = { version = "1.0.2", features = ["axum"] }

# Enable multiple frameworks
web-server-abstraction = { version = "1.0.2", features = ["axum", "rocket", "poem"] }
```

### Basic Example

```rust
use web_server_abstraction::{
    WebServer, HttpMethod, Response, StatusCode,
    middleware::{LoggingMiddleware, CorsMiddleware},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        .route("/users", HttpMethod::POST, |req| async {
            // Parse JSON body
            let user: serde_json::Value = req.json().await?;
            Ok(Response::new(StatusCode::CREATED)
                .header("content-type", "application/json")
                .body(serde_json::to_string(&user)?))
        });

    // Bind and run the server
    let bound_server = server.bind("127.0.0.1:3000").await?;
    println!("Server running on http://127.0.0.1:3000");
    bound_server.run().await?;

    Ok(())
}
```

### Framework-Specific Adapters

```rust
use web_server_abstraction::WebServer;

// Use specific framework adapters
#[cfg(feature = "axum")]
let server = WebServer::with_axum_adapter();

#[cfg(feature = "actix-web")]
let server = WebServer::with_actix_adapter();

#[cfg(feature = "warp")]
let server = WebServer::with_warp_adapter();

// Or use the mock adapter for testing
let server = WebServer::with_mock_adapter();
```

## Architecture

### Core Abstractions

The crate is built around several key abstractions:

1. **WebServer**: The main entry point for building web applications
2. **Handler**: Trait for request handlers that convert requests to responses
3. **Middleware**: Composable middleware for cross-cutting concerns
4. **Adapter**: Framework-specific implementations that bridge to actual web frameworks

### Type System

```rust
// Core types are standardized across frameworks
pub struct Request { /* ... */ }
pub struct Response { /* ... */ }
pub enum HttpMethod { GET, POST, PUT, DELETE, /* ... */ }
pub struct StatusCode(pub u16);
```

### Middleware System

Built-in middleware includes:

- **LoggingMiddleware**: Request/response logging with configurable detail levels
- **CorsMiddleware**: Cross-Origin Resource Sharing with full configuration support
- **AuthMiddleware**: Authentication checks with bearer token validation
- **TimeoutMiddleware**: Request timeouts with configurable durations
- **RateLimitMiddleware**: Rate limiting with sliding window algorithm
- **CompressionMiddleware**: Response compression (gzip, deflate)
- **SecurityHeadersMiddleware**: Security headers (HSTS, CSP, X-Frame-Options, etc.)
- **MetricsMiddleware**: Request metrics collection and monitoring
- **CacheMiddleware**: Response caching with TTL and invalidation strategies

```rust
use web_server_abstraction::middleware::*;

let server = WebServer::new()
    .middleware(LoggingMiddleware::new().log_bodies(true))
    .middleware(CorsMiddleware::new()
        .allow_origin("https://example.com")
        .allow_methods(vec!["GET".to_string(), "POST".to_string()])
        .allow_credentials(true))
    .middleware(AuthMiddleware::new()
        .with_bearer_tokens(vec!["secret-token".to_string()]))
    .middleware(RateLimitMiddleware::new(100, Duration::from_secs(60)))
    .middleware(CompressionMiddleware::new().min_size(1024))
    .middleware(SecurityHeadersMiddleware::new())
    .middleware(MetricsMiddleware::new())
    .middleware(CacheMiddleware::new(Duration::from_secs(300)));
```

## Framework Integration Plan

### Current Design Principles

1. **Common HTTP Abstractions**: Use standardized types from the `http` crate
2. **Tower Compatibility**: Leverage the Tower ecosystem where possible
3. **Zero-Cost Abstractions**: Minimal runtime overhead
4. **Ergonomic APIs**: Focus on developer experience

### Adapter Implementation Strategy

Each framework adapter follows this pattern:

```rust
// 1. Convert our types to framework types
fn convert_request(req: Request) -> FrameworkRequest;
fn convert_response(resp: FrameworkResponse) -> Response;

// 2. Implement the adapter interface
impl FrameworkAdapter {
    pub async fn bind(&mut self, addr: &str) -> Result<()>;
    pub async fn run(self) -> Result<()>;
    pub fn route(&mut self, path: &str, method: HttpMethod, handler: HandlerFn);
    pub fn middleware(&mut self, middleware: Box<dyn Middleware>);
}
```

### Integration Status

#### Axum ✅ **COMPLETE**

- ✅ Basic route registration
- ✅ Request/response type conversion
- ✅ Middleware integration with Tower ServiceBuilder
- ✅ Full HTTP method support
- ✅ Async handler support

#### Actix-Web ✅ **COMPLETE**

- ✅ Route registration and handlers
- ✅ Request/response conversion
- ✅ HTTP server binding and running
- ✅ Built-in logging middleware
- ✅ Full HTTP method support

#### Warp ✅ **COMPLETE**

- ✅ Filter-based routing system
- ✅ Request/response handling
- ✅ Middleware composition
- ✅ Async handler support
- ✅ Server binding and execution

#### Rocket ✅ **COMPLETE**

- ✅ Production-ready adapter implementation
- ✅ Route registration with Rocket's Handler trait
- ✅ Request/response type conversion
- ✅ Middleware integration via Fairings
- ✅ Full HTTP method support
- ✅ Server configuration and binding
- ✅ Comprehensive error handling

#### Salvo ✅ **COMPLETE**

- ✅ Production-ready adapter implementation
- ✅ High-performance web framework integration
- ✅ Modular design with extractors
- ✅ Router and Service integration
- ✅ Middleware fairing system
- ✅ Full HTTP method support
- ✅ TcpListener binding and server execution

#### Poem ✅ **COMPLETE**

- ✅ Production-ready adapter implementation
- ✅ Fast and lightweight framework integration
- ✅ Type-safe Endpoint trait implementation
- ✅ Built-in middleware (Tracing, NormalizePath)
- ✅ Comprehensive request/response conversion
- ✅ Full HTTP method support
- ✅ TcpListener and Server integration

## Testing

The crate includes a mock adapter for easy testing:

```rust
#[tokio::test]
async fn test_my_routes() {
    use web_server_abstraction::MockAdapter;

    let server = WebServer::with_mock_adapter()
        .route("/test", HttpMethod::GET, |_| async {
            Ok(Response::ok().body("test"))
        });

    let bound_server = server.bind("127.0.0.1:0").await.unwrap();

    // Mock adapter provides testing utilities
    // (In practice, you'd make actual HTTP requests)
}
```

Run tests with:

```bash
cargo test
```

Run examples with:

```bash
cargo run --example basic_server
```

## Contributing

We welcome contributions! Areas where help is needed:

1. **Framework Adapters**: Implementing adapters for different frameworks
2. **Middleware**: Adding common middleware implementations
3. **Documentation**: Improving docs and examples
4. **Testing**: Adding comprehensive test coverage
5. **Performance**: Benchmarking and optimization

### Adding a New Framework Adapter

1. Create a new module in `src/adapters/`
2. Implement the required methods: `bind`, `run`, `route`, `middleware`
3. Add conversion functions between framework types and our types
4. Add a feature flag in `Cargo.toml`
5. Update the `AdapterType` enum in `core.rs`
6. Add tests and documentation

## Performance Considerations

- **Zero-cost abstractions**: The abstraction layer adds minimal overhead
- **Compile-time dispatch**: Framework adapters use static dispatch where possible
- **Memory efficiency**: Minimal allocations in hot paths
- **Async-first**: Built for modern async Rust performance characteristics

### Benchmarking and Profiling

The crate includes comprehensive benchmarking infrastructure:

```rust
use web_server_abstraction::benchmarks::{
    PerformanceProfiler, BenchmarkConfig, BenchmarkSuite
};

// Configure and run performance benchmarks
let config = BenchmarkConfig {
    duration: Duration::from_secs(30),
    concurrent_requests: 100,
    warmup_duration: Duration::from_secs(5),
};

let profiler = PerformanceProfiler::new(config);
let results = profiler.benchmark_scenario("load_test").await?;

// Analyze results with statistical metrics
println!("Average response time: {:?}", results.mean());
println!("95th percentile: {:?}", results.percentile(95.0));
println!("Requests per second: {}", results.requests_per_second());
```

Features include:

- **Statistical Analysis**: Mean, median, percentiles, standard deviation
- **Memory Profiling**: Memory usage tracking and leak detection
- **Scenario Comparison**: Compare performance across different configurations
- **Optimization Recommendations**: Automated performance suggestions
- **Async-first**: Built for modern async Rust performance characteristics

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Roadmap

### Version 0.1.0 ✅ **COMPLETED**

- [x] Core abstractions and API design
- [x] Mock adapter for testing
- [x] Basic middleware system
- [x] Documentation and examples

### Version 0.2.0 ✅ **COMPLETED**

- [x] Complete Axum adapter
- [x] Actix-Web adapter
- [x] Enhanced middleware ecosystem (9 middleware types implemented)
- [x] Performance benchmarks and profiling infrastructure

### Version 0.3.0 ✅ **COMPLETED**

- [x] Warp adapter
- [x] Advanced routing features (path parameters, wildcards)
- [x] WebSocket support (basic implementation)
- [x] HTTP method convenience functions (get, post, put, delete, patch)
- [x] Framework adapter scaffolding (Rocket, Salvo, Poem - basic structure in place)

### Version 1.0.0 ✅ **COMPLETED**

- [x] Complete Rocket adapter
- [x] Complete Salvo adapter
- [x] Complete Poem adapter
- [x] Comprehensive middleware library
- [x] Production-ready performance
- [x] Stable API
- [x] WebSocket support
- [x] Advanced routing features (wildcards, parameters)
- [x] Mountable interface support
- [x] Authentication integration## Why This Approach?

### Problem Statement

Many Rust crates need to support multiple web frameworks, leading to:

- Duplicate implementation effort
- Maintenance burden across multiple framework versions
- User lock-in to specific frameworks
- Inconsistent APIs across different framework integrations

### Solution Benefits

1. **Write Once, Run Anywhere**: Implement your web logic once
2. **Framework Flexibility**: Users can choose their preferred framework
3. **Easier Testing**: Mock adapter makes testing straightforward
4. **Future-Proof**: Easy to add support for new frameworks
5. **Consistent API**: Same interface regardless of underlying framework

### Trade-offs

- **Additional Abstraction**: One more layer between your code and the framework
- **Learning Curve**: New API to learn (though designed to be intuitive)
- **Feature Lag**: Advanced framework-specific features may not be immediately available

We believe the benefits outweigh these trade-offs for most use cases, especially for libraries and applications that need broad framework compatibility.
