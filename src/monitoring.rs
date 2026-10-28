//! Production Monitoring and Logging System
//!
//! This module provides comprehensive monitoring, logging, and observability features
//! including metrics collection, distributed tracing, health checks, and alerting.

use crate::{
    config::MonitoringConfig,
    types::{Request, Response},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime},
};
use tracing::{debug, error, info, warn};

/// Production monitoring system
pub struct MonitoringSystem {
    #[allow(dead_code)]
    config: MonitoringConfig,
    metrics: Arc<RwLock<MetricsRegistry>>,
    health_checks: Arc<RwLock<Vec<HealthCheck>>>,
    alerts: Arc<Mutex<Vec<Alert>>>,
    traces: Arc<Mutex<Vec<TraceSpan>>>,
}

impl MonitoringSystem {
    pub fn new(config: MonitoringConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(MetricsRegistry::new())),
            health_checks: Arc::new(RwLock::new(Vec::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
            traces: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Record a request metric
    pub fn record_request(&self, request: &Request, response: &Response, duration: Duration) {
        let mut metrics = self.metrics.write().unwrap();

        // Record request count
        metrics.increment_counter(
            "requests_total",
            &[
                ("method", request.method.to_string()),
                ("status", response.status.as_u16().to_string()),
            ],
        );

        // Record response time
        metrics.record_histogram(
            "request_duration_seconds",
            duration.as_secs_f64(),
            &[
                ("method", request.method.to_string()),
                ("endpoint", request.uri.path().to_string()),
            ],
        );

        // Record status code distribution
        metrics.increment_counter(
            "responses_by_status",
            &[
                ("status_code", response.status.as_u16().to_string()),
                (
                    "status_class",
                    format!("{}xx", response.status.as_u16() / 100),
                ),
            ],
        );

        // Track error rates
        if response.status.as_u16() >= 400 {
            metrics.increment_counter(
                "errors_total",
                &[
                    ("status", response.status.as_u16().to_string()),
                    ("endpoint", request.uri.path().to_string()),
                ],
            );
        }
    }

    /// Record a custom metric
    pub fn record_metric(&self, name: &str, value: f64, labels: &[(&str, String)]) {
        let mut metrics = self.metrics.write().unwrap();
        metrics.record_gauge(name, value, labels);
    }

    /// Start a distributed trace span
    pub fn start_trace(&self, operation: &str, request: &Request) -> TraceContext {
        let trace_id = generate_trace_id();
        let span_id = generate_span_id();

        let span = TraceSpan {
            trace_id: trace_id.clone(),
            span_id: span_id.clone(),
            parent_span_id: None,
            operation_name: operation.to_string(),
            start_time: Instant::now(),
            end_time: None,
            duration: None,
            tags: {
                let mut tags = HashMap::new();
                tags.insert("http.method".to_string(), request.method.to_string());
                tags.insert("http.url".to_string(), request.uri.to_string());
                if let Some(user_agent) = request.headers.get("user-agent") {
                    tags.insert("http.user_agent".to_string(), user_agent.clone());
                }
                tags
            },
            logs: Vec::new(),
        };

        {
            let mut traces = self.traces.lock().unwrap();
            traces.push(span);
        }

        TraceContext {
            trace_id,
            span_id,
            monitoring: Arc::downgrade(&self.metrics),
        }
    }

    /// Add a health check
    pub fn add_health_check(&self, check: HealthCheck) {
        let mut checks = self.health_checks.write().unwrap();
        checks.push(check);
    }

    /// Run all health checks
    pub async fn run_health_checks(&self) -> HealthStatus {
        let checks = self.health_checks.read().unwrap().clone();
        let mut results = Vec::new();
        let mut overall_status = HealthLevel::Healthy;

        for check in checks {
            let start_time = Instant::now();
            let result = (check.check_fn)().await;
            let duration = start_time.elapsed();

            // Update overall status based on individual check
            if result.status == HealthLevel::Critical {
                overall_status = HealthLevel::Critical;
            } else if result.status == HealthLevel::Warning
                && overall_status == HealthLevel::Healthy
            {
                overall_status = HealthLevel::Warning;
            }

            let check_name = check.name.clone();
            let result_status = result.status;
            let result_message = result.message.clone();

            let check_result = HealthCheckResult {
                name: check.name,
                status: result.status,
                message: result.message,
                duration,
                timestamp: SystemTime::now(),
            };

            results.push(check_result);

            // Log health check results
            match result_status {
                HealthLevel::Healthy => {
                    debug!("Health check '{}' passed: {}", check_name, result_message)
                }
                HealthLevel::Warning => {
                    warn!("Health check '{}' warning: {}", check_name, result_message)
                }
                HealthLevel::Critical => {
                    error!("Health check '{}' failed: {}", check_name, result_message)
                }
            }
        }

        HealthStatus {
            overall_status,
            checks: results,
            timestamp: SystemTime::now(),
        }
    }

    /// Create an alert
    pub fn create_alert(&self, alert: Alert) {
        info!("Alert created: {} - {}", alert.severity, alert.message);

        let mut alerts = self.alerts.lock().unwrap();
        alerts.push(alert);

        // Keep only last 100 alerts
        if alerts.len() > 100 {
            let excess = alerts.len() - 100;
            alerts.drain(0..excess);
        }
    }

    /// Get system metrics
    pub fn get_metrics(&self) -> MetricsSnapshot {
        let metrics = self.metrics.read().unwrap();
        metrics.snapshot()
    }

    /// Get recent alerts
    pub fn get_recent_alerts(&self, since: SystemTime) -> Vec<Alert> {
        let alerts = self.alerts.lock().unwrap();
        alerts
            .iter()
            .filter(|alert| alert.timestamp > since)
            .cloned()
            .collect()
    }

    /// Get performance statistics
    pub fn get_performance_stats(&self) -> PerformanceStats {
        let metrics = self.metrics.read().unwrap();

        let total_requests = metrics.get_counter("requests_total").unwrap_or(0.0);
        let error_requests = metrics.get_counter("errors_total").unwrap_or(0.0);
        let error_rate = if total_requests > 0.0 {
            error_requests / total_requests
        } else {
            0.0
        };

        let avg_response_time = metrics
            .get_histogram_avg("request_duration_seconds")
            .unwrap_or(0.0);
        let p95_response_time = metrics
            .get_histogram_percentile("request_duration_seconds", 95.0)
            .unwrap_or(0.0);

        PerformanceStats {
            total_requests: total_requests as u64,
            error_rate,
            avg_response_time,
            p95_response_time,
            timestamp: SystemTime::now(),
        }
    }
}

/// Metrics registry for storing and aggregating metrics
#[derive(Debug)]
pub struct MetricsRegistry {
    counters: HashMap<String, CounterMetric>,
    gauges: HashMap<String, GaugeMetric>,
    histograms: HashMap<String, HistogramMetric>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
        }
    }

