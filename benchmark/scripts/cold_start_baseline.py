#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
"""
Cold start baseline benchmark.

Simulates traditional serverless cold start patterns for comparison
with Aetherless CRIU-based warm pools.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, asdict
from datetime import datetime, timezone
from pathlib import Path
from statistics import mean, median, stdev
from typing import List, Optional


@dataclass
class LatencyMetrics:
    """Latency statistics following research-level methodology."""
    min_ns: int
    max_ns: int
    mean_ns: float
    median_ns: int
    p95_ns: int
    p99_ns: int
    std_dev_ns: float
    samples: Optional[List[int]] = None

    @classmethod
    def from_samples(cls, samples_ns: List[int], keep_raw: bool = True) -> "LatencyMetrics":
        if not samples_ns:
            return cls(0, 0, 0.0, 0, 0, 0, 0.0)
        
        sorted_samples = sorted(samples_ns)
        n = len(sorted_samples)
        
        return cls(
            min_ns=sorted_samples[0],
            max_ns=sorted_samples[-1],
            mean_ns=mean(sorted_samples),
            median_ns=sorted_samples[n // 2],
            p95_ns=sorted_samples[int(n * 0.95)],
            p99_ns=sorted_samples[int(n * 0.99)],
            std_dev_ns=stdev(sorted_samples) if n > 1 else 0.0,
            samples=sorted_samples if keep_raw else None,
        )


@dataclass
class BenchmarkResult:
    """Single benchmark result."""
    name: str
    category: str
    iterations: int
    metrics: LatencyMetrics
    metadata: dict


def benchmark_python_spawn(iterations: int) -> BenchmarkResult:
    """Benchmark raw Python interpreter spawn time."""
    samples = []
    
    for _ in range(iterations):
        start = time.perf_counter_ns()
        proc = subprocess.run(
            [sys.executable, "-c", "print('ready')"],
            capture_output=True,
            check=True,
        )
        elapsed = time.perf_counter_ns() - start
        samples.append(elapsed)
    
    return BenchmarkResult(
        name="baseline_python_spawn",
        category="cold_start",
        iterations=iterations,
        metrics=LatencyMetrics.from_samples(samples),
        metadata={"runtime": "python3", "operation": "process_spawn"},
    )


def benchmark_python_import_heavy(iterations: int) -> BenchmarkResult:
    """Benchmark Python spawn with heavy imports (simulates real-world Lambda)."""
    samples = []
    
    # Script that imports common Lambda dependencies
    script = """
