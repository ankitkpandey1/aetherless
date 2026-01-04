// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Aetherless eBPF Data Plane Library
//!
//! XDP-based network redirection for serverless function routing.
//! Uses Aya to load and manage eBPF programs for kernel-bypass networking.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

use aya::maps::HashMap as BpfHashMap;
use aya::programs::{Xdp, XdpFlags};
use aya::Ebpf;
use tokio::sync::RwLock;

use aetherless_core::{AetherError, EbpfError, Port, ProcessId};

/// Key for the port-to-PID BPF map.
/// Layout must match the eBPF program's key structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PortKey {
    pub port: u16,
    pub _padding: u16,
}

unsafe impl aya::Pod for PortKey {}

/// Value for the port-to-PID BPF map.
/// Contains the target process ID and socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PortValue {
    pub pid: u32,
    pub addr: u32, // IPv4 address in network byte order
}

unsafe impl aya::Pod for PortValue {}

/// eBPF program manager for XDP-based packet redirection.
///
/// Manages the lifecycle of XDP programs and BPF maps for
/// routing incoming packets to the correct function handlers.
#[derive(Debug)]
pub struct XdpManager {
    /// The loaded BPF object (None if not loaded).
    bpf: Option<Ebpf>,
    /// Port to PID mapping (userspace mirror).
    port_map: Arc<RwLock<HashMap<u16, PortValue>>>,
    /// Interface the XDP program is attached to.
    interface: String,
    /// Whether the XDP program is loaded and attached.
    loaded: bool,
}

impl XdpManager {
    /// Create a new XDP manager for the specified interface.
    ///
    /// # Arguments
    /// * `interface` - Network interface name (e.g., "eth0", "lo")
    pub fn new(interface: impl Into<String>) -> Self {
        Self {
            bpf: None,
            port_map: Arc::new(RwLock::new(HashMap::new())),
            interface: interface.into(),
            loaded: false,
        }
    }

    /// Get the interface name.
    pub fn interface(&self) -> &str {
        &self.interface
    }

    /// Check if the XDP program is loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Load an XDP program from a BPF object file.
    ///
    /// # Arguments
    /// * `program_path` - Path to the compiled BPF object file (.o)
    /// * `program_name` - Name of the XDP program section in the object
    ///
    /// # Errors
    /// Returns EbpfError if loading or attaching fails.
    ///
    /// # Privileges
    /// Requires CAP_BPF and CAP_NET_ADMIN capabilities (typically root).
    pub fn load_program<P: AsRef<Path>>(
        &mut self,
        program_path: P,
        program_name: &str,
    ) -> Result<(), AetherError> {
        let path = program_path.as_ref();

        // Load the BPF object file
        let mut bpf = Ebpf::load_file(path).map_err(|e| {
            AetherError::Ebpf(EbpfError::LoadFailed {
                reason: format!("Failed to load BPF object '{}': {}", program_name, e),
            })
        })?;

        // Get the XDP program
        let program: &mut Xdp = bpf
            .program_mut(program_name)
            .ok_or_else(|| {
                AetherError::Ebpf(EbpfError::LoadFailed {
                    reason: format!("Program '{}' not found in BPF object", program_name),
                })
            })?
            .try_into()
            .map_err(|e: aya::programs::ProgramError| {
                AetherError::Ebpf(EbpfError::LoadFailed {
                    reason: format!("Program '{}' has invalid type: {}", program_name, e),
                })
            })?;

        program.load().map_err(|e| {
            AetherError::Ebpf(EbpfError::LoadFailed {
                reason: format!("Failed to load program '{}': {}", program_name, e),
            })
        })?;

        program
            .attach(&self.interface, XdpFlags::default())
            .map_err(|e| {
                AetherError::Ebpf(EbpfError::AttachFailed {
                    interface: self.interface.clone(),
                    reason: format!("Failed to attach XDP program: {}", e),
                })
            })?;

        tracing::info!(
            interface = %self.interface,
            program = %program_name,
            path = %path.display(),
            "XDP program loaded and attached"
        );

        self.bpf = Some(bpf);
        self.loaded = true;

        Ok(())
    }