    /// Increment a counter metric
    pub fn increment_counter(&mut self, name: &str, labels: &[(&str, String)]) {
        let key = format!("{}:{}", name, serialize_labels(labels));
        let counter = self
            .counters
            .entry(key)
            .or_insert_with(|| CounterMetric::new(name, labels));
        counter.increment();
    }

    /// Record a gauge value
    pub fn record_gauge(&mut self, name: &str, value: f64, labels: &[(&str, String)]) {
        let key = format!("{}:{}", name, serialize_labels(labels));
        let gauge = self
            .gauges
            .entry(key)
            .or_insert_with(|| GaugeMetric::new(name, labels));
        gauge.set(value);
    }

    /// Record a histogram value
    pub fn record_histogram(&mut self, name: &str, value: f64, labels: &[(&str, String)]) {
        let key = format!("{}:{}", name, serialize_labels(labels));
        let histogram = self
            .histograms
            .entry(key)
            .or_insert_with(|| HistogramMetric::new(name, labels));
        histogram.record(value);
    }

    /// Get counter value
    pub fn get_counter(&self, name: &str) -> Option<f64> {
        self.counters
            .values()
            .filter(|c| c.name == name)
            .map(|c| c.value)
            .sum::<f64>()
            .into()
    }

    /// Get histogram average
    pub fn get_histogram_avg(&self, name: &str) -> Option<f64> {
        let histograms: Vec<_> = self
            .histograms
            .values()
            .filter(|h| h.name == name)
            .collect();

        if histograms.is_empty() {
            return None;
        }

        let total_sum: f64 = histograms.iter().map(|h| h.sum).sum();
        let total_count: u64 = histograms.iter().map(|h| h.count).sum();

        if total_count > 0 {
            Some(total_sum / total_count as f64)
        } else {
            None
        }
    }

