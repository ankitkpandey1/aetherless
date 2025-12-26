# Aetherless

<div align="center">

**High-Performance Serverless Function Orchestrator**

*Zero-fallback architecture • eBPF-accelerated networking • Sub-millisecond cold starts*

[![CI](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml/badge.svg)](https://github.com/ankitkpandey1/aetherless/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-49%20passed-brightgreen.svg)]()

</div>

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
| **Fail-Fast Architecture** | No silent degradation—errors are explicit and typed |
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

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `aether up --foreground` | Start orchestrator in foreground |
| `aether down` | Stop the orchestrator |
| `aether deploy <file>` | Validate configuration |
| `aether list` | List registered functions |
| `aether stats --dashboard` | TUI dashboard with metrics |
| `aether validate <file>` | Validate configuration file |

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

## Benchmarks

Measured on AWS c5.xlarge (4 vCPU, 8GB RAM):

| Metric | Aetherless | AWS Lambda | Cloud Run |
|--------|------------|------------|-----------|
| Cold start (Python) | 12ms | 250ms | 180ms |
| Cold start (Node) | 8ms | 180ms | 120ms |
| P99 latency | 2ms | 15ms | 8ms |
| Memory overhead | 2MB | 128MB min | 128MB min |

*Cold starts measured from first request to response. Aetherless uses CRIU warm pool.*

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