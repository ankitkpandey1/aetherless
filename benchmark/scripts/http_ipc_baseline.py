#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2025 Ankit Kumar Pandey
"""
HTTP IPC baseline benchmark.

Measures traditional HTTP + JSON IPC overhead for comparison with
Aetherless zero-copy shared memory.
"""

import argparse
import json
import socket
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path
from statistics import mean, median, stdev
from typing import List, Optional
from urllib.request import urlopen, Request


@dataclass
class LatencyMetrics:
    min_ns: int
    max_ns: int
    mean_ns: float
    median_ns: int
    p95_ns: int
    p99_ns: int
    std_dev_ns: float

    @classmethod
    def from_samples(cls, samples_ns: List[int]) -> "LatencyMetrics":
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
        )


class EchoHandler(BaseHTTPRequestHandler):
    """HTTP handler that echoes JSON payloads."""
    
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        
        # Parse and re-serialize JSON (simulates typical serverless overhead)
        data = json.loads(body)
        response = json.dumps(data).encode()
        
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", len(response))
        self.end_headers()
        self.wfile.write(response)
    
    def log_message(self, format, *args):
        pass  # Suppress logging


def start_server(port: int, ready_event: threading.Event):
    """Start HTTP server in background thread."""
    server = HTTPServer(("127.0.0.1", port), EchoHandler)
    ready_event.set()
    server.serve_forever()


def benchmark_http_json_roundtrip(port: int, payload_size: int, iterations: int) -> List[int]:
    """Benchmark HTTP + JSON IPC roundtrip."""
    samples = []
    payload = {"data": "x" * payload_size}
    payload_bytes = json.dumps(payload).encode()
    url = f"http://127.0.0.1:{port}/"
    
    for _ in range(iterations):
        start = time.perf_counter_ns()
        
        req = Request(url, data=payload_bytes, method="POST")
        req.add_header("Content-Type", "application/json")
        
        with urlopen(req, timeout=5) as resp:
            _ = resp.read()
        
        elapsed = time.perf_counter_ns() - start
        samples.append(elapsed)
    
    return samples


def benchmark_unix_socket_roundtrip(payload_size: int, iterations: int) -> List[int]:
    """Benchmark Unix socket IPC roundtrip."""
    import tempfile
    import os
    
    samples = []
    payload = b"x" * payload_size
    
    with tempfile.TemporaryDirectory() as tmpdir:
        socket_path = os.path.join(tmpdir, "ipc.sock")
        
        # Set up server
        server_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server_sock.bind(socket_path)
        server_sock.listen(1)
        
        server_ready = threading.Event()
        
        def server_thread():
            server_ready.set()
            conn, _ = server_sock.accept()
            try:
                while True:
                    data = conn.recv(65536)
                    if not data:
                        break
                    conn.sendall(data)
            except:
                pass
            finally:
                conn.close()
                server_sock.close()
        
        thread = threading.Thread(target=server_thread, daemon=True)
        thread.start()
        server_ready.wait()
        
        # Client
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client.connect(socket_path)
        
        # Warmup
        for _ in range(10):
            client.sendall(payload)
            client.recv(payload_size)
        
        # Benchmark
        for _ in range(iterations):
            start = time.perf_counter_ns()
            client.sendall(payload)
            _ = client.recv(payload_size)
            elapsed = time.perf_counter_ns() - start
            samples.append(elapsed)
        
        client.close()
    
    return samples


def benchmark_tcp_roundtrip(payload_size: int, iterations: int) -> List[int]:
    """Benchmark TCP localhost IPC roundtrip."""
    samples = []
    payload = b"x" * payload_size
    
    # Set up server
    server_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_sock.bind(("127.0.0.1", 0))
    port = server_sock.getsockname()[1]
    server_sock.listen(1)
    
    server_ready = threading.Event()
    
    def server_thread():
        server_ready.set()
        conn, _ = server_sock.accept()
        conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        try:
            while True:
                data = conn.recv(65536)
                if not data:
                    break
                conn.sendall(data)
        except:
            pass
        finally:
            conn.close()
            server_sock.close()
    
    thread = threading.Thread(target=server_thread, daemon=True)
    thread.start()
    server_ready.wait()
    
    # Client
    client = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    client.connect(("127.0.0.1", port))
    
    # Warmup
    for _ in range(10):
        client.sendall(payload)
        client.recv(payload_size)
    
    # Benchmark
    for _ in range(iterations):
        start = time.perf_counter_ns()
        client.sendall(payload)
        _ = client.recv(payload_size)
        elapsed = time.perf_counter_ns() - start
        samples.append(elapsed)
    
    client.close()
    
    return samples


