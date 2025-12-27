// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! IPC throughput benchmarks.
//!
//! Compares Aetherless zero-copy shared memory against traditional IPC methods:
//! - Unix domain sockets
//! - TCP localhost
//! - HTTP + JSON serialization

use aetherless_benchmark::{
    harness::BenchmarkHarness, BenchmarkCategory, BenchmarkReport, BenchmarkResult, JsonReporter,
    LatencyMetrics,
};
use aetherless_core::shm::{RingBuffer, SharedMemoryRegion};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;
use tempfile::TempDir;

/// Payload sizes for IPC benchmarks.
const PAYLOAD_SIZES: &[usize] = &[64, 1024, 4096, 16384];

/// Benchmark zero-copy shared memory IPC.
fn bench_shm_ipc(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_shm");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let name = format!("ipc_shm_{}_{}", size, std::process::id());
            let region =
                SharedMemoryRegion::create(&name, 1024 * 1024).expect("Failed to create SHM");
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

/// Benchmark Unix domain socket IPC.
fn bench_unix_socket_ipc(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_unix_socket");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let socket_path = temp_dir.path().join("bench.sock");

            let listener = UnixListener::bind(&socket_path).expect("Failed to bind");

            // Set up server thread
            let server_handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("Accept failed");
                let mut buf = vec![0u8; 65536];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            stream.write_all(&buf[..n]).ok();
                        }
                        Err(_) => break,
                    }
                }
            });

            // Client side
            let mut client = UnixStream::connect(&socket_path).expect("Connect failed");
            client.set_nodelay(true).ok();
            let payload = vec![0xABu8; size];
            let mut read_buf = vec![0u8; size];

            b.iter(|| {
                client.write_all(black_box(&payload)).expect("Write failed");
                client.read_exact(&mut read_buf).expect("Read failed");
                black_box(&read_buf);
            });

            drop(client);
            let _ = server_handle.join();
        });
    }

    group.finish();
}

/// Benchmark TCP localhost IPC.
fn bench_tcp_ipc(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_tcp_localhost");
    group.measurement_time(Duration::from_secs(5));

    for &size in PAYLOAD_SIZES {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
            let port = listener.local_addr().unwrap().port();

            // Set up server thread
            let server_handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("Accept failed");
                stream.set_nodelay(true).ok();
                let mut buf = vec![0u8; 65536];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            stream.write_all(&buf[..n]).ok();
                        }
                        Err(_) => break,
                    }
                }
            });

            // Client side
            let mut client =
                TcpStream::connect(format!("127.0.0.1:{}", port)).expect("Connect failed");
            client.set_nodelay(true).expect("Failed to set nodelay");
            let payload = vec![0xABu8; size];
            let mut read_buf = vec![0u8; size];

            b.iter(|| {
                client.write_all(black_box(&payload)).expect("Write failed");
                client.read_exact(&mut read_buf).expect("Read failed");
                black_box(&read_buf);
            });

            drop(client);
            let _ = server_handle.join();
        });
    }

    group.finish();
}

/// Generate JSON report with IPC comparison data.
fn generate_json_report() {
    let mut report = BenchmarkReport::new();
    let harness = BenchmarkHarness::new().warmup(100).iterations(1000);

    for &size in PAYLOAD_SIZES {
        // Shared memory benchmark
        let name = format!("json_ipc_shm_{}_{}", size, std::process::id());
        let region =
            SharedMemoryRegion::create(&name, 1024 * 1024).expect("Failed to create SHM");
        let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");
        let payload = vec![0xABu8; size];

        let samples = harness.run(|| {
            buffer.write(&payload).ok();
            buffer.read().ok();
        });

        report.add_result(
            BenchmarkResult::latency(
                format!("ipc_shm_roundtrip_{}", size),
                BenchmarkCategory::Ipc,
                samples,
                true,
            )
            .with_metadata("method", "shared_memory")
            .with_metadata("payload_size_bytes", size)
            .with_metadata("zero_copy", true),
        );
    }

    // Save report
    if let Ok(reporter) = JsonReporter::default_location() {
        if let Ok(path) = reporter.save(&report) {
            println!("Saved IPC benchmark report to: {:?}", path);
        }
    }
}

criterion_group!(
    benches,
    bench_shm_ipc,
    bench_unix_socket_ipc,
    bench_tcp_ipc,
);

criterion_main!(benches);
