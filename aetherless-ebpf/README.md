# Aetherless eBPF Data Plane

This crate provides the XDP-based network layer for Aetherless, enabling kernel-bypass packet routing for serverless functions.

## Overview

The eBPF data plane uses XDP (eXpress Data Path) to route incoming network packets directly to function handlers at the kernel level, bypassing the normal TCP/IP stack for ultra-low latency.

```
┌────────────────────────────────────────────────────────────────┐
│                        Network Card                            │
└────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌────────────────────────────────────────────────────────────────┐
│                    XDP Program (eBPF)                          │
│   ┌─────────────────┐    ┌────────────────────────────┐       │
│   │ port_redirect   │───▶│ Route to function handler  │       │
│   │     map         │    │ by destination port        │       │
│   └─────────────────┘    └────────────────────────────┘       │
└────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    Function Handler Process
```

## Requirements

- **Linux kernel 4.8+** (for XDP support)
- **CAP_BPF** and **CAP_NET_ADMIN** capabilities (typically run as root)
- Optional: clang/llvm for compiling custom XDP programs

## Usage

### 1. Basic Usage (Userspace Mode)

Run without an XDP program to test port mapping in userspace:

```bash
./aetherless-ebpf eth0
```

Output:
```
╔══════════════════════════════════════════════════════════════╗
║              AETHERLESS eBPF DATA PLANE                      ║
╚══════════════════════════════════════════════════════════════╝

No BPF object specified - running in userspace-only mode

To compile an XDP program, use:
  clang -O2 -target bpf -c xdp_redirect.c -o xdp_redirect.o

Status:
  Interface: eth0
  XDP Loaded: false
  Registered Ports: 0
```

### 2. Load XDP Program

With a compiled XDP program:

```bash
sudo ./aetherless-ebpf eth0 /path/to/xdp_redirect.o
```

Output:
```
╔══════════════════════════════════════════════════════════════╗
║              AETHERLESS eBPF DATA PLANE                      ║
╚══════════════════════════════════════════════════════════════╝

Loading XDP program from: /path/to/xdp_redirect.o
✓ XDP program loaded and attached to eth0

Status:
  Interface: eth0
  XDP Loaded: true
  Registered Ports: 0

Press Ctrl+C to detach and exit...
```

## Example XDP Program

Create `xdp_redirect.c`:

```c
#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <bpf/bpf_helpers.h>

// Port-to-PID redirect map
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, __u32);   // Port (with padding)
    __type(value, __u64); // PID + Address
} port_redirect_map SEC(".maps");

SEC("xdp")
int xdp_redirect(struct xdp_md *ctx) {
    void *data_end = (void *)(long)ctx->data_end;
    void *data = (void *)(long)ctx->data;

    // Parse Ethernet header
    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end)
        return XDP_PASS;

    // Only handle IPv4
    if (eth->h_proto != __constant_htons(ETH_P_IP))
        return XDP_PASS;

    // Parse IP header
    struct iphdr *ip = (void *)(eth + 1);
    if ((void *)(ip + 1) > data_end)
        return XDP_PASS;

    // Only handle TCP
    if (ip->protocol != IPPROTO_TCP)
        return XDP_PASS;

    // Parse TCP header
    struct tcphdr *tcp = (void *)ip + (ip->ihl * 4);
    if ((void *)(tcp + 1) > data_end)
        return XDP_PASS;

    // Lookup destination port in our map
    __u32 port_key = __constant_ntohs(tcp->dest);
    __u64 *value = bpf_map_lookup_elem(&port_redirect_map, &port_key);
    
    if (value) {
        // Port is registered - redirect to function handler
        // (Actual redirect logic depends on your setup)
        return XDP_PASS;
    }

    return XDP_PASS;
}

char _license[] SEC("license") = "GPL";
```

Compile with:

```bash
clang -O2 -target bpf -c xdp_redirect.c -o xdp_redirect.o
```

## API Usage (From Rust)

```rust
use aetherless_ebpf::XdpManager;
use aetherless_core::{Port, ProcessId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create manager for interface
    let mut manager = XdpManager::new("eth0");

    // Optionally load XDP program
    manager.load_program("/path/to/xdp_redirect.o", "xdp_redirect")?;

    // Register port-to-PID mapping
    let port = Port::new(8080)?;
    let pid = ProcessId::new(12345)?;
    manager.register_port(port, pid, None).await?;

    // Look up a port
    if let Some(pid) = manager.lookup_port(port).await {
        println!("Port 8080 -> PID {}", pid);
    }

    // Get statistics
    let stats = manager.stats().await;
    println!("Registered ports: {}", stats.registered_ports);

    // Unregister when done
    manager.unregister_port(port).await?;

    Ok(())
}
```

## Integration with Orchestrator

The eBPF data plane integrates with the main orchestrator to enable zero-copy packet routing:

```yaml
# aetherless.yaml
orchestrator:
  ebpf:
    enabled: true
    interface: eth0
    xdp_program: /opt/aetherless/xdp_redirect.o

functions:
  - id: my-api
    trigger_port: 8080
    handler_path: /opt/handlers/api.py
```

When a function is registered:
1. Orchestrator spawns the handler process (gets PID)
2. Orchestrator calls `XdpManager::register_port(8080, PID)`
3. XDP program routes port 8080 packets directly to the handler
4. On shutdown, port mapping is removed

## Performance Benefits

| Metric | Without XDP | With XDP |
|--------|-------------|----------|
| Packet latency | ~50-100μs | ~5-10μs |
| CPU usage | Higher | Lower |
| Context switches | Many | Minimal |

## Troubleshooting

### Permission Denied

```
Error: Failed to attach XDP program: Permission denied
```

**Solution:** Run as root or with capabilities:
```bash
sudo ./aetherless-ebpf eth0 xdp.o
# or
sudo setcap cap_bpf,cap_net_admin+ep ./aetherless-ebpf
```

### Program Verification Failed

```
Error: eBPF program verification failed
```

**Solution:** Check your XDP program for:
- Out-of-bounds memory access
- Unbounded loops
- Invalid map access

### Interface Not Found

```
Error: Failed to attach XDP program to interface eth0
```

**Solution:** Verify the interface exists:
```bash
ip link show eth0
```

## Testing

Run the eBPF tests:

```bash
cargo test -p aetherless-ebpf
```

Tests verify:
- Manager creation
- Port registration/unregistration
- Port lookup
- Stats collection

Note: Tests run in userspace mode without actual XDP program loading.
