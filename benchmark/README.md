# Aetherless Benchmarking Framework

Research-level benchmarking framework for validating Aetherless performance claims against traditional serverless approaches.

## Quick Start

```bash
# Build the benchmark crate
cargo build --release

# Run quick benchmarks
cargo run --release --bin run_benchmarks -- --quick

# Run full benchmarks (takes longer)
./scripts/run_all.sh
```

## Benchmark Categories

| Category | Description | Key Metrics |
|----------|-------------|-------------|
| **Cold Start** | CRIU restore vs fresh process spawn | Time to READY signal |
| **IPC** | Shared memory vs sockets vs HTTP | Latency, throughput |
| **Ring Buffer** | Zero-copy buffer microbenchmarks | Write/read latency at various sizes |
| **E2E Latency** | Full request lifecycle | Request-to-response time |

## Running Benchmarks

### Using Criterion (Statistical Analysis)

```bash
# Run specific benchmark suite
cargo bench --bench ring_buffer
cargo bench --bench cold_start
cargo bench --bench ipc_throughput
cargo bench --bench e2e_latency

# HTML reports generated in target/criterion/
```

### Using CLI Runner

```bash
# Run all categories
cargo run --release --bin run_benchmarks

# Run specific categories
cargo run --release --bin run_benchmarks -- --category cold_start

# Quick mode (fewer iterations)
cargo run --release --bin run_benchmarks -- --quick
```

### Python Baselines

```bash
# Cold start baselines (simulated Lambda, HTTP handler)
python3 scripts/cold_start_baseline.py --iterations 20

# IPC baselines (HTTP+JSON, Unix sockets, TCP)
python3 scripts/http_ipc_baseline.py --iterations 100
```

## Output Format

All benchmarks output JSON files to `data/` with standardized metrics:

```json
{
  "benchmark_suite": "aetherless-benchmarks",
  "version": "0.1.0",
  "timestamp": "2025-12-27T12:34:56Z",
  "system_info": { ... },
  "results": [
    {
      "name": "ring_buffer_roundtrip_1024",
      "category": "ring_buffer",
      "iterations": 1000,
      "metrics": {
        "min_ns": 800,
        "max_ns": 5000,
        "mean_ns": 1200.5,
        "median_ns": 1100,
        "p95_ns": 2500,
        "p99_ns": 3800,
        "std_dev_ns": 450.2
      },
      "metadata": {
        "payload_size_bytes": 1024
      }
    }
  ]
}
```

## Metrics Collected

### Latency Metrics
- **min/max**: Range bounds
- **mean**: Arithmetic average
- **median (p50)**: 50th percentile
- **p95/p99**: Tail latencies
- **std_dev**: Distribution spread

### Throughput Metrics
- **messages_per_sec**: Operations per second
- **bytes_per_sec**: Data throughput

### System Info
- OS, kernel version
- CPU model and core count
- Total memory

## Directory Structure

```
benchmark/
├── Cargo.toml              # Crate configuration
├── README.md               # This file
├── src/
│   ├── lib.rs              # Module exports
│   ├── metrics.rs          # Metrics types
│   ├── reporter.rs         # JSON output
│   ├── harness.rs          # Timing utilities
│   └── bin/
│       └── run_benchmarks.rs
├── benches/                # Criterion benchmarks
│   ├── cold_start.rs
│   ├── ipc_throughput.rs
│   ├── ring_buffer.rs
│   └── e2e_latency.rs
├── scripts/                # Baseline comparison
│   ├── run_all.sh
│   ├── cold_start_baseline.py
│   └── http_ipc_baseline.py
└── data/                   # Output directory
    └── *.json
```

## Methodology

This framework follows research-level benchmarking practices:

1. **Warmup phases** before measurement
2. **Statistical analysis** with percentiles
3. **System metadata** capture for reproducibility
4. **Baseline comparisons** against industry standards
5. **JSON output** for visualization (Stage 2)

## References

- [SeBS: Serverless Benchmark Suite](https://github.com/spcl/sebs)
- [STeLLAR: Serverless Tail-Latency Analyzer](https://arxiv.org/abs/2106.01434)
- [Cold Start Latency in Serverless Computing: A Systematic Review](https://arxiv.org/abs/2310.08336)
