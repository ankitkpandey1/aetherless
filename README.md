# Aetherless

<div align="center">

**⚡ High-Performance Serverless Function Orchestrator**

*Zero-fallback • eBPF-accelerated • Sub-millisecond cold starts*

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-36%20passed-brightgreen.svg)]()

</div>

---

## 🚀 What is Aetherless?

Aetherless is a **blazing-fast serverless function orchestrator** that eliminates cold start latency. Built in Rust with zero-compromise performance:

| Feature | Benefit |
|---------|---------|
| **eBPF/XDP Network Layer** | Kernel-bypass packet routing—microsecond latency |
| **CRIU Warm Pools** | Process snapshots restore in <15ms |
| **Zero-Copy Shared Memory** | Lock-free IPC with CRC32 validation |
| **Fail-Fast Architecture** | No silent degradation—errors are explicit |

### Why Aetherless?

```
Traditional Serverless          Aetherless
──────────────────────          ──────────
Cold start: 100-500ms    →      Cold start: <15ms (CRIU restore)
Network: userspace       →      Network: eBPF/XDP kernel bypass  
IPC: JSON over HTTP      →      IPC: Zero-copy shared memory
Errors: Generic          →      Errors: Strongly typed enums
```

---

## 📦 Installation

### Quick Install

```bash
# Clone and build
git clone https://github.com/yourusername/aetherless.git
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

## ⚡ Quick Start: Deploy Your First Function

Let's deploy a Python HTTP function in **under 2 minutes**.

### Step 1: Create Your Function Handler

Create `/opt/handlers/hello.py`:

```python
#!/usr/bin/env python3
"""
Aetherless Function Handler - Hello World API
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
            'message': 'Hello from Aetherless! 🚀',
            'function': os.environ.get('AETHER_FUNCTION_ID'),
            'path': self.path
        }
        self.wfile.write(json.dumps(response, indent=2).encode())
    
    def log_message(self, format, *args):
        func_id = os.environ.get('AETHER_FUNCTION_ID', 'handler')
        print(f"[{func_id}] {format % args}")

def main():
    function_id = os.environ.get('AETHER_FUNCTION_ID', 'hello')
    port = int(os.environ.get('AETHER_TRIGGER_PORT', '8080'))
    
    # Connect to Aetherless orchestrator
    socket_path = os.environ['AETHER_SOCKET']
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    sock.send(b'READY')  # Signal ready to orchestrator
    
    print(f"[{function_id}] Starting on port {port}...")
    server = HTTPServer(('0.0.0.0', port), Handler)
    server.serve_forever()

if __name__ == '__main__':
    main()
```

```bash
chmod +x /opt/handlers/hello.py
```

### Step 2: Create Configuration

Create `hello.yaml`:

```yaml
functions:
  - id: hello-api
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /opt/handlers/hello.py
    timeout_ms: 30000
    environment:
      PYTHONUNBUFFERED: "1"
```

### Step 3: Validate Configuration

```bash
$ aether validate hello.yaml

✓ Configuration is valid

Orchestrator Settings:
  SHM Buffer Size:    4194304 bytes
  Warm Pool Size:     10
  Restore Timeout:    15ms

Functions (1):
  - hello-api (port: 8080, memory: 128MB, timeout: 30000ms)
```

### Step 4: Start the Orchestrator

```bash
$ aether -c hello.yaml up --foreground

╔══════════════════════════════════════════════════════════════╗
║              AETHERLESS ORCHESTRATOR                         ║
╚══════════════════════════════════════════════════════════════╝

▶ Spawning function: hello-api
[hello-api] Starting on port 8080...
  ✓ hello-api started (PID: 12345, Port: 8080)

╔══════════════════════════════════════════════════════════════╗
║ Status: 1 functions running                                 ║
╠══════════════════════════════════════════════════════════════╣
║ ● hello-api            → http://localhost:8080  [Running]
╚══════════════════════════════════════════════════════════════╝

Press Ctrl+C to stop...
```

### Step 5: Test Your Function

```bash
$ curl http://localhost:8080/users

{
  "message": "Hello from Aetherless! 🚀",
  "function": "hello-api",
  "path": "/users"
}
```

**🎉 That's it!** Your function is running with sub-millisecond IPC overhead.

---

## 📖 Usage Guide

### CLI Commands

| Command | Description |
|---------|-------------|
| `aether up --foreground` | Start orchestrator in foreground |
| `aether down` | Stop the orchestrator |
| `aether deploy <file>` | Hot-load function configuration |
| `aether list` | List registered functions |
| `aether stats --dashboard` | TUI dashboard with metrics |
| `aether validate <file>` | Validate configuration |

### Global Options

```bash
aether -c config.yaml up    # Custom config file
aether -v up                 # Verbose logging
```

---

## 🔧 Configuration Reference

### Full Configuration Schema

```yaml
orchestrator:
  shm_buffer_size: 4194304    # Shared memory size (4MB default)
  warm_pool_size: 10          # Number of warm instances
  restore_timeout_ms: 15      # CRIU restore limit (STRICT!)
  snapshot_dir: /dev/shm/aetherless

functions:
  - id: my-function           # Unique identifier
    memory_limit_mb: 128      # Memory limit (1MB - 16GB)
    trigger_port: 8080        # HTTP port (must be unique)
    handler_path: /path/to/handler.py
    timeout_ms: 30000         # Request timeout
    environment:              # Environment variables
      KEY: "value"
```

### Handler Protocol

Every handler must:

1. **Read** `AETHER_SOCKET` environment variable
2. **Connect** to the Unix socket
3. **Send** `READY` message (exactly 5 bytes)
4. **Start** serving on `AETHER_TRIGGER_PORT`

---

## 📚 Examples

### Example 1: REST API (Python)

```yaml
# api.yaml
functions:
  - id: rest-api
    memory_limit_mb: 256
    trigger_port: 3000
    handler_path: /opt/handlers/api.py
    timeout_ms: 60000
    environment:
      DATABASE_URL: "postgres://localhost/mydb"
```

```python
#!/usr/bin/env python3
# /opt/handlers/api.py
import os, socket, json
from http.server import HTTPServer, BaseHTTPRequestHandler

class APIHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        routes = {
            '/users': {'users': [{'id': 1, 'name': 'Alice'}]},
            '/health': {'status': 'healthy'},
        }
        response = routes.get(self.path, {'error': 'Not found'})
        status = 200 if self.path in routes else 404
        
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(response).encode())

    def do_POST(self):
        content_length = int(self.headers.get('Content-Length', 0))
        body = json.loads(self.rfile.read(content_length))
        
        self.send_response(201)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps({'created': body}).encode())

if __name__ == '__main__':
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(os.environ['AETHER_SOCKET'])
    sock.send(b'READY')
    
    port = int(os.environ.get('AETHER_TRIGGER_PORT', '3000'))
    HTTPServer(('0.0.0.0', port), APIHandler).serve_forever()
```

### Example 2: Image Processor (Python)

```yaml
# image-processor.yaml
functions:
  - id: image-resize
    memory_limit_mb: 1024
    trigger_port: 8081
    handler_path: /opt/handlers/image.py
    timeout_ms: 120000
    environment:
      MAX_SIZE: "1024"
      FORMAT: "webp"
```

```python
#!/usr/bin/env python3
# /opt/handlers/image.py
import os, socket, io
from PIL import Image
from http.server import HTTPServer, BaseHTTPRequestHandler

class ImageHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers['Content-Length'])
        image_data = self.rfile.read(content_length)
        
        # Resize image
        img = Image.open(io.BytesIO(image_data))
        max_size = int(os.environ.get('MAX_SIZE', '1024'))
        img.thumbnail((max_size, max_size))
        
        # Convert format
        output = io.BytesIO()
        fmt = os.environ.get('FORMAT', 'webp').upper()
        img.save(output, format=fmt, quality=85)
        
        self.send_response(200)
        self.send_header('Content-Type', f'image/{fmt.lower()}')
        self.end_headers()
        self.wfile.write(output.getvalue())

if __name__ == '__main__':
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(os.environ['AETHER_SOCKET'])
    sock.send(b'READY')
    
    port = int(os.environ.get('AETHER_TRIGGER_PORT', '8081'))
    HTTPServer(('0.0.0.0', port), ImageHandler).serve_forever()
```

### Example 3: Rust Handler

```yaml
# rust-handler.yaml
functions:
  - id: rust-api
    memory_limit_mb: 64
    trigger_port: 9000
    handler_path: /opt/handlers/rust-handler
    timeout_ms: 10000
```

```rust
// src/main.rs
use std::os::unix::net::UnixStream;
use std::io::Write;
use std::net::TcpListener;

fn main() {
    // Connect to orchestrator
    let socket_path = std::env::var("AETHER_SOCKET").unwrap();
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    stream.write_all(b"READY").unwrap();
    
    // Start server
    let port = std::env::var("AETHER_TRIGGER_PORT").unwrap_or("9000".into());
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
    
    for stream in listener.incoming().flatten() {
        let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"ok\"}";
        let _ = std::io::Write::write_all(&mut &stream, response.as_bytes());
    }
}
```

### Example 4: Multi-Service Architecture

```yaml
# microservices.yaml
orchestrator:
  warm_pool_size: 20
  restore_timeout_ms: 10

functions:
  - id: auth-service
    memory_limit_mb: 128
    trigger_port: 9000
    handler_path: /opt/handlers/auth.py

  - id: user-service
    memory_limit_mb: 256
    trigger_port: 9001
    handler_path: /opt/handlers/users.py

  - id: notification-service
    memory_limit_mb: 64
    trigger_port: 9002
    handler_path: /opt/handlers/notify.py
```

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI (aether)                            │
│                  clap + ratatui TUI                         │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴─────────────────────────────────┐
│                     Core Orchestrator                        │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────┐   │
│  │  Registry   │  │ Unix Socket  │  │  Process Manager   │   │
│  │  (DashMap)  │  │  Handshake   │  │  (Spawn + Monitor) │   │
│  └─────────────┘  └──────────────┘  └────────────────────┘   │
└───────────────────────────────────────────────────────────────┘
         │                    │                     │
┌────────┴────────┐  ┌────────┴────────┐  ┌────────┴────────┐
│   eBPF/XDP      │  │    SHM IPC      │  │  CRIU Manager   │
│  Port→PID map   │  │  Ring Buffer    │  │  Dump/Restore   │
│  Kernel bypass  │  │  CRC32 check    │  │  15ms enforce   │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

---

## 🧪 Testing

```bash
# Run all tests
cargo test --workspace

# Run with verbose output
cargo test --workspace -- --nocapture

# Lint
cargo clippy --workspace
```

---

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md).

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with 🦀 Rust • Powered by eBPF • Accelerated by CRIU**

[Documentation](https://docs.aetherless.dev) • [Examples](./examples) • [Discord](https://discord.gg/aetherless)

</div>