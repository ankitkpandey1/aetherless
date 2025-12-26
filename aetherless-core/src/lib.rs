//! Aetherless Core Library
//!
//! Core orchestrator library for the Aetherless serverless platform.
//! Provides function registry, state machine, configuration parsing,
//! shared memory IPC, and CRIU lifecycle management.

pub mod config;
pub mod criu;
pub mod error;
pub mod registry;
pub mod shm;
pub mod state;
pub mod types;

// Re-export commonly used types
pub use config::{Config, ConfigLoader, FunctionConfig, OrchestratorConfig};
pub use error::{AetherError, AetherResult, EbpfError, HardValidationError};
pub use registry::FunctionRegistry;
pub use state::{FunctionState, FunctionStateMachine};
pub use types::{FunctionId, HandlerPath, MemoryLimit, Port, ProcessId};
