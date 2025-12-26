// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Function state machine with typed state transitions.
//!
//! Implements the function lifecycle: Uninitialized → WarmSnapshot → Running → Suspended.
//! Invalid transitions result in StateTransitionError.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::StateTransitionError;
use crate::types::FunctionId;

/// Function lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionState {
    /// Initial state - function registered but not yet initialized.
    Uninitialized,

    /// Function has a warm snapshot ready for fast restore.
    WarmSnapshot,

    /// Function is actively running and processing requests.
    Running,

    /// Function is suspended (paused) but can be resumed.
    Suspended,
}

impl FunctionState {
    /// Get the state name for error messages.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Uninitialized => "Uninitialized",
            Self::WarmSnapshot => "WarmSnapshot",
            Self::Running => "Running",
            Self::Suspended => "Suspended",
        }
    }

    /// Check if transition to the target state is valid.
    pub fn can_transition_to(&self, target: FunctionState) -> bool {
        matches!(
            (self, target),
            // From Uninitialized
            (Self::Uninitialized, Self::WarmSnapshot) |
            (Self::Uninitialized, Self::Running) |
            // From WarmSnapshot
            (Self::WarmSnapshot, Self::Running) |
            (Self::WarmSnapshot, Self::Uninitialized) |
            // From Running
            (Self::Running, Self::Suspended) |
            (Self::Running, Self::WarmSnapshot) |
            // From Suspended
            (Self::Suspended, Self::Running) |
            (Self::Suspended, Self::WarmSnapshot) |
            (Self::Suspended, Self::Uninitialized)
        )
    }
}

impl std::fmt::Display for FunctionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// State machine for a function's lifecycle.
/// Enforces valid state transitions and tracks timing metrics.
#[derive(Debug)]
pub struct FunctionStateMachine {
    function_id: FunctionId,
    current_state: FunctionState,
    last_transition: Instant,
    transition_count: u64,
}

impl FunctionStateMachine {
    /// Create a new state machine for a function.
    pub fn new(function_id: FunctionId) -> Self {
        Self {
            function_id,
            current_state: FunctionState::Uninitialized,
            last_transition: Instant::now(),
            transition_count: 0,
        }
    }

    /// Get the current state.
    pub fn state(&self) -> FunctionState {
        self.current_state
    }

    /// Get the function ID.
    pub fn function_id(&self) -> &FunctionId {
        &self.function_id
    }

    /// Get time since last transition.
    pub fn time_in_current_state(&self) -> std::time::Duration {
        self.last_transition.elapsed()
    }

    /// Get total number of transitions.
    pub fn transition_count(&self) -> u64 {
        self.transition_count
    }

    /// Attempt to transition to a new state.
    /// Returns Ok(()) if successful, or StateTransitionError if invalid.
    pub fn transition_to(&mut self, target: FunctionState) -> Result<(), StateTransitionError> {
        if !self.current_state.can_transition_to(target) {
            return Err(StateTransitionError::InvalidTransition {
                function_id: self.function_id.clone(),
                from: self.current_state.name(),
                to: target.name(),
            });
        }

        tracing::debug!(
            function_id = %self.function_id,
            from = self.current_state.name(),
            to = target.name(),
            "State transition"
        );

        self.current_state = target;
        self.last_transition = Instant::now();
        self.transition_count += 1;

        Ok(())
    }

    /// Check if the function can be invoked (is in a runnable state).
    pub fn is_invokable(&self) -> bool {
        matches!(
            self.current_state,
            FunctionState::Running | FunctionState::WarmSnapshot
        )
    }

    /// Check if the function has a warm snapshot available.
    pub fn has_warm_snapshot(&self) -> bool {
        matches!(self.current_state, FunctionState::WarmSnapshot)
    }
}

/// Metrics for the state machine.
#[derive(Debug, Clone, Serialize)]
pub struct StateMachineMetrics {
    pub function_id: String,
    pub current_state: String,
    pub time_in_state_ms: u64,
    pub transition_count: u64,
}

impl From<&FunctionStateMachine> for StateMachineMetrics {
    fn from(sm: &FunctionStateMachine) -> Self {
        Self {
            function_id: sm.function_id.to_string(),
            current_state: sm.current_state.name().to_string(),
            time_in_state_ms: sm.time_in_current_state().as_millis() as u64,
            transition_count: sm.transition_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_function_id() -> FunctionId {
        FunctionId::new("test-function").unwrap()
    }

    #[test]
    fn test_initial_state() {
        let sm = FunctionStateMachine::new(make_function_id());
        assert_eq!(sm.state(), FunctionState::Uninitialized);
        assert_eq!(sm.transition_count(), 0);
    }

    #[test]
    fn test_valid_transitions() {
        let mut sm = FunctionStateMachine::new(make_function_id());

        // Uninitialized → WarmSnapshot
        assert!(sm.transition_to(FunctionState::WarmSnapshot).is_ok());
        assert_eq!(sm.state(), FunctionState::WarmSnapshot);
        assert_eq!(sm.transition_count(), 1);

        // WarmSnapshot → Running
        assert!(sm.transition_to(FunctionState::Running).is_ok());
        assert_eq!(sm.state(), FunctionState::Running);

        // Running → Suspended
        assert!(sm.transition_to(FunctionState::Suspended).is_ok());
        assert_eq!(sm.state(), FunctionState::Suspended);

        // Suspended → Running
        assert!(sm.transition_to(FunctionState::Running).is_ok());
        assert_eq!(sm.state(), FunctionState::Running);
    }

    #[test]
    fn test_invalid_transitions() {
        let mut sm = FunctionStateMachine::new(make_function_id());

        // Uninitialized → Suspended (invalid)
        assert!(sm.transition_to(FunctionState::Suspended).is_err());
        assert_eq!(sm.state(), FunctionState::Uninitialized);
    }

    #[test]
    fn test_is_invokable() {
        let mut sm = FunctionStateMachine::new(make_function_id());
        assert!(!sm.is_invokable());

        sm.transition_to(FunctionState::WarmSnapshot).unwrap();
        assert!(sm.has_warm_snapshot());
        assert!(sm.is_invokable());

        sm.transition_to(FunctionState::Running).unwrap();
        assert!(sm.is_invokable());
    }
}
