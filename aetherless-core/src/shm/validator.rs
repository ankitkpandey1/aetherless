// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Pandey

//! Payload validation using checksums.
//!
//! Validates integrity of buffer data at every read.
//! Fails immediately on checksum mismatch - NO fallback.

use crate::error::SharedMemoryError;

/// Maximum payload size (16 MB).
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

/// Validator for IPC payloads.
///
/// Checks integrity using CRC32 checksums and validates size limits.
pub struct PayloadValidator;

impl PayloadValidator {
    /// Validate a payload before writing.
    pub fn validate_for_write(payload: &[u8]) -> Result<(), SharedMemoryError> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(SharedMemoryError::PayloadTooLarge {
                size: payload.len(),
                max: MAX_PAYLOAD_SIZE,
            });
        }

        if payload.is_empty() {
            return Err(SharedMemoryError::InvalidBufferState {
                reason: "Cannot write empty payload".to_string(),
            });
        }

        Ok(())
    }

    /// Calculate CRC32 checksum for a payload.
    pub fn calculate_checksum(payload: &[u8]) -> u32 {
        crc32fast::hash(payload)
    }

    /// Validate a payload after reading.
    ///
    /// Verifies checksum matches expected value.
    /// FAILS IMMEDIATELY on mismatch - this is a critical error with no fallback.
    pub fn validate_checksum(payload: &[u8], expected: u32) -> Result<(), SharedMemoryError> {
        let actual = Self::calculate_checksum(payload);

        if actual != expected {
            return Err(SharedMemoryError::ChecksumMismatch { expected, actual });
        }

        Ok(())
    }

    /// Validate payload structure (if using a structured format).
    ///
    /// This can be extended to validate specific payload formats.
    pub fn validate_structure(payload: &[u8]) -> Result<PayloadInfo, SharedMemoryError> {
        if payload.len() < 4 {
            return Err(SharedMemoryError::InvalidBufferState {
                reason: "Payload too short for header".to_string(),
            });
        }

        // Read payload type from first 4 bytes
        let payload_type = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);

        Ok(PayloadInfo {
            payload_type,
            data_offset: 4,
            data_length: payload.len() - 4,
        })
    }
}

/// Information about a validated payload.
#[derive(Debug, Clone)]
pub struct PayloadInfo {
    /// Type identifier for the payload.
    pub payload_type: u32,
    /// Offset to the actual data within the payload.
    pub data_offset: usize,
    /// Length of the data portion.
    pub data_length: usize,
}

/// Payload types for IPC messages.
#[allow(dead_code)]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadType {
    /// Function invocation request.
    InvokeRequest = 1,
    /// Function invocation response.
    InvokeResponse = 2,
    /// Health check ping.
    HealthPing = 3,
    /// Health check pong.
    HealthPong = 4,
    /// Shutdown signal.
    Shutdown = 5,
}

impl TryFrom<u32> for PayloadType {
    type Error = SharedMemoryError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::InvokeRequest),
            2 => Ok(Self::InvokeResponse),
            3 => Ok(Self::HealthPing),
            4 => Ok(Self::HealthPong),
            5 => Ok(Self::Shutdown),
            _ => Err(SharedMemoryError::InvalidBufferState {
                reason: format!("Unknown payload type: {}", value),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_validation() {
        let payload = b"Hello, World!";
        let checksum = PayloadValidator::calculate_checksum(payload);

        assert!(PayloadValidator::validate_checksum(payload, checksum).is_ok());
        assert!(PayloadValidator::validate_checksum(payload, checksum + 1).is_err());
    }

    #[test]
    fn test_size_validation() {
        let small_payload = vec![0u8; 100];
        assert!(PayloadValidator::validate_for_write(&small_payload).is_ok());

        let empty_payload: &[u8] = &[];
        assert!(PayloadValidator::validate_for_write(empty_payload).is_err());
    }

    #[test]
    fn test_payload_type_conversion() {
        assert_eq!(
            PayloadType::try_from(1).unwrap(),
            PayloadType::InvokeRequest
        );
        assert_eq!(
            PayloadType::try_from(2).unwrap(),
            PayloadType::InvokeResponse
        );
        assert!(PayloadType::try_from(99).is_err());
    }
}
