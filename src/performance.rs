//! Performance Benchmarking System
//!
//! This module provides comprehensive performance benchmarking capabilities
//! for validating "ultra-low latency" requirements across different framework adapters.

use crate::{
    core::HandlerFn,
    types::{HttpMethod, Request},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::sleep;

#[cfg(feature = "benchmarks")]
use criterion::{Criterion, black_box, criterion_group, criterion_main};

/// Performance metrics for benchmarking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub request_count: u64,
    pub total_duration: Duration,
    pub min_latency: Duration,
    pub max_latency: Duration,
    pub avg_latency: Duration,
    pub p50_latency: Duration,
    pub p95_latency: Duration,
    pub p99_latency: Duration,
    pub throughput_rps: f64,
    pub memory_usage_mb: f64,
    pub cpu_usage_percent: f64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            request_count: 0,
            total_duration: Duration::from_secs(0),
            min_latency: Duration::from_secs(u64::MAX),
            max_latency: Duration::from_secs(0),
            avg_latency: Duration::from_secs(0),
            p50_latency: Duration::from_secs(0),
            p95_latency: Duration::from_secs(0),
            p99_latency: Duration::from_secs(0),
            throughput_rps: 0.0,
            memory_usage_mb: 0.0,
            cpu_usage_percent: 0.0,
        }
    }
}

/// Benchmark configuration
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub duration: Duration,
    pub concurrent_requests: usize,
    pub target_rps: Option<f64>,
    pub warmup_duration: Duration,
    pub request_size_bytes: usize,
    pub response_size_bytes: usize,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(10),
            concurrent_requests: 100,
            target_rps: None,
            warmup_duration: Duration::from_secs(2),
            request_size_bytes: 1024,
            response_size_bytes: 1024,
        }
    }
}

/// Ultra-low latency benchmark configuration
impl BenchmarkConfig {
    /// Configuration for ultra-low latency benchmarks (sub-millisecond targets)
    pub fn ultra_low_latency() -> Self {
        Self {
            duration: Duration::from_secs(30),
            concurrent_requests: 1000,
            target_rps: Some(100_000.0), // 100k RPS target
            warmup_duration: Duration::from_secs(5),
            request_size_bytes: 64, // Small payloads for low latency
            response_size_bytes: 64,
        }
    }

    /// Configuration for high throughput benchmarks
    pub fn high_throughput() -> Self {
        Self {
            duration: Duration::from_secs(60),
            concurrent_requests: 2000,
            target_rps: Some(50_000.0),
            warmup_duration: Duration::from_secs(10),
            request_size_bytes: 8192, // Larger payloads
            response_size_bytes: 8192,
        }
    }

    /// Configuration for stress testing
    pub fn stress_test() -> Self {
        Self {
            duration: Duration::from_secs(300), // 5 minutes
            concurrent_requests: 5000,
            target_rps: None, // No rate limiting
            warmup_duration: Duration::from_secs(30),
            request_size_bytes: 16384,
            response_size_bytes: 16384,
        }
    }
}

/// Latency collector for tracking request latencies
#[derive(Debug, Clone)]
pub struct LatencyCollector {
    latencies: Arc<Mutex<Vec<Duration>>>,
    start_time: Instant,
}

impl LatencyCollector {
    pub fn new() -> Self {
        Self {
            latencies: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
        }
    }

    /// Record a request latency
    pub fn record_latency(&self, latency: Duration) {
        let mut latencies = self.latencies.lock().unwrap();
        latencies.push(latency);
    }

