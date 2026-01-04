// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! SMP CPU affinity and multi-core scalability benchmarks.
//!
//! Measures the overhead of CPU pinning and validates even distribution
//! of workloads across multiple CPU cores.

use aetherless_benchmark::{
    harness::BenchmarkHarness, BenchmarkCategory, BenchmarkReport, BenchmarkResult, JsonReporter,
};
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Benchmark CPU affinity pinning overhead.
fn bench_cpu_affinity_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("smp_affinity");
    
    // Benchmark spawning without affinity
    group.bench_function("spawn_no_affinity", |b| {
        b.iter(|| {
            let child = Command::new("sleep")
                .arg("0")
                .stdout(Stdio::null())
                .spawn()
                .expect("Failed to spawn");
            let _ = child.wait_with_output();
        });
    });
    
    // Benchmark spawning with taskset (simulates affinity pinning)
    group.bench_function("spawn_with_taskset", |b| {
        b.iter(|| {
            let child = Command::new("taskset")
                .arg("-c")
                .arg("0")
                .arg("sleep")
                .arg("0")
                .stdout(Stdio::null())
                .spawn()
                .expect("Failed to spawn");
            let _ = child.wait_with_output();
        });
    });
    
    group.finish();
}

/// Benchmark parallel process spawning across cores.
fn bench_parallel_spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("smp_parallel_spawn");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    
    let num_cpus = num_cpus::get();
    
    for num_procs in [1, 2, 4, 8].iter().filter(|&&n| n <= num_cpus) {
        group.bench_with_input(
            BenchmarkId::new("parallel_procs", num_procs),
            num_procs,
            |b, &num| {
                b.iter(|| {
                    let handles: Vec<_> = (0..num)
                        .map(|i| {
                            let cpu = i % num_cpus;
                            std::thread::spawn(move || {
                                let child = Command::new("taskset")
                                    .arg("-c")
                                    .arg(cpu.to_string())
                                    .arg("python3")
                                    .arg("-c")
                                    .arg("print('done')")
                                    .stdout(Stdio::null())
                                    .spawn()
                                    .expect("spawn");
                                child.wait_with_output().expect("wait");
                            })
                        })
                        .collect();
                    
                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark distribution evenness across CPUs.
fn bench_distribution_evenness(c: &mut Criterion) {
    c.bench_function("smp_distribution_test", |b| {
        let num_cpus = num_cpus::get();
        let counter = Arc::new(AtomicUsize::new(0));
        
        b.iter(|| {
            let mut assignments: Vec<usize> = vec![0; num_cpus];
            
            // Simulate 100 allocations
            for _ in 0..100 {
                let cpu = counter.fetch_add(1, Ordering::Relaxed) % num_cpus;
                assignments[cpu] += 1;
            }
            
            // All should be evenly distributed (10 each for 10 CPUs, etc.)
            let expected = 100 / num_cpus;
            for count in &assignments {
                assert!(
                    (*count as i32 - expected as i32).abs() <= 1,
                    "Uneven distribution"
                );
            }
        });
    });
}

/// Generate JSON report with SMP benchmark data.
#[allow(dead_code)]
fn generate_smp_report() {
    let mut report = BenchmarkReport::new();
    let harness = BenchmarkHarness::new().warmup(3).iterations(20);
    
    // Affinity overhead
    let samples = harness.run(|| {
        let child = Command::new("taskset")
            .arg("-c")
            .arg("0")
            .arg("sleep")
            .arg("0")
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn");
        let _ = child.wait_with_output();
    });
    
    report.add_result(
        BenchmarkResult::latency(
            "smp_affinity_overhead",
            BenchmarkCategory::ColdStart,
            samples,
            true,
        )
        .with_metadata("operation", "taskset_pin")
        .with_metadata("cpu", "0"),
    );
    
    // Save report
    if let Ok(reporter) = JsonReporter::default_location() {
        if let Ok(path) = reporter.save(&report) {
            println!("Saved SMP benchmark report to: {:?}", path);
        }
    }
}

criterion_group!(
    benches,
    bench_cpu_affinity_overhead,
    bench_parallel_spawn,
    bench_distribution_evenness,
);

criterion_main!(benches);
