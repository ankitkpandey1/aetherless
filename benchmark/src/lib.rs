// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Aetherless Benchmarking Framework
//!
//! A research-level benchmarking framework for measuring and comparing
//! Aetherless performance against traditional serverless approaches.
//!
//! # Benchmark Categories
//!
//! - **Cold Start**: CRIU restore vs fresh spawn vs baseline comparisons
//! - **IPC Performance**: Zero-copy shared memory vs Unix sockets vs HTTP
//! - **Ring Buffer**: Microbenchmarks at various payload sizes
//! - **End-to-End Latency**: Full request lifecycle with concurrent load
//!
//! # Data Output
//!
//! All benchmarks output JSON files with standardized metrics for visualization.

pub mod harness;
pub mod metrics;
pub mod reporter;

pub use harness::BenchmarkHarness;
pub use metrics::{
    BenchmarkCategory, BenchmarkReport, BenchmarkResult, LatencyMetrics, SystemInfo,
};
pub use reporter::JsonReporter;