    /// Get histogram percentile
    pub fn get_histogram_percentile(&self, name: &str, percentile: f64) -> Option<f64> {
        let histograms: Vec<_> = self
            .histograms
            .values()
            .filter(|h| h.name == name)
            .collect();

        if histograms.is_empty() {
            return None;
        }

        // Simplified percentile calculation (in production, use proper histogram buckets)
        let mut all_values = Vec::new();
        for histogram in histograms {
            for &value in &histogram.values {
                all_values.push(value);
            }
        }

        if all_values.is_empty() {
            return None;
        }

        all_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let index = ((percentile / 100.0) * (all_values.len() - 1) as f64) as usize;
        Some(all_values[index])
    }

    /// Create a snapshot of all metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            counters: self.counters.clone(),
            gauges: self.gauges.clone(),
            histograms: self.histograms.clone(),
            timestamp: SystemTime::now(),
        }
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Counter metric
#[derive(Debug, Clone)]
pub struct CounterMetric {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
}

impl CounterMetric {
    pub fn new(name: &str, labels: &[(&str, String)]) -> Self {
        Self {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            value: 0.0,
        }
    }

    pub fn increment(&mut self) {
        self.value += 1.0;
    }

    pub fn add(&mut self, value: f64) {
        self.value += value;
    }
}

/// Gauge metric
#[derive(Debug, Clone)]
pub struct GaugeMetric {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
}

impl GaugeMetric {
    pub fn new(name: &str, labels: &[(&str, String)]) -> Self {
        Self {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            value: 0.0,
        }
    }

    pub fn set(&mut self, value: f64) {
        self.value = value;
    }
}

/// Histogram metric
#[derive(Debug, Clone)]
pub struct HistogramMetric {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub values: Vec<f64>,
    pub sum: f64,
    pub count: u64,
}

impl HistogramMetric {
    pub fn new(name: &str, labels: &[(&str, String)]) -> Self {
        Self {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            values: Vec::new(),
            sum: 0.0,
            count: 0,
        }
    }

    pub fn record(&mut self, value: f64) {
        self.values.push(value);
        self.sum += value;
        self.count += 1;

        // Keep only last 1000 values to prevent memory growth
        if self.values.len() > 1000 {
            let removed = self.values.remove(0);
            self.sum -= removed;
            self.count -= 1;
        }
    }
}

/// Metrics snapshot
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub counters: HashMap<String, CounterMetric>,
    pub gauges: HashMap<String, GaugeMetric>,
    pub histograms: HashMap<String, HistogramMetric>,
    pub timestamp: SystemTime,
}

/// Distributed tracing context
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    monitoring: std::sync::Weak<RwLock<MetricsRegistry>>,
}

impl TraceContext {
    /// Create a child span
    pub fn create_child(&self, _operation: &str) -> TraceContext {
        TraceContext {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            monitoring: self.monitoring.clone(),
        }
    }

    /// Add a tag to the current span
    pub fn set_tag(&self, key: &str, value: &str) {
        debug!(
            "Trace {} span {}: {} = {}",
            self.trace_id, self.span_id, key, value
        );
    }