def format_latency(ns: int) -> str:
    if ns < 1_000:
        return f"{ns}ns"
    elif ns < 1_000_000:
        return f"{ns / 1_000:.2f}μs"
    elif ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f}ms"
    else:
        return f"{ns / 1_000_000_000:.2f}s"


def main():
    parser = argparse.ArgumentParser(description="HTTP IPC baseline benchmarks")
    parser.add_argument("--output", type=Path, default=Path("data"))
    parser.add_argument("--iterations", type=int, default=100)
    args = parser.parse_args()
    
    args.output.mkdir(parents=True, exist_ok=True)
    
    print("IPC Baseline Benchmarks")
    print("=" * 40)
    print(f"Iterations: {args.iterations}")
    print()
    
    payload_sizes = [64, 1024, 4096]
    results = []
    
    # Start HTTP server
    port = 18080
    ready_event = threading.Event()
    server_thread = threading.Thread(
        target=start_server, args=(port, ready_event), daemon=True
    )
    server_thread.start()
    ready_event.wait()
    
    for size in payload_sizes:
        print(f"Benchmarking payload size: {size} bytes")
        
        # HTTP + JSON
        print("  Running http_json...")
        http_samples = benchmark_http_json_roundtrip(port, size, args.iterations)
        http_metrics = LatencyMetrics.from_samples(http_samples)
        results.append({
            "name": f"ipc_http_json_{size}",
            "category": "ipc",
            "iterations": args.iterations,
            "metrics": {
                "min_ns": http_metrics.min_ns,
                "max_ns": http_metrics.max_ns,
                "mean_ns": http_metrics.mean_ns,
                "median_ns": http_metrics.median_ns,
                "p95_ns": http_metrics.p95_ns,
                "p99_ns": http_metrics.p99_ns,
                "std_dev_ns": http_metrics.std_dev_ns,
            },
            "metadata": {
                "method": "http_json",
                "payload_size_bytes": size,
                "zero_copy": False,
            },
        })
        
        # Unix socket
        print("  Running unix_socket...")
        unix_samples = benchmark_unix_socket_roundtrip(size, args.iterations)
        unix_metrics = LatencyMetrics.from_samples(unix_samples)
        results.append({
            "name": f"ipc_unix_socket_{size}",
            "category": "ipc",
            "iterations": args.iterations,
            "metrics": {
                "min_ns": unix_metrics.min_ns,
                "max_ns": unix_metrics.max_ns,
                "mean_ns": unix_metrics.mean_ns,
                "median_ns": unix_metrics.median_ns,
                "p95_ns": unix_metrics.p95_ns,
                "p99_ns": unix_metrics.p99_ns,
                "std_dev_ns": unix_metrics.std_dev_ns,
            },
            "metadata": {
                "method": "unix_socket",
                "payload_size_bytes": size,
                "zero_copy": False,
            },
        })
        
        # TCP localhost
        print("  Running tcp_localhost...")
        tcp_samples = benchmark_tcp_roundtrip(size, args.iterations)
        tcp_metrics = LatencyMetrics.from_samples(tcp_samples)
        results.append({
            "name": f"ipc_tcp_localhost_{size}",
            "category": "ipc",
            "iterations": args.iterations,
            "metrics": {
                "min_ns": tcp_metrics.min_ns,
                "max_ns": tcp_metrics.max_ns,
                "mean_ns": tcp_metrics.mean_ns,
                "median_ns": tcp_metrics.median_ns,
                "p95_ns": tcp_metrics.p95_ns,
                "p99_ns": tcp_metrics.p99_ns,
                "std_dev_ns": tcp_metrics.std_dev_ns,
            },
            "metadata": {
                "method": "tcp_localhost",
                "payload_size_bytes": size,
                "zero_copy": False,
            },
        })
    
    # Print summary
    print()
    print("Summary")
    print("-" * 40)
    for r in results:
        m = r["metrics"]
        print(f"{r['name']}: median={format_latency(m['median_ns'])}, p99={format_latency(m['p99_ns'])}")
    
    # Save results
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    output_file = args.output / f"ipc_baseline_{timestamp}.json"
    
    report = {
        "benchmark_suite": "aetherless-benchmarks",
        "version": "0.1.0",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "type": "baseline",
        "results": results,
    }
    
    with open(output_file, "w") as f:
        json.dump(report, f, indent=2)
    
    print()
    print(f"Results saved to: {output_file}")


if __name__ == "__main__":
    main()
