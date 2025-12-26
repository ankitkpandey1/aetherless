# Aetherless

<div align="center">

**High-Performance Serverless Function Orchestrator**

*Zero-fallback • eBPF-accelerated • Sub-millisecond cold starts*

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

---

## 🚀 Introduction

Aetherless is a **zero-compromise serverless function orchestrator** built in Rust. It combines cutting-edge technologies to achieve unprecedented performance:

- **eBPF/XDP** for kernel-bypass network routing
- **CRIU** for process checkpoint/restore with warm pools
- **Zero-copy shared memory** for lock-free IPC
- **Strict latency enforcement** (15ms restore limit)

### Design Philosophy

| Principle | Implementation |
|-----------|----------------|
| **No Fallbacks** | If eBPF or SHM fails, return error—never degrade silently |
| **Fail-Fast** | Invalid config terminates process immediately |
| **Explicit Errors** | Custom enum types, no `Box<dyn Error>` or generics |
| **Type Safety** | Newtype pattern validates inputs at construction |

---

## 📦 Installation

### Prerequisites

```bash
# Rust toolchain (1.70+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# CRIU for process snapshots (optional)
sudo apt install criu

# eBPF tools (optional, for XDP programs)
sudo apt install linux-tools-common linux-tools-$(uname -r)
```

### Build from Source

```bash
git clone https://github.com/yourusername/aetherless.git
cd aetherless

# Build all crates
cargo build --release

# Install CLI globally
cargo install --path aetherless-cli
```

### Verify Installation

```bash
aether --version
# aether 0.1.0

aether --help
```

---

## ⚡ Quick Start

### 1. Create Configuration

```yaml
# aetherless.yaml
orchestrator:
  shm_buffer_size: 4194304   # 4 MB
  warm_pool_size: 10
  restore_timeout_ms: 15     # Strict!
  snapshot_dir: /dev/shm/aetherless

functions:
  - id: hello-world
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /usr/local/bin/my-handler
    timeout_ms: 30000
```

### 2. Validate Configuration

```bash
aether validate aetherless.yaml
# ✓ Configuration is valid
# Functions (1):
#   - hello-world (port: 8080, memory: 128MB, timeout: 30000ms)
```

### 3. Start Orchestrator

```bash
# Foreground mode
aether up --foreground

# Or with verbose logging
aether -v up --foreground
```

### 4. Deploy Functions

```bash
aether deploy function.yaml
# ✓ Function(s) deployed successfully
#   - hello-world (port: 8080, memory: 128MB)
```

### 5. Monitor with TUI

```bash
aether stats --dashboard
```

---

## 📖 Usage Guide

### CLI Commands

| Command | Description |
|---------|-------------|
| `aether up` | Start the orchestrator |
| `aether down` | Stop the orchestrator |
| `aether deploy <file>` | Hot-load function configuration |
| `aether list` | List registered functions |
| `aether stats` | Show eBPF/SHM/CRIU statistics |
| `aether validate <file>` | Validate configuration file |

### Command Options

```bash
# Global options
aether -c custom.yaml up     # Custom config file
aether -v up                  # Verbose logging

# Up command
aether up --foreground       # Run in foreground

# Deploy command
aether deploy func.yaml --force  # Force reload existing

# Stats command
aether stats --dashboard     # TUI dashboard
aether stats --watch         # Continuous updates
```

---

## 🔧 Advanced Usage

### Configuration Schema

```yaml
orchestrator:
  # Shared memory buffer size (bytes)
  # Range: 64KB - 1GB, Default: 4MB
  shm_buffer_size: 4194304

  # Number of warm function instances
  # Range: 1 - 1000, Default: 10
  warm_pool_size: 10

  # CRIU restore timeout (milliseconds)
  # STRICT ENFORCEMENT: Exceeds = kill + error
  # Range: 1 - 100ms, Default: 15ms
  restore_timeout_ms: 15

  # Snapshot directory (use /dev/shm for speed)
  snapshot_dir: /dev/shm/aetherless

functions:
  - id: my-function           # Alphanumeric, hyphens, underscores
    memory_limit_mb: 128      # 1 MB - 16 GB
    trigger_port: 8080        # 1 - 65535 (unique per function)
    handler_path: /path/to/binary
    timeout_ms: 30000         # 1 - 900000 (15 min max)
    environment:              # Optional env vars
      KEY: "value"
```

### Function Handler Protocol

Function handlers must:

1. Read the socket path from `AETHER_SOCKET` env var
2. Connect to the Unix socket
3. Send `READY` message when initialized
4. Process events from shared memory

