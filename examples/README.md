# Aetherless Examples

This directory contains working examples to help you get started with Aetherless.

## Quick Start

```bash
# From the project root
cd ~/ZeroLambda
cargo build --release

# Run the hello world example
./target/release/aether -c examples/hello.yaml up --foreground

# In another terminal, test it
curl http://localhost:8080/
```

## Examples

### 1. Hello World (`hello.yaml` + `hello.py`)

Simple HTTP handler that responds with JSON.

```bash
./target/release/aether -c examples/hello.yaml up --foreground
curl http://localhost:8080/hello
```

### 2. REST API (`rest_api.yaml` + `rest_api.py`)

REST API with routing, GET and POST endpoints.

```bash
./target/release/aether -c examples/rest_api.yaml up --foreground

# GET requests
curl http://localhost:3000/users
curl http://localhost:3000/health

# POST request
curl -X POST http://localhost:3000/users -H "Content-Type: application/json" -d '{"name":"Alice"}'
```

### 3. Multi-Service (`multi_service.yaml`)

Multiple functions running simultaneously on different ports.

```bash
./target/release/aether -c examples/multi_service.yaml up --foreground

# Auth service on port 9000
curl http://localhost:9000/

# User service on port 9001
curl http://localhost:9001/users
```

## Handler Protocol

All handlers must:

1. Read `AETHER_SOCKET` environment variable
2. Connect to the Unix socket
3. Send `READY` (5 bytes)
4. Start serving on `AETHER_TRIGGER_PORT`

See `hello.py` for the simplest example.
