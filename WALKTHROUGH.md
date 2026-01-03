# Aetherless Code Walkthrough

This document provides a detailed implementation walkthrough of Aetherless, explaining the code with examples. It covers the core APIs—including the more complex ones like eBPF/BPF maps, CRIU checkpoint/restore, and lock-free ring buffers—to help contributors understand the codebase.

## Table of Contents

1. [Project Structure](#project-structure)
2. [Request Lifecycle](#request-lifecycle)
3. [Handler Spawn and Handshake Protocol](#handler-spawn-and-handshake-protocol)
4. [eBPF/XDP Deep Dive](#ebpfxdp-deep-dive)
5. [Lock-Free Ring Buffer](#lock-free-ring-buffer)
6. [CRIU Checkpoint/Restore](#criu-checkpointrestore)
7. [State Machine](#state-machine)
8. [Error Handling Pattern](#error-handling-pattern)
9. [Contributing Guide](#contributing-guide)

---

## Project Structure

```
aetherless/
├── aetherless-cli/          # CLI binary (aether command)
│   └── src/
│       ├── main.rs          # Entry point, clap CLI parsing
│       └── commands/        # Command implementations
│           ├── up.rs        # Start orchestrator
│           ├── down.rs      # Stop orchestrator
│           ├── deploy.rs    # Validate config
│           └── stats.rs     # TUI dashboard
│
├── aetherless-core/         # Shared library
│   └── src/
│       ├── config.rs        # YAML config parsing + validation
│       ├── registry.rs      # Thread-safe function storage (DashMap)
│       ├── state.rs         # FSM for function lifecycle
│       ├── error.rs         # Explicit error enums (no Box<dyn Error>)
│       ├── types.rs         # Newtype wrappers (Port, FunctionId, etc.)
│       ├── shm/             # Shared memory IPC
│       │   ├── region.rs    # POSIX shm_open/mmap
│       │   └── ring_buffer.rs # Lock-free SPSC ring buffer
│       └── criu/            # Checkpoint/restore
│           ├── snapshot.rs  # CRIU dump/restore + latency enforcement
│           └── process.rs   # Process lifecycle helpers
│
└── aetherless-ebpf/         # XDP data plane
    └── src/main.rs          # BPF program loader using Aya
```

---

## Request Lifecycle

Here's how a request flows through Aetherless:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           REQUEST LIFECYCLE                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. PACKET ARRIVES                                                           │
│     ↓                                                                        │
│  2. XDP PROGRAM (kernel)                                                     │
│     └─ bpf_map_lookup_elem(&port_redirect_map, &dest_port)                  │
│     └─ Route to handler PID/address                                         │
│     ↓                                                                        │
│  3. IF HANDLER NOT RUNNING:                                                  │
│     └─ CRIU restore from warm snapshot (<15ms)                              │
│     └─ Process resumes exactly where frozen                                 │
│     ↓                                                                        │
│  4. HANDLER PROCESSES REQUEST                                                │
│     └─ Shared memory ring buffer for IPC (zero-copy)                        │
│     ↓                                                                        │
│  5. RESPONSE SENT BACK                                                       │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Handler Spawn and Handshake Protocol

The orchestrator uses a Unix socket handshake to ensure handlers are ready before routing traffic.

### Source: [aetherless-cli/src/commands/up.rs](aetherless-cli/src/commands/up.rs)

### The Protocol

```
ORCHESTRATOR                              HANDLER
     │                                        │
     │  1. Create UnixListener                │
     ├──────────────────┐                     │
     │                  │                     │
     │  2. spawn(python handler.py)           │
     │ ───────────────────────────────────────►
     │                                        │
     │     $AETHER_SOCKET = /tmp/aetherless/my-func.sock
     │     $AETHER_TRIGGER_PORT = 8080
     │                                        │
     │  3. accept() (non-blocking, polling)   │
     ├──────────────────┐                     │
     │                  │                     │
     │                  │  4. connect()       │
     │ ◄────────────────┼─────────────────────┤
     │                  │                     │
     │                  │  5. send("READY")   │
     │ ◄────────────────┼─────────────────────┤
     │                  │                     │
     │  6. ✓ Handler ready, start routing     │
     └──────────────────┘                     │
```

### Code Example

**Orchestrator side (Rust):**

```rust
// Create Unix listener BEFORE spawning the process
let socket_path = socket_dir.join(format!("{}.sock", config.id));
let listener = UnixListener::bind(&socket_path)?;
listener.set_nonblocking(true)?;

// Set environment variables for the handler
env_vars.insert("AETHER_SOCKET".to_string(), socket_path.to_string_lossy().to_string());
env_vars.insert("AETHER_TRIGGER_PORT".to_string(), config.trigger_port.value().to_string());

// Spawn the handler process
let child = Command::new(&program)
    .args(&args)
    .envs(&env_vars)
    .spawn()?;

// Wait for READY signal (with timeout)
let start = Instant::now();
while start.elapsed() < READY_TIMEOUT {
    match listener.accept() {
        Ok((mut stream, _)) => {
            let mut buf = [0u8; 16];
            if stream.read(&mut buf).is_ok() && &buf[..5] == b"READY" {
                // Handler is ready!
                break;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // No connection yet, poll again
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(e) => return Err(e.into()),
    }
}
```

**Handler side (Python):**

```python
import os, socket

# 1. Do your initialization (import libraries, connect to DB, etc.)
initialize_my_handler()

# 2. Connect to orchestrator and signal ready
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')  # Exactly 5 bytes!

# 3. Start serving
port = int(os.environ.get('AETHER_TRIGGER_PORT', 8080))
HTTPServer(('0.0.0.0', port), MyHandler).serve_forever()
```

> **Why this design?** The handshake ensures the orchestrator never routes traffic to a handler that isn't fully initialized. Without it, requests could arrive before the handler is ready to serve them.

---

## eBPF/XDP Deep Dive

eBPF (extended Berkeley Packet Filter) is a technology that allows running sandboxed programs in the Linux kernel. XDP (eXpress Data Path) runs eBPF at the earliest point in the network stack—before `sk_buff` allocation.

### What is eBPF?

| Concept | Explanation |
|---------|-------------|
| **eBPF Program** | Bytecode that runs in kernel space, verified for safety before loading |
| **XDP Hook** | The earliest point to intercept packets (at the NIC driver level) |
| **BPF Maps** | Key-value stores shared between kernel eBPF programs and userspace |
| **Verifier** | Kernel component that ensures eBPF programs can't crash the kernel |

### Why XDP?

```
Traditional Network Stack:
  Packet → NIC → IRQ → sk_buff alloc → TCP/IP → Socket → User space
                 ↑
                100-500μs total latency

XDP Path:
  Packet → NIC → XDP Hook → Route decision → Handler
                 ↑
                5-10μs total latency
```

### Source: [aetherless-ebpf/src/main.rs](aetherless-ebpf/src/main.rs)

### BPF Map Structure

The `port_redirect_map` maps incoming ports to handler processes:

```rust
/// Key for the port-to-PID BPF map.
/// Layout must match the eBPF program's key structure.
#[repr(C)]                        // Use C memory layout (no Rust padding)
#[derive(Clone, Copy)]
pub struct PortKey {
    pub port: u16,                // 2 bytes
    pub _padding: u16,            // 2 bytes (alignment)
}                                 // Total: 4 bytes

/// Value for the port-to-PID BPF map.
#[repr(C)]
pub struct PortValue {
    pub pid: u32,                 // 4 bytes - target process ID
    pub addr: u32,                // 4 bytes - IPv4 address (network order)
}                                 // Total: 8 bytes

// Mark as safe to transfer to kernel
unsafe impl aya::Pod for PortKey {}
unsafe impl aya::Pod for PortValue {}
```

> **Why `#[repr(C)]`?** eBPF programs are compiled from C. The Rust structures must have identical memory layouts to what the kernel expects.

> **Why `unsafe impl Pod`?** `Pod` (Plain Old Data) tells Aya this struct can be safely copied byte-by-byte to/from kernel space.

### Loading an XDP Program

```rust
pub fn load_program<P: AsRef<Path>>(
    &mut self,
    program_path: P,      // Path to compiled .o file
    program_name: &str,   // Section name in the BPF object
) -> Result<(), AetherError> {
    
    // Step 1: Load the BPF object file
    let mut bpf = Ebpf::load_file(path).map_err(|e| {
        AetherError::Ebpf(EbpfError::LoadFailed {
            reason: format!("Failed to load BPF object: {}", e),
        })
    })?;

    // Step 2: Get the XDP program from the object
    let program: &mut Xdp = bpf
        .program_mut(program_name)       // Find by section name
        .ok_or_else(|| EbpfError::LoadFailed { ... })?
        .try_into()?;                    // Cast to XDP type

    // Step 3: Load program into kernel (verifier runs here!)
    program.load()?;

    // Step 4: Attach to network interface
    program.attach(&self.interface, XdpFlags::default())?;

    self.bpf = Some(bpf);
    self.loaded = true;
    Ok(())
}
```

### Registering a Port Mapping

When a function starts, we register its port in the BPF map:

```rust
pub async fn register_port(
    &mut self,
    port: Port,
    pid: ProcessId,
    addr: Option<Ipv4Addr>,
) -> Result<(), AetherError> {
    let key = PortKey { port: port.value(), _padding: 0 };
    let value = PortValue {
        pid: pid.value(),
        addr: addr.unwrap_or(Ipv4Addr::LOCALHOST).into(),
    };

    // Update userspace mirror (for lookups without syscall)
    {
        let mut map = self.port_map.write().await;
        map.insert(port.value(), value);
    }

    // Update kernel BPF map
    if let Some(ref mut bpf) = self.bpf {
        let mut bpf_map: BpfHashMap<_, PortKey, PortValue> =
            BpfHashMap::try_from(bpf.map_mut("port_redirect_map")?)?;
        
        bpf_map.insert(key, value, 0)?;  // 0 = BPF_ANY (create or update)
    }

    Ok(())
}
```

> **Why a userspace mirror?** Reading from BPF maps requires a syscall. For hot paths, we cache the mappings in userspace.

### How the XDP Program Routes Packets

The actual XDP program (C code, compiled to BPF bytecode):

```c
// Simplified XDP program logic
SEC("xdp")
int xdp_redirect(struct xdp_md *ctx) {
    // Parse packet headers (ethernet, IP, TCP)
    __u16 dest_port = parse_tcp_dest_port(ctx);
    
    // O(1) hash map lookup - runs in kernel, lock-free
    struct port_value *target = bpf_map_lookup_elem(&port_redirect_map, &dest_port);
    
    if (target) {
        // Found! Route to the handler
        return bpf_redirect(target->ifindex, 0);
    }
    
    // Not in our map, let normal stack handle it
    return XDP_PASS;
}
```

---

## Lock-Free Ring Buffer

The ring buffer enables zero-copy IPC between the orchestrator and handlers using shared memory.

### Source: [aetherless-core/src/shm/ring_buffer.rs](aetherless-core/src/shm/ring_buffer.rs)

### Memory Layout

```
┌────────────────────────────────────────────────────────────────────┐
│              Shared Memory Region (/dev/shm/aetherless-{name})     │
├────────────────────────────────────────────────────────────────────┤
│  HEADER (24 bytes, cache-line aligned)                             │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ head: AtomicU64 (8 bytes)  ← write position (producer)     │    │
│  │ tail: AtomicU64 (8 bytes)  ← read position (consumer)      │    │
│  │ capacity: AtomicU64 (8 bytes)                              │    │
│  └────────────────────────────────────────────────────────────┘    │
├────────────────────────────────────────────────────────────────────┤
│  DATA REGION (remaining bytes)                                      │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ ┌─────────┬─────────┬─────────────────────────────────┐    │    │
│  │ │ len (4) │ crc (4) │ payload (variable)              │    │    │
│  │ │ bytes   │ bytes   │                                 │    │    │
│  │ └─────────┴─────────┴─────────────────────────────────┘    │    │
│  │                                                            │    │
│  │ ┌─────────┬─────────┬─────────────────────────────────┐    │    │
│  │ │ len     │ crc     │ next payload...                 │    │    │
│  │ └─────────┴─────────┴─────────────────────────────────┘    │    │
│  └────────────────────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────────────────────┘
```

### Why SPSC (Single-Producer Single-Consumer)?

| SPSC | MPMC |
|------|------|
| 2 atomics (head, tail) | 4+ atomics + CAS loops |
| Wait-free reads/writes | May spin under contention |
| ~1-2μs latency | ~5-10μs latency |
| Perfect for 1 orchestrator → 1 handler | Overkill for our model |

### Atomic Memory Ordering

The ring buffer uses specific memory orderings to ensure correctness:

```rust
// Header structure with atomic fields
#[repr(C)]
struct RingBufferHeader {
    head: AtomicU64,     // Write position (producer updates)
    tail: AtomicU64,     // Read position (consumer updates)
    capacity: AtomicU64,
}
```

| Ordering | When Used | What It Guarantees |
|----------|-----------|-------------------|
| `Acquire` | Reading head/tail | All writes before the store are visible |
| `Release` | Updating head/tail | All prior writes are visible to acquiring readers |
| `Relaxed` | (Not used here) | Only atomicity, no ordering guarantees |

### Write Path

```rust
pub fn write(&self, payload: &[u8]) -> Result<(), SharedMemoryError> {
    let payload_len = payload.len();
    let entry_size = Self::align_up(ENTRY_HEADER_SIZE + payload_len, 8);

    // Check if there's enough space
    if entry_size > self.available_space() {
        return Err(SharedMemoryError::RingBufferFull { size: payload_len });
    }

    // Calculate CRC32 checksum
    let checksum = crc32fast::hash(payload);

    let entry_header = EntryHeader {
        length: payload_len as u32,
        checksum,
    };

    let head = self.head();  // Acquire ordering
    let offset = (head as usize) % self.capacity();

    unsafe {
        let data = self.data_ptr();

        // 1. Write entry header (length + checksum)
        let header_dest = data.add(offset) as *mut EntryHeader;
        std::ptr::write_unaligned(header_dest, entry_header);

        // 2. Write payload
        let payload_dest = data.add(offset + ENTRY_HEADER_SIZE);
        std::ptr::copy_nonoverlapping(payload.as_ptr(), payload_dest, payload_len);

        // 3. Update head with Release ordering
        //    This ensures all the writes above are visible to readers
        (*self.header_mut())
            .head
            .store(head + entry_size as u64, Ordering::Release);
    }

    Ok(())
}
```

### Read Path with Checksum Validation

```rust
pub fn read(&self) -> Result<Vec<u8>, SharedMemoryError> {
    if self.readable_bytes() < ENTRY_HEADER_SIZE {
        return Err(SharedMemoryError::RingBufferEmpty);
    }

    let tail = self.tail();  // Acquire ordering
    let offset = (tail as usize) % self.capacity();

    unsafe {
        let data = self.data_ptr();

        // 1. Read entry header
        let header_src = data.add(offset) as *const EntryHeader;
        let entry_header: EntryHeader = std::ptr::read_unaligned(header_src);

        let payload_len = entry_header.length as usize;
        let expected_checksum = entry_header.checksum;

        // 2. Read payload
        let mut payload = vec![0u8; payload_len];
        let payload_src = data.add(offset + ENTRY_HEADER_SIZE);
        std::ptr::copy_nonoverlapping(payload_src, payload.as_mut_ptr(), payload_len);

        // 3. Validate checksum - FAIL IMMEDIATELY (no fallback!)
        let actual_checksum = crc32fast::hash(&payload);
        if actual_checksum != expected_checksum {
            return Err(SharedMemoryError::ChecksumMismatch {
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        // 4. Update tail with Release ordering
        (*self.header_mut())
            .tail
            .store(tail + entry_size as u64, Ordering::Release);

        Ok(payload)
    }
}
```

> **Why CRC32?** Shared memory corruption is rare but catastrophic. A 4-byte checksum (~1μs per 64KB) catches memory errors, buffer overflows, and incomplete writes.

---

## CRIU Checkpoint/Restore

CRIU (Checkpoint/Restore In Userspace) freezes a running process and saves its complete state to disk. Later, it can restore the process exactly where it left off.

### Source: [aetherless-core/src/criu/snapshot.rs](aetherless-core/src/criu/snapshot.rs)

### What CRIU Captures

| State | How It's Saved |
|-------|----------------|
| Memory | Page by page via `/proc/{pid}/pagemap` |
| File descriptors | Socket states, file offsets |
| Registers | CPU state at freeze time |
| Signals | Pending signals, handlers |
| Namespaces | PID, network, mount namespaces |

### Creating a Snapshot (Dump)

```rust
pub fn dump(
    &mut self,
    function_id: &FunctionId,
    pid: u32,
) -> Result<SnapshotMetadata, CriuError> {
    let dump_path = self.snapshot_path(function_id);

    // Remove old dump if exists
    if dump_path.exists() {
        std::fs::remove_dir_all(&dump_path)?;
    }
    std::fs::create_dir_all(&dump_path)?;

    let start = Instant::now();

    // Execute CRIU dump command
    let output = Command::new(&self.criu_path)
        .arg("dump")
        .arg("-t").arg(pid.to_string())    // Target PID
        .arg("-D").arg(&dump_path)          // Dump directory
        .arg("-j")                          // Shell job mode
        .arg("--shell-job")
        .arg("--tcp-established")           // Handle TCP connections
        .output()?;

    if !output.status.success() {
        return Err(CriuError::DumpFailed {
            reason: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    tracing::info!(
        function_id = %function_id,
        pid = pid,
        elapsed_ms = start.elapsed().as_millis(),
        "CRIU dump completed"
    );

    Ok(SnapshotMetadata {
        function_id: function_id.clone(),
        path: dump_path,
        original_pid: pid,
        created_at: std::time::SystemTime::now(),
    })
}
```

### Restoring with Strict Latency Enforcement

This is where the "no fallback" philosophy is critical:

```rust
/// Default restore timeout - 15 milliseconds, strictly enforced
pub const DEFAULT_RESTORE_TIMEOUT_MS: u64 = 15;

pub fn restore(&self, function_id: &FunctionId) -> Result<u32, CriuError> {
    let metadata = self.snapshots.get(function_id)
        .ok_or_else(|| CriuError::SnapshotNotFound { ... })?;

    let start = Instant::now();

    // Execute CRIU restore
    let output = Command::new(&self.criu_path)
        .arg("restore")
        .arg("-D").arg(&metadata.path)
        .arg("-j")
        .arg("--shell-job")
        .arg("-d")                              // Detach after restore
        .arg("--pidfile").arg(metadata.path.join("restored.pid"))
        .output()?;

    let elapsed_ms = start.elapsed().as_millis() as u64;

    // STRICT LATENCY ENFORCEMENT - check BEFORE success
    if elapsed_ms > self.restore_timeout_ms {
        // Try to kill the restored process
        if let Ok(pid_str) = std::fs::read_to_string(metadata.path.join("restored.pid")) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                tracing::error!(
                    function_id = %function_id,
                    elapsed_ms = elapsed_ms,
                    limit_ms = self.restore_timeout_ms,
                    "Latency violation - killed restored process"
                );
            }
        }

        // Return error - NO FALLBACK
        return Err(CriuError::LatencyViolation {
            actual_ms: elapsed_ms,
            limit_ms: self.restore_timeout_ms,
        });
    }

    if !output.status.success() {
        return Err(CriuError::RestoreFailed { ... });
    }

    // Read and return the new PID
    let pid_str = std::fs::read_to_string(metadata.path.join("restored.pid"))?;
    let pid = pid_str.trim().parse::<u32>()?;

    Ok(pid)
}
```

> **Why kill on latency violation?** A slow restore defeats the purpose of Aetherless. We'd rather fail fast and let the operator know than silently accept degraded performance.

---

## State Machine

The function lifecycle is modeled as a finite state machine with explicit, compile-time-checked transitions.

### Source: [aetherless-core/src/state.rs](aetherless-core/src/state.rs)

### States

```
┌─────────────────┐        ┌─────────────────┐
│  Uninitialized  │───────►│  WarmSnapshot   │
│  (registered)   │        │  (snapshot ready)│
└────────┬────────┘        └────────┬────────┘
         │                          │
         │                          │
         ▼                          ▼
┌─────────────────┐◄───────┌─────────────────┐
│    Running      │        │    Suspended    │
│  (processing)   │◄──────►│   (paused)      │
└─────────────────┘        └─────────────────┘
```

### Transition Validation with Pattern Matching

```rust
impl FunctionState {
    /// Check if transition to the target state is valid.
    pub fn can_transition_to(&self, target: FunctionState) -> bool {
        matches!(
            (self, target),
            // From Uninitialized
            (Self::Uninitialized, Self::WarmSnapshot) |
            (Self::Uninitialized, Self::Running) |
            // From WarmSnapshot
            (Self::WarmSnapshot, Self::Running) |
            (Self::WarmSnapshot, Self::Uninitialized) |
            // From Running
            (Self::Running, Self::Suspended) |
            (Self::Running, Self::WarmSnapshot) |
            // From Suspended
            (Self::Suspended, Self::Running) |
            (Self::Suspended, Self::WarmSnapshot) |
            (Self::Suspended, Self::Uninitialized)
        )
    }
}
```

> **Why `matches!` over a transition table?** Pattern matching is checked at compile time and is self-documenting. Adding a new state requires updating this list—the compiler will tell you if transitions are missing.

### State Machine Usage

```rust
impl FunctionStateMachine {
    pub fn transition_to(&mut self, target: FunctionState) -> Result<(), StateTransitionError> {
        // Validate transition
        if !self.current_state.can_transition_to(target) {
            return Err(StateTransitionError::InvalidTransition {
                function_id: self.function_id.clone(),
                from: self.current_state.name(),
                to: target.name(),
            });
        }

        // Log the transition
        tracing::debug!(
            function_id = %self.function_id,
            from = self.current_state.name(),
            to = target.name(),
            "State transition"
        );

        // Update state
        self.current_state = target;
        self.last_transition = Instant::now();
        self.transition_count += 1;

        Ok(())
    }
}
```

---

## Error Handling Pattern

Aetherless uses explicit enum error types—no `Box<dyn Error>`, no `anyhow::Result`.

### Source: [aetherless-core/src/error.rs](aetherless-core/src/error.rs)

### The Error Hierarchy

```rust
/// Top-level error type
#[derive(Debug, Error)]
pub enum AetherError {
    // Configuration errors - fail fast at startup
    #[error("Hard validation error: {0}")]
    HardValidation(#[from] HardValidationError),

    // State machine errors
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(#[from] StateTransitionError),

    // Shared memory errors - no fallback to alternative IPC
    #[error("Shared memory error: {0}")]
    SharedMemory(#[from] SharedMemoryError),

    // CRIU errors - strict latency enforcement
    #[error("CRIU error: {0}")]
    Criu(#[from] CriuError),

    // eBPF errors - no fallback to userspace routing
    #[error("eBPF error: {0}")]
    Ebpf(#[from] EbpfError),
}

/// eBPF-specific errors
#[derive(Debug, Error)]
pub enum EbpfError {
    #[error("Failed to load eBPF program: {reason}")]
    LoadFailed { reason: String },

    #[error("Failed to attach XDP to {interface}: {reason}")]
    AttachFailed { interface: String, reason: String },

    #[error("BPF map '{name}' not found")]
    MapNotFound { name: String },

    #[error("BPF map update failed for port {port}: {reason}")]
    MapUpdateFailed { port: Port, reason: String },

    #[error("eBPF program verification failed: {reason}")]
    VerificationFailed { reason: String },
}
```

### Why Explicit Types?

```rust
// With explicit types - full context for handling
match manager.load_program("/path/to/xdp.o", "xdp_redirect") {
    Ok(()) => println!("Loaded!"),
    Err(AetherError::Ebpf(EbpfError::LoadFailed { reason })) => {
        // Can provide specific guidance
        eprintln!("Load failed: {}. Check if BPF object is compiled correctly.", reason);
    }
    Err(AetherError::Ebpf(EbpfError::AttachFailed { interface, reason })) => {
        // Different handling for attach errors
        eprintln!("Can't attach to {}: {}. Check permissions.", interface, reason);
    }
    Err(e) => eprintln!("Other error: {}", e),
}

// With Box<dyn Error> - lost context
match manager.load_program() {
    Ok(()) => println!("Loaded!"),
    Err(e) => eprintln!("Error: {}", e), // Can only print message
}
```

---

## Contributing Guide

### Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate tests
cargo test -p aetherless-core

# With logging
RUST_LOG=debug cargo test -- --nocapture
```

### Code Style

```bash
# Format code
cargo fmt --all

# Lint
cargo clippy --all-targets -- -D warnings
```

### Adding a New Error Type

1. Add the error variant to the appropriate enum in `error.rs`:

```rust
#[derive(Debug, Error)]
pub enum SharedMemoryError {
    // ... existing variants ...

    #[error("New error condition: {reason}")]
    NewCondition { reason: String },
}
```

2. Use it with the `?` operator:

```rust
fn some_operation() -> Result<(), AetherError> {
    if bad_condition {
        return Err(SharedMemoryError::NewCondition {
            reason: "explanation".to_string(),
        }.into());
    }
    Ok(())
}
```

### Adding a New State Transition

1. Add the transition in `state.rs` `can_transition_to`:

```rust
matches!(
    (self, target),
    // ... existing transitions ...
    (Self::NewState, Self::Running) |  // Add new valid transition
)
```

2. Add the new state variant:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionState {
    Uninitialized,
    WarmSnapshot,
    Running,
    Suspended,
    NewState,  // Add new state
}
```

### eBPF Development

To modify the XDP program:

```bash
# Write your C code
vim xdp_redirect.c

# Compile to BPF bytecode
clang -O2 -target bpf -c xdp_redirect.c -o xdp_redirect.o

# Test with the loader
sudo ./target/debug/aetherless-ebpf eth0 xdp_redirect.o
```

---

## Quick Reference

| Component | Key File | Purpose |
|-----------|----------|---------|
| Handler handshake | `up.rs:160-275` | Unix socket READY protocol |
| XDP loading | `main.rs:94-149` | Load and attach eBPF program |
| Port mapping | `main.rs:157-203` | Register port→PID in BPF map |
| Ring buffer write | `ring_buffer.rs:147-200` | Lock-free message enqueue |
| Ring buffer read | `ring_buffer.rs:206-269` | Lock-free message dequeue + CRC |
| CRIU dump | `snapshot.rs:134-204` | Checkpoint process state |
| CRIU restore | `snapshot.rs:213-302` | Restore with latency enforcement |
| State transitions | `state.rs:44-61` | Valid FSM transitions |
| Error types | `error.rs` | All explicit error enums |

---

## Metrics & Observability

Aetherless provides built-in observability without external sidecars.

### Prometheus Metrics
The orchestrator exposes a `/metrics` endpoint on port `9090` (default).

| Metric Name | Type | Description |
|-------------|------|-------------|
| `function_restores_total` | Counter | Number of warm snapshot restores |
| `function_restore_duration_seconds` | Histogram | Latency of restores (buckets <2ms to 100ms) |
| `warm_pool_size` | Gauge | Current number of ready-to-use snapshots |
| `function_cold_starts_total` | Counter | Full cold starts (process spawn) |

### TUI Dashboard Architecture
The live dashboard (`aether stats --dashboard`) runs as a separate process to ensure orchestrator stability.

1. **Orchestrator** writes stat snapshots to `/dev/shm/aetherless-stats.json` every 100ms.
2. **TUI Process** (`ratatui`) polls this file for lock-free updates.
3. **Reasoning**: Decoupling visualization from the core loop prevents TUI rendering stalls from affecting request processing latency.
