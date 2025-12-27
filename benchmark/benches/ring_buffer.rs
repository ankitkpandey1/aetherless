// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Ring buffer microbenchmarks.
//!
//! Measures the performance of Aetherless's zero-copy shared memory ring buffer
//! at various payload sizes.

use aetherless_benchmark::{BenchmarkCategory, BenchmarkReport, BenchmarkResult, JsonReporter};
use aetherless_core::shm::{RingBuffer, SharedMemoryRegion};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

/// Payload sizes to benchmark (in bytes).
const PAYLOAD_SIZES: &[usize] = &[64, 256, 1024, 4096, 16384, 65536];

/// Benchmark ring buffer write operations.
fn bench_ring_buffer_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_write");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            // Create a fresh ring buffer for each benchmark
            let name = format!("bench_write_{}_{}", size, std::process::id());
            let region = SharedMemoryRegion::create(&name, 1024 * 1024)
                .expect("Failed to create SHM region");
            let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
            let payload = vec![0xABu8; size];

            b.iter(|| {
                // Reset buffer state by recreating
                buffer.write(black_box(&payload)).ok();
                buffer.read().ok();
            });
        });
    }

    group.finish();
}

/// Benchmark ring buffer read operations.
fn bench_ring_buffer_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_read");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let name = format!("bench_read_{}_{}", size, std::process::id());
            let region = SharedMemoryRegion::create(&name, 1024 * 1024)
                .expect("Failed to create SHM region");
            let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
            let payload = vec![0xABu8; size];

            b.iter(|| {
                buffer.write(&payload).ok();
                black_box(buffer.read().ok());
            });
        });
    }

    group.finish();
}

/// Benchmark full write-read cycle latency.
fn bench_ring_buffer_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer_roundtrip");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64 * 2)); // Write + read

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let name = format!("bench_rt_{}_{}", size, std::process::id());
            let region = SharedMemoryRegion::create(&name, 1024 * 1024)
                .expect("Failed to create SHM region");
            let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
            let payload = vec![0xABu8; size];

            b.iter(|| {
                buffer.write(black_box(&payload)).expect("Write failed");
                let result = buffer.read().expect("Read failed");
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark CRC32 checksum calculation overhead.
fn bench_crc32_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc32_overhead");
    group.measurement_time(Duration::from_secs(3));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let payload = vec![0xABu8; size];

            b.iter(|| {
                black_box(crc32fast::hash(black_box(&payload)));
            });
        });
    }

    group.finish();
}

/// Run benchmarks and generate JSON report for visualization.
#[allow(dead_code)]
fn generate_json_report() {
    use aetherless_benchmark::harness::BenchmarkHarness;

    let mut report = BenchmarkReport::new();
    let harness = BenchmarkHarness::new().warmup(50).iterations(1000);

    for &size in PAYLOAD_SIZES {
        let name = format!("json_bench_{}_{}", size, std::process::id());
        let region =
            SharedMemoryRegion::create(&name, 1024 * 1024).expect("Failed to create SHM region");
        let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
        let payload = vec![0xABu8; size];

        // Benchmark roundtrip
        let samples = harness.run(|| {
            buffer.write(&payload).ok();
            buffer.read().ok();
        });

        let result = BenchmarkResult::latency(
            format!("ring_buffer_roundtrip_{}", size),
            BenchmarkCategory::RingBuffer,
            samples,
            true,
        )
        .with_metadata("payload_size_bytes", size);

        report.add_result(result);
    }

    // Save report
    if let Ok(reporter) = JsonReporter::default_location() {
        if let Ok(path) = reporter.save(&report) {
            println!("Saved benchmark report to: {:?}", path);
        }
    }
}

criterion_group!(
    benches,
    bench_ring_buffer_write,
    bench_ring_buffer_read,
    bench_ring_buffer_roundtrip,
    bench_crc32_overhead,
);

criterion_main!(benches);

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_benchmark_can_run() {
        // Just verify the setup works
        let name = format!("test_bench_{}", std::process::id());
        let region =
            SharedMemoryRegion::create(&name, 64 * 1024).expect("Failed to create SHM region");
        let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
        let payload = b"test payload";

        buffer.write(payload).expect("Write failed");
        let result = buffer.read().expect("Read failed");
        assert_eq!(result, payload);
    }
}
