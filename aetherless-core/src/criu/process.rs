//! Function process management.
//!
//! Spawns function processes and waits for READY signal on Unix socket.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::CriuError;
use crate::types::{FunctionId, HandlerPath};

/// Timeout for waiting for READY signal.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Ready signal message.
const READY_SIGNAL: &[u8] = b"READY";

/// Function process wrapper.
///
/// Manages the lifecycle of a function process including spawning
/// and waiting for the READY signal.
pub struct FunctionProcess {
    /// Function ID.
    function_id: FunctionId,
    /// Child process handle.
    child: Child,
    /// Path to the control socket.
    socket_path: PathBuf,
    /// Process ID.
    pid: u32,
    /// Unix stream for communication.
    stream: Option<UnixStream>,
}

impl FunctionProcess {
    /// Spawn a new function process.
    ///
    /// Creates a Unix socket for control communication, spawns the handler
    /// process, and waits for the READY signal.
    ///
    /// # Arguments
    /// * `function_id` - ID of the function
    /// * `handler_path` - Path to the handler executable
    /// * `socket_dir` - Directory for the control socket
    ///
    /// # Errors
    /// Returns CriuError if spawn fails or READY timeout is reached.
    pub fn spawn(
        function_id: &FunctionId,
        handler_path: &HandlerPath,
        socket_dir: &Path,
    ) -> Result<Self, CriuError> {
        // Create socket path
        let socket_path = socket_dir.join(format!("{}.sock", function_id));

        // Remove old socket if exists
        let _ = std::fs::remove_file(&socket_path);

        // Create Unix listener
        let listener = UnixListener::bind(&socket_path).map_err(|e| CriuError::UnixSocket {
            reason: format!("Failed to bind socket {}: {}", socket_path.display(), e),
        })?;

        // Set socket to non-blocking for timeout handling
        listener
            .set_nonblocking(true)
            .map_err(|e| CriuError::UnixSocket {
                reason: format!("Failed to set non-blocking: {}", e),
            })?;

        // Spawn the handler process
        let child = Command::new(handler_path.as_path())
            .env("AETHER_SOCKET", &socket_path)
            .env("AETHER_FUNCTION_ID", function_id.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CriuError::SpawnFailed {
                reason: format!("Failed to spawn {}: {}", handler_path, e),
            })?;

        let pid = child.id();

        tracing::debug!(
            function_id = %function_id,
            pid = pid,
            handler = %handler_path,
            "Spawned function process"
        );

        // Wait for READY signal with timeout
        let start = Instant::now();
        let mut stream = None;

        while start.elapsed() < READY_TIMEOUT {
            match listener.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).ok();
                    s.set_read_timeout(Some(Duration::from_secs(5))).ok();

                    let mut buf = [0u8; 16];
                    match s.read(&mut buf) {
                        Ok(n) if n >= READY_SIGNAL.len() => {
                            if &buf[..READY_SIGNAL.len()] == READY_SIGNAL {
                                tracing::info!(
                                    function_id = %function_id,
                                    pid = pid,
                                    elapsed_ms = start.elapsed().as_millis(),
                                    "Function sent READY signal"
                                );
                                stream = Some(s);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(CriuError::UnixSocket {
                        reason: format!("Accept error: {}", e),
                    });
                }
            }
        }

        if stream.is_none() {
            // Kill the process since it didn't respond
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();

            return Err(CriuError::ReadyTimeout);
        }

        Ok(Self {
            function_id: function_id.clone(),
            child,
            socket_path,
            pid,
            stream,
        })
    }

    /// Get the process ID.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Get the function ID.
    pub fn function_id(&self) -> &FunctionId {
        &self.function_id
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a message to the process.
    pub fn send(&mut self, message: &[u8]) -> Result<(), CriuError> {
        if let Some(ref mut stream) = self.stream {
            stream
                .write_all(message)
                .map_err(|e| CriuError::UnixSocket {
                    reason: format!("Send failed: {}", e),
                })?;
            stream.flush().map_err(|e| CriuError::UnixSocket {
                reason: format!("Flush failed: {}", e),
            })?;
            Ok(())
        } else {
            Err(CriuError::UnixSocket {
                reason: "No connection to process".to_string(),
            })
        }
    }

    /// Check if the process is still running.
    pub fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// Kill the process.
    pub fn kill(&mut self) -> Result<(), CriuError> {
        self.child.kill().map_err(|e| CriuError::SpawnFailed {
            reason: format!("Failed to kill process: {}", e),
        })?;
        self.child.wait().ok();
        Ok(())
    }
}

impl Drop for FunctionProcess {
    fn drop(&mut self) {
        // Clean up socket
        let _ = std::fs::remove_file(&self.socket_path);

        // Try to kill the process if still running
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ready_signal_constant() {
        assert_eq!(READY_SIGNAL, b"READY");
    }
}
