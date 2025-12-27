// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! CLI tool to run all benchmarks and generate reports.

use aetherless_benchmark::{BenchmarkCategory, BenchmarkReport, BenchmarkResult, JsonReporter};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "run_benchmarks")]
#[command(about = "Run Aetherless benchmarks and generate JSON reports")]
struct Args {
    /// Output directory for benchmark data
    #[arg(short, long, default_value = "data")]
    output: PathBuf,

    /// Number of iterations for each benchmark
    #[arg(short, long, default_value_t = 100)]
    iterations: u64,

    /// Categories to run (all if not specified)
    #[arg(short, long)]
    category: Option<Vec<String>>,

    /// Run in quick mode (fewer iterations)
    #[arg(long)]
    quick: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let iterations = if args.quick { 10 } else { args.iterations };

    println!("Aetherless Benchmark Suite");
    println!("==========================");
    println!("Output directory: {:?}", args.output);
    println!("Iterations: {}", iterations);
    println!();

    let reporter = JsonReporter::new(&args.output)?;
    let mut report = BenchmarkReport::new();

    // Determine which categories to run
    let run_all = args.category.is_none();
    let categories: Vec<String> = args.category.unwrap_or_default();

    let should_run =
        |cat: &str| -> bool { run_all || categories.iter().any(|c| c.eq_ignore_ascii_case(cat)) };

    // Ring buffer benchmarks
    if should_run("ring_buffer") || should_run("ipc") {
        println!("Running ring buffer benchmarks...");
        run_ring_buffer_benchmarks(&mut report, iterations);
    }

    // Cold start benchmarks
    if should_run("cold_start") {
        println!("Running cold start benchmarks...");
        run_cold_start_benchmarks(&mut report, iterations);
    }

    // IPC comparison benchmarks
    if should_run("ipc") {
        println!("Running IPC comparison benchmarks...");
        run_ipc_benchmarks(&mut report, iterations);
    }

    // Save report
    let path = reporter.save(&report)?;
    println!();
    println!("Benchmark report saved to: {:?}", path);
    println!();

    // Print summary
    print_summary(&report);

    Ok(())
}

fn run_ring_buffer_benchmarks(report: &mut BenchmarkReport, iterations: u64) {
    use aetherless_benchmark::harness::BenchmarkHarness;
    use aetherless_core::shm::{RingBuffer, SharedMemoryRegion};

    let harness = BenchmarkHarness::new()
        .warmup(iterations / 10)
        .iterations(iterations);

    let payload_sizes = [64, 1024, 4096, 16384, 65536];

    for size in payload_sizes {
        let name = format!("bench_rb_{}_{}", size, std::process::id());
        if let Ok(region) = SharedMemoryRegion::create(&name, 1024 * 1024) {
            if let Ok(buffer) = RingBuffer::new(region) {
                let payload = vec![0xABu8; size];

                let samples = harness.run(|| {
                    buffer.write(&payload).ok();
                    buffer.read().ok();
                });

                report.add_result(
                    BenchmarkResult::latency(
                        format!("ring_buffer_roundtrip_{}", size),
                        BenchmarkCategory::RingBuffer,
                        samples,
                        true,
                    )
                    .with_metadata("payload_size_bytes", size),
                );

                println!("  ✓ ring_buffer_roundtrip_{}", size);
            }
        }
    }
}

fn run_cold_start_benchmarks(report: &mut BenchmarkReport, iterations: u64) {
    use aetherless_benchmark::harness::BenchmarkHarness;
    use std::process::{Command, Stdio};

    let harness = BenchmarkHarness::new()
        .warmup(5)
        .iterations(iterations.min(50)); // Cold starts are slow

    // Python process spawn
    let samples = harness.run(|| {
        let child = Command::new("python3")
            .arg("-c")
            .arg("print('ready')")
            .stdout(Stdio::piped())
            .spawn();
        if let Ok(c) = child {
            let _ = c.wait_with_output();
        }
    });

    report.add_result(
        BenchmarkResult::latency(
            "cold_start_python_process",
            BenchmarkCategory::ColdStart,
            samples,
            true,
        )
        .with_metadata("runtime", "python3"),
    );
    println!("  ✓ cold_start_python_process");

    // Node.js (if available)
    if Command::new("node").arg("--version").output().is_ok() {
        let samples = harness.run(|| {
            let child = Command::new("node")
                .arg("-e")
                .arg("console.log('ready')")
                .stdout(Stdio::piped())
                .spawn();
            if let Ok(c) = child {
                let _ = c.wait_with_output();
            }
        });

        report.add_result(
            BenchmarkResult::latency(
                "cold_start_nodejs_process",
                BenchmarkCategory::ColdStart,
                samples,
                true,
            )
            .with_metadata("runtime", "nodejs"),
        );
        println!("  ✓ cold_start_nodejs_process");
    }
}

fn run_ipc_benchmarks(report: &mut BenchmarkReport, iterations: u64) {
    use aetherless_benchmark::harness::BenchmarkHarness;
    use aetherless_core::shm::{RingBuffer, SharedMemoryRegion};

    let harness = BenchmarkHarness::new()
        .warmup(iterations / 10)
        .iterations(iterations);

    // Shared memory IPC
    let name = format!("bench_ipc_shm_{}", std::process::id());
    if let Ok(region) = SharedMemoryRegion::create(&name, 1024 * 1024) {
        if let Ok(buffer) = RingBuffer::new(region) {
            let payload = vec![0xABu8; 1024];

            let samples = harness.run(|| {
                buffer.write(&payload).ok();
                buffer.read().ok();
            });

            report.add_result(
                BenchmarkResult::latency(
                    "ipc_shared_memory_1024",
                    BenchmarkCategory::Ipc,
                    samples,
                    true,
                )
                .with_metadata("method", "shared_memory")
                .with_metadata("payload_size_bytes", 1024)
                .with_metadata("zero_copy", true),
            );
            println!("  ✓ ipc_shared_memory_1024");
        }
    }
}

fn print_summary(report: &BenchmarkReport) {
    use aetherless_benchmark::LatencyMetrics;

    println!("Summary");
    println!("-------");
    println!();

    for result in &report.results {
        if let Some(latency) = &result.latency {
            println!(
                "{}: median={}, p99={}",
                result.name,
                LatencyMetrics::format_latency(latency.median_ns),
                LatencyMetrics::format_latency(latency.p99_ns)
            );
        }
    }
}
