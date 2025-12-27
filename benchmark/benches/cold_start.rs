// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Cold start latency benchmarks.
//!
//! Measures cold start performance of Aetherless compared to baseline approaches.
//! Key metrics: process spawn time, initialization time, time to READY signal.

use aetherless_benchmark::{
    harness::BenchmarkHarness, BenchmarkCategory, BenchmarkReport, BenchmarkResult, JsonReporter,
};
use criterion::{criterion_group, criterion_main, Criterion};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Benchmark fresh Python process spawn (baseline - simulates process creation overhead).
fn bench_python_process_spawn(c: &mut Criterion) {
    c.bench_function("cold_start_python_spawn", |b| {
        b.iter(|| {
            let child = Command::new("python3")
                .arg("-c")
                .arg("print('ready')")
                .stdout(Stdio::piped())
                .spawn()
                .expect("Failed to spawn python");

            let output = child.wait_with_output().expect("Failed to wait");
            assert!(output.status.success());
        });
    });
}

/// Benchmark Python HTTP server cold start (simulates traditional serverless).
fn bench_python_http_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_start_http");
    group.sample_size(20); // Fewer samples due to longer duration
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("python_http_server", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;

            for _ in 0..iters {
                let temp_dir = TempDir::new().expect("Failed to create temp dir");
                let socket_path = temp_dir.path().join("test.sock");

                // Create handler script
                let handler_script = temp_dir.path().join("handler.py");
                std::fs::write(
                    &handler_script,
                    r#"#!/usr/bin/env python3
import os, socket
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'ok')
    def log_message(self, *args): pass

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')
HTTPServer(('127.0.0.1', 0), Handler).handle_request()
"#,
                )
                .expect("Failed to write handler");

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&handler_script).unwrap().permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&handler_script, perms).unwrap();
                }

                let listener = UnixListener::bind(&socket_path).expect("Failed to bind socket");
                listener.set_nonblocking(true).unwrap();

                let start = Instant::now();

                let mut child = Command::new("python3")
                    .arg(&handler_script)
                    .env("AETHER_SOCKET", &socket_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("Failed to spawn handler");

                // Wait for READY signal
                let timeout = Duration::from_secs(10);
                let poll_start = Instant::now();
                let mut ready = false;

                while poll_start.elapsed() < timeout {
                    if let Ok((mut stream, _)) = listener.accept() {
                        stream.set_nonblocking(false).ok();
                        let mut buf = [0u8; 8];
                        if stream.read(&mut buf).unwrap_or(0) >= 5 && &buf[..5] == b"READY" {
                            ready = true;
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }

                let elapsed = start.elapsed();

                let _ = child.kill();
                let _ = child.wait();

                if ready {
                    total += elapsed;
                }
            }

            total
        });
    });

    group.finish();
}

/// Benchmark Node.js cold start (if available).
fn bench_nodejs_process_spawn(c: &mut Criterion) {
    // Check if node is available
    if Command::new("node").arg("--version").output().is_err() {
        println!("Node.js not available, skipping benchmark");
        return;
    }

    c.bench_function("cold_start_nodejs_spawn", |b| {
        b.iter(|| {
            let child = Command::new("node")
                .arg("-e")
                .arg("console.log('ready')")
                .stdout(Stdio::piped())
                .spawn()
                .expect("Failed to spawn node");

            let output = child.wait_with_output().expect("Failed to wait");
            assert!(output.status.success());
        });
    });
}

/// Generate JSON report with cold start measurements.
fn generate_json_report() {
    let mut report = BenchmarkReport::new();
    let harness = BenchmarkHarness::new().warmup(5).iterations(50);

    // Python process spawn
    let samples = harness.run(|| {
        let child = Command::new("python3")
            .arg("-c")
            .arg("print('ready')")
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn");
        let _ = child.wait_with_output();
    });

    report.add_result(
        BenchmarkResult::latency(
            "cold_start_python_process_spawn",
            BenchmarkCategory::ColdStart,
            samples,
            true,
        )
        .with_metadata("runtime", "python3")
        .with_metadata("operation", "process_spawn"),
    );

    // Node.js process spawn (if available)
    if Command::new("node").arg("--version").output().is_ok() {
        let samples = harness.run(|| {
            let child = Command::new("node")
                .arg("-e")
                .arg("console.log('ready')")
                .stdout(Stdio::piped())
                .spawn()
                .expect("Failed to spawn");
            let _ = child.wait_with_output();
        });

        report.add_result(
            BenchmarkResult::latency(
                "cold_start_nodejs_process_spawn",
                BenchmarkCategory::ColdStart,
                samples,
                true,
            )
            .with_metadata("runtime", "nodejs")
            .with_metadata("operation", "process_spawn"),
        );
    }

    // Save report
    if let Ok(reporter) = JsonReporter::default_location() {
        if let Ok(path) = reporter.save(&report) {
            println!("Saved cold start benchmark report to: {:?}", path);
        }
    }
}

criterion_group!(
    benches,
    bench_python_process_spawn,
    bench_python_http_cold_start,
    bench_nodejs_process_spawn,
);

criterion_main!(benches);
