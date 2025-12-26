// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Shared Memory IPC module.
//!
//! Zero-copy inter-process communication using POSIX shared memory.
//! Provides lock-free ring buffer for high-performance event passing.

mod region;
mod ring_buffer;
mod validator;

pub use region::SharedMemoryRegion;
pub use ring_buffer::RingBuffer;
pub use validator::PayloadValidator;