    /// Register a port mapping in the BPF map.
    ///
    /// # Arguments
    /// * `port` - TCP port to route
    /// * `pid` - Process ID of the handler
    /// * `addr` - IPv4 address to route to (default: 127.0.0.1)
    pub async fn register_port(
        &mut self,
        port: Port,
        pid: ProcessId,
        addr: Option<Ipv4Addr>,
    ) -> Result<(), AetherError> {
        let key = PortKey {
            port: port.value(),
            _padding: 0,
        };
        let value = PortValue {
            pid: pid.value(),
            addr: addr.unwrap_or(Ipv4Addr::LOCALHOST).into(),
        };

        // Update userspace mirror
        {
            let mut map = self.port_map.write().await;
            map.insert(port.value(), value);
        }

        // Update BPF map if loaded
        if let Some(ref mut bpf) = self.bpf {
            let mut bpf_map: BpfHashMap<_, PortKey, PortValue> =
                BpfHashMap::try_from(bpf.map_mut("port_redirect_map").ok_or_else(|| {
                    AetherError::Ebpf(EbpfError::MapNotFound {
                        name: "port_redirect_map".to_string(),
                    })
                })?)
                .map_err(|e| {
                    AetherError::Ebpf(EbpfError::MapOperationFailed {
                        operation: "open".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            bpf_map.insert(key, value, 0).map_err(|e| {
                AetherError::Ebpf(EbpfError::MapOperationFailed {
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }

        tracing::info!(port = %port, pid = %pid, "Registered port mapping");
        Ok(())
    }

    /// Unregister a port mapping from the BPF map.
    pub async fn unregister_port(&mut self, port: Port) -> Result<(), AetherError> {
        let key = PortKey {
            port: port.value(),
            _padding: 0,
        };

        // Update userspace mirror
        {
            let mut map = self.port_map.write().await;
            map.remove(&port.value());
        }

        // Update BPF map if loaded
        if let Some(ref mut bpf) = self.bpf {
            let mut bpf_map: BpfHashMap<_, PortKey, PortValue> =
                BpfHashMap::try_from(bpf.map_mut("port_redirect_map").ok_or_else(|| {
                    AetherError::Ebpf(EbpfError::MapNotFound {
                        name: "port_redirect_map".to_string(),
                    })
                })?)
                .map_err(|e| {
                    AetherError::Ebpf(EbpfError::MapOperationFailed {
                        operation: "open".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            // Ignore error if key doesn't exist
            let _ = bpf_map.remove(&key);
        }

        tracing::info!(port = %port, "Unregistered port mapping");
        Ok(())
    }

    /// Get the process ID for a port from the userspace cache.
    pub async fn lookup_port(&self, port: Port) -> Option<u32> {
        let map = self.port_map.read().await;
        map.get(&port.value()).map(|v| v.pid)
    }

    /// Get all registered port mappings.
    pub async fn list_ports(&self) -> Vec<(u16, u32)> {
        let map = self.port_map.read().await;
        map.iter().map(|(k, v)| (*k, v.pid)).collect()
    }

    /// Get statistics about the XDP manager.
    pub async fn stats(&self) -> XdpStats {
        let map = self.port_map.read().await;
        XdpStats {
            registered_ports: map.len(),
            interface: self.interface.clone(),
            loaded: self.loaded,
            ports: map.keys().copied().collect(),
        }
    }

    /// Detach the XDP program from the interface.
    pub fn detach(&mut self) {
        if self.loaded {
            // The program is automatically detached when Bpf is dropped
            self.bpf = None;
            self.loaded = false;
            tracing::info!(interface = %self.interface, "XDP program detached");
        }
    }
}

impl Drop for XdpManager {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Statistics from the XDP manager.
#[derive(Debug, Clone)]
pub struct XdpStats {
    pub registered_ports: usize,
    pub interface: String,
    pub loaded: bool,
    pub ports: Vec<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_xdp_manager_creation() {
        let manager = XdpManager::new("lo");
        assert_eq!(manager.interface(), "lo");
        assert!(!manager.is_loaded());
    }

    #[tokio::test]
    async fn test_port_registration_userspace() {
        let mut manager = XdpManager::new("lo");

        let port = Port::new(8080).unwrap();
        let pid = ProcessId::new(1234).unwrap();

        manager.register_port(port, pid, None).await.unwrap();

        let lookup = manager.lookup_port(port).await;
        assert_eq!(lookup, Some(1234));
    }

    #[tokio::test]
    async fn test_port_unregistration() {
        let mut manager = XdpManager::new("lo");

        let port = Port::new(8080).unwrap();
        let pid = ProcessId::new(1234).unwrap();

        manager.register_port(port, pid, None).await.unwrap();
        manager.unregister_port(port).await.unwrap();

        let lookup = manager.lookup_port(port).await;
        assert_eq!(lookup, None);
    }

    #[tokio::test]
    async fn test_list_ports() {
        let mut manager = XdpManager::new("lo");

        manager
            .register_port(Port::new(8080).unwrap(), ProcessId::new(100).unwrap(), None)
            .await
            .unwrap();
        manager
            .register_port(Port::new(8081).unwrap(), ProcessId::new(101).unwrap(), None)
            .await
            .unwrap();

        let ports = manager.list_ports().await;
        assert_eq!(ports.len(), 2);
    }

    #[tokio::test]
    async fn test_stats() {
        let mut manager = XdpManager::new("eth0");

        manager
            .register_port(Port::new(3000).unwrap(), ProcessId::new(999).unwrap(), None)
            .await
            .unwrap();

        let stats = manager.stats().await;
        assert_eq!(stats.interface, "eth0");
        assert_eq!(stats.registered_ports, 1);
        assert!(!stats.loaded);
        assert!(stats.ports.contains(&3000));
    }
}
