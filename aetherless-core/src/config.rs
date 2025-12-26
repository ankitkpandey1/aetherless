// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! YAML configuration parser with strict schema validation.
//!
//! Validates function configurations at boot-up time.
//! Any invalid field results in a HardValidationError that prevents startup.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AetherError, AetherResult, HardValidationError};
use crate::types::{FunctionId, HandlerPath, MemoryLimit, Port};

/// Raw configuration as parsed from YAML (before validation).
#[derive(Debug, Deserialize)]
struct RawFunctionConfig {
    id: String,
    memory_limit_mb: u64,
    trigger_port: u16,
    handler_path: String,
    #[serde(default)]
    environment: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30000 // 30 seconds
}

/// Raw orchestrator configuration.
#[derive(Debug, Deserialize)]
struct RawOrchestratorConfig {
    #[serde(default = "default_shm_size")]
    shm_buffer_size: usize,
    #[serde(default = "default_warm_pool_size")]
    warm_pool_size: usize,
    #[serde(default = "default_restore_timeout_ms")]
    restore_timeout_ms: u64,
    #[serde(default = "default_snapshot_dir")]
    snapshot_dir: String,
}

fn default_shm_size() -> usize {
    4 * 1024 * 1024 // 4MB
}

fn default_warm_pool_size() -> usize {
    10
}

fn default_restore_timeout_ms() -> u64 {
    15 // Strict: 15ms restore timeout
}

fn default_snapshot_dir() -> String {
    "/dev/shm/aetherless".to_string()
}

impl Default for RawOrchestratorConfig {
    fn default() -> Self {
        Self {
            shm_buffer_size: default_shm_size(),
            warm_pool_size: default_warm_pool_size(),
            restore_timeout_ms: default_restore_timeout_ms(),
            snapshot_dir: default_snapshot_dir(),
        }
    }
}

/// Raw root configuration file.
#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    orchestrator: RawOrchestratorConfig,
    functions: Vec<RawFunctionConfig>,
}

/// Validated function configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionConfig {
    pub id: FunctionId,
    pub memory_limit: MemoryLimit,
    pub trigger_port: Port,
    pub handler_path: HandlerPath,
    pub environment: HashMap<String, String>,
    pub timeout_ms: u64,
}

/// Validated orchestrator configuration.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub shm_buffer_size: usize,
    pub warm_pool_size: usize,
    pub restore_timeout_ms: u64,
    pub snapshot_dir: std::path::PathBuf,
}

/// Complete validated configuration.
#[derive(Debug)]
pub struct Config {
    pub orchestrator: OrchestratorConfig,
    pub functions: Vec<FunctionConfig>,
}

