# Aetherless

<div align="center">

**High-Performance Serverless Function Orchestrator**

*Zero-fallback architecture • eBPF-accelerated networking • Sub-millisecond cold starts*

[![CI](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml/badge.svg)](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml)
[![Smoke Bench](https://github.com/ankitkpandey1/aetherless/actions/workflows/bench.yml/badge.svg)](https://github.com/ankitkpandey1/aetherless/actions/workflows/bench.yml)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

</div>

---

## TL;DR

**Serverless function orchestrator using CRIU warm pools, eBPF/XDP kernel-bypass networking, and lock-free shared memory IPC—achieving cold starts under 15ms (26× faster than AWS Lambda) with sub-microsecond IPC latency (bench scripts included).**

---

## Design & Implementation

| Subsystem | Description | Key Files |
|-----------|-------------|-----------|
| **CRIU Warm Pools** | Checkpoint/restore processes in <15ms with strict latency enforcement | [`aetherless-core/src/criu/`](aetherless-core/src/criu/) |
| **eBPF/XDP Data Plane** | Kernel-bypass packet routing using Aya; port→PID BPF hash maps | [`aetherless-ebpf/src/main.rs`](aetherless-ebpf/src/main.rs) |
| **Lock-Free Ring Buffer** | SPSC shared memory IPC with atomic head/tail, CRC32 validation | [`aetherless-core/src/shm/ring_buffer.rs`](aetherless-core/src/shm/ring_buffer.rs) |
| **Explicit Error Types** | No `Box<dyn Error>`—all errors are typed enums for exhaustive handling | [`aetherless-core/src/error.rs`](aetherless-core/src/error.rs) |
| **State Machine** | Compile-time checked FSM transitions via `matches!` patterns | [`aetherless-core/src/state.rs`](aetherless-core/src/state.rs) |
| **State Machine** | Compile-time checked FSM transitions via `matches!` patterns | [`aetherless-core/src/state.rs`](aetherless-core/src/state.rs) |
| **Handler Protocol** | Unix socket handshake ensures handlers are ready before routing traffic | [`aetherless-cli/src/commands/up.rs`](aetherless-cli/src/commands/up.rs) |
| **Autoscaler** | Dynamic horizontal scaling based on request load | [`aetherless-core/src/autoscaler.rs`](aetherless-core/src/autoscaler.rs) |
| **Cluster/Gossip** | UDP-based node discovery and state syncing | [`aetherless-core/src/cluster.rs`](aetherless-core/src/cluster.rs) |
| **SMP CPU Affinity** | NUMA-aware process pinning for even multi-core distribution | [`aetherless-cli/src/cpu_affinity.rs`](aetherless-cli/src/cpu_affinity.rs) |

---

## Quickstart

### Docker (recommended)

```bash
docker build -t aetherless-bench -f bench/Dockerfile .
docker run --rm -v $(pwd)/bench/results:/out aetherless-bench
```

### Native (Ubuntu 22.04)

```bash
./bench/setup_env.sh && ./bench/compare.sh --smoke --output-dir bench/results
```

---

## Benchmarks

Run [`bench/compare.sh`](bench/compare.sh) to produce:

| Metric | Description |
|--------|-------------|
| **p50 / p95 / p99** | Latency percentiles |
| **mean** | Average latency |
| **throughput** | Messages per second |

Results are saved to:
- `bench/results/*.json` — Raw benchmark data
- `bench/results/*.svg` — Visualization charts (when available)

### Results Summary

| Benchmark | p50 | p95 | p99 | vs Baseline |
|-----------|-----|-----|-----|-------------|
| **Ring Buffer (1KB)** | 148ns | 220ns | 350ns | 3,333× vs HTTP |
| **Cold Start (CRIU)** | 9.5ms | 11.2ms | 12.3ms | 26× vs Lambda |
| **XDP Routing** | 5μs | — | — | 20× vs userspace |

---

## CI / Status

| Badge | Description |
|-------|-------------|
| [![CI](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml/badge.svg)](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml) | Build, test, clippy, fmt |
| [![Smoke Bench](https://github.com/ankitkpandey1/aetherless/actions/workflows/bench.yml/badge.svg)](https://github.com/ankitkpandey1/aetherless/actions/workflows/bench.yml) | Smoke benchmarks (artifacts uploaded) |

> **Note:** Smoke benchmarks run in CI on every push. Results are uploaded as artifacts.

---

## The Problem

Serverless platforms promise instant scale, but the reality is different:

- **Cold starts of 100-500ms** make real-time applications impossible
- **15-30% of requests** hit cold starts in production traffic
- **Cost overhead** from keeping instances warm to avoid latency
- **Silent failures** when fallback paths mask critical errors

If you've ever waited for a Lambda function to cold-start during a user request, you know the problem.

## The Solution

Aetherless eliminates cold start latency by combining three technologies:

| Technology | What It Does | Result |
|------------|--------------|--------|
| **CRIU Warm Pools** | Snapshots initialized processes and restores them on-demand | Cold starts under 15ms |
| **eBPF/XDP Networking** | Routes packets in the kernel, bypassing the network stack | Microsecond packet latency |
| **Zero-Copy Shared Memory** | Passes data between orchestrator and handlers without copying | Sub-microsecond IPC |

```
Traditional Serverless          Aetherless
──────────────────────          ──────────
Cold start: 100-500ms    →      Cold start: <15ms (33x faster)
Network: userspace TCP   →      Network: eBPF/XDP kernel bypass  
IPC: JSON over HTTP      →      IPC: Zero-copy shared memory
Errors: "Something failed" →    Errors: Typed, actionable
```

---

## Use Cases

### Real-Time APIs

When every millisecond matters—payment processing, live bidding, game servers. Cold starts kill user experience. Aetherless ensures the first request is as fast as the hundredth.

### Edge Computing

Deploy functions at the edge where resources are constrained. CRIU snapshots are smaller than container images, and eBPF reduces CPU overhead compared to userspace proxies.

### Event-Driven Microservices

Scale to zero without the cold start penalty. Process Kafka messages, webhooks, or queue events with consistent latency whether the function was idle for 5 seconds or 5 hours.

### Cost-Sensitive Workloads

Stop paying for warm instances you don't need. With 15ms cold starts, aggressive scale-to-zero becomes practical. No more keeping idle containers running "just in case."

### Latency-Critical Pipelines

ML inference, image processing, data transformation—any pipeline where you chain functions together. Traditional serverless adds 100ms+ per hop. Aetherless adds microseconds.

---

## Key Features

| Feature | Benefit |
|---------|---------|
| **eBPF/XDP Network Layer** | Kernel-bypass packet routing with microsecond latency |
| **CRIU Warm Pools** | Process snapshots restore in under 15ms |
| **Zero-Copy Shared Memory** | Lock-free IPC with CRC32 validation |
| **Autoscaling** | HPA-like horizontal scaling based on load metrics |
| **Distributed State** | Gossip-based cluster management for multi-node deployments |
| **Language Agnostic** | Python, Node, Rust, Go—any process with a TCP port |
| **Single Binary** | One Rust binary, no container runtime required |

---

## Installation

### Quick Install

```bash
# Clone and build
git clone https://github.com/ankitkpandey1/aetherless.git
cd aetherless
cargo build --release

# Install CLI
cargo install --path aetherless-cli

# Verify
aether --version
```

### Prerequisites

```bash
# Rust toolchain (1.70+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Optional: CRIU for warm pools
sudo apt install criu
```

---

## Quick Start

Deploy a Python HTTP function in under 2 minutes.

### Step 1: Create Your Handler

```python
#!/usr/bin/env python3
# /opt/handlers/hello.py
import os, socket, json
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        response = {'message': 'Hello from Aetherless!', 'path': self.path}
        self.wfile.write(json.dumps(response).encode())
    
    def log_message(self, format, *args):
        pass  # Suppress logs

# Connect to orchestrator and signal ready
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')

# Start serving
port = int(os.environ.get('AETHER_TRIGGER_PORT', 8080))
HTTPServer(('0.0.0.0', port), Handler).serve_forever()
```

### Step 2: Create Configuration

```yaml
# hello.yaml
functions:
  - id: hello-api
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /opt/handlers/hello.py
    timeout_ms: 30000
```

### Step 3: Start the Orchestrator

```bash
chmod +x /opt/handlers/hello.py
aether -c hello.yaml up --foreground
```

### Step 4: Test

```bash
curl http://localhost:8080/test
# {"message": "Hello from Aetherless!", "path": "/test"}
```

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│                    CLI (aether)                             │
│          up • deploy • list • stats • validate              │
└─────────────────────────┬──────────────────────────────────┘
                          │
┌─────────────────────────┴──────────────────────────────────┐
│                   Core Library                              │
│   Registry • State Machine • Config • Shared Memory • CRIU │
└─────────────────────────┬──────────────────────────────────┘
                          │
┌─────────────────────────┴──────────────────────────────────┐
│                  eBPF Data Plane                            │
│        XDP Program • Port-to-PID Routing • BPF Maps        │
└────────────────────────────────────────────────────────────┘
```

For detailed architecture and design decisions, see [ARCHITECTURE.md](ARCHITECTURE.md).

For implementation details and code walkthrough, see [WALKTHROUGH.md](WALKTHROUGH.md).

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `aether up --foreground [--warm-pool]` | Start orchestrator (foreground, opt. warm pool) |
| `aether down` | Stop the orchestrator |
| `aether deploy <file> [--force]` | Validate and deploy config (use --force for dry-run) |
| `aether list` | List registered functions |
| `aether stats --dashboard` | TUI dashboard with live metrics |
| `aether validate <file>` | Validate configuration file |
| `curl localhost:9090/metrics` | Access Prometheus metrics |

---

## Configuration Reference

```yaml
orchestrator:
  shm_buffer_size: 4194304    # Shared memory size (4MB default)
  warm_pool_size: 10          # Number of warm instances
  restore_timeout_ms: 15      # CRIU restore limit (strict!)
  snapshot_dir: /dev/shm/aetherless

functions:
  - id: my-function           # Unique identifier
    memory_limit_mb: 256      # 1-16384 MB
    trigger_port: 8080        # 1-65535, unique per function
    handler_path: /path/to/handler
    timeout_ms: 30000         # 1-900000 ms
    environment:
      KEY: "value"
```

---

## Handler Protocol

Every handler must:

1. Read `AETHER_SOCKET` environment variable
2. Connect to the Unix socket
3. Send `READY` (exactly 5 bytes)
4. Start serving on `AETHER_TRIGGER_PORT`

See [examples/](examples/) for Python and multi-service examples.

---

## eBPF/XDP (Advanced)

For production deployments requiring lowest latency:

```bash
# Build the eBPF loader
cargo build --release -p aetherless-ebpf

# Run with XDP program (requires root)
sudo ./target/release/aetherless-ebpf eth0 /path/to/xdp_redirect.o
```

| Mode | Latency | Use Case |
|------|---------|----------|
| Userspace only | ~50-100μs | Development |
| XDP mode | ~5-10μs | Production |

See [aetherless-ebpf/README.md](aetherless-ebpf/README.md) for details.

---

## Comparison

| Feature | Aetherless | AWS Lambda | Knative | OpenFaaS |
|---------|------------|------------|---------|----------|
| Cold start | <15ms | 100-500ms | 500ms+ | 100-300ms |
| Networking | eBPF/XDP | Userspace | Userspace | Userspace |
| IPC | Shared memory | HTTP | HTTP | HTTP |
| Container required | No | Yes | Yes | Yes |
| Language support | Any process | Runtimes | Containers | Containers |
| Fail-safe errors | Yes | No | No | No |

---

## Contributing

Contributions are welcome! Please see the [Contributing Guide](CONTRIBUTING.md).

---

## License

Apache 2.0. See [LICENSE](LICENSE) for details.

Copyright 2025 Ankit Kumar Pandey. See [NOTICE](NOTICE) for attribution.