    /// Calculate performance metrics from collected latencies
    pub fn calculate_metrics(&self) -> PerformanceMetrics {
        let latencies = self.latencies.lock().unwrap();
        let mut sorted_latencies = latencies.clone();
        sorted_latencies.sort();

        let request_count = sorted_latencies.len() as u64;
        let total_duration = self.start_time.elapsed();

        if request_count == 0 {
            return PerformanceMetrics::default();
        }

        let min_latency = sorted_latencies[0];
        let max_latency = sorted_latencies[sorted_latencies.len() - 1];

        let avg_latency = Duration::from_nanos(
            (sorted_latencies.iter().map(|d| d.as_nanos()).sum::<u128>() / request_count as u128)
                as u64,
        );

        let p50_latency = sorted_latencies[sorted_latencies.len() * 50 / 100];
        let p95_latency = sorted_latencies[sorted_latencies.len() * 95 / 100];
        let p99_latency = sorted_latencies[sorted_latencies.len() * 99 / 100];

        let throughput_rps = request_count as f64 / total_duration.as_secs_f64();

        PerformanceMetrics {
            request_count,
            total_duration,
            min_latency,
            max_latency,
            avg_latency,
            p50_latency,
            p95_latency,
            p99_latency,
            throughput_rps,
            memory_usage_mb: get_memory_usage_mb(),
            cpu_usage_percent: get_cpu_usage_percent(),
        }
    }
}

impl Default for LatencyCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Framework adapter benchmark runner
pub struct AdapterBenchmark {
    pub adapter_name: String,
    pub config: BenchmarkConfig,
}

impl AdapterBenchmark {
    pub fn new(adapter_name: &str, config: BenchmarkConfig) -> Self {
        Self {
            adapter_name: adapter_name.to_string(),
            config,
        }
    }

    /// Run a complete benchmark suite for an adapter
    pub async fn run_benchmark_suite(&self, handler: HandlerFn) -> BenchmarkResults {
        let mut results = BenchmarkResults::new(&self.adapter_name);

        // Ultra-low latency benchmark
        println!(
            "Running ultra-low latency benchmark for {}...",
            self.adapter_name
        );
        let ultra_low_config = BenchmarkConfig::ultra_low_latency();
        let ultra_low_metrics = self
            .run_single_benchmark(&ultra_low_config, handler.clone())
            .await;
        results.add_result("ultra_low_latency", ultra_low_metrics);

        // High throughput benchmark
        println!(
            "Running high throughput benchmark for {}...",
            self.adapter_name
        );
        let high_throughput_config = BenchmarkConfig::high_throughput();
        let high_throughput_metrics = self
            .run_single_benchmark(&high_throughput_config, handler.clone())
            .await;
        results.add_result("high_throughput", high_throughput_metrics);

        // Standard benchmark
        println!("Running standard benchmark for {}...", self.adapter_name);
        let standard_metrics = self
            .run_single_benchmark(&self.config, handler.clone())
            .await;
        results.add_result("standard", standard_metrics);

        results
    }

    /// Run a single benchmark with the given configuration
    pub async fn run_single_benchmark(
        &self,
        config: &BenchmarkConfig,
        handler: HandlerFn,
    ) -> PerformanceMetrics {
        let collector = LatencyCollector::new();

        // Warmup phase
        println!("Warming up for {:?}...", config.warmup_duration);
        self.run_warmup(config, handler.clone()).await;

        // Main benchmark phase
        println!("Running benchmark for {:?}...", config.duration);
        let _start_time = Instant::now();
        let tasks = self
            .spawn_benchmark_tasks(config, handler, collector.clone())
            .await;

        // Wait for all tasks to complete or timeout
        let timeout = tokio::time::timeout(
            config.duration + Duration::from_secs(10), // Grace period
            futures::future::join_all(tasks),
        );

        match timeout.await {
            Ok(_) => println!("Benchmark completed successfully"),
            Err(_) => println!("Benchmark timed out"),
        }

        collector.calculate_metrics()
    }

    /// Run warmup requests
    async fn run_warmup(&self, config: &BenchmarkConfig, handler: HandlerFn) {
        let warmup_requests = (config.concurrent_requests / 10).max(10);
        let mut tasks = Vec::new();

        for _ in 0..warmup_requests {
            let handler_clone = handler.clone();
            tasks.push(tokio::spawn(async move {
                let request = create_benchmark_request();
                let _ = handler_clone(request).await;
            }));
        }

        let _ =
            tokio::time::timeout(config.warmup_duration, futures::future::join_all(tasks)).await;
    }