    /// Log an event
    pub fn log_event(&self, message: &str) {
        info!("Trace {} span {}: {}", self.trace_id, self.span_id, message);
    }
}

/// Trace span
#[derive(Debug, Clone)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub duration: Option<Duration>,
    pub tags: HashMap<String, String>,
    pub logs: Vec<TraceLog>,
}

/// Trace log entry
#[derive(Debug, Clone)]
pub struct TraceLog {
    pub timestamp: Instant,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Debug, Clone)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Health check system
pub type HealthCheckFn = Box<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = HealthCheckResult> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct HealthCheck {
    pub name: String,
    pub check_fn: Arc<HealthCheckFn>,
}

impl std::fmt::Debug for HealthCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthCheck")
            .field("name", &self.name)
            .finish()
    }
}

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub name: String,
    pub status: HealthLevel,
    pub message: String,
    pub duration: Duration,
    pub timestamp: SystemTime,
}

/// Health status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthLevel {
    Healthy,
    Warning,
    Critical,
}

/// Overall health status
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub overall_status: HealthLevel,
    pub checks: Vec<HealthCheckResult>,
    pub timestamp: SystemTime,
}

/// Alert system
#[derive(Debug, Clone)]
pub struct Alert {
    pub id: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub source: String,
    pub timestamp: SystemTime,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "INFO"),
            AlertSeverity::Warning => write!(f, "WARNING"),
            AlertSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceStats {
    pub total_requests: u64,
    pub error_rate: f64,
    pub avg_response_time: f64,
    pub p95_response_time: f64,
    pub timestamp: SystemTime,
}

/// Utility functions
fn serialize_labels(labels: &[(&str, String)]) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(",")
}

fn generate_trace_id() -> String {
    format!("{:016x}", rand::random::<u64>())
}

fn generate_span_id() -> String {
    format!("{:08x}", rand::random::<u32>())
}

/// Built-in health checks
pub mod health_checks {
    use super::*;

