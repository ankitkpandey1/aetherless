// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! CPU Affinity and NUMA-aware process allocation.
//!
//! Provides even distribution of handler processes across CPU cores
//! with awareness of NUMA topology for optimal memory/cache locality.

use nix::sched::{sched_setaffinity, CpuSet};
use nix::unistd::Pid;
use std::sync::atomic::{AtomicUsize, Ordering};

/// CPU allocator that distributes processes evenly across cores.
/// 
/// Features:
/// - Round-robin CPU assignment for even load distribution
/// - NUMA-aware allocation (processes stay on same NUMA node when possible)
/// - Cache locality optimization (adjacent instances on same socket)
pub struct CpuAllocator {
    /// Total number of logical CPUs available
    num_cpus: usize,
    /// Next CPU to assign (atomic for thread-safety)
    next_cpu: AtomicUsize,
    /// NUMA node topology (if available)
    numa_nodes: Vec<Vec<usize>>,
}

#[allow(dead_code)]
impl CpuAllocator {
    /// Create a new CPU allocator.
    /// 
    /// Automatically detects CPU count and NUMA topology.
    pub fn new() -> Self {
        let num_cpus = num_cpus::get();
        let numa_nodes = Self::detect_numa_topology(num_cpus);
        
        tracing::info!(
            num_cpus = num_cpus,
            numa_nodes = numa_nodes.len(),
            "CpuAllocator initialized"
        );
        
        Self {
            num_cpus,
            next_cpu: AtomicUsize::new(0),
            numa_nodes,
        }
    }

    /// Detect NUMA topology by reading /sys/devices/system/node/
    /// Falls back to single-node if NUMA info unavailable.
    fn detect_numa_topology(num_cpus: usize) -> Vec<Vec<usize>> {
        let mut nodes: Vec<Vec<usize>> = Vec::new();
        
        // Try to read NUMA node info from sysfs
        let node_path = std::path::Path::new("/sys/devices/system/node");
        if node_path.exists() {
            if let Ok(entries) = std::fs::read_dir(node_path) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("node") {
                        if let Ok(node_num) = name_str[4..].parse::<usize>() {
                            // Read CPUs for this node
                            let cpulist_path = entry.path().join("cpulist");
                            if let Ok(cpulist) = std::fs::read_to_string(cpulist_path) {
                                let cpus = parse_cpu_list(&cpulist.trim());
                                if !cpus.is_empty() {
                                    // Ensure we have enough slots
                                    while nodes.len() <= node_num {
                                        nodes.push(Vec::new());
                                    }
                                    nodes[node_num] = cpus;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback: single node with all CPUs
        if nodes.is_empty() {
            nodes.push((0..num_cpus).collect());
        }

        nodes
    }

    /// Allocate the next CPU core using round-robin.
    /// Returns the CPU index to pin to.
    pub fn allocate(&self) -> usize {
        self.next_cpu.fetch_add(1, Ordering::Relaxed) % self.num_cpus
    }

    /// Allocate a CPU from a specific NUMA node (for memory locality).
    /// Falls back to any CPU if the node is invalid.
    pub fn allocate_on_node(&self, node: usize) -> usize {
        if node < self.numa_nodes.len() && !self.numa_nodes[node].is_empty() {
            let node_cpus = &self.numa_nodes[node];
            let idx = self.next_cpu.fetch_add(1, Ordering::Relaxed) % node_cpus.len();
            node_cpus[idx]
        } else {
            self.allocate()
        }
    }

    /// Pin a process to a specific CPU core.
    /// 
    /// # Arguments
    /// * `pid` - Process ID to pin
    /// 
    /// # Returns
    /// The CPU core the process was pinned to, or an error.
    pub fn pin_process(&self, pid: u32) -> Result<usize, nix::Error> {
        let cpu = self.allocate();
        self.pin_to_cpu(pid, cpu)?;
        Ok(cpu)
    }

    /// Pin a process to a specific CPU core on the same NUMA node.
    /// This optimizes for memory locality.
    /// 
    /// # Arguments
    /// * `pid` - Process ID to pin
    /// * `preferred_node` - Preferred NUMA node (0 if unsure)
    pub fn pin_process_numa(&self, pid: u32, preferred_node: usize) -> Result<usize, nix::Error> {
        let cpu = self.allocate_on_node(preferred_node);
        self.pin_to_cpu(pid, cpu)?;
        Ok(cpu)
    }

    /// Pin process to a specific CPU.
    fn pin_to_cpu(&self, pid: u32, cpu: usize) -> Result<(), nix::Error> {
        let mut cpuset = CpuSet::new();
        cpuset.set(cpu)?;
        sched_setaffinity(Pid::from_raw(pid as i32), &cpuset)?;
        
        tracing::debug!(pid = pid, cpu = cpu, "Process pinned to CPU");
        Ok(())
    }

    /// Get the number of available CPUs.
    pub fn num_cpus(&self) -> usize {
        self.num_cpus
    }

    /// Get the number of NUMA nodes.
    pub fn num_numa_nodes(&self) -> usize {
        self.numa_nodes.len()
    }
}

impl Default for CpuAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a CPU list string like "0-3,8-11" into a Vec of CPU indices.
fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut cpus = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() == 2 {
                if let (Ok(start), Ok(end)) = (range[0].parse::<usize>(), range[1].parse::<usize>()) {
                    cpus.extend(start..=end);
                }
            }
        } else if let Ok(cpu) = part.parse::<usize>() {
            cpus.push(cpu);
        }
    }
    cpus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_list() {
        assert_eq!(parse_cpu_list("0-3"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("0,2,4"), vec![0, 2, 4]);
        assert_eq!(parse_cpu_list("0-1,4-5"), vec![0, 1, 4, 5]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
    }

    #[test]
    fn test_allocator_round_robin() {
        let allocator = CpuAllocator::new();
        let first = allocator.allocate();
        let second = allocator.allocate();
        // They should be different (unless single CPU system)
        if allocator.num_cpus() > 1 {
            assert_ne!(first, second);
        }
    }

    #[test]
    fn test_allocator_wraps_around() {
        let allocator = CpuAllocator::new();
        let num = allocator.num_cpus();
        // Allocate num+1 times, should wrap around
        for _ in 0..num {
            allocator.allocate();
        }
        let wrapped = allocator.allocate();
        assert!(wrapped < num);
    }
}
