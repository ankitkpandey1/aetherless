// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Newtype wrappers for validated inputs.
//!
//! Following the "Newtype" pattern in Rust to ensure valid state by construction.
//! All types validate their invariants at creation time.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::HardValidationError;

/// Minimum allowed memory limit: 1 MB
const MIN_MEMORY_LIMIT: u64 = 1024 * 1024;
/// Maximum allowed memory limit: 16 GB
const MAX_MEMORY_LIMIT: u64 = 16 * 1024 * 1024 * 1024;

/// Validated function identifier.
/// Must be non-empty, alphanumeric with hyphens/underscores, max 64 chars.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FunctionId(String);

impl FunctionId {
    /// Create a new FunctionId with validation.
    pub fn new(id: impl Into<String>) -> Result<Self, HardValidationError> {
        let id = id.into();

        if id.is_empty() {
            return Err(HardValidationError::InvalidFieldValue {
                field: "function_id",
                value: id,
                reason: "Function ID cannot be empty".to_string(),
            });
        }

        if id.len() > 64 {
            return Err(HardValidationError::InvalidFieldValue {
                field: "function_id",
                value: id.clone(),
                reason: format!("Function ID too long: {} chars (max 64)", id.len()),
            });
        }

        // Validate characters: alphanumeric, hyphens, underscores
        if !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(HardValidationError::InvalidFieldValue {
                field: "function_id",
                value: id,
                reason: "Function ID must contain only alphanumeric characters, hyphens, and underscores".to_string(),
            });
        }

        Ok(Self(id))
    }

    /// Get the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for FunctionId {
    type Error = HardValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FunctionId> for String {
    fn from(id: FunctionId) -> Self {
        id.0
    }
}

/// Validated network port.
/// Must be in range 1-65535 (0 is reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Port(u16);

impl Port {
    /// Create a new Port with validation.
    pub fn new(port: u16) -> Result<Self, HardValidationError> {
        if port == 0 {
            return Err(HardValidationError::InvalidPort {
                port,
                reason: "Port 0 is reserved and cannot be used".to_string(),
            });
        }
        Ok(Self(port))
    }

    /// Get the inner port value.
    pub fn value(&self) -> u16 {
        self.0
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<u16> for Port {
    type Error = HardValidationError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Port> for u16 {
    fn from(port: Port) -> Self {
        port.0
    }
}

/// Validated memory limit in bytes.
/// Must be between MIN_MEMORY_LIMIT and MAX_MEMORY_LIMIT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct MemoryLimit(u64);

impl MemoryLimit {
    /// Create a new MemoryLimit with bounds validation.
    pub fn new(bytes: u64) -> Result<Self, HardValidationError> {
        if !(MIN_MEMORY_LIMIT..=MAX_MEMORY_LIMIT).contains(&bytes) {
            return Err(HardValidationError::MemoryLimitOutOfBounds {
                limit_bytes: bytes,
                min: MIN_MEMORY_LIMIT,
                max: MAX_MEMORY_LIMIT,
            });
        }
        Ok(Self(bytes))
    }

    /// Create from megabytes for convenience.
    pub fn from_mb(mb: u64) -> Result<Self, HardValidationError> {
        Self::new(mb * 1024 * 1024)
    }

    /// Get the memory limit in bytes.
    pub fn bytes(&self) -> u64 {
        self.0
    }

    /// Get the memory limit in megabytes.
    pub fn megabytes(&self) -> u64 {
        self.0 / (1024 * 1024)
    }
}

impl fmt::Display for MemoryLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}MB", self.megabytes())
    }
}

impl TryFrom<u64> for MemoryLimit {
    type Error = HardValidationError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<MemoryLimit> for u64 {
    fn from(limit: MemoryLimit) -> Self {
        limit.0
    }
}

/// Validated handler path.
/// Must exist and be executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PathBuf", into = "PathBuf")]
pub struct HandlerPath(PathBuf);

impl HandlerPath {
    /// Create a new HandlerPath with existence and permission validation.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, HardValidationError> {
        let path = path.into();

        if !path.exists() {
            return Err(HardValidationError::HandlerPathNotFound { path });
        }

        // Check if the path is executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = path.metadata() {
                let mode = metadata.permissions().mode();
                if mode & 0o111 == 0 {
                    return Err(HardValidationError::HandlerNotExecutable { path });
                }
            }
        }

        Ok(Self(path))
    }

    /// Create without validation (for testing or trusted paths).
    ///
    /// # Safety
    /// Caller must ensure the path is valid.
    pub fn new_unchecked(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Get the inner path.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl fmt::Display for HandlerPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl TryFrom<PathBuf> for HandlerPath {
    type Error = HardValidationError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<HandlerPath> for PathBuf {
    fn from(handler: HandlerPath) -> Self {
        handler.0
    }
}

/// Validated process ID.
/// Must be positive (non-zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessId(u32);

impl ProcessId {
    /// Create a new ProcessId with validation.
    pub fn new(pid: u32) -> Result<Self, HardValidationError> {
        if pid == 0 {
            return Err(HardValidationError::InvalidFieldValue {
                field: "process_id",
                value: "0".to_string(),
                reason: "Process ID 0 is reserved".to_string(),
            });
        }
        Ok(Self(pid))
    }

    /// Get the inner PID value.
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<ProcessId> for u32 {
    fn from(pid: ProcessId) -> Self {
        pid.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_id_valid() {
        assert!(FunctionId::new("my-function").is_ok());
        assert!(FunctionId::new("function_123").is_ok());
        assert!(FunctionId::new("MyFunc").is_ok());
    }

    #[test]
    fn test_function_id_invalid() {
        assert!(FunctionId::new("").is_err());
        assert!(FunctionId::new("a".repeat(65)).is_err());
        assert!(FunctionId::new("func@name").is_err());
        assert!(FunctionId::new("func name").is_err());
    }

    #[test]
    fn test_port_valid() {
        assert!(Port::new(8080).is_ok());
        assert!(Port::new(1).is_ok());
        assert!(Port::new(65535).is_ok());
    }

    #[test]
    fn test_port_invalid() {
        assert!(Port::new(0).is_err());
    }

    #[test]
    fn test_memory_limit_valid() {
        assert!(MemoryLimit::from_mb(128).is_ok());
        assert!(MemoryLimit::new(MIN_MEMORY_LIMIT).is_ok());
        assert!(MemoryLimit::new(MAX_MEMORY_LIMIT).is_ok());
    }

    #[test]
    fn test_memory_limit_invalid() {
        assert!(MemoryLimit::new(0).is_err());
        assert!(MemoryLimit::new(MIN_MEMORY_LIMIT - 1).is_err());
        assert!(MemoryLimit::new(MAX_MEMORY_LIMIT + 1).is_err());
    }

    #[test]
    fn test_process_id_valid() {
        assert!(ProcessId::new(1).is_ok());
        assert!(ProcessId::new(12345).is_ok());
    }

    #[test]
    fn test_process_id_invalid() {
        assert!(ProcessId::new(0).is_err());
    }
}