/// Configuration loader with strict validation.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load and validate configuration from a YAML file.
    /// Returns HardValidationError for any invalid fields.
    pub fn load_file(path: impl AsRef<Path>) -> AetherResult<Config> {
        let path = path.as_ref();

        if !path.exists() {
            return Err(AetherError::ConfigNotFound {
                path: path.to_path_buf(),
            });
        }

        let content = std::fs::read_to_string(path).map_err(|e| AetherError::Io {
            context: "reading config file",
            source: e,
        })?;

        Self::load_string(&content)
    }

    /// Load and validate configuration from a YAML string.
    pub fn load_string(content: &str) -> AetherResult<Config> {
        let raw: RawConfig =
            serde_yaml::from_str(content).map_err(|e| AetherError::ConfigParse {
                message: format!("YAML parse error: {}", e),
            })?;

        Self::validate(raw)
    }

    /// Validate raw configuration and convert to validated types.
    fn validate(raw: RawConfig) -> AetherResult<Config> {
        // Validate orchestrator config
        let orchestrator = Self::validate_orchestrator(raw.orchestrator)?;

        // Validate all functions
        let mut functions = Vec::with_capacity(raw.functions.len());
        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_ports = std::collections::HashSet::new();

        for (index, raw_func) in raw.functions.into_iter().enumerate() {
            let func = Self::validate_function(raw_func, index)?;

            // Check for duplicate IDs
            if !seen_ids.insert(func.id.as_str().to_string()) {
                return Err(HardValidationError::DuplicateFunctionId {
                    id: func.id.to_string(),
                }
                .into());
            }

            // Check for duplicate ports
            if !seen_ports.insert(func.trigger_port.value()) {
                return Err(HardValidationError::InvalidPort {
                    port: func.trigger_port.value(),
                    reason: format!(
                        "Port {} is already used by another function",
                        func.trigger_port
                    ),
                }
                .into());
            }

            functions.push(func);
        }

        if functions.is_empty() {
            return Err(HardValidationError::SchemaValidation {
                message: "At least one function must be defined".to_string(),
            }
            .into());
        }

        Ok(Config {
            orchestrator,
            functions,
        })
    }

    /// Validate orchestrator configuration.
    fn validate_orchestrator(raw: RawOrchestratorConfig) -> AetherResult<OrchestratorConfig> {
        // Validate SHM buffer size (min 64KB, max 1GB)
        const MIN_SHM_SIZE: usize = 64 * 1024;
        const MAX_SHM_SIZE: usize = 1024 * 1024 * 1024;

        if raw.shm_buffer_size < MIN_SHM_SIZE || raw.shm_buffer_size > MAX_SHM_SIZE {
            return Err(HardValidationError::InvalidFieldValue {
                field: "shm_buffer_size",
                value: raw.shm_buffer_size.to_string(),
                reason: format!(
                    "Must be between {} and {} bytes",
                    MIN_SHM_SIZE, MAX_SHM_SIZE
                ),
            }
            .into());
        }

        // Validate warm pool size
        if raw.warm_pool_size == 0 || raw.warm_pool_size > 1000 {
            return Err(HardValidationError::InvalidFieldValue {
                field: "warm_pool_size",
                value: raw.warm_pool_size.to_string(),
                reason: "Must be between 1 and 1000".to_string(),
            }
            .into());
        }

        // Validate restore timeout (max 100ms for performance)
        if raw.restore_timeout_ms > 100 {
            return Err(HardValidationError::InvalidFieldValue {
                field: "restore_timeout_ms",
                value: raw.restore_timeout_ms.to_string(),
                reason: "Restore timeout must not exceed 100ms for latency requirements"
                    .to_string(),
            }
            .into());
        }

        let snapshot_dir = std::path::PathBuf::from(&raw.snapshot_dir);

        Ok(OrchestratorConfig {
            shm_buffer_size: raw.shm_buffer_size,
            warm_pool_size: raw.warm_pool_size,
            restore_timeout_ms: raw.restore_timeout_ms,
            snapshot_dir,
        })
    }

    /// Validate a single function configuration.
    fn validate_function(raw: RawFunctionConfig, index: usize) -> AetherResult<FunctionConfig> {
        let context = format!("function at index {}", index);

        // Validate function ID
        let id = FunctionId::new(&raw.id).map_err(|mut e| {
            if let HardValidationError::InvalidFieldValue { ref mut field, .. } = e {
                *field = "id";
            }
            e
        })?;

        // Validate memory limit
        let memory_limit = MemoryLimit::from_mb(raw.memory_limit_mb).map_err(|e| {
            HardValidationError::InvalidFieldValue {
                field: "memory_limit_mb",
                value: raw.memory_limit_mb.to_string(),
                reason: e.to_string(),
            }
        })?;

        // Validate trigger port
        let trigger_port = Port::new(raw.trigger_port)?;

        // Validate handler path (existence check is optional at config load time)
        // In production, we'd validate the path exists
        let handler_path = HandlerPath::new_unchecked(&raw.handler_path);

        // Validate timeout
        if raw.timeout_ms == 0 {
            return Err(HardValidationError::InvalidFieldValue {
                field: "timeout_ms",
                value: "0".to_string(),
                reason: "Timeout must be greater than 0".to_string(),
            }
            .into());
        }

        if raw.timeout_ms > 900_000 {
            // 15 minutes max
            return Err(HardValidationError::InvalidFieldValue {
                field: "timeout_ms",
                value: raw.timeout_ms.to_string(),
                reason: "Timeout must not exceed 15 minutes (900000ms)".to_string(),
            }
            .into());
        }

        // Validate environment variables
        for key in raw.environment.keys() {
            if key.is_empty() {
                return Err(HardValidationError::InvalidFieldValue {
                    field: "environment",
                    value: format!("empty key in {}", context),
                    reason: "Environment variable names cannot be empty".to_string(),
                }
                .into());
            }
        }

        Ok(FunctionConfig {
            id,
            memory_limit,
            trigger_port,
            handler_path,
            environment: raw.environment,
            timeout_ms: raw.timeout_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CONFIG: &str = r#"
orchestrator:
  shm_buffer_size: 4194304
  warm_pool_size: 10
  restore_timeout_ms: 15
  snapshot_dir: /dev/shm/aetherless

functions:
  - id: hello-world
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
    timeout_ms: 30000
    environment:
      DEBUG: "true"
"#;

    #[test]
    fn test_valid_config() {
        let config = ConfigLoader::load_string(VALID_CONFIG).unwrap();
        assert_eq!(config.functions.len(), 1);
        assert_eq!(config.functions[0].id.as_str(), "hello-world");
        assert_eq!(config.orchestrator.warm_pool_size, 10);
    }

    #[test]
    fn test_missing_functions() {
        let yaml = r#"
orchestrator:
  warm_pool_size: 10
functions: []
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_function_id() {
        let yaml = r#"
functions:
  - id: ""
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_port_zero() {
        let yaml = r#"
functions:
  - id: test-func
    memory_limit_mb: 128
    trigger_port: 0
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_ports() {
        let yaml = r#"
functions:
  - id: func1
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
  - id: func2
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_ids() {
        let yaml = r#"
functions:
  - id: my-func
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
  - id: my-func
    memory_limit_mb: 256
    trigger_port: 8081
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_memory_limit() {
        let yaml = r#"
functions:
  - id: test-func
    memory_limit_mb: 0
    trigger_port: 8080
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_timeout_too_high() {
        let yaml = r#"
orchestrator:
  restore_timeout_ms: 200
functions:
  - id: test-func
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
"#;
        let result = ConfigLoader::load_string(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_defaults_applied() {
        let yaml = r#"
functions:
  - id: test-func
    memory_limit_mb: 128
    trigger_port: 8080
    handler_path: /bin/echo
"#;
        let config = ConfigLoader::load_string(yaml).unwrap();
        assert_eq!(config.functions[0].timeout_ms, 30000);
        assert_eq!(config.orchestrator.restore_timeout_ms, 15);
    }
}
