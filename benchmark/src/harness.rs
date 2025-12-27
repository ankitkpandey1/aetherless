// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Benchmark harness for running and timing operations.
//!
//! Provides utilities for measuring execution time with high precision
//! and collecting samples for statistical analysis.

use std::time::{Duration, Instant};

/// A benchmark harness for measuring operation latency.
pub struct BenchmarkHarness {
    /// Number of warmup iterations before measurement
    warmup_iterations: u64,
    /// Number of measurement iterations
    measurement_iterations: u64,
    /// Whether to keep raw sample data
    keep_raw_samples: bool,
}

impl BenchmarkHarness {
    /// Create a new benchmark harness with default settings.
    pub fn new() -> Self {
        Self {
            warmup_iterations: 10,
            measurement_iterations: 100,
            keep_raw_samples: true,
        }
    }

    /// Set the number of warmup iterations.
    pub fn warmup(mut self, iterations: u64) -> Self {
        self.warmup_iterations = iterations;
        self
    }

    /// Set the number of measurement iterations.
    pub fn iterations(mut self, iterations: u64) -> Self {
        self.measurement_iterations = iterations;
        self
    }

    /// Set whether to keep raw sample data.
    pub fn keep_samples(mut self, keep: bool) -> Self {
        self.keep_raw_samples = keep;
        self
    }

    /// Run a benchmark and collect latency samples.
    ///
    /// The closure should perform a single iteration of the operation being measured.
    /// Returns a vector of latency samples in nanoseconds.
    pub fn run<F>(&self, mut operation: F) -> Vec<u64>
    where
        F: FnMut(),
    {
        // Warmup phase
        for _ in 0..self.warmup_iterations {
            operation();
        }

        // Measurement phase
        let mut samples = Vec::with_capacity(self.measurement_iterations as usize);
        for _ in 0..self.measurement_iterations {
            let start = Instant::now();
            operation();
            let elapsed = start.elapsed();
            samples.push(elapsed.as_nanos() as u64);
        }

        samples
    }

    /// Run a benchmark with setup and teardown phases.
    ///
    /// Setup is called before each iteration, teardown after.
    /// Only the operation time is measured.
    pub fn run_with_setup<S, T, O>(&self, mut setup: S, mut operation: O, mut teardown: T) -> Vec<u64>
    where
        S: FnMut(),
        O: FnMut(),
        T: FnMut(),
    {
        // Warmup phase
        for _ in 0..self.warmup_iterations {
            setup();
            operation();
            teardown();
        }

        // Measurement phase
        let mut samples = Vec::with_capacity(self.measurement_iterations as usize);
        for _ in 0..self.measurement_iterations {
            setup();

            let start = Instant::now();
            operation();
            let elapsed = start.elapsed();

            teardown();
            samples.push(elapsed.as_nanos() as u64);
        }

        samples
    }

    /// Run a throughput benchmark for a fixed duration.
    ///
    /// Returns (total_operations, total_duration_ns).
    pub fn run_throughput<F>(&self, duration: Duration, mut operation: F) -> (u64, u64)
    where
        F: FnMut() -> u64, // Returns bytes processed per operation
    {
        // Warmup
        for _ in 0..self.warmup_iterations {
            operation();
        }

        // Measurement
        let start = Instant::now();
        let mut operations = 0u64;

        while start.elapsed() < duration {
            let _bytes = operation();
            operations += 1;
        }

        let elapsed = start.elapsed();
        (operations, elapsed.as_nanos() as u64)
    }

    /// Check if raw samples should be kept.
    pub fn should_keep_samples(&self) -> bool {
        self.keep_raw_samples
    }
}

impl Default for BenchmarkHarness {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer for measuring individual operations.
pub struct Timer {
    start: Instant,
}

impl Timer {
    /// Start a new timer.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Stop the timer and return elapsed nanoseconds.
    pub fn stop(self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }

    /// Stop the timer and return elapsed duration.
    pub fn elapsed(self) -> Duration {
        self.start.elapsed()
    }
}

/// Measure the execution time of a closure.
pub fn measure<F, T>(f: F) -> (T, Duration)
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    (result, elapsed)
}

/// Measure multiple executions and return samples.
pub fn measure_n<F>(iterations: u64, mut f: F) -> Vec<u64>
where
    F: FnMut(),
{
    let mut samples = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_nanos() as u64);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_harness_basic() {
        let harness = BenchmarkHarness::new().warmup(5).iterations(20);

        let samples = harness.run(|| {
            thread::sleep(Duration::from_micros(100));
        });

        assert_eq!(samples.len(), 20);
        // Each sample should be at least 100μs
        for sample in &samples {
            assert!(*sample >= 100_000, "Sample {} < 100μs", sample);
        }
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start();
        thread::sleep(Duration::from_millis(10));
        let elapsed = timer.stop();

        // Should be at least 10ms
        assert!(elapsed >= 10_000_000, "Elapsed {} < 10ms", elapsed);
    }

    #[test]
    fn test_measure() {
        let (result, duration) = measure(|| {
            thread::sleep(Duration::from_millis(5));
            42
        });

        assert_eq!(result, 42);
        assert!(duration >= Duration::from_millis(5));
    }

    #[test]
    fn test_measure_n() {
        let samples = measure_n(10, || {
            thread::sleep(Duration::from_micros(50));
        });

        assert_eq!(samples.len(), 10);
    }
}
