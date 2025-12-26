# Aetherless Architecture

This document describes the architecture of Aetherless, the design rationale behind key decisions, and provides a code walkthrough to help contributors understand how the system works.

## Table of Contents

1. [System Overview](#system-overview)
2. [Design Philosophy](#design-philosophy)
3. [Component Architecture](#component-architecture)
4. [Why CRIU for Snapshots](#why-criu-for-snapshots)
5. [Why eBPF/XDP for Networking](#why-ebpfxdp-for-networking)
6. [Why Lock-Free Shared Memory](#why-lock-free-shared-memory)
7. [Why Explicit Error Types](#why-explicit-error-types)
8. [Code Walkthrough](#code-walkthrough)

---

## System Overview

Aetherless is a serverless function orchestrator with three main layers:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              CLI Layer                                   │
│                         (aetherless-cli)                                 │
│   ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────┐  │
│   │   up    │  │ deploy  │  │  list   │  │  stats  │  │   validate  │  │
│   └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬──────┘  │
└────────┼────────────┼───────────┼───────────┼────────────────┼──────────┘
         │            │           │           │                │
         ▼            ▼           ▼           ▼                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           Core Library                                   │
│                        (aetherless-core)                                 │
│                                                                          │
│   ┌──────────────┐   ┌──────────────┐   ┌──────────────────────────┐   │
│   │   Registry   │   │ State Machine│   │   Configuration Loader   │   │
│   │  (DashMap)   │   │  (FSM)       │   │   (YAML Parser)          │   │
│   └──────────────┘   └──────────────┘   └──────────────────────────┘   │
│                                                                          │
│   ┌────────────────┐   ┌────────────────┐   ┌────────────────────────┐ │
│   │  Shared Memory │   │ CRIU Manager   │   │   Error Types          │ │
│   │  (Ring Buffer) │   │ (Dump/Restore) │   │   (Explicit Enums)     │ │
│   └────────────────┘   └────────────────┘   └────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          eBPF Data Plane                                 │
│                        (aetherless-ebpf)                                 │
│   ┌────────────────────────────────────────────────────────────────────┐│
│   │                     XDP Manager                                     ││
│   │   Port→PID Map  |  Program Loading  |  Interface Attachment       ││
│   └────────────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Design Philosophy

Aetherless is built on four core principles:

### 1. No Fallbacks

**What it means:** When a critical component fails, the system returns an error—it never silently degrades to a slower path.

**Why:** Silent degradation masks problems in production. If eBPF loading fails, we don't fall back to iptables because:
- The operator expects eBPF-level performance
- Fallback hides the real problem until it's too late
- Mixed modes are harder to debug

```rust
// What we DO:
if ebpf_failed {
    return Err(AetherError::Ebpf(EbpfError::LoadFailed { ... }));
}

// What we DON'T do:
if ebpf_failed {
    log::warn!("eBPF failed, using slow path");
    use_iptables_fallback();  // Silent degradation
}
```

### 2. Fail-Fast Validation

**What it means:** Invalid configuration terminates the process immediately at startup.

**Why:** Catching errors at startup is far cheaper than catching them in production:
- A typo in a port number discovered at startup costs 0
- The same typo discovered during a production incident costs hours
- We validate exhaustively: ports, memory limits, paths, timeouts, duplicates

### 3. Explicit Error Types

**What it means:** Every error is a strongly-typed enum variant, never `Box<dyn Error>` or `anyhow::Result`.

**Why:** Error context is critical for debugging:
- `Box<dyn Error>` loses type information at compile time
- Generic errors lead to generic error messages
- Typed errors enable exhaustive handling with `match`

### 4. Single-Producer Single-Consumer (SPSC)

**What it means:** Our shared memory IPC uses SPSC design, not MPMC.

**Why:** SPSC is dramatically simpler and faster:
- No contention between multiple writers
- Wait-free reads and writes with atomics
- Matches our model: one orchestrator, one handler per function

---

## Component Architecture

### Crate Responsibilities

| Crate | Purpose |
|-------|---------|
| `aetherless-core` | Shared library: config, registry, state machine, SHM, CRIU |
| `aetherless-cli` | CLI tool and orchestrator process |
| `aetherless-ebpf` | XDP program loader and BPF map manager |

### Module Map

```
aetherless-core/
├── config.rs      → YAML parsing, validation, defaults
├── registry.rs    → Thread-safe function storage (DashMap)
├── state.rs       → Function lifecycle FSM
├── error.rs       → All error type definitions
├── types.rs       → Newtype wrappers (Port, FunctionId, etc.)
├── shm/
│   ├── region.rs     → POSIX shm_open/mmap wrapper
│   ├── ring_buffer.rs → Lock-free SPSC ring buffer
│   └── validator.rs   → CRC32 payload validation
└── criu/
    ├── snapshot.rs    → Snapshot creation/restore
    └── process.rs     → Process lifecycle helpers
```

---

## Why CRIU for Snapshots

### The Problem: Cold Start Latency

Traditional serverless cold starts take 100-500ms because:
1. Process spawn: ~10ms
2. Runtime initialization: ~50-200ms (Python/Node/Java)
3. Library loading: ~50-100ms
4. User code initialization: ~10-100ms

### Alternative Approaches Considered

| Approach | Latency | Why We Rejected It |
|----------|---------|-------------------|
| Pre-warmed processes | ~10ms | Memory overhead for idle processes |
| Container snapshots | ~50-100ms | Docker/containerd overhead |
| Firecracker microVMs | ~125ms | Still too slow for sub-15ms target |
| Language-level snapshots | Varies | Language-specific, not universal |
| **CRIU (our choice)** | **<15ms** | **Universal, kernel-level, fastest** |

### Why CRIU Wins

CRIU (Checkpoint/Restore In Userspace) operates at the OS level:

1. **Complete State Capture:** Memory, file descriptors, sockets, signals—everything
2. **Language Agnostic:** Works with Python, Node, Rust, Java, Go—any process
3. **Kernel-Level Speed:** Restores via `mmap()`, not `read()` + `copy()`
4. **Proven at Scale:** Used by Google, Facebook, and container runtimes

### Our CRIU Pipeline

```
┌──────────────────────────────────────────────────────────────────┐
│                    WARM POOL CREATION                             │
├──────────────────────────────────────────────────────────────────┤
│  1. Spawn handler process                                         │
│  2. Wait for initialization (READY signal)                        │
│  3. Freeze process with CRIU dump                                 │
│  4. Store snapshot in /dev/shm (memory-backed)                    │
│  5. Keep N warm snapshots ready                                   │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│                    REQUEST HANDLING                               │
├──────────────────────────────────────────────────────────────────┤
│  1. Request arrives at trigger port                               │
│  2. CRIU restore from warm snapshot (<15ms)                       │
│  3. Process resumes exactly where it was frozen                   │
│  4. Handle request                                                │
│  5. Return process to pool or create new snapshot                 │
└──────────────────────────────────────────────────────────────────┘
```

### The 15ms Latency Enforcement

We **strictly enforce** a 15ms restore timeout:

```rust
const RESTORE_TIMEOUT_MS: u64 = 15;

let restore_result = criu.restore(&snapshot_path)?;
if restore_result.duration > Duration::from_millis(RESTORE_TIMEOUT_MS) {
    // FAIL IMMEDIATELY - no mercy
    return Err(CriuError::LatencyViolation {
        actual_ms: restore_result.duration.as_millis(),
        limit_ms: RESTORE_TIMEOUT_MS,
    });
}
```

**Why so strict?** Because if restore exceeds 15ms, we've lost our competitive advantage. The operator needs to know immediately—via error, not log line—that their cold starts are too slow.

---

## Why eBPF/XDP for Networking

### The Problem: Network Stack Overhead

Traditional packet path for serverless:

```
Packet → NIC → Kernel Network Stack → TCP/IP → Socket → User Space → Handler
         ↑                                                              ↓
    100-500μs latency from packet arrival to handler
```

Each step adds latency:
- Interrupt handling: ~10μs
- Protocol processing: ~20-50μs
- Socket buffer copies: ~20-50μs
- Context switch: ~10μs

### Alternative Approaches Considered

| Approach | Latency | Why We Rejected It |
|----------|---------|-------------------|
| Standard sockets | ~100-500μs | Too slow |
| SO_REUSEPORT | ~50-100μs | Still userspace |
| DPDK | ~5-10μs | Requires dedicated NIC, complex |
| Kernel bypass (custom) | ~5-10μs | Requires kernel module |
| **XDP (our choice)** | **~5-10μs** | **In-kernel, safe, maintainable** |

### Why XDP Wins

XDP (eXpress Data Path) is eBPF running at the earliest point in the network stack:

1. **Kernel-Level, Not Kernel-Module:** Verified bytecode, can't crash the kernel
2. **Pre-Allocation:** Runs before `sk_buff` allocation overhead
3. **JIT Compiled:** Near-native performance
4. **Live Update:** Can update routing without restart
5. **Observability:** Full access to BPF tracing

### Our eBPF Map Design

```
┌───────────────────────────────────────────────────────────────────────┐
│                        BPF_MAP_TYPE_HASH                              │
│                       "port_redirect_map"                             │
├───────────────────────────────────────────────────────────────────────┤
│  Key: PortKey (4 bytes)              Value: PortValue (8 bytes)       │
│  ┌────────┬────────────┐             ┌────────┬────────────────────┐  │
│  │ port   │ _padding   │             │  pid   │ target_addr        │  │
│  │ u16    │ u16        │             │  u32   │ u32 (IPv4)         │  │
│  └────────┴────────────┘             └────────┴────────────────────┘  │
├───────────────────────────────────────────────────────────────────────┤
│  Example Entries:                                                     │
│    8080 → (12345, 127.0.0.1)   // API handler                        │
│    8081 → (12346, 127.0.0.1)   // Image processor                    │
│    3000 → (12347, 127.0.0.1)   // Auth service                       │
├───────────────────────────────────────────────────────────────────────┤
│  Properties:                                                          │
│    max_entries: 1024                                                  │
│    key_size: 4 bytes                                                  │
│    value_size: 8 bytes                                                │
│    access: O(1) hash lookup                                           │
└───────────────────────────────────────────────────────────────────────┘
```

### High-Concurrency Handling

The BPF hash map handles high concurrency through:

1. **Per-CPU Hash Buckets:** Each CPU can access different buckets without contention
2. **RCU-Protected Reads:** Readers never block, even during updates
3. **Lock-Free Lookups:** `bpf_map_lookup_elem()` is wait-free on read path
4. **Atomic Updates:** Updates are atomic from reader's perspective

```c
// XDP program - runs per-packet, per-CPU
SEC("xdp")
int xdp_redirect(struct xdp_md *ctx) {
    __u16 dest_port = parse_tcp_dest_port(ctx);
    
    // O(1) lock-free lookup - safe under high concurrency
    struct port_value *target = bpf_map_lookup_elem(&port_redirect_map, &dest_port);
    
    if (target) {
        // Route to handler (per-CPU, no contention)
        return bpf_redirect(target->ifindex, 0);
    }
    return XDP_PASS;
}
```

**Why not per-CPU maps?** Because our routing table is small (~1000 entries max) and rarely updated. The simplicity of a shared hash map outweighs the marginal performance gain of per-CPU arrays.

---

## Why Lock-Free Shared Memory

### The Problem: IPC Overhead

Traditional IPC methods for function invocation:

| Method | Latency | Copies |
|--------|---------|--------|
| HTTP/JSON | ~1-10ms | 4+ (serialize, socket, deserialize) |
| gRPC | ~100-500μs | 2-3 |
| Unix socket | ~10-50μs | 2 |
| Pipe | ~5-20μs | 2 |
| **Shared memory** | **~1-5μs** | **0 (zero-copy)** |

### Why Shared Memory

1. **Zero Copy:** Data stays in place; pointers change, not bytes
2. **No Syscalls:** Once mapped, reads/writes are memory operations
3. **Cache Friendly:** Both processes share CPU cache lines
4. **Deterministic:** No network jitter or kernel scheduling delays

### Our Ring Buffer Design

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Shared Memory Layout                              │
│                    (/dev/shm/aetherless-{name})                     │
├─────────────────────────────────────────────────────────────────────┤
│  HEADER (24 bytes, cache-line aligned)                              │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ head: AtomicU64           (producer write position)          │   │
│  │ tail: AtomicU64           (consumer read position)           │   │
│  │ capacity: AtomicU64       (buffer size)                      │   │
│  └──────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────┤
│  DATA REGION                                                         │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ ┌─────────┬─────────┬─────────────────────────────────────┐  │   │
│  │ │ len (4) │ crc (4) │ payload (variable, up to 64KB)      │  │   │
│  │ └─────────┴─────────┴─────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

### Why SPSC Over MPMC

We chose Single-Producer Single-Consumer over Multi-Producer Multi-Consumer:

| SPSC | MPMC |
|------|------|
| 2 atomics (head, tail) | 4+ atomics + CAS loops |
| Wait-free reads/writes | May spin under contention |
| ~1-2μs latency | ~5-10μs latency |
| Perfect for our model | Overkill for our model |

**Our model:** One orchestrator (producer) → One handler (consumer) per function.

### CRC32 Validation

Every payload includes a CRC32 checksum:

```rust
// Write path
let checksum = crc32fast::hash(payload);
buffer.write_u32(payload.len());
buffer.write_u32(checksum);
buffer.write_bytes(payload);

// Read path
let stored_checksum = buffer.read_u32();
let computed_checksum = crc32fast::hash(payload);
if stored_checksum != computed_checksum {
    return Err(SharedMemoryError::CorruptedData { ... });
}
```

**Why bother?** Shared memory corruption is rare but catastrophic. A 4-byte checksum catches:
- Memory corruption from cosmic rays or hardware errors
- Buffer overflows from bugs
- Incomplete writes from crashes

Cost: ~1μs per 64KB payload. Worth it.

---

## Why Explicit Error Types

### The Problem: Generic Errors Lose Context

```rust
// What we see in logs with Box<dyn Error>:
Error: "something went wrong"

// What we see with our explicit types:
Error: AetherError::Ebpf(EbpfError::AttachFailed {
    interface: "eth0",
    reason: "XDP program not found in BPF object"
})
```

### Our Error Hierarchy

```
AetherError (root)
├── HardValidationError      → Config validation failures
│   ├── MissingRequiredField
│   ├── InvalidFieldValue
│   ├── MemoryLimitOutOfBounds
│   └── InvalidPort
│
├── StateTransitionError     → Invalid FSM transitions
│
├── SharedMemoryError        → SHM operations
│   ├── CreateFailed
│   ├── MapFailed
│   ├── CorruptedData
│   └── UnixSocket
│
├── CriuError                → Checkpoint/restore
│   ├── DumpFailed
│   ├── RestoreFailed
│   └── LatencyViolation
│
└── EbpfError                → XDP/BPF operations
    ├── LoadFailed
    ├── AttachFailed
    ├── MapNotFound
    └── MapOperationFailed
```

### Implementation Pattern

```rust
#[derive(Debug, Error)]
pub enum EbpfError {
    #[error("Failed to load eBPF program: {reason}")]
    LoadFailed { reason: String },
    
    #[error("Failed to attach XDP to {interface}: {reason}")]
    AttachFailed { interface: String, reason: String },
    // ...
}

// Usage - caller gets full context
match xdp_manager.load_program("/path/to/xdp.o", "xdp_redirect") {
    Ok(()) => println!("Loaded!"),
    Err(AetherError::Ebpf(EbpfError::LoadFailed { reason })) => {
        eprintln!("Load failed: {}", reason);
        // Can suggest specific fixes based on error type
    }
    Err(AetherError::Ebpf(EbpfError::AttachFailed { interface, reason })) => {
        eprintln!("Attach to {} failed: {}", interface, reason);
        // Different handling for attach vs load
    }
    Err(e) => eprintln!("Other error: {}", e),
}
```

---

## Code Walkthrough

### Flow 1: Starting the Orchestrator

When you run `aether -c config.yaml up --foreground`:

```
main.rs → CLI parse → commands::up::execute()
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
     ConfigLoader::load_file()      FunctionRegistry::new_shared()
              │                               │
              └───────────────┬───────────────┘
                              ▼
              For each function in config:
              ┌───────────────────────────────┐
              │ 1. registry.register(func)   │
              │ 2. Create Unix socket        │
              │ 3. Spawn handler process     │
              │ 4. Wait for READY signal     │
              │ 5. Transition to Running     │
              └───────────────────────────────┘
```

### Flow 2: Handler Protocol

The Unix socket handshake ensures handlers are ready before traffic:

```
ORCHESTRATOR                              HANDLER
     │                                        │
     │  spawn(python handler.py)              │
     │ ───────────────────────────────────────►
     │                                        │
     │         ◄ ── $AETHER_SOCKET ──         │
     │                                        │
     │  UnixListener::bind()                  │
     ├──────────────────┐                     │
     │                  │ (waiting)           │
     │                  │                     │
     │                  │      connect()      │
     │ ◄────────────────┼─────────────────────┤
     │                  │                     │
     │                  │   send("READY")     │
     │ ◄────────────────┼─────────────────────┤
     │                  │                     │
     │  ✓ Handler ready │                     │
     ├──────────────────┘                     │
     │                                        │
     │      (traffic begins)                  │
```

### Flow 3: State Machine

Valid transitions enforced at compile-time patterns:

```rust
impl FunctionState {
    pub fn can_transition_to(&self, target: FunctionState) -> bool {
        matches!(
            (self, target),
            (Uninitialized, WarmSnapshot) |
            (Uninitialized, Running) |
            (WarmSnapshot, Running) |
            (Running, Suspended) |
            (Running, WarmSnapshot) |
            (Suspended, Running) |
            (Suspended, WarmSnapshot)
        )
    }
}
```

**Why `matches!` over a state table?** Pattern matching is:
- Checked at compile time
- Self-documenting
- Easy to extend

---

## Key Files Reference

| File | Purpose |
|------|---------|
| [`aetherless-cli/src/main.rs`](aetherless-cli/src/main.rs) | CLI entry point |
| [`aetherless-cli/src/commands/up.rs`](aetherless-cli/src/commands/up.rs) | Orchestrator logic |
| [`aetherless-core/src/config.rs`](aetherless-core/src/config.rs) | YAML parsing |
| [`aetherless-core/src/registry.rs`](aetherless-core/src/registry.rs) | Function storage |
| [`aetherless-core/src/state.rs`](aetherless-core/src/state.rs) | FSM implementation |
| [`aetherless-core/src/error.rs`](aetherless-core/src/error.rs) | Error types |
| [`aetherless-core/src/shm/`](aetherless-core/src/shm/) | Shared memory IPC |
| [`aetherless-core/src/criu/`](aetherless-core/src/criu/) | CRIU lifecycle |
| [`aetherless-ebpf/src/main.rs`](aetherless-ebpf/src/main.rs) | XDP manager |

---

## Testing Strategy

```
49 Total Tests
├── Unit Tests (36)
│   ├── config::tests      # Parsing, validation, edge cases
│   ├── registry::tests    # Concurrent access, state transitions
│   ├── state::tests       # FSM validity
│   ├── types::tests       # Newtype validation
│   └── shm::tests         # Ring buffer, checksums
│
├── Integration Tests (8)
│   ├── test_handler_spawn_with_socket_handshake
│   ├── test_config_loading_and_validation
│   ├── test_e2e_http_handler  ← Full request flow
│   └── ...
│
└── eBPF Tests (5)
    └── XdpManager userspace operations
```

---

## Future Architecture

| Feature | Status | Notes |
|---------|--------|-------|
| Prometheus metrics | Planned | `/metrics` endpoint |
| gRPC control plane | Planned | Remote orchestrator mgmt |
| Kubernetes operator | Planned | Native k8s integration |
| io_uring for SHM | Research | Async I/O for ring buffer |
| Warm pool manager | Planned | Automatic snapshot lifecycle |
