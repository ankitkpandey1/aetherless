// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Pandey

//! CRIU lifecycle management module.
//!
//! Provides process snapshot/restore using CRIU for fast cold start.
//! Enforces strict 15ms latency constraint on restore operations.

mod process;
mod snapshot;

pub use process::FunctionProcess;
pub use snapshot::SnapshotManager;