import json
import os
import sys
import http.server
import urllib.request
print('ready')
"""
    
    for _ in range(iterations):
        start = time.perf_counter_ns()
        proc = subprocess.run(
            [sys.executable, "-c", script],
            capture_output=True,
            check=True,
        )
        elapsed = time.perf_counter_ns() - start
        samples.append(elapsed)
    
    return BenchmarkResult(
        name="baseline_python_import_heavy",
        category="cold_start",
        iterations=iterations,
        metrics=LatencyMetrics.from_samples(samples),
        metadata={
            "runtime": "python3",
            "operation": "process_spawn_with_imports",
            "imports": ["json", "os", "sys", "http.server", "urllib.request"],
        },
    )


def benchmark_simulated_lambda_cold_start(iterations: int) -> BenchmarkResult:
    """
    Simulate AWS Lambda cold start overhead.
    
    This adds artificial delays to approximate:
    - Container initialization (~50-100ms)
    - Runtime bootstrap (~20-50ms)
    - Network setup (~10-20ms)
    """
    samples = []
    
    # Simulated overhead (conservative estimates)
    CONTAINER_INIT_MS = 75  # Average container spin-up
    RUNTIME_BOOTSTRAP_MS = 35  # Runtime initialization
    
    for _ in range(iterations):
        start = time.perf_counter_ns()
        
        # Simulate container initialization
        time.sleep(CONTAINER_INIT_MS / 1000)
        
        # Actual Python spawn
        proc = subprocess.run(
            [sys.executable, "-c", "import json; print('ready')"],
            capture_output=True,
            check=True,
        )
        
        # Simulate runtime bootstrap overhead
        time.sleep(RUNTIME_BOOTSTRAP_MS / 1000)
        
        elapsed = time.perf_counter_ns() - start
        samples.append(elapsed)
    
    return BenchmarkResult(
        name="baseline_lambda_simulated_cold_start",
        category="cold_start",
        iterations=iterations,
        metrics=LatencyMetrics.from_samples(samples),
        metadata={
            "runtime": "python3",
            "operation": "simulated_lambda",
            "simulated_container_init_ms": CONTAINER_INIT_MS,
            "simulated_runtime_bootstrap_ms": RUNTIME_BOOTSTRAP_MS,
            "note": "Simulated overhead, not actual AWS Lambda",
        },
    )


def benchmark_http_handler_cold_start(iterations: int) -> BenchmarkResult:
    """Benchmark cold start with full HTTP handler initialization."""
    samples = []
    
    handler_script = '''
import os
import socket
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'ok')
    def log_message(self, *args): pass

# Signal ready via Unix socket
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['BENCHMARK_SOCKET'])
sock.send(b'READY')
sock.close()
'''
    
    for _ in range(iterations):
        with tempfile.TemporaryDirectory() as tmpdir:
            socket_path = os.path.join(tmpdir, "bench.sock")
            handler_path = os.path.join(tmpdir, "handler.py")
            
            with open(handler_path, "w") as f:
                f.write(handler_script)
            
            # Create Unix socket listener
            server_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            server_sock.bind(socket_path)
            server_sock.listen(1)
            server_sock.settimeout(10)
            
            start = time.perf_counter_ns()
            
            # Spawn handler
            proc = subprocess.Popen(
                [sys.executable, handler_path],
                env={**os.environ, "BENCHMARK_SOCKET": socket_path},
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            
            try:
                # Wait for READY signal
                conn, _ = server_sock.accept()
                data = conn.recv(8)
                if data.startswith(b"READY"):
                    elapsed = time.perf_counter_ns() - start
                    samples.append(elapsed)
                conn.close()
            except socket.timeout:
                pass
            finally:
                proc.terminate()
                proc.wait()
                server_sock.close()
    
    return BenchmarkResult(
        name="baseline_python_http_handler",
        category="cold_start",
        iterations=len(samples),
        metrics=LatencyMetrics.from_samples(samples),
        metadata={
            "runtime": "python3",
            "operation": "http_handler_cold_start",
            "includes_socket_handshake": True,
        },
    )


def format_latency(ns: int) -> str:
    """Format nanoseconds in human-readable form."""
    if ns < 1_000:
        return f"{ns}ns"
    elif ns < 1_000_000:
        return f"{ns / 1_000:.2f}μs"
    elif ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f}ms"
    else:
        return f"{ns / 1_000_000_000:.2f}s"


def main():
    parser = argparse.ArgumentParser(description="Cold start baseline benchmarks")
    parser.add_argument("--output", type=Path, default=Path("data"),
                        help="Output directory for results")
    parser.add_argument("--iterations", type=int, default=20,
                        help="Number of iterations per benchmark")
    args = parser.parse_args()
    
    args.output.mkdir(parents=True, exist_ok=True)
    
    print("Cold Start Baseline Benchmarks")
    print("=" * 40)
    print(f"Iterations: {args.iterations}")
    print()
    
    results = []
    
    # Run benchmarks
    print("Running python_spawn...")
    results.append(benchmark_python_spawn(args.iterations))
    
    print("Running python_import_heavy...")
    results.append(benchmark_python_import_heavy(args.iterations))
    
    print("Running simulated_lambda_cold_start...")
    results.append(benchmark_simulated_lambda_cold_start(args.iterations))
    
    print("Running http_handler_cold_start...")
    results.append(benchmark_http_handler_cold_start(args.iterations))
    
    # Print summary
    print()
    print("Summary")
    print("-" * 40)
    for result in results:
        m = result.metrics
        print(f"{result.name}:")
        print(f"  median: {format_latency(m.median_ns)}, p99: {format_latency(m.p99_ns)}")
    
    # Save results
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    output_file = args.output / f"cold_start_baseline_{timestamp}.json"
    
    report = {
        "benchmark_suite": "aetherless-benchmarks",
        "version": "0.1.0",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "type": "baseline",
        "results": [
            {
                "name": r.name,
                "category": r.category,
                "iterations": r.iterations,
                "metrics": {
                    "min_ns": r.metrics.min_ns,
                    "max_ns": r.metrics.max_ns,
                    "mean_ns": r.metrics.mean_ns,
                    "median_ns": r.metrics.median_ns,
                    "p95_ns": r.metrics.p95_ns,
                    "p99_ns": r.metrics.p99_ns,
                    "std_dev_ns": r.metrics.std_dev_ns,
                },
                "metadata": r.metadata,
            }
            for r in results
        ],
    }
    
    with open(output_file, "w") as f:
        json.dump(report, f, indent=2)
    
    print()
    print(f"Results saved to: {output_file}")


if __name__ == "__main__":
    main()
