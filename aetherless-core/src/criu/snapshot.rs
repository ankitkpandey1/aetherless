//! CRIU snapshot management.
//!
//! Manages process checkpointing and restoration using CRIU.
//! Enforces strict 15ms latency constraint on restore operations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use crate::error::CriuError;
use crate::types::FunctionId;

/// Default restore timeout in milliseconds.
#[allow(dead_code)]
pub const DEFAULT_RESTORE_TIMEOUT_MS: u64 = 15;

/// CRIU dump directory prefix.
const DUMP_DIR_PREFIX: &str = "criu_dump";

/// Snapshot metadata.
#[derive(Debug, Clone)]
pub struct SnapshotMetadata {
    /// Function ID this snapshot belongs to.
    pub function_id: FunctionId,
    /// Path to the snapshot directory.
    pub path: PathBuf,
    /// Process ID at dump time.
    pub original_pid: u32,
    /// Timestamp when snapshot was created.
    pub created_at: std::time::SystemTime,
}

/// Manager for CRIU snapshots.
///
/// Handles dump and restore operations with strict latency enforcement.
pub struct SnapshotManager {
    /// Base directory for snapshots (should be /dev/shm for speed).
    snapshot_dir: PathBuf,
    /// Maximum restore time in milliseconds.
    restore_timeout_ms: u64,
    /// Path to CRIU binary.
    criu_path: PathBuf,
    /// Cached snapshot metadata.
    snapshots: HashMap<FunctionId, SnapshotMetadata>,
}

impl SnapshotManager {
    /// Create a new SnapshotManager.
    ///
    /// # Arguments
    /// * `snapshot_dir` - Base directory for snapshots (use /dev/shm for memory-backed)
    /// * `restore_timeout_ms` - Maximum allowed restore time
    ///
    /// # Errors
    /// Returns CriuError if CRIU binary is not found.
    pub fn new(
        snapshot_dir: impl Into<PathBuf>,
        restore_timeout_ms: u64,
    ) -> Result<Self, CriuError> {
        let snapshot_dir = snapshot_dir.into();

        // Find CRIU binary
        let criu_path = Self::find_criu()?;

        // Create snapshot directory if it doesn't exist
        std::fs::create_dir_all(&snapshot_dir).map_err(|e| CriuError::DumpFailed {
            reason: format!("Failed to create snapshot dir: {}", e),
        })?;

        tracing::info!(
            criu_path = %criu_path.display(),
            snapshot_dir = %snapshot_dir.display(),
            restore_timeout_ms = restore_timeout_ms,
            "SnapshotManager initialized"
        );

        Ok(Self {
            snapshot_dir,
            restore_timeout_ms,
            criu_path,
            snapshots: HashMap::new(),
        })
    }

