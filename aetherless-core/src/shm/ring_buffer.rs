// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Lock-free circular ring buffer for IPC.
//!
//! Uses atomic head/tail pointers for wait-free single-producer single-consumer
//! communication between the Orchestrator and Function processes.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::SharedMemoryError;
use crate::shm::SharedMemoryRegion;

/// Header size in bytes (head + tail + capacity as u64).
const HEADER_SIZE: usize = 24;

/// Alignment for entries (8 bytes).
const ENTRY_ALIGNMENT: usize = 8;

/// Ring buffer header stored at the start of shared memory.
#[repr(C)]
struct RingBufferHeader {
    /// Write position (owned by producer).
    head: AtomicU64,
    /// Read position (owned by consumer).
    tail: AtomicU64,
    /// Total capacity in bytes (excluding header).
    capacity: AtomicU64,
}

/// Entry header for each message in the buffer.
#[repr(C)]
#[derive(Clone, Copy)]
struct EntryHeader {
    /// Length of the payload in bytes.
    length: u32,
    /// CRC32 checksum of the payload.
    checksum: u32,
}

const ENTRY_HEADER_SIZE: usize = std::mem::size_of::<EntryHeader>();

/// Lock-free ring buffer for zero-copy IPC.
///
/// Single-producer, single-consumer (SPSC) design using atomic operations
/// for the head and tail pointers. No locks required.
pub struct RingBuffer {
    /// Underlying shared memory region.
    region: SharedMemoryRegion,
}

impl RingBuffer {
    /// Create a new ring buffer in the given shared memory region.
    pub fn new(region: SharedMemoryRegion) -> Result<Self, SharedMemoryError> {
        let size = region.size();

        if size < HEADER_SIZE + 64 {
            return Err(SharedMemoryError::InvalidBufferState {
                reason: format!("Region too small: {} bytes", size),
            });
        }

        let buffer = Self { region };

        // Initialize the header
        // SAFETY: We just created the region and have exclusive access
        unsafe {
            let header = buffer.header_mut();
            (*header).head.store(0, Ordering::Release);
            (*header).tail.store(0, Ordering::Release);
            (*header)
                .capacity
                .store((size - HEADER_SIZE) as u64, Ordering::Release);
        }

        Ok(buffer)
    }

    /// Open an existing ring buffer from shared memory.
    pub fn open(region: SharedMemoryRegion) -> Result<Self, SharedMemoryError> {
        let size = region.size();

        if size < HEADER_SIZE + 64 {
            return Err(SharedMemoryError::InvalidBufferState {
                reason: format!("Region too small: {} bytes", size),
            });
        }

        Ok(Self { region })
    }

    /// Get pointer to the header.
    fn header(&self) -> *const RingBufferHeader {
        self.region.as_ptr() as *const RingBufferHeader
    }

    /// Get mutable pointer to the header.
    fn header_mut(&self) -> *mut RingBufferHeader {
        self.region.as_ptr() as *mut RingBufferHeader
    }

    /// Get pointer to the data area (after header).
    fn data_ptr(&self) -> *mut u8 {
        // SAFETY: HEADER_SIZE is within the region bounds
        unsafe { self.region.as_ptr().add(HEADER_SIZE) }
    }

    /// Get the capacity of the data area.
    pub fn capacity(&self) -> usize {
        // SAFETY: header is always valid
        unsafe { (*self.header()).capacity.load(Ordering::Acquire) as usize }
    }

    /// Get current write position.
    fn head(&self) -> u64 {
        // SAFETY: header is always valid
        unsafe { (*self.header()).head.load(Ordering::Acquire) }
    }

    /// Get current read position.
    fn tail(&self) -> u64 {
        // SAFETY: header is always valid
        unsafe { (*self.header()).tail.load(Ordering::Acquire) }
    }

    /// Calculate available space for writing.
    pub fn available_space(&self) -> usize {
        let head = self.head();
        let tail = self.tail();
        let capacity = self.capacity() as u64;

        // Available = capacity - (head - tail)
        // This works correctly even with wraparound
        (capacity - (head - tail)) as usize
    }

    /// Calculate amount of data ready to read.
    pub fn readable_bytes(&self) -> usize {
        let head = self.head();
        let tail = self.tail();
        (head - tail) as usize
    }

