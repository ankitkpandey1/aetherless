# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2025-12-26

### Initial Release

Aetherless v1.0.0 - High-performance serverless function orchestrator with eBPF-accelerated networking and sub-15ms cold starts.

### Features

- **CRIU Warm Pools**: Process checkpoint/restore for sub-15ms cold starts
- **eBPF/XDP Data Plane**: Kernel-bypass packet routing with microsecond latency
- **Zero-Copy Shared Memory**: Lock-free SPSC ring buffer with CRC32 validation
- **Fail-Fast Architecture**: No silent degradation, explicit error types
- **CLI Orchestrator**: Full lifecycle management with TUI dashboard
- **Language Agnostic**: Works with Python, Node, Rust, Go, or any TCP-capable process

### Components

- `aetherless-core` - Core library (config, registry, state machine, SHM, CRIU)
- `aetherless-cli` - CLI tool and orchestrator (`aether` binary)
- `aetherless-ebpf` - XDP program loader and BPF map manager

### CLI Commands

| Command | Description |
|---------|-------------|
| `aether up` | Start orchestrator |
| `aether down` | Stop orchestrator |
| `aether deploy` | Validate configuration |
| `aether list` | List functions |
| `aether stats` | Show metrics/TUI |
| `aether validate` | Validate config file |

### Documentation

- Comprehensive README with quick start guide
- ARCHITECTURE.md with design rationale
- CONTRIBUTING.md with coding guidelines
- Example handlers (Python)

### Tests

- 49 tests (36 unit + 8 integration + 5 eBPF)
- Full CI pipeline (test, clippy, fmt, build, docs)

### Requirements

- Rust 1.70+
- Linux (for eBPF/XDP features)
- Optional: CRIU for warm pools

### License

Apache-2.0

---

[1.0.0]: https://github.com/ankitkpandey1/aetherless/releases/tag/v1.0.0