    /// Spawn benchmark tasks
    async fn spawn_benchmark_tasks(
        &self,
        config: &BenchmarkConfig,
        handler: HandlerFn,
        collector: LatencyCollector,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let mut tasks = Vec::new();
        let end_time = Instant::now() + config.duration;
        let concurrent_requests = config.concurrent_requests;
        let target_rps = config.target_rps;

        for _ in 0..concurrent_requests {
            let handler_clone = handler.clone();
            let collector_clone = collector.clone();
            let end_time_clone = end_time;

            tasks.push(tokio::spawn(async move {
                while Instant::now() < end_time_clone {
                    let request_start = Instant::now();
                    let request = create_benchmark_request();

                    match handler_clone(request).await {
                        Ok(_) => {
                            let latency = request_start.elapsed();
                            collector_clone.record_latency(latency);
                        }
                        Err(_) => {
                            // Count errors but continue
                        }
                    }

                    // Optional: Add small delay for rate limiting
                    if let Some(target_rps) = target_rps {
                        let target_interval =
                            Duration::from_secs_f64(concurrent_requests as f64 / target_rps);
                        sleep(target_interval).await;
                    }
                }
            }));
        }

        tasks
    }
}

/// Benchmark results collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResults {
    pub adapter_name: String,
    pub results: HashMap<String, PerformanceMetrics>,
    pub timestamp: u64,
}

impl BenchmarkResults {
    pub fn new(adapter_name: &str) -> Self {
        Self {
            adapter_name: adapter_name.to_string(),
            results: HashMap::new(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    pub fn add_result(&mut self, benchmark_name: &str, metrics: PerformanceMetrics) {
        self.results.insert(benchmark_name.to_string(), metrics);
    }

    /// Check if adapter meets ultra-low latency requirements
    pub fn meets_ultra_low_latency_requirements(&self) -> bool {
        if let Some(metrics) = self.results.get("ultra_low_latency") {
            // Ultra-low latency requirements:
            // - P99 latency < 1ms
            // - P95 latency < 500μs
            // - Average latency < 200μs
            // - Throughput > 50k RPS
            metrics.p99_latency < Duration::from_millis(1)
                && metrics.p95_latency < Duration::from_micros(500)
                && metrics.avg_latency < Duration::from_micros(200)
                && metrics.throughput_rps > 50_000.0
        } else {
            false
        }
    }

    /// Generate benchmark report
    pub fn generate_report(&self) -> String {
        let mut report = format!("# Benchmark Report for {}\n\n", self.adapter_name);

        for (benchmark_name, metrics) in &self.results {
            report.push_str(&format!("## {} Benchmark\n\n", benchmark_name));
            report.push_str(&format!("- **Requests**: {}\n", metrics.request_count));
            report.push_str(&format!("- **Duration**: {:?}\n", metrics.total_duration));
            report.push_str(&format!(
                "- **Throughput**: {:.2} RPS\n",
                metrics.throughput_rps
            ));
            report.push_str(&format!(
                "- **Average Latency**: {:?}\n",
                metrics.avg_latency
            ));
            report.push_str(&format!("- **P50 Latency**: {:?}\n", metrics.p50_latency));
            report.push_str(&format!("- **P95 Latency**: {:?}\n", metrics.p95_latency));
            report.push_str(&format!("- **P99 Latency**: {:?}\n", metrics.p99_latency));
            report.push_str(&format!("- **Min Latency**: {:?}\n", metrics.min_latency));
            report.push_str(&format!("- **Max Latency**: {:?}\n", metrics.max_latency));
            report.push_str(&format!(
                "- **Memory Usage**: {:.2} MB\n",
                metrics.memory_usage_mb
            ));
            report.push_str(&format!(
                "- **CPU Usage**: {:.2}%\n\n",
                metrics.cpu_usage_percent
            ));
        }

        // Ultra-low latency assessment
        if self.meets_ultra_low_latency_requirements() {
            report.push_str("✅ **PASSED**: Meets ultra-low latency requirements\n\n");
        } else {
            report.push_str("❌ **FAILED**: Does not meet ultra-low latency requirements\n\n");
        }

        report
    }

    /// Save results to JSON file
    pub fn save_to_file(&self, path: &str) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load results from JSON file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let results = serde_json::from_str(&json)?;
        Ok(results)
    }
}

/// Create a benchmark request
fn create_benchmark_request() -> Request {
    Request {
        method: HttpMethod::GET,
        uri: http::Uri::from_static("http://localhost:8080/benchmark"),
        version: http::Version::HTTP_11,
        headers: {
            let mut headers = crate::types::Headers::new();
            headers.insert("user-agent".to_string(), "benchmark-client/1.0".to_string());
            headers.insert("accept".to_string(), "application/json".to_string());
            headers
        },
        body: crate::types::Body::from_string("benchmark request"),
        extensions: std::collections::HashMap::new(),
        path_params: std::collections::HashMap::new(),
        cookies: std::collections::HashMap::new(),
        form_data: None,
        multipart: None,
    }
}

/// Get current memory usage in MB (simplified implementation)
fn get_memory_usage_mb() -> f64 {
    // In a real implementation, you'd use a system monitoring crate
    // For now, return a placeholder value
    128.0
}

/// Get current CPU usage percentage (simplified implementation)
fn get_cpu_usage_percent() -> f64 {
    // In a real implementation, you'd use a system monitoring crate
    // For now, return a placeholder value
    15.0
}

/// Criterion.rs benchmark functions (when benchmarks feature is enabled)
#[cfg(feature = "benchmarks")]
pub mod criterion_benchmarks {
    use super::*;
    use criterion::{Criterion, black_box};

    pub fn benchmark_handler_performance(c: &mut Criterion) {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let handler: HandlerFn =
            Arc::new(|_req| Box::pin(async { Ok(Response::new(StatusCode::OK)) }));

        c.bench_function("handler_latency", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let request = black_box(create_benchmark_request());
                    black_box(handler(request).await)
                })
            })
        });
    }

    pub fn benchmark_middleware_overhead(c: &mut Criterion) {
        let rt = tokio::runtime::Runtime::new().unwrap();

        c.bench_function("middleware_processing", |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut request = black_box(create_benchmark_request());
                    // Simulate middleware processing
                    black_box(&mut request);
                })
            })
        });
    }

    criterion_group!(
        benches,
        benchmark_handler_performance,
        benchmark_middleware_overhead
    );
}

