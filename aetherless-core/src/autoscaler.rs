// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Autoscaler component for dynamic function scaling.
//!
//! Implements Horizontal Pod Autoscaler (HPA) logic:
//! desired_replicas = ceil[current_replicas * ( current_metric / desired_metric )]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingPolicy {
    pub min_replicas: usize,
    pub max_replicas: usize,
    pub target_concurrency: f64, // Request per second or active requests per replica
    pub scale_up_stabilization_window: u64, // seconds
    pub scale_down_stabilization_window: u64, // seconds
}

impl Default for ScalingPolicy {
    fn default() -> Self {
        Self {
            min_replicas: 1,
            max_replicas: 10,
            target_concurrency: 50.0,
            scale_up_stabilization_window: 0,
            scale_down_stabilization_window: 30,
        }
    }
}

pub struct Autoscaler {
    policy: ScalingPolicy,
}

impl Autoscaler {
    pub fn new(policy: ScalingPolicy) -> Self {
        Self { policy }
    }

    /// Calculate desired replicas based on total current load (e.g. RPS).
    pub fn calculate_replicas(&self, _current_replicas: usize, total_load: f64) -> usize {
        if total_load <= 0.0 {
            return self.policy.min_replicas; // Idle state
        }

        let desired = (total_load / self.policy.target_concurrency).ceil() as usize;

        // Clamp to min/max
        desired.clamp(self.policy.min_replicas, self.policy.max_replicas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_up() {
        let policy = ScalingPolicy {
            target_concurrency: 10.0,
            ..Default::default()
        };
        let scaler = Autoscaler::new(policy);

        // 1 replica, 20 reqs -> should scale to 2
        assert_eq!(scaler.calculate_replicas(1, 20.0), 2);
        
        // 1 replica, 15 reqs -> should scale to 2 (1.5 ceil)
        assert_eq!(scaler.calculate_replicas(1, 15.0), 2);
    }

    #[test]
    fn test_scale_down() {
        let policy = ScalingPolicy {
            target_concurrency: 10.0,
            ..Default::default()
        };
        let scaler = Autoscaler::new(policy);

        // 5 total reqs, target 10 -> need 1 replica
        assert_eq!(scaler.calculate_replicas(2, 5.0), 1);
        
        // 0 total reqs -> min replicas (1)
        assert_eq!(scaler.calculate_replicas(5, 0.0), 1);
        
        // 100 reqs, max 10, target 10 -> 10 replicas
        assert_eq!(scaler.calculate_replicas(5, 100.0), 10);
        
        // 200 reqs, max 10 -> capped at 10
        assert_eq!(scaler.calculate_replicas(5, 200.0), 10);
    }
}
