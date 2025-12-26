//! SharedMemoryRegion - POSIX shared memory wrapper.
//!
//! Provides safe abstraction over mmap and shm_open for zero-copy IPC.
//! All unsafe operations are encapsulated with bounds checking.

use std::ffi::CString;
use std::ptr::NonNull;

use crate::error::SharedMemoryError;

/// Represents a mapped shared memory region.
///
/// This struct owns the mapped memory and will unmap it on drop.
/// The memory can be shared between processes using the same name.
pub struct SharedMemoryRegion {
    /// Name of the shared memory object.
    name: String,
    /// Pointer to the mapped memory.
    ptr: NonNull<u8>,
    /// Size of the mapped region in bytes.
    size: usize,
    /// File descriptor for the shared memory object.
    fd: i32,
    /// Whether this instance created the SHM (and should unlink on drop).
    is_owner: bool,
}

// SAFETY: SharedMemoryRegion can be sent between threads as it owns its memory.
unsafe impl Send for SharedMemoryRegion {}

// SAFETY: SharedMemoryRegion can be shared between threads with proper synchronization.
// The RingBuffer provides atomic access guarantees.
unsafe impl Sync for SharedMemoryRegion {}

impl SharedMemoryRegion {
    /// Minimum size for a shared memory region.
    pub const MIN_SIZE: usize = 4096;

    /// Maximum size for a shared memory region (1 GB).
    pub const MAX_SIZE: usize = 1024 * 1024 * 1024;

    /// Create a new shared memory region.
    ///
    /// # Arguments
    /// * `name` - Name of the shared memory object (will be prefixed with /)
    /// * `size` - Size in bytes (must be between MIN_SIZE and MAX_SIZE)
    ///
    /// # Errors
    /// Returns SharedMemoryError if creation or mapping fails.
    pub fn create(name: &str, size: usize) -> Result<Self, SharedMemoryError> {
        // Validate size bounds
        if size < Self::MIN_SIZE {
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("Size {} is below minimum {}", size, Self::MIN_SIZE),
            });
        }
        if size > Self::MAX_SIZE {
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("Size {} exceeds maximum {}", size, Self::MAX_SIZE),
            });
        }

        // Validate name
        if name.is_empty() {
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: "Name cannot be empty".to_string(),
            });
        }

        let shm_name = format!("/{}", name);
        let c_name =
            CString::new(shm_name.as_str()).map_err(|e| SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("Invalid name: {}", e),
            })?;

        // Create shared memory object
        // SAFETY: c_name is a valid CString, flags are valid POSIX flags
        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                0o600,
            )
        };

        if fd < 0 {
            // Try to open existing if EEXIST
            let errno = std::io::Error::last_os_error();
            if errno.raw_os_error() == Some(libc::EEXIST) {
                return Err(SharedMemoryError::CreateFailed {
                    name: name.to_string(),
                    reason: "Shared memory already exists".to_string(),
                });
            }
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("shm_open failed: {}", errno),
            });
        }

        // Set size
        // SAFETY: fd is a valid file descriptor
        let result = unsafe { libc::ftruncate(fd, size as libc::off_t) };
        if result < 0 {
            let errno = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            unsafe { libc::shm_unlink(c_name.as_ptr()) };
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("ftruncate failed: {}", errno),
            });
        }

        // Map the memory
        // SAFETY: fd is valid, size is validated, offset 0 is valid
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            let errno = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            unsafe { libc::shm_unlink(c_name.as_ptr()) };
            return Err(SharedMemoryError::MapFailed {
                reason: format!("mmap failed: {}", errno),
            });
        }

        // Zero-initialize the memory
        // SAFETY: ptr is valid, size is the correct length
        unsafe {
            std::ptr::write_bytes(ptr as *mut u8, 0, size);
        }

        let ptr = NonNull::new(ptr as *mut u8).expect("mmap returned null but not MAP_FAILED");

        tracing::debug!(name = %name, size = size, "Created shared memory region");

        Ok(Self {
            name: name.to_string(),
            ptr,
            size,
            fd,
            is_owner: true,
        })
    }

    /// Open an existing shared memory region.
    pub fn open(name: &str, size: usize) -> Result<Self, SharedMemoryError> {
        if !(Self::MIN_SIZE..=Self::MAX_SIZE).contains(&size) {
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("Invalid size: {}", size),
            });
        }

        let shm_name = format!("/{}", name);
        let c_name =
            CString::new(shm_name.as_str()).map_err(|e| SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("Invalid name: {}", e),
            })?;

        // Open existing shared memory
        // SAFETY: c_name is a valid CString
        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0) };

        if fd < 0 {
            return Err(SharedMemoryError::CreateFailed {
                name: name.to_string(),
                reason: format!("shm_open failed: {}", std::io::Error::last_os_error()),
            });
        }

        // Map the memory
        // SAFETY: fd is valid, size should match
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            let errno = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(SharedMemoryError::MapFailed {
                reason: format!("mmap failed: {}", errno),
            });
        }

        let ptr = NonNull::new(ptr as *mut u8).expect("mmap returned null but not MAP_FAILED");

        tracing::debug!(name = %name, size = size, "Opened shared memory region");

        Ok(Self {
            name: name.to_string(),
            ptr,
            size,
            fd,
            is_owner: false,
        })
    }

    /// Get the name of this shared memory region.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the size of this shared memory region.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get a raw pointer to the shared memory.
    ///
    /// # Safety
    /// Caller must ensure proper synchronization when accessing the memory.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Get a slice view of the shared memory.
    ///
    /// # Safety
    /// Caller must ensure no concurrent writes to the accessed region.
    pub unsafe fn as_slice(&self) -> &[u8] {
        std::slice::from_raw_parts(self.ptr.as_ptr(), self.size)
    }

    /// Get a mutable slice view of the shared memory.
    ///
    /// # Safety
    /// Caller must ensure exclusive access to the accessed region.
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size)
    }
}

impl Drop for SharedMemoryRegion {
    fn drop(&mut self) {
        // Unmap the memory
        // SAFETY: ptr and size were set during creation
        let result = unsafe { libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.size) };
        if result < 0 {
            tracing::error!(
                name = %self.name,
                error = %std::io::Error::last_os_error(),
                "Failed to unmap shared memory"
            );
        }

        // Close the file descriptor
        // SAFETY: fd was opened during creation
        unsafe { libc::close(self.fd) };

        // If we're the owner, unlink the shared memory
        if self.is_owner {
            let shm_name = format!("/{}", self.name);
            if let Ok(c_name) = CString::new(shm_name.as_str()) {
                // SAFETY: c_name is a valid CString
                unsafe { libc::shm_unlink(c_name.as_ptr()) };
                tracing::debug!(name = %self.name, "Unlinked shared memory region");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shm_size_validation() {
        // Too small
        assert!(SharedMemoryRegion::create("test-small", 100).is_err());

        // Too large
        assert!(
            SharedMemoryRegion::create("test-large", SharedMemoryRegion::MAX_SIZE + 1).is_err()
        );
    }

    #[test]
    fn test_shm_empty_name() {
        assert!(SharedMemoryRegion::create("", 4096).is_err());
    }
}
