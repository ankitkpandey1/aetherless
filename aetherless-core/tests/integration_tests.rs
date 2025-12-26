// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! End-to-end integration tests for Aetherless.
//!
//! These tests verify the complete flow from configuration to running handlers.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

/// Test handler spawning with Unix socket handshake
#[test]
fn test_handler_spawn_with_socket_handshake() {
    // Create temp directory for socket
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("test.sock");

    // Create test handler script
    let handler_script = temp_dir.path().join("handler.py");
    std::fs::write(
        &handler_script,
        r#"#!/usr/bin/env python3
import os
import socket
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')
# Exit after sending READY
"#,
    )
    .expect("Failed to write handler script");

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&handler_script)
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&handler_script, perms).unwrap();
    }

    // Create Unix listener
    let listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("Failed to bind socket");
    listener
        .set_nonblocking(true)
        .expect("Failed to set nonblocking");

    // Spawn handler
    let mut child = Command::new("python3")
        .arg(&handler_script)
        .env("AETHER_SOCKET", &socket_path)
        .env("AETHER_FUNCTION_ID", "test-func")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn handler");

    // Wait for READY signal
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let mut ready_received = false;

    while start.elapsed() < timeout {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .ok();

                let mut buf = [0u8; 16];
                if let Ok(n) = stream.read(&mut buf) {
                    if n >= 5 && &buf[..5] == b"READY" {
                        ready_received = true;
                        break;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }

    // Cleanup
    let _ = child.kill();
    let _ = child.wait();

    assert!(ready_received, "Handler did not send READY signal");
}

/// Test configuration loading and validation
#[test]
fn test_config_loading_and_validation() {
    use aetherless_core::ConfigLoader;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("test.yaml");

    // Write valid config
    std::fs::write(
        &config_path,
        r#"
orchestrator:
  shm_buffer_size: 4194304
  warm_pool_size: 5
  restore_timeout_ms: 15

functions:
  - id: test-function
    memory_limit_mb: 128
    trigger_port: 9999
    handler_path: /bin/echo
    timeout_ms: 30000
"#,
    )
    .expect("Failed to write config");

    // Load and validate
    let config = ConfigLoader::load_file(config_path.to_str().unwrap())
        .expect("Failed to load config");

    assert_eq!(config.functions.len(), 1);
    assert_eq!(config.functions[0].id.as_str(), "test-function");
    assert_eq!(config.functions[0].trigger_port.value(), 9999);
    assert_eq!(config.orchestrator.warm_pool_size, 5);
}

/// Test invalid configuration is rejected
#[test]
fn test_invalid_config_rejected() {
    use aetherless_core::ConfigLoader;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config_path = temp_dir.path().join("invalid.yaml");

    // Write invalid config (duplicate ports)
    std::fs::write(
        &config_path,
        r#"
functions:
  - id: func1
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
  - id: func2
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
"#,
    )
    .expect("Failed to write config");

    let result = ConfigLoader::load_file(config_path.to_str().unwrap());
    assert!(result.is_err(), "Duplicate ports should be rejected");
}

/// Test state machine transitions
#[test]
fn test_state_machine_transitions() {
    use aetherless_core::{FunctionId, FunctionState, FunctionStateMachine};

    let func_id = FunctionId::new("test-func").unwrap();
    let mut sm = FunctionStateMachine::new(func_id);
    assert_eq!(sm.state(), FunctionState::Uninitialized);

    // Valid: Uninitialized -> WarmSnapshot
    sm.transition_to(FunctionState::WarmSnapshot).unwrap();
    assert_eq!(sm.state(), FunctionState::WarmSnapshot);

    // Valid: WarmSnapshot -> Running
    sm.transition_to(FunctionState::Running).unwrap();
    assert_eq!(sm.state(), FunctionState::Running);

    // Valid: Running -> Suspended
    sm.transition_to(FunctionState::Suspended).unwrap();
    assert_eq!(sm.state(), FunctionState::Suspended);
}

/// Test registry concurrent access
#[test]
fn test_registry_concurrent_access() {
    use aetherless_core::{FunctionConfig, FunctionId, FunctionRegistry, HandlerPath, MemoryLimit, Port};
    use std::sync::Arc;
    use std::thread;

    let registry = FunctionRegistry::new_shared();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let reg = Arc::clone(&registry);
            thread::spawn(move || {
                let config = FunctionConfig {
                    id: FunctionId::new(format!("func-{}", i)).unwrap(),
                    memory_limit: MemoryLimit::from_mb(128).unwrap(),
                    trigger_port: Port::new(3000 + i as u16).unwrap(),
                    handler_path: HandlerPath::new("/bin/echo").unwrap(),
                    timeout_ms: 30000,
                    environment: Default::default(),
                };
                reg.register(config).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(registry.len(), 10);
}

/// Test shared memory ring buffer
#[test]
fn test_ring_buffer_write_read() {
    use aetherless_core::shm::{RingBuffer, SharedMemoryRegion};

    // Create shared memory region with unique name
    let name = format!("test_ring_{}", std::process::id());
    let region = SharedMemoryRegion::create(&name, 64 * 1024)
        .expect("Failed to create SHM region");

    let buffer = RingBuffer::new(region).expect("Failed to create ring buffer");

    // Write data
    let payload = b"Hello, Aetherless!";
    buffer.write(payload).expect("Failed to write");

    // Read data
    let read_data = buffer.read().expect("Failed to read");
    assert_eq!(read_data, payload);

    // Region automatically unlinked on drop
}

/// Test checksum validation
#[test]
fn test_checksum_validation() {
    use aetherless_core::shm::PayloadValidator;

    let payload = b"Test payload data";
    let checksum = PayloadValidator::calculate_checksum(payload);

    // Valid checksum
    assert!(PayloadValidator::validate_checksum(payload, checksum).is_ok());

    // Invalid checksum
    assert!(PayloadValidator::validate_checksum(payload, checksum + 1).is_err());
}

/// Test full E2E: config -> spawn -> HTTP request
#[test]
fn test_e2e_http_handler() {
    use std::net::TcpListener;

    // Find available port
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    // Create temp directory
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("handler.sock");
    let handler_path = temp_dir.path().join("handler.py");

    // Write HTTP handler
    std::fs::write(
        &handler_path,
        format!(
            r#"#!/usr/bin/env python3
import os
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps({{"status": "ok", "test": "e2e"}}).encode())
    def log_message(self, format, *args):
        pass

# Connect to orchestrator
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ['AETHER_SOCKET'])
sock.send(b'READY')

# Start server
server = HTTPServer(('127.0.0.1', {}), Handler)
server.handle_request()  # Handle one request then exit
"#,
            port
        ),
    )
    .expect("Failed to write handler");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&handler_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&handler_path, perms).unwrap();
    }

    // Create socket listener
    let listener = std::os::unix::net::UnixListener::bind(&socket_path)
        .expect("Failed to bind socket");
    listener.set_nonblocking(true).unwrap();

    // Spawn handler
    let mut child = Command::new("python3")
        .arg(&handler_path)
        .env("AETHER_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn");

    // Wait for READY
    let start = std::time::Instant::now();
    let mut ready = false;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_nonblocking(false).ok();
            let mut buf = [0u8; 8];
            if stream.read(&mut buf).unwrap_or(0) >= 5 && &buf[..5] == b"READY" {
                ready = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(ready, "Handler did not send READY");

    // Give server time to start
    std::thread::sleep(Duration::from_millis(200));

    // Make HTTP request
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .expect("Failed to connect to HTTP server");
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();

    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("Failed to send request");

    let mut response = String::new();
    stream.read_to_string(&mut response).ok();

    // Cleanup
    let _ = child.kill();
    let _ = child.wait();

    // Verify response
    assert!(response.contains("200 OK"), "Expected 200 OK response");
    assert!(
        response.contains(r#""status": "ok""#),
        "Expected JSON response with status ok"
    );
    assert!(
        response.contains(r#""test": "e2e""#),
        "Expected JSON response with test e2e"
    );
}