#[cfg(feature = "benchmarks")]
criterion_main!(criterion_benchmarks::benches);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_latency_collector() {
        let collector = LatencyCollector::new();

        // Record some test latencies
        collector.record_latency(Duration::from_micros(100));
        collector.record_latency(Duration::from_micros(200));
        collector.record_latency(Duration::from_micros(150));

        let metrics = collector.calculate_metrics();
        assert_eq!(metrics.request_count, 3);
        assert_eq!(metrics.min_latency, Duration::from_micros(100));
        assert_eq!(metrics.max_latency, Duration::from_micros(200));
    }

    #[tokio::test]
    async fn test_benchmark_config() {
        let ultra_low = BenchmarkConfig::ultra_low_latency();
        assert_eq!(ultra_low.concurrent_requests, 1000);
        assert_eq!(ultra_low.target_rps, Some(100_000.0));

        let stress = BenchmarkConfig::stress_test();
        assert_eq!(stress.concurrent_requests, 5000);
        assert_eq!(stress.duration, Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_benchmark_results() {
        let mut results = BenchmarkResults::new("test_adapter");

        let metrics = PerformanceMetrics {
            request_count: 10000,
            p99_latency: Duration::from_micros(800), // Under 1ms
            p95_latency: Duration::from_micros(400), // Under 500μs
            avg_latency: Duration::from_micros(150), // Under 200μs
            throughput_rps: 60_000.0,                // Over 50k RPS
            ..Default::default()
        };

        results.add_result("ultra_low_latency", metrics);
        assert!(results.meets_ultra_low_latency_requirements());

        let report = results.generate_report();
        assert!(report.contains("✅ **PASSED**"));
    }
}
