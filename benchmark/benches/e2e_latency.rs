// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! End-to-end request latency benchmarks.
//!
//! Measures the full request lifecycle from client request to response,
//! including handler orchestration overhead.

use aetherless_benchmark::{
    BenchmarkReport, JsonReporter,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Concurrency levels to test.
const CONCURRENCY_LEVELS: &[usize] = &[1, 10, 50];

/// Benchmark warm request latency (no cold start).
fn bench_warm_request_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_warm_request");
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(10));

    // Set up a simple echo server
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let port = listener.local_addr().unwrap().port();
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let server_handle = std::thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("Failed to set nonblocking");
        while running_clone.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_nonblocking(false).ok();
                let mut buf = [0u8; 4096];
                if let Ok(n) = stream.read(&mut buf) {
                    // Simple HTTP response
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        n,
                        String::from_utf8_lossy(&buf[..n])
                    );
                    stream.write_all(response.as_bytes()).ok();
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    group.bench_function("single_request", |b| {
        let request = format!(
            "GET / HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
            port
        );

        b.iter(|| {
            let mut stream =
                TcpStream::connect(format!("127.0.0.1:{}", port)).expect("Connect failed");
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            stream.write_all(request.as_bytes()).expect("Write failed");

            let mut response = Vec::new();
            let _ = stream.read_to_end(&mut response);
            assert!(!response.is_empty());
        });
    });

    running.store(false, Ordering::Relaxed);
    let _ = server_handle.join();

    group.finish();
}

/// Benchmark concurrent request handling.
fn bench_concurrent_requests(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_concurrent_requests");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(20));

    for &concurrency in CONCURRENCY_LEVELS {
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            &concurrency,
            |b, &concurrency| {
                // Set up server
                let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
                let port = listener.local_addr().unwrap().port();
                let running = Arc::new(AtomicBool::new(true));
                let running_clone = running.clone();

                let server_handle = std::thread::spawn(move || {
                    let mut handles = Vec::new();
                    listener.set_nonblocking(true).ok();

                    while running_clone.load(Ordering::Relaxed) {
                        if let Ok((mut stream, _)) = listener.accept() {
                            let handle = std::thread::spawn(move || {
                                stream.set_nonblocking(false).ok();
                                let mut buf = [0u8; 4096];
                                if stream.read(&mut buf).is_ok() {
                                    let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                                    stream.write_all(response.as_bytes()).ok();
                                }
                            });
                            handles.push(handle);
                        }
                        std::thread::sleep(Duration::from_micros(100));
                    }

                    for h in handles {
                        let _ = h.join();
                    }
                });

                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;

                    for _ in 0..iters {
                        let start = Instant::now();

                        let handles: Vec<_> = (0..concurrency)
                            .map(|_| {
                                std::thread::spawn(move || {
                                    let mut stream = TcpStream::connect(format!(
                                        "127.0.0.1:{}",
                                        port
                                    ))
                                    .expect("Connect failed");
                                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                                    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
                                    stream.write_all(request.as_bytes()).ok();
                                    let mut buf = Vec::new();
                                    let _ = stream.read_to_end(&mut buf);
                                })
                            })
                            .collect();

                        for h in handles {
                            let _ = h.join();
                        }

                        total += start.elapsed();
                    }

                    total
                });

                running.store(false, Ordering::Relaxed);
                let _ = server_handle.join();
            },
        );
    }

    group.finish();
}

/// Benchmark request with full Aetherless handler protocol.
fn bench_handler_protocol_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_handler_protocol");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("socket_handshake", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;

            for _ in 0..iters {
                let temp_dir = TempDir::new().expect("Failed to create temp dir");
                let socket_path = temp_dir.path().join("handler.sock");
                let handler_path = temp_dir.path().join("handler.py");

                std::fs::write(
                    &handler_path,
                    r#"#!/usr/bin/env python3
import os, socket
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')
"#,
                )
                .expect("Failed to write handler");

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&handler_path).unwrap().permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&handler_path, perms).unwrap();
                }

                let listener = UnixListener::bind(&socket_path).expect("Failed to bind");
                listener.set_nonblocking(true).unwrap();

                let start = Instant::now();

                let mut child = Command::new("python3")
                    .arg(&handler_path)
                    .env("AETHER_SOCKET", &socket_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("Failed to spawn");

                // Wait for READY
                let timeout = Duration::from_secs(5);
                let poll_start = Instant::now();
                while poll_start.elapsed() < timeout {
                    if let Ok((mut stream, _)) = listener.accept() {
                        stream.set_nonblocking(false).ok();
                        let mut buf = [0u8; 8];
                        if stream.read(&mut buf).unwrap_or(0) >= 5 && &buf[..5] == b"READY" {
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }

                total += start.elapsed();

                let _ = child.kill();
                let _ = child.wait();
            }

            total
        });
    });

    group.finish();
}

/// Generate JSON report with end-to-end latency data.
#[allow(dead_code)]
fn generate_json_report() {
    let report = BenchmarkReport::new();

    // Note: This would run actual E2E tests and collect samples
    // For now, we just set up the report structure

    if let Ok(reporter) = JsonReporter::default_location() {
        if let Ok(path) = reporter.save(&report) {
            println!("Saved E2E benchmark report to: {:?}", path);
        }
    }
}

criterion_group!(
    benches,
    bench_warm_request_latency,
    bench_concurrent_requests,
    bench_handler_protocol_overhead,
);

criterion_main!(benches);