```rust
// Example handler (Rust)
use std::os::unix::net::UnixStream;
use std::io::Write;

fn main() {
    let socket_path = std::env::var("AETHER_SOCKET").unwrap();
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    
    // Signal ready
    stream.write_all(b"READY").unwrap();
    
    // Process events...
}
```

### Shared Memory IPC

The ring buffer uses lock-free atomics for SPSC communication:

```
┌──────────────────────────────────────────────────┐
│ Header (24 bytes)                                │
│   head: AtomicU64 (write position)              │
│   tail: AtomicU64 (read position)               │
│   capacity: AtomicU64                           │
├──────────────────────────────────────────────────┤
│ Entry 1: [length:4][checksum:4][payload...]     │
│ Entry 2: [length:4][checksum:4][payload...]     │
│ ...                                             │
└──────────────────────────────────────────────────┘
```

### CRIU Warm Pool

Functions go through a state machine:

```
Uninitialized → WarmSnapshot → Running → Suspended
      ↑              ↓           ↓           ↓
      └──────────────┴───────────┴───────────┘
```

- **WarmSnapshot**: Process dumped to `/dev/shm`, ready for instant restore
- **Restore constraint**: Must complete in ≤15ms or process is killed

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        CLI (aether)                         │
│                    clap + ratatui TUI                       │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┼─────────────────────────────────┐
│                      Core Orchestrator                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐   │
│  │  Registry   │  │ Config      │  │ State Machine       │   │
│  │  (DashMap)  │  │ Validator   │  │ (Typed Transitions) │   │
│  └─────────────┘  └─────────────┘  └─────────────────────┘   │
└───────────────────────────────────────────────────────────────┘
         │                    │                     │
┌────────┴────────┐  ┌────────┴────────┐  ┌────────┴────────┐
│   eBPF/XDP      │  │    SHM IPC      │  │  CRIU Manager   │
│  Port→PID map   │  │  Ring Buffer    │  │  Dump/Restore   │
│  Packet redirect│  │  CRC32 check    │  │  15ms enforce   │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

### Crate Structure

| Crate | Description |
|-------|-------------|
| `aetherless-core` | Core library (registry, state, shm, criu) |
| `aetherless-ebpf` | XDP program loader and BPF map manager |
| `aetherless-cli` | CLI tool and TUI dashboard |

### Error Handling

All errors are explicit enum variants:

```rust
pub enum AetherError {
    HardValidation(HardValidationError),  // Fail-fast
    InvalidStateTransition(StateTransitionError),
    SharedMemory(SharedMemoryError),      // No fallback
    Criu(CriuError),                      // Latency enforced
    Ebpf(EbpfError),                      // No userspace fallback
}
```

---

## 📚 Examples

### Example 1: HTTP Handler

```yaml
# http-handler.yaml
functions:
  - id: http-api
    memory_limit_mb: 256
    trigger_port: 3000
    handler_path: /opt/handlers/http-server
    timeout_ms: 60000
    environment:
      RUST_LOG: "info"
      DATABASE_URL: "postgres://localhost/mydb"
```

### Example 2: Image Processor

```yaml
# image-processor.yaml
functions:
  - id: image-resize
    memory_limit_mb: 1024
    trigger_port: 8081
    handler_path: /opt/handlers/image-processor
    timeout_ms: 120000
    environment:
      MAX_IMAGE_SIZE: "52428800"  # 50MB
      OUTPUT_FORMAT: "webp"
      QUALITY: "85"
```

### Example 3: Multi-Function Setup

```yaml
# multi-function.yaml
orchestrator:
  warm_pool_size: 20
  restore_timeout_ms: 10  # Aggressive

functions:
  - id: auth-service
    memory_limit_mb: 128
    trigger_port: 9000
    handler_path: /opt/handlers/auth

  - id: data-processor
    memory_limit_mb: 512
    trigger_port: 9001
    handler_path: /opt/handlers/processor

  - id: notification-sender
    memory_limit_mb: 64
    trigger_port: 9002
    handler_path: /opt/handlers/notifier
```

---

## 🧪 Testing

```bash
# Run all tests
cargo test --workspace

# Run specific module tests
cargo test -p aetherless-core config::tests

# Run with output
cargo test --workspace -- --nocapture
```

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🤝 Contributing

Contributions welcome! Please read our contributing guidelines before submitting PRs.

---

<div align="center">
<sub>Built with 🦀 Rust • Powered by eBPF • Accelerated by CRIU</sub>
</div>