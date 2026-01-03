// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Warm Pool Manager for CRIU-based function lifecycle.
//!
//! Manages a pool of pre-warmed function snapshots that can be restored
//! in under 15ms. This is the key component for eliminating cold starts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use aetherless_core::criu::SnapshotManager;
use aetherless_core::error::CriuError;
use aetherless_core::types::FunctionId;
use aetherless_core::{FunctionConfig, FunctionState};

/// Warm pool entry tracking a single function's snapshot state.
#[derive(Debug, Clone)]
pub struct WarmPoolEntry {
    pub function_id: FunctionId,
    pub config: FunctionConfig,
    pub snapshot_pid: Option<u32>,
    pub has_snapshot: bool,
    pub restore_count: u64,
    pub last_restore_ms: Option<u64>,
}

/// Statistics from the warm pool.
#[derive(Debug, Clone, Default)]
pub struct WarmPoolStats {
    pub total_functions: usize,
    pub warm_count: usize,
    pub cold_count: usize,
    pub total_restores: u64,
    pub avg_restore_ms: Option<f64>,
}

/// Warm Pool Manager - manages CRIU snapshots for all functions.
///
/// Coordinates with the orchestrator to:
/// 1. Create snapshots after handlers send READY
/// 2. Restore from snapshots when a cold function is invoked
/// 3. Track restore latency and enforce 15ms limit
pub struct WarmPoolManager {
    /// CRIU snapshot manager (None if disabled)
    snapshot_manager: Option<SnapshotManager>,
    /// Pool entries keyed by function ID
    entries: Arc<RwLock<HashMap<String, WarmPoolEntry>>>,
    /// Target pool size per function
    pool_size: usize,
}

impl WarmPoolManager {
    /// Create a new warm pool manager.
    ///
    /// # Arguments
    /// * `snapshot_dir` - Directory for snapshots (use /dev/shm for speed)
    /// * `restore_timeout_ms` - Maximum restore time (default: 15ms)
    /// * `pool_size` - Number of warm instances to keep per function
    pub fn new(
        snapshot_dir: impl Into<PathBuf>,
        restore_timeout_ms: u64,
        pool_size: usize,
    ) -> Result<Self, CriuError> {
        let snapshot_manager = SnapshotManager::new(snapshot_dir, restore_timeout_ms)?;

        Ok(Self {
            snapshot_manager: Some(snapshot_manager),
            entries: Arc::new(RwLock::new(HashMap::new())),
            pool_size,
        })
    }

    /// Create a disabled warm pool manager (for when CRIU is unavailable).
    pub fn disabled() -> Self {
        Self {
            snapshot_manager: None,
            entries: Arc::new(RwLock::new(HashMap::new())),
            pool_size: 0,
        }
    }

    /// Check if warm pool is enabled.
    pub fn is_enabled(&self) -> bool {
        self.snapshot_manager.is_some()
    }

    /// Register a function for warm pool management.
    pub async fn register(&self, config: FunctionConfig) {
        if !self.is_enabled() {
            return;
        }

        let entry = WarmPoolEntry {
            function_id: config.id.clone(),
            config,
            snapshot_pid: None,
            has_snapshot: false,
            restore_count: 0,
            last_restore_ms: None,
        };

        let mut entries = self.entries.write().await;
        entries.insert(entry.function_id.to_string(), entry);
    }

    /// Create a snapshot of a running function process.
    ///
    /// Call this after the handler has sent READY but before serving traffic.
    pub async fn create_snapshot(
        &mut self,
        function_id: &FunctionId,
        pid: u32,
    ) -> Result<(), CriuError> {
        let snapshot_manager = match &mut self.snapshot_manager {
            Some(sm) => sm,
            None => return Ok(()),
        };

        tracing::info!(
            function_id = %function_id,
            pid = pid,
            "Creating warm pool snapshot"
        );

        // Create the CRIU snapshot
        let _metadata = snapshot_manager.dump(function_id, pid)?;

        // Update entry
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(function_id.as_str()) {
            entry.snapshot_pid = Some(pid);
            entry.has_snapshot = true;
        }

        // Update metrics
        crate::metrics::WARM_POOL_SIZE
            .with_label_values(&[function_id.as_str()])
            .set(1);

        tracing::info!(
            function_id = %function_id,
            "Snapshot created successfully"
        );

        Ok(())
    }

