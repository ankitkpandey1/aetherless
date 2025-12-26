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

---

## 📚 Examples

### Example 1: Python HTTP Handler

**Configuration (`python-http.yaml`):**

```yaml
functions:
  - id: python-http
    memory_limit_mb: 256
    trigger_port: 8080
    handler_path: /opt/handlers/python-http.py
    timeout_ms: 30000
    environment:
      PYTHONUNBUFFERED: "1"
```

**Handler (`python-http.py`):**

```python
#!/usr/bin/env python3
"""
Aetherless Python HTTP Handler Example
"""
import os
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        response = {
            'status': 'ok',
            'function': os.environ.get('AETHER_FUNCTION_ID', 'unknown'),
            'path': self.path
        }
        self.wfile.write(json.dumps(response).encode())

def main():
    # Connect to Aetherless orchestrator
    socket_path = os.environ['AETHER_SOCKET']
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    
    # Send READY signal
    sock.send(b'READY')
    print(f"Connected to orchestrator, starting HTTP server...")
    
    # Start HTTP server
    server = HTTPServer(('0.0.0.0', 8080), Handler)
    server.serve_forever()

if __name__ == '__main__':
    main()
```

---

### Example 2: Python Image Processor

**Configuration (`image-processor.yaml`):**

```yaml
functions:
  - id: image-resize
    memory_limit_mb: 1024
    trigger_port: 8081
    handler_path: /opt/handlers/image-processor.py
    timeout_ms: 120000
    environment:
      MAX_IMAGE_SIZE: "52428800"
      OUTPUT_FORMAT: "webp"
      QUALITY: "85"
```

**Handler (`image-processor.py`):**

```python
#!/usr/bin/env python3
"""
Aetherless Image Processor Handler
"""
import os
import socket
import io
from PIL import Image
from http.server import HTTPServer, BaseHTTPRequestHandler

class ImageHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers['Content-Length'])
        image_data = self.rfile.read(content_length)
        
        # Process image
        img = Image.open(io.BytesIO(image_data))
        img.thumbnail((800, 800))
        
        # Convert to output format
        output = io.BytesIO()
        output_format = os.environ.get('OUTPUT_FORMAT', 'webp')
        quality = int(os.environ.get('QUALITY', '85'))
        img.save(output, format=output_format.upper(), quality=quality)
        
        # Send response
        self.send_response(200)
        self.send_header('Content-Type', f'image/{output_format}')
        self.end_headers()
        self.wfile.write(output.getvalue())

def main():
    # Connect to Aetherless
    socket_path = os.environ['AETHER_SOCKET']
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    sock.send(b'READY')
    
    print("Image processor ready")
    server = HTTPServer(('0.0.0.0', 8081), ImageHandler)
    server.serve_forever()

if __name__ == '__main__':
    main()
```

---

### Example 3: Rust Handler

**Configuration (`rust-handler.yaml`):**

```yaml
functions:
  - id: rust-api
    memory_limit_mb: 64
    trigger_port: 3000
    handler_path: /opt/handlers/rust-handler
    timeout_ms: 30000
    environment:
      RUST_LOG: "info"
```

**Handler (`main.rs`):**

```rust
use std::os::unix::net::UnixStream;
use std::io::{Read, Write, BufReader, BufRead};
use std::net::TcpListener;

fn main() {
    // Connect to Aetherless orchestrator
    let socket_path = std::env::var("AETHER_SOCKET")
        .expect("AETHER_SOCKET not set");
    
    let mut stream = UnixStream::connect(&socket_path)
        .expect("Failed to connect to orchestrator");
    
    // Send READY signal
    stream.write_all(b"READY").expect("Failed to send READY");
    println!("Connected to orchestrator");
    
    // Start TCP server
    let listener = TcpListener::bind("0.0.0.0:3000")
        .expect("Failed to bind");
    
    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\r\n\
                {\"status\":\"ok\"}";
            stream.write_all(response.as_bytes()).ok();
        }
    }
}
```

---

### Example 4: Multi-Function Setup

**Configuration (`multi-function.yaml`):**

```yaml
orchestrator:
  warm_pool_size: 20
  restore_timeout_ms: 10

functions:
  - id: auth-service
    memory_limit_mb: 128
    trigger_port: 9000
    handler_path: /opt/handlers/auth.py
    timeout_ms: 5000

  - id: data-processor
    memory_limit_mb: 512
    trigger_port: 9001
    handler_path: /opt/handlers/processor.py
    timeout_ms: 60000

  - id: notification-sender
    memory_limit_mb: 64
    trigger_port: 9002
    handler_path: /opt/handlers/notifier.py
    timeout_ms: 10000
```

**Auth Handler (`auth.py`):**

```python
#!/usr/bin/env python3
import os, socket, json
from http.server import HTTPServer, BaseHTTPRequestHandler

class AuthHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get('Content-Length', 0))
        body = json.loads(self.rfile.read(content_length)) if content_length else {}
        
        # Simple auth check
        token = body.get('token', '')
        is_valid = len(token) > 10  # Demo validation
        
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps({'authenticated': is_valid}).encode())

if __name__ == '__main__':
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(os.environ['AETHER_SOCKET'])
    sock.send(b'READY')
    HTTPServer(('0.0.0.0', 9000), AuthHandler).serve_forever()
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