    /// Write a payload to the buffer.
    ///
    /// Returns SharedMemoryError::RingBufferFull if there isn't enough space.
    pub fn write(&self, payload: &[u8]) -> Result<(), SharedMemoryError> {
        let payload_len = payload.len();

        // Calculate total entry size (header + payload, aligned)
        let entry_size = Self::align_up(ENTRY_HEADER_SIZE + payload_len, ENTRY_ALIGNMENT);

        if entry_size > self.available_space() {
            return Err(SharedMemoryError::RingBufferFull { size: payload_len });
        }

        // Calculate checksum
        let checksum = crc32fast::hash(payload);

        let entry_header = EntryHeader {
            length: payload_len as u32,
            checksum,
        };

        let capacity = self.capacity();
        let head = self.head();
        let offset = (head as usize) % capacity;

        // SAFETY: We've verified there's enough space
        unsafe {
            let data = self.data_ptr();

            // Write entry header
            let header_dest = data.add(offset) as *mut EntryHeader;
            std::ptr::write_unaligned(header_dest, entry_header);

            // Write payload
            let payload_dest = data.add(offset + ENTRY_HEADER_SIZE);

            // Handle wraparound
            let first_chunk = std::cmp::min(payload_len, capacity - offset - ENTRY_HEADER_SIZE);
            std::ptr::copy_nonoverlapping(payload.as_ptr(), payload_dest, first_chunk);

            if first_chunk < payload_len {
                // Wrap around to beginning
                std::ptr::copy_nonoverlapping(
                    payload.as_ptr().add(first_chunk),
                    data,
                    payload_len - first_chunk,
                );
            }

            // Update head with release ordering
            (*self.header_mut())
                .head
                .store(head + entry_size as u64, Ordering::Release);
        }

        Ok(())
    }

    /// Read a payload from the buffer.
    ///
    /// Returns the payload bytes and validates the checksum.
    /// Returns SharedMemoryError::RingBufferEmpty if no data available.
    pub fn read(&self) -> Result<Vec<u8>, SharedMemoryError> {
        if self.readable_bytes() < ENTRY_HEADER_SIZE {
            return Err(SharedMemoryError::RingBufferEmpty);
        }

        let capacity = self.capacity();
        let tail = self.tail();
        let offset = (tail as usize) % capacity;

        // SAFETY: We've verified there's data to read
        unsafe {
            let data = self.data_ptr();

            // Read entry header
            let header_src = data.add(offset) as *const EntryHeader;
            let entry_header: EntryHeader = std::ptr::read_unaligned(header_src);

            let payload_len = entry_header.length as usize;
            let expected_checksum = entry_header.checksum;

            // Calculate entry size
            let entry_size = Self::align_up(ENTRY_HEADER_SIZE + payload_len, ENTRY_ALIGNMENT);

            // Validate we have enough data
            if self.readable_bytes() < entry_size {
                return Err(SharedMemoryError::InvalidBufferState {
                    reason: "Incomplete entry in buffer".to_string(),
                });
            }

            // Read payload
            let mut payload = vec![0u8; payload_len];
            let payload_src = data.add(offset + ENTRY_HEADER_SIZE);

            // Handle wraparound
            let first_chunk = std::cmp::min(payload_len, capacity - offset - ENTRY_HEADER_SIZE);
            std::ptr::copy_nonoverlapping(payload_src, payload.as_mut_ptr(), first_chunk);

            if first_chunk < payload_len {
                // Wrap around
                std::ptr::copy_nonoverlapping(
                    data,
                    payload.as_mut_ptr().add(first_chunk),
                    payload_len - first_chunk,
                );
            }

            // Validate checksum - FAIL IMMEDIATELY on mismatch (no fallback)
            let actual_checksum = crc32fast::hash(&payload);
            if actual_checksum != expected_checksum {
                return Err(SharedMemoryError::ChecksumMismatch {
                    expected: expected_checksum,
                    actual: actual_checksum,
                });
            }

            // Update tail with release ordering
            (*self.header_mut())
                .tail
                .store(tail + entry_size as u64, Ordering::Release);

            Ok(payload)
        }
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.readable_bytes() == 0
    }

    /// Align value up to the given alignment.
    const fn align_up(value: usize, alignment: usize) -> usize {
        (value + alignment - 1) & !(alignment - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require actual shared memory which may not work in all environments
    // In a real test environment, we'd mock the shared memory region

    #[test]
    fn test_align_up() {
        assert_eq!(RingBuffer::align_up(1, 8), 8);
        assert_eq!(RingBuffer::align_up(8, 8), 8);
        assert_eq!(RingBuffer::align_up(9, 8), 16);
        assert_eq!(RingBuffer::align_up(0, 8), 0);
    }
}