    /// Restore a function from its warm snapshot.
    ///
    /// Returns the new process ID or an error if restore fails or exceeds latency limit.
    pub async fn restore(&mut self, function_id: &FunctionId) -> Result<u32, CriuError> {
        let snapshot_manager = match &self.snapshot_manager {
            Some(sm) => sm,
            None => {
                return Err(CriuError::DumpFailed {
                    reason: "Warm pool not enabled".to_string(),
                });
            }
        };

        let start = std::time::Instant::now();

        // Check if we have a snapshot
        {
            let entries = self.entries.read().await;
            if let Some(entry) = entries.get(function_id.as_str()) {
                if !entry.has_snapshot {
                    return Err(CriuError::SnapshotNotFound {
                        function_id: function_id.clone(),
                    });
                }
            } else {
                return Err(CriuError::SnapshotNotFound {
                    function_id: function_id.clone(),
                });
            }
        }

        // Perform the restore
        let new_pid = snapshot_manager.restore(function_id)?;

        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;

        // Update stats
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(function_id.as_str()) {
                entry.restore_count += 1;
                entry.last_restore_ms = Some(elapsed_ms);
            }
        }

        // Update metrics
        crate::metrics::FUNCTION_RESTORES
            .with_label_values(&[function_id.as_str()])
            .inc();
        crate::metrics::RESTORE_DURATION
            .with_label_values(&[function_id.as_str()])
            .observe(elapsed.as_secs_f64());

        tracing::info!(
            function_id = %function_id,
            new_pid = new_pid,
            elapsed_ms = elapsed_ms,
            "Restored from warm snapshot"
        );

        Ok(new_pid)
    }

    /// Check if a function has a warm snapshot available.
    pub async fn has_snapshot(&self, function_id: &FunctionId) -> bool {
        if !self.is_enabled() {
            return false;
        }

        let entries = self.entries.read().await;
        entries
            .get(function_id.as_str())
            .map(|e| e.has_snapshot)
            .unwrap_or(false)
    }

    /// Get the current state for a function (warm or cold).
    pub async fn get_state(&self, function_id: &FunctionId) -> FunctionState {
        if !self.is_enabled() {
            return FunctionState::Uninitialized;
        }

        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(function_id.as_str()) {
            if entry.has_snapshot {
                FunctionState::WarmSnapshot
            } else {
                FunctionState::Uninitialized
            }
        } else {
            FunctionState::Uninitialized
        }
    }

    /// Delete a function's snapshot.
    pub async fn delete_snapshot(&mut self, function_id: &FunctionId) -> Result<(), CriuError> {
        let snapshot_manager = match &mut self.snapshot_manager {
            Some(sm) => sm,
            None => return Ok(()),
        };

        snapshot_manager.delete_snapshot(function_id)?;

        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(function_id.as_str()) {
            entry.has_snapshot = false;
            entry.snapshot_pid = None;
        }

        // Update metrics
        crate::metrics::WARM_POOL_SIZE
            .with_label_values(&[function_id.as_str()])
            .set(0);

        Ok(())
    }

    /// Get warm pool statistics.
    pub async fn stats(&self) -> WarmPoolStats {
        if self.snapshot_manager.is_none() {
            return WarmPoolStats::default();
        }

        let entries = self.entries.read().await;

        let total_functions = entries.len();
        let warm_count = entries.values().filter(|e| e.has_snapshot).count();
        let cold_count = total_functions - warm_count;
        let total_restores: u64 = entries.values().map(|e| e.restore_count).sum();

        let restore_times: Vec<u64> = entries.values().filter_map(|e| e.last_restore_ms).collect();

        let avg_restore_ms = if restore_times.is_empty() {
            None
        } else {
            Some(restore_times.iter().sum::<u64>() as f64 / restore_times.len() as f64)
        };

        WarmPoolStats {
            total_functions,
            warm_count,
            cold_count,
            total_restores,
            avg_restore_ms,
        }
    }

    /// Get all entries (for TUI display).
    pub async fn list_entries(&self) -> Vec<WarmPoolEntry> {
        let entries = self.entries.read().await;
        entries.values().cloned().collect()
    }

    /// Get target pool size.
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_warm_pool_disabled() {
        let pool = WarmPoolManager::disabled();
        assert!(!pool.is_enabled());
    }

    #[tokio::test]
    async fn test_warm_pool_stats_default() {
        let pool = WarmPoolManager::disabled();
        let stats = pool.stats().await;
        assert_eq!(stats.total_functions, 0);
        assert_eq!(stats.warm_count, 0);
    }
}
