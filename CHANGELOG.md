# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-12-26

### Added
- Initial release of Aetherless serverless orchestrator
- **Core Orchestrator**
  - Function registry with thread-safe DashMap
  - State machine (Uninitialized → WarmSnapshot → Running → Suspended)
  - YAML configuration parser with strict validation
  - Custom enum error types (HardValidationError, CriuError, etc.)
  - Newtype pattern for validated inputs (Port, MemoryLimit, FunctionId)
- **Shared Memory IPC**
  - POSIX shared memory regions (mmap/shm_open)
  - Lock-free SPSC ring buffer with atomics
  - CRC32 checksum payload validation
- **CRIU Lifecycle Manager**
  - Process snapshot/restore via CRIU
  - 15ms restore latency enforcement
  - Unix socket READY handshake protocol
- **eBPF Data Plane**
  - XDP manager for port-to-PID mapping
  - Userspace BPF map representation
- **CLI & TUI**
  - `aether up` - Start orchestrator
  - `aether down` - Stop orchestrator
  - `aether deploy` - Hot-load functions
  - `aether list` - List registered functions
  - `aether stats` - Show statistics
  - `aether validate` - Validate configuration
  - TUI dashboard with ratatui

### Security
- No fallback architecture (critical errors only)
- Fail-fast on invalid configuration
- Checksummed IPC payloads

[Unreleased]: https://github.com/ankitkpandey1/aetherless/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ankitkpandey1/aetherless/releases/tag/v0.1.0
