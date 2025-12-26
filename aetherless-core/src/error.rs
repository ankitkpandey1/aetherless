//! Custom error types for Aetherless.
//!
//! This module defines explicit enum error types as per coding guidelines.
//! No `Box<dyn Error>`, no `anyhow::Result` - all errors are strongly typed.

use std::path::PathBuf;

use thiserror::Error;

use crate::types::{FunctionId, Port};

/// Top-level error type for the Aetherless orchestrator.
/// All errors are explicit variants - no catch-all or generic handling.
#[derive(Debug, Error)]
pub enum AetherError {
    // =========================================================================
    // Configuration Errors - Fail-Fast on Invalid Config
    // =========================================================================
    #[error("Hard validation error: {0}")]
    HardValidation(#[from] HardValidationError),

    #[error("Configuration file not found: {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("Configuration parse error: {message}")]
    ConfigParse { message: String },

    // =========================================================================
    // State Machine Errors
    // =========================================================================
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(#[from] StateTransitionError),

    #[error("Function not found: {0}")]
    FunctionNotFound(FunctionId),

    #[error("Function already exists: {0}")]
    FunctionAlreadyExists(FunctionId),

    // =========================================================================
    // Shared Memory Errors - No Fallback to Alternative IPC
    // =========================================================================
    #[error("Shared memory error: {0}")]
    SharedMemory(#[from] SharedMemoryError),

    // =========================================================================
    // CRIU Errors - Strict Latency Enforcement
    // =========================================================================
    #[error("CRIU error: {0}")]
    Criu(#[from] CriuError),

    // =========================================================================
    // eBPF Errors - No Fallback to Userspace Routing
    // =========================================================================
    #[error("eBPF error: {0}")]
    Ebpf(#[from] EbpfError),

    // =========================================================================
    // System Errors
    // =========================================================================
    #[error("IO error: {context} - {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("System call failed: {syscall} - {message}")]
    Syscall {
        syscall: &'static str,
        message: String,
    },
}

/// Hard validation errors cause immediate process termination.
/// Used when configuration is invalid and the system cannot safely start.
#[derive(Debug, Error)]
pub enum HardValidationError {
    #[error("Missing required field: {field} in {context}")]
    MissingRequiredField {
        field: &'static str,
        context: String,
    },

    #[error("Invalid field value: {field} = {value} - {reason}")]
    InvalidFieldValue {
        field: &'static str,
        value: String,
        reason: String,
    },

    #[error("Memory limit out of bounds: {limit_bytes} bytes (min: {min}, max: {max})")]
    MemoryLimitOutOfBounds {
        limit_bytes: u64,
        min: u64,
        max: u64,
    },

    #[error("Invalid port: {port} - {reason}")]
    InvalidPort { port: u16, reason: String },

    #[error("Handler path does not exist: {path}")]
    HandlerPathNotFound { path: PathBuf },

    #[error("Handler path is not executable: {path}")]
    HandlerNotExecutable { path: PathBuf },

    #[error("Duplicate function ID: {id}")]
    DuplicateFunctionId { id: String },

    #[error("Schema validation failed: {message}")]
    SchemaValidation { message: String },
}

/// State transition errors for the function state machine.
#[derive(Debug, Error)]
pub enum StateTransitionError {
    #[error("Cannot transition from {from} to {to} for function {function_id}")]
    InvalidTransition {
        function_id: FunctionId,
        from: &'static str,
        to: &'static str,
    },

    #[error("Function {function_id} is in terminal state: {state}")]
    TerminalState {
        function_id: FunctionId,
        state: &'static str,
    },
}

/// Shared memory errors - critical failures with no fallback.
#[derive(Debug, Error)]
pub enum SharedMemoryError {
    #[error("Failed to create shared memory region: {name} - {reason}")]
    CreateFailed { name: String, reason: String },

    #[error("Failed to map shared memory: {reason}")]
    MapFailed { reason: String },

    #[error("Failed to unmap shared memory: {reason}")]
    UnmapFailed { reason: String },

    #[error("Ring buffer full - cannot write {size} bytes")]
    RingBufferFull { size: usize },

    #[error("Ring buffer empty - no data available")]
    RingBufferEmpty,

    #[error("Payload checksum mismatch: expected {expected:#010x}, got {actual:#010x}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("Payload size exceeds maximum: {size} > {max}")]
    PayloadTooLarge { size: usize, max: usize },

    #[error("Invalid buffer state: {reason}")]
    InvalidBufferState { reason: String },
}

/// CRIU lifecycle errors with strict latency enforcement.
#[derive(Debug, Error)]
pub enum CriuError {
    #[error("CRIU binary not found at expected path")]
    BinaryNotFound,

    #[error("Failed to spawn function process: {reason}")]
    SpawnFailed { reason: String },

    #[error("Process did not send READY signal within timeout")]
    ReadyTimeout,

    #[error("CRIU dump failed: {reason}")]
    DumpFailed { reason: String },

    #[error("CRIU restore failed: {reason}")]
    RestoreFailed { reason: String },

    #[error("Latency violation: restore took {actual_ms}ms, limit is {limit_ms}ms")]
    LatencyViolation { actual_ms: u64, limit_ms: u64 },

    #[error("Snapshot not found for function: {function_id}")]
    SnapshotNotFound { function_id: FunctionId },

    #[error("Unix socket error: {reason}")]
    UnixSocket { reason: String },
}

/// eBPF errors - no fallback to userspace routing.
#[derive(Debug, Error)]
pub enum EbpfError {
    #[error("Failed to load eBPF program: {reason}")]
    LoadFailed { reason: String },

    #[error("Failed to attach XDP program to interface {interface}: {reason}")]
    AttachFailed { interface: String, reason: String },

    #[error("BPF map '{name}' not found")]
    MapNotFound { name: String },

    #[error("BPF map is full - cannot add port {port}")]
    MapFull { port: Port },

    #[error("BPF map lookup failed for port {port}")]
    MapLookupFailed { port: Port },

    #[error("BPF map update failed for port {port}: {reason}")]
    MapUpdateFailed { port: Port, reason: String },

    #[error("BPF map operation '{operation}' failed: {reason}")]
    MapOperationFailed { operation: String, reason: String },

    #[error("Packet drop: malformed packet")]
    MalformedPacket,

    #[error("eBPF program verification failed: {reason}")]
    VerificationFailed { reason: String },
}

/// Result type alias using AetherError.
pub type AetherResult<T> = Result<T, AetherError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hard_validation_error_display() {
        let err = HardValidationError::MissingRequiredField {
            field: "memory_limit",
            context: "function 'my-func'".to_string(),
        };
        assert!(err.to_string().contains("memory_limit"));
        assert!(err.to_string().contains("my-func"));
    }

    #[test]
    fn test_error_chain() {
        let validation_err = HardValidationError::InvalidPort {
            port: 0,
            reason: "Port must be non-zero".to_string(),
        };
        let aether_err: AetherError = validation_err.into();
        assert!(matches!(aether_err, AetherError::HardValidation(_)));
    }
}