    /// Create a memory usage health check
    pub fn memory_usage(threshold_mb: u64) -> HealthCheck {
        HealthCheck {
            name: "memory_usage".to_string(),
            check_fn: Arc::new(Box::new(move || {
                Box::pin(async move {
                    // Simplified memory check (in production, use proper system metrics)
                    let memory_mb = get_memory_usage_mb();

                    if memory_mb > threshold_mb {
                        HealthCheckResult {
                            name: "memory_usage".to_string(),
                            status: HealthLevel::Critical,
                            message: format!(
                                "Memory usage {}MB exceeds threshold {}MB",
                                memory_mb, threshold_mb
                            ),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    } else if memory_mb > threshold_mb * 80 / 100 {
                        HealthCheckResult {
                            name: "memory_usage".to_string(),
                            status: HealthLevel::Warning,
                            message: format!(
                                "Memory usage {}MB approaching threshold {}MB",
                                memory_mb, threshold_mb
                            ),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    } else {
                        HealthCheckResult {
                            name: "memory_usage".to_string(),
                            status: HealthLevel::Healthy,
                            message: format!("Memory usage {}MB is normal", memory_mb),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    }
                })
            })),
        }
    }

    /// Create a disk space health check
    pub fn disk_space(path: String, threshold_gb: u64) -> HealthCheck {
        HealthCheck {
            name: format!("disk_space_{}", path),
            check_fn: Arc::new(Box::new(move || {
                let path = path.clone();
                Box::pin(async move {
                    // Simplified disk space check
                    let free_gb = get_free_disk_space_gb(&path);

                    if free_gb < threshold_gb {
                        HealthCheckResult {
                            name: format!("disk_space_{}", path),
                            status: HealthLevel::Critical,
                            message: format!(
                                "Free disk space {}GB below threshold {}GB",
                                free_gb, threshold_gb
                            ),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    } else if free_gb < threshold_gb * 2 {
                        HealthCheckResult {
                            name: format!("disk_space_{}", path),
                            status: HealthLevel::Warning,
                            message: format!("Free disk space {}GB low", free_gb),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    } else {
                        HealthCheckResult {
                            name: format!("disk_space_{}", path),
                            status: HealthLevel::Healthy,
                            message: format!("Free disk space {}GB is sufficient", free_gb),
                            duration: Duration::from_millis(1),
                            timestamp: SystemTime::now(),
                        }
                    }
                })
            })),
        }
    }

    /// Create a database connection health check
    pub fn database_connection(connection_string: String) -> HealthCheck {
        HealthCheck {
            name: "database_connection".to_string(),
            check_fn: Arc::new(Box::new(move || {
                let _connection_string = connection_string.clone();
                Box::pin(async move {
                    // Simplified database connection check
                    let start = Instant::now();
                    let connected = test_database_connection().await;
                    let duration = start.elapsed();

                    if connected {
                        HealthCheckResult {
                            name: "database_connection".to_string(),
                            status: HealthLevel::Healthy,
                            message: "Database connection successful".to_string(),
                            duration,
                            timestamp: SystemTime::now(),
                        }
                    } else {
                        HealthCheckResult {
                            name: "database_connection".to_string(),
                            status: HealthLevel::Critical,
                            message: "Database connection failed".to_string(),
                            duration,
                            timestamp: SystemTime::now(),
                        }
                    }
                })
            })),
        }
    }

    // Simplified system metric functions (in production, use proper system libraries)
    fn get_memory_usage_mb() -> u64 {
        // Placeholder - in production, use system crates like sysinfo
        100
    }

    fn get_free_disk_space_gb(_path: &str) -> u64 {
        // Placeholder - in production, use system crates like sysinfo
        10
    }

    async fn test_database_connection() -> bool {
        // Placeholder - in production, implement actual database connectivity test
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_monitoring_system() {
        let config = MonitoringConfig::default();
        let monitoring = MonitoringSystem::new(config);

        // Test metric recording
        monitoring.record_metric("test_metric", 42.0, &[("label", "value".to_string())]);

        let metrics = monitoring.get_metrics();
        assert!(!metrics.gauges.is_empty());
    }

    #[tokio::test]
    async fn test_health_checks() {
        let config = MonitoringConfig::default();
        let monitoring = MonitoringSystem::new(config);

        monitoring.add_health_check(health_checks::memory_usage(1000));

        let health_status = monitoring.run_health_checks().await;
        assert_eq!(health_status.checks.len(), 1);
    }

    #[test]
    fn test_metrics_registry() {
        let mut registry = MetricsRegistry::new();

        registry.increment_counter("test_counter", &[("method", "GET".to_string())]);
        registry.record_gauge("test_gauge", 42.0, &[]);
        registry.record_histogram("test_histogram", 1.5, &[]);

        assert_eq!(registry.get_counter("test_counter"), Some(1.0));
        assert_eq!(registry.get_histogram_avg("test_histogram"), Some(1.5));
    }

    #[test]
    fn test_trace_context() {
        let trace = TraceContext {
            trace_id: "test_trace".to_string(),
            span_id: "test_span".to_string(),
            monitoring: std::sync::Weak::new(),
        };

        let child = trace.create_child("child_operation");
        assert_eq!(child.trace_id, "test_trace");
        assert_ne!(child.span_id, "test_span");
    }

    #[test]
    fn test_alert_creation() {
        let config = MonitoringConfig::default();
        let monitoring = MonitoringSystem::new(config);

        let alert = Alert {
            id: "test_alert".to_string(),
            severity: AlertSeverity::Warning,
            message: "Test alert message".to_string(),
            source: "test_source".to_string(),
            timestamp: SystemTime::now(),
            metadata: HashMap::new(),
        };

        monitoring.create_alert(alert);

        let recent_alerts =
            monitoring.get_recent_alerts(SystemTime::now() - Duration::from_secs(60));
        assert_eq!(recent_alerts.len(), 1);
    }
}
