// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Standardized metrics types for benchmark results.
//!
//! This module defines the data structures used to capture and serialize
//! benchmark measurements following industry-standard methodology.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sysinfo::System;

/// Categories of benchmarks supported by the framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkCategory {
    /// Cold start latency measurements
    ColdStart,
    /// Inter-process communication performance
    Ipc,
    /// Network stack and eBPF/XDP benchmarks
    Network,
    /// End-to-end request lifecycle
    EndToEnd,
    /// Ring buffer microbenchmarks
    RingBuffer,
}

impl std::fmt::Display for BenchmarkCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchmarkCategory::ColdStart => write!(f, "cold_start"),
            BenchmarkCategory::Ipc => write!(f, "ipc"),
            BenchmarkCategory::Network => write!(f, "network"),
            BenchmarkCategory::EndToEnd => write!(f, "e2e"),
            BenchmarkCategory::RingBuffer => write!(f, "ring_buffer"),
        }
    }
}

/// Latency metrics with statistical analysis.
///
/// Follows research-level benchmarking methodology with percentile distributions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMetrics {
    /// Minimum observed latency in nanoseconds
    pub min_ns: u64,
    /// Maximum observed latency in nanoseconds
    pub max_ns: u64,
    /// Arithmetic mean latency in nanoseconds
    pub mean_ns: f64,
    /// Median (p50) latency in nanoseconds
    pub median_ns: u64,
    /// 95th percentile latency in nanoseconds
    pub p95_ns: u64,
    /// 99th percentile latency in nanoseconds
    pub p99_ns: u64,
    /// Standard deviation in nanoseconds
    pub std_dev_ns: f64,
    /// Raw sample data for visualization (optional, may be truncated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<Vec<u64>>,
}

impl LatencyMetrics {
    /// Calculate metrics from a vector of latency samples (in nanoseconds).
    pub fn from_samples(mut samples: Vec<u64>, keep_raw: bool) -> Self {
        if samples.is_empty() {
            return Self {
                min_ns: 0,
                max_ns: 0,
                mean_ns: 0.0,
                median_ns: 0,
                p95_ns: 0,
                p99_ns: 0,
                std_dev_ns: 0.0,
                samples: None,
            };
        }

        samples.sort_unstable();
        let len = samples.len();

        let min_ns = samples[0];
        let max_ns = samples[len - 1];
        let sum: u64 = samples.iter().sum();
        let mean_ns = sum as f64 / len as f64;
        let median_ns = samples[len / 2];
        let p95_ns = samples[(len as f64 * 0.95) as usize];
        let p99_ns = samples[(len as f64 * 0.99) as usize];

        // Calculate standard deviation
        let variance: f64 = samples
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean_ns;
                diff * diff
            })
            .sum::<f64>()
            / len as f64;
        let std_dev_ns = variance.sqrt();

        // Optionally keep raw samples (truncate if too large for visualization)
        let raw_samples = if keep_raw {
            if len > 10000 {
                // Downsample for storage efficiency
                Some(samples.iter().step_by(len / 1000).copied().collect())
            } else {
                Some(samples)
            }
        } else {
            None
        };

        Self {
            min_ns,
            max_ns,
            mean_ns,
            median_ns,
            p95_ns,
            p99_ns,
            std_dev_ns,
            samples: raw_samples,
        }
    }

    /// Format latency in human-readable form (auto-selects ns/μs/ms).
    pub fn format_latency(ns: u64) -> String {
        if ns < 1_000 {
            format!("{}ns", ns)
        } else if ns < 1_000_000 {
            format!("{:.2}μs", ns as f64 / 1_000.0)
        } else if ns < 1_000_000_000 {
            format!("{:.2}ms", ns as f64 / 1_000_000.0)
        } else {
            format!("{:.2}s", ns as f64 / 1_000_000_000.0)
        }
    }
}

/// Throughput metrics for IPC and network benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputMetrics {
    /// Messages processed per second
    pub messages_per_sec: f64,
    /// Bytes processed per second
    pub bytes_per_sec: f64,
    /// Total messages processed
    pub total_messages: u64,
    /// Total bytes processed
    pub total_bytes: u64,
    /// Duration of the benchmark in nanoseconds
    pub duration_ns: u64,
}

impl ThroughputMetrics {
    /// Calculate throughput from message count, byte count, and duration.
    pub fn calculate(messages: u64, bytes: u64, duration_ns: u64) -> Self {
        let duration_secs = duration_ns as f64 / 1_000_000_000.0;
        Self {
            messages_per_sec: messages as f64 / duration_secs,
            bytes_per_sec: bytes as f64 / duration_secs,
            total_messages: messages,
            total_bytes: bytes,
            duration_ns,
        }
    }

    /// Format throughput in human-readable form.
    pub fn format_bytes_per_sec(bps: f64) -> String {
        if bps < 1_000.0 {
            format!("{:.2} B/s", bps)
        } else if bps < 1_000_000.0 {
            format!("{:.2} KB/s", bps / 1_000.0)
        } else if bps < 1_000_000_000.0 {
            format!("{:.2} MB/s", bps / 1_000_000.0)
        } else {
            format!("{:.2} GB/s", bps / 1_000_000_000.0)
        }
    }
}