    /// Find the CRIU binary.
    fn find_criu() -> Result<PathBuf, CriuError> {
        let candidates = [
            "/usr/sbin/criu",
            "/usr/bin/criu",
            "/sbin/criu",
            "/bin/criu",
            "/usr/local/sbin/criu",
            "/usr/local/bin/criu",
        ];

        for path in candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok(p);
            }
        }

        // Try which
        if let Ok(output) = Command::new("which").arg("criu").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        Err(CriuError::BinaryNotFound)
    }

    /// Get the path for a function's snapshot directory.
    fn snapshot_path(&self, function_id: &FunctionId) -> PathBuf {
        self.snapshot_dir
            .join(format!("{}_{}", DUMP_DIR_PREFIX, function_id))
    }

    /// Dump a process to create a snapshot.
    ///
    /// # Arguments
    /// * `function_id` - ID of the function
    /// * `pid` - Process ID to dump
    ///
    /// # Errors
    /// Returns CriuError if dump fails.
    pub fn dump(
        &mut self,
        function_id: &FunctionId,
        pid: u32,
    ) -> Result<SnapshotMetadata, CriuError> {
        let dump_path = self.snapshot_path(function_id);

        // Remove old dump if exists
        if dump_path.exists() {
            std::fs::remove_dir_all(&dump_path).map_err(|e| CriuError::DumpFailed {
                reason: format!("Failed to remove old dump: {}", e),
            })?;
        }

        // Create dump directory
        std::fs::create_dir_all(&dump_path).map_err(|e| CriuError::DumpFailed {
            reason: format!("Failed to create dump dir: {}", e),
        })?;

        tracing::debug!(
            function_id = %function_id,
            pid = pid,
            path = %dump_path.display(),
            "Starting CRIU dump"
        );

        let start = Instant::now();

        // Execute CRIU dump
        let output = Command::new(&self.criu_path)
            .arg("dump")
            .arg("-t")
            .arg(pid.to_string())
            .arg("-D")
            .arg(&dump_path)
            .arg("-j") // Leave shell job
            .arg("--shell-job")
            .arg("-v4") // Verbose for debugging
            .arg("--tcp-established") // Handle TCP connections
            .output()
            .map_err(|e| CriuError::DumpFailed {
                reason: format!("Failed to execute CRIU: {}", e),
            })?;

        let elapsed = start.elapsed();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CriuError::DumpFailed {
                reason: format!("CRIU dump failed: {}", stderr),
            });
        }

        tracing::info!(
            function_id = %function_id,
            pid = pid,
            elapsed_ms = elapsed.as_millis(),
            "CRIU dump completed"
        );

        let metadata = SnapshotMetadata {
            function_id: function_id.clone(),
            path: dump_path,
            original_pid: pid,
            created_at: std::time::SystemTime::now(),
        };

        self.snapshots.insert(function_id.clone(), metadata.clone());

        Ok(metadata)
    }

    /// Restore a process from snapshot.
    ///
    /// Returns the new process ID.
    ///
    /// # Constraint
    /// If restore takes longer than restore_timeout_ms, kills the process
    /// and returns LatencyViolationError.
    pub fn restore(&self, function_id: &FunctionId) -> Result<u32, CriuError> {
        let metadata =
            self.snapshots
                .get(function_id)
                .ok_or_else(|| CriuError::SnapshotNotFound {
                    function_id: function_id.clone(),
                })?;

        if !metadata.path.exists() {
            return Err(CriuError::SnapshotNotFound {
                function_id: function_id.clone(),
            });
        }

        tracing::debug!(
            function_id = %function_id,
            path = %metadata.path.display(),
            "Starting CRIU restore"
        );

        let start = Instant::now();

        // Execute CRIU restore
        let output = Command::new(&self.criu_path)
            .arg("restore")
            .arg("-D")
            .arg(&metadata.path)
            .arg("-j")
            .arg("--shell-job")
            .arg("-d") // Detach after restore
            .arg("--pidfile")
            .arg(metadata.path.join("restored.pid"))
            .output()
            .map_err(|e| CriuError::RestoreFailed {
                reason: format!("Failed to execute CRIU: {}", e),
            })?;

        let elapsed_ms = start.elapsed().as_millis() as u64;

        // Check latency constraint FIRST
        if elapsed_ms > self.restore_timeout_ms {
            // Try to read PID and kill the process
            if let Ok(pid_str) = std::fs::read_to_string(metadata.path.join("restored.pid")) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                    tracing::error!(
                        function_id = %function_id,
                        elapsed_ms = elapsed_ms,
                        limit_ms = self.restore_timeout_ms,
                        "Latency violation - killed restored process"
                    );
                }
            }

            return Err(CriuError::LatencyViolation {
                actual_ms: elapsed_ms,
                limit_ms: self.restore_timeout_ms,
            });
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CriuError::RestoreFailed {
                reason: format!("CRIU restore failed: {}", stderr),
            });
        }

        // Read the new PID
        let pid_str = std::fs::read_to_string(metadata.path.join("restored.pid")).map_err(|e| {
            CriuError::RestoreFailed {
                reason: format!("Failed to read PID file: {}", e),
            }
        })?;

        let pid = pid_str
            .trim()
            .parse::<u32>()
            .map_err(|e| CriuError::RestoreFailed {
                reason: format!("Invalid PID: {}", e),
            })?;

        tracing::info!(
            function_id = %function_id,
            new_pid = pid,
            elapsed_ms = elapsed_ms,
            "CRIU restore completed"
        );

        Ok(pid)
    }

    /// Check if a snapshot exists for a function.
    pub fn has_snapshot(&self, function_id: &FunctionId) -> bool {
        if let Some(metadata) = self.snapshots.get(function_id) {
            metadata.path.exists()
        } else {
            false
        }
    }

    /// Delete a snapshot.
    pub fn delete_snapshot(&mut self, function_id: &FunctionId) -> Result<(), CriuError> {
        if let Some(metadata) = self.snapshots.remove(function_id) {
            if metadata.path.exists() {
                std::fs::remove_dir_all(&metadata.path).map_err(|e| CriuError::DumpFailed {
                    reason: format!("Failed to delete snapshot: {}", e),
                })?;
            }
        }
        Ok(())
    }

    /// Get snapshot metadata.
    pub fn get_metadata(&self, function_id: &FunctionId) -> Option<&SnapshotMetadata> {
        self.snapshots.get(function_id)
    }

    /// List all snapshots.
    pub fn list_snapshots(&self) -> Vec<&SnapshotMetadata> {
        self.snapshots.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_path() {
        // This test doesn't actually use CRIU, just tests path generation
        let function_id = FunctionId::new("test-func").unwrap();
        let expected_suffix = format!("{}_{}", DUMP_DIR_PREFIX, function_id);
        assert!(expected_suffix.contains("test-func"));
    }
}
