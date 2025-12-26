//! Thread-safe function registry using DashMap.
//!
//! Provides concurrent access to registered functions and their state machines.

use std::sync::Arc;

use dashmap::DashMap;

use crate::config::FunctionConfig;
use crate::error::{AetherError, AetherResult};
use crate::state::{FunctionState, FunctionStateMachine, StateMachineMetrics};
use crate::types::FunctionId;

/// Entry in the function registry.
#[derive(Debug)]
pub struct FunctionEntry {
    /// Function configuration.
    pub config: FunctionConfig,
    /// State machine managing the function lifecycle.
    pub state_machine: FunctionStateMachine,
}

impl FunctionEntry {
    /// Create a new function entry.
    pub fn new(config: FunctionConfig) -> Self {
        let state_machine = FunctionStateMachine::new(config.id.clone());
        Self {
            config,
            state_machine,
        }
    }
}

/// Thread-safe registry for managing functions.
/// Uses DashMap for lock-free concurrent access.
#[derive(Debug)]
pub struct FunctionRegistry {
    /// Map of function ID to function entry.
    functions: DashMap<FunctionId, FunctionEntry>,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            functions: DashMap::new(),
        }
    }

    /// Create a registry wrapped in an Arc for sharing across threads.
    pub fn new_shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Register a new function.
    /// Returns HardValidationError if function already exists.
    pub fn register(&self, config: FunctionConfig) -> AetherResult<()> {
        let id = config.id.clone();

        // Check for duplicate - fail fast
        if self.functions.contains_key(&id) {
            return Err(AetherError::FunctionAlreadyExists(id));
        }

        let entry = FunctionEntry::new(config);
        self.functions.insert(id, entry);

        Ok(())
    }

    /// Unregister a function.
    pub fn unregister(&self, id: &FunctionId) -> AetherResult<FunctionEntry> {
        self.functions
            .remove(id)
            .map(|(_, entry)| entry)
            .ok_or_else(|| AetherError::FunctionNotFound(id.clone()))
    }

    /// Get the current state of a function.
    pub fn get_state(&self, id: &FunctionId) -> AetherResult<FunctionState> {
        self.functions
            .get(id)
            .map(|entry| entry.state_machine.state())
            .ok_or_else(|| AetherError::FunctionNotFound(id.clone()))
    }

    /// Transition a function to a new state.
    pub fn transition(&self, id: &FunctionId, target: FunctionState) -> AetherResult<()> {
        let mut entry = self
            .functions
            .get_mut(id)
            .ok_or_else(|| AetherError::FunctionNotFound(id.clone()))?;

        entry.state_machine.transition_to(target)?;
        Ok(())
    }

    /// Check if a function exists.
    pub fn contains(&self, id: &FunctionId) -> bool {
        self.functions.contains_key(id)
    }

    /// Get the number of registered functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Get a list of all function IDs.
    pub fn function_ids(&self) -> Vec<FunctionId> {
        self.functions.iter().map(|r| r.key().clone()).collect()
    }

    /// Get functions in a specific state.
    pub fn functions_in_state(&self, state: FunctionState) -> Vec<FunctionId> {
        self.functions
            .iter()
            .filter(|r| r.state_machine.state() == state)
            .map(|r| r.key().clone())
            .collect()
    }

    /// Get metrics for all functions.
    pub fn metrics(&self) -> Vec<StateMachineMetrics> {
        self.functions
            .iter()
            .map(|r| StateMachineMetrics::from(&r.state_machine))
            .collect()
    }

    /// Get the configuration for a function.
    pub fn get_config(&self, id: &FunctionId) -> AetherResult<FunctionConfig> {
        self.functions
            .get(id)
            .map(|entry| entry.config.clone())
            .ok_or_else(|| AetherError::FunctionNotFound(id.clone()))
    }

    /// Update the configuration for a function (hot-reload).
    pub fn update_config(&self, config: FunctionConfig) -> AetherResult<()> {
        let mut entry = self
            .functions
            .get_mut(&config.id)
            .ok_or_else(|| AetherError::FunctionNotFound(config.id.clone()))?;

        entry.config = config;
        Ok(())
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryLimit, Port};

    fn make_config(name: &str) -> FunctionConfig {
        FunctionConfig {
            id: FunctionId::new(name).unwrap(),
            memory_limit: MemoryLimit::from_mb(128).unwrap(),
            trigger_port: Port::new(8080).unwrap(),
            handler_path: crate::types::HandlerPath::new_unchecked("/bin/echo"),
            environment: std::collections::HashMap::new(),
            timeout_ms: 30000,
        }
    }

    #[test]
    fn test_register_and_get() {
        let registry = FunctionRegistry::new();
        let config = make_config("test-func");

        assert!(registry.register(config).is_ok());
        assert!(registry.contains(&FunctionId::new("test-func").unwrap()));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_duplicate_registration() {
        let registry = FunctionRegistry::new();
        let config1 = make_config("test-func");
        let config2 = make_config("test-func");

        assert!(registry.register(config1).is_ok());
        assert!(registry.register(config2).is_err());
    }

    #[test]
    fn test_state_transitions() {
        let registry = FunctionRegistry::new();
        let config = make_config("test-func");
        let id = config.id.clone();

        registry.register(config).unwrap();

        assert_eq!(
            registry.get_state(&id).unwrap(),
            FunctionState::Uninitialized
        );

        registry
            .transition(&id, FunctionState::WarmSnapshot)
            .unwrap();
        assert_eq!(
            registry.get_state(&id).unwrap(),
            FunctionState::WarmSnapshot
        );

        registry.transition(&id, FunctionState::Running).unwrap();
        assert_eq!(registry.get_state(&id).unwrap(), FunctionState::Running);
    }

    #[test]
    fn test_functions_in_state() {
        let registry = FunctionRegistry::new();

        registry.register(make_config("func1")).unwrap();
        registry.register(make_config("func2")).unwrap();
        registry.register(make_config("func3")).unwrap();

        let id2 = FunctionId::new("func2").unwrap();
        registry
            .transition(&id2, FunctionState::WarmSnapshot)
            .unwrap();

        let uninitialized = registry.functions_in_state(FunctionState::Uninitialized);
        assert_eq!(uninitialized.len(), 2);

        let warm = registry.functions_in_state(FunctionState::WarmSnapshot);
        assert_eq!(warm.len(), 1);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let registry = Arc::new(FunctionRegistry::new());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let reg = Arc::clone(&registry);
                thread::spawn(move || {
                    let config = make_config(&format!("func-{}", i));
                    reg.register(config).unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.len(), 10);
    }
}