/// System information captured at benchmark time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Operating system name
    pub os: String,
    /// OS version
    pub os_version: String,
    /// Kernel version (Linux)
    pub kernel_version: Option<String>,
    /// CPU model name
    pub cpu_model: String,
    /// Number of CPU cores
    pub cpu_cores: usize,
    /// Total system memory in bytes
    pub memory_bytes: u64,
    /// Hostname
    pub hostname: String,
}

impl SystemInfo {
    /// Collect current system information.
    pub fn collect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        Self {
            os: System::name().unwrap_or_else(|| "Unknown".to_string()),
            os_version: System::os_version().unwrap_or_else(|| "Unknown".to_string()),
            kernel_version: System::kernel_version(),
            cpu_model: sys
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_string())
                .unwrap_or_else(|| "Unknown".to_string()),
            cpu_cores: sys.cpus().len(),
            memory_bytes: sys.total_memory(),
            hostname: System::host_name().unwrap_or_else(|| "Unknown".to_string()),
        }
    }
}

/// A single benchmark result with all associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Name of the benchmark
    pub name: String,
    /// Category of the benchmark
    pub category: BenchmarkCategory,
    /// Latency metrics (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyMetrics>,
    /// Throughput metrics (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<ThroughputMetrics>,
    /// Number of iterations/samples
    pub iterations: u64,
    /// Additional metadata specific to this benchmark
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl BenchmarkResult {
    /// Create a new latency-focused benchmark result.
    pub fn latency(
        name: impl Into<String>,
        category: BenchmarkCategory,
        samples: Vec<u64>,
        keep_raw_samples: bool,
    ) -> Self {
        let iterations = samples.len() as u64;
        Self {
            name: name.into(),
            category,
            latency: Some(LatencyMetrics::from_samples(samples, keep_raw_samples)),
            throughput: None,
            iterations,
            metadata: HashMap::new(),
        }
    }

    /// Create a new throughput-focused benchmark result.
    pub fn throughput(
        name: impl Into<String>,
        category: BenchmarkCategory,
        messages: u64,
        bytes: u64,
        duration_ns: u64,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            latency: None,
            throughput: Some(ThroughputMetrics::calculate(messages, bytes, duration_ns)),
            iterations: messages,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the result.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.metadata
            .insert(key.into(), serde_json::to_value(value).unwrap());
        self
    }
}

/// Complete benchmark suite report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Suite identifier
    pub benchmark_suite: String,
    /// Framework version
    pub version: String,
    /// Timestamp when benchmarks were run
    pub timestamp: DateTime<Utc>,
    /// System information
    pub system_info: SystemInfo,
    /// Individual benchmark results
    pub results: Vec<BenchmarkResult>,
}

impl BenchmarkReport {
    /// Create a new benchmark report.
    pub fn new() -> Self {
        Self {
            benchmark_suite: "aetherless-benchmarks".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: Utc::now(),
            system_info: SystemInfo::collect(),
            results: Vec::new(),
        }
    }

    /// Add a result to the report.
    pub fn add_result(&mut self, result: BenchmarkResult) {
        self.results.push(result);
    }
}

impl Default for BenchmarkReport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_metrics_from_samples() {
        let samples = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        let metrics = LatencyMetrics::from_samples(samples, false);

        assert_eq!(metrics.min_ns, 100);
        assert_eq!(metrics.max_ns, 1000);
        assert_eq!(metrics.median_ns, 600);
        assert!((metrics.mean_ns - 550.0).abs() < 0.01);
        assert!(metrics.samples.is_none());
    }

    #[test]
    fn test_latency_format() {
        assert_eq!(LatencyMetrics::format_latency(500), "500ns");
        assert_eq!(LatencyMetrics::format_latency(1500), "1.50μs");
        assert_eq!(LatencyMetrics::format_latency(1_500_000), "1.50ms");
        assert_eq!(LatencyMetrics::format_latency(1_500_000_000), "1.50s");
    }

    #[test]
    fn test_throughput_calculation() {
        let metrics = ThroughputMetrics::calculate(1000, 1_000_000, 1_000_000_000);
        assert!((metrics.messages_per_sec - 1000.0).abs() < 0.01);
        assert!((metrics.bytes_per_sec - 1_000_000.0).abs() < 0.01);
    }

    #[test]
    fn test_system_info_collect() {
        let info = SystemInfo::collect();
        assert!(!info.os.is_empty());
        assert!(info.cpu_cores > 0);
        assert!(info.memory_bytes > 0);
    }

    #[test]
    fn test_benchmark_result_serialization() {
        let result = BenchmarkResult::latency(
            "test_benchmark",
            BenchmarkCategory::ColdStart,
            vec![100, 200, 300],
            false,
        )
        .with_metadata("payload_size", 1024);

        let json = serde_json::to_string_pretty(&result).unwrap();
        assert!(json.contains("test_benchmark"));
        assert!(json.contains("cold_start"));
        assert!(json.contains("payload_size"));
    }
}
