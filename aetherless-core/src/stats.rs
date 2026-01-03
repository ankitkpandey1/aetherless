// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::FunctionId;
use crate::FunctionState;

/// Shared statistics structure used for IPC between orchestrator and TUI.
/// Written to /dev/shm/aetherless-stats.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AetherlessStats {
    pub functions: HashMap<FunctionId, FunctionStatus>,
    pub shm_latency_us: u64,
    pub active_instances: usize,
    pub warm_pool_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionStatus {
    pub id: FunctionId,
    pub state: FunctionState,
    pub pid: Option<u32>,
    pub port: u16,
    pub memory_mb: u64,
    pub restore_count: u64,
    pub last_restore_ms: Option<u64>,
}
