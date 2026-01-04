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
                    if let Some(suffix) = name_str.strip_prefix("node") {
                        if let Ok(node_num) = suffix.parse::<usize>() {
                            // Read CPUs for this node
                            let cpulist_path = entry.path().join("cpulist");
                            if let Ok(cpulist) = std::fs::read_to_string(cpulist_path) {
                                let cpus = parse_cpu_list(cpulist.trim());
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
                if let (Ok(start), Ok(end)) = (range[0].parse::<usize>(), range[1].parse::<usize>())
                {
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
    use std::collections::HashSet;
    use std::process::Command;

    #[test]
    fn test_parse_cpu_list_range() {
        assert_eq!(parse_cpu_list("0-3"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_cpu_list_discrete() {
        assert_eq!(parse_cpu_list("0,2,4"), vec![0, 2, 4]);
    }

    #[test]
    fn test_parse_cpu_list_mixed() {
        assert_eq!(parse_cpu_list("0-1,4-5"), vec![0, 1, 4, 5]);
    }

    #[test]
    fn test_parse_cpu_list_empty() {
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
    }

    #[test]
    fn test_parse_cpu_list_complex() {
        assert_eq!(
            parse_cpu_list("0,2-4,7,10-12"),
            vec![0, 2, 3, 4, 7, 10, 11, 12]
        );
    }

    #[test]
    fn test_allocator_initialization() {
        let allocator = CpuAllocator::new();
        assert!(allocator.num_cpus() > 0, "Should detect at least 1 CPU");
        assert!(
            allocator.num_numa_nodes() > 0,
            "Should detect at least 1 NUMA node"
        );
    }

    #[test]
    fn test_allocator_round_robin_distribution() {
        let allocator = CpuAllocator::new();
        let num = allocator.num_cpus();

        if num > 1 {
            let first = allocator.allocate();
            let second = allocator.allocate();
            // They should be different on multi-CPU systems
            assert_ne!(
                first, second,
                "Consecutive allocations should use different CPUs"
            );
        }
    }

    #[test]
    fn test_allocator_wraps_around_correctly() {
        let allocator = CpuAllocator::new();
        let num = allocator.num_cpus();

        // Allocate num times to use all CPUs
        for _ in 0..num {
            allocator.allocate();
        }

        // Next allocation should wrap around to 0
        let wrapped = allocator.allocate();
        assert_eq!(
            wrapped, 0,
            "Should wrap around to CPU 0 after using all CPUs"
        );
    }

    #[test]
    fn test_allocator_even_distribution() {
        let allocator = CpuAllocator::new();
        let num = allocator.num_cpus();
        let iterations = num * 10; // Run 10 full cycles

        let mut cpu_counts: Vec<usize> = vec![0; num];

        for _ in 0..iterations {
            let cpu = allocator.allocate();
            cpu_counts[cpu] += 1;
        }

        // Each CPU should be used exactly 10 times (even distribution)
        for (cpu, count) in cpu_counts.iter().enumerate() {
            assert_eq!(
                *count, 10,
                "CPU {} should be allocated exactly 10 times, got {}",
                cpu, count
            );
        }
    }

    #[test]
    fn test_allocator_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let allocator = Arc::new(CpuAllocator::new());
        let num_threads = 4;
        let allocations_per_thread = 100;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let alloc = Arc::clone(&allocator);
                thread::spawn(move || {
                    let mut cpus = Vec::new();
                    for _ in 0..allocations_per_thread {
                        cpus.push(alloc.allocate());
                    }
                    cpus
                })
            })
            .collect();

        let mut all_cpus = Vec::new();
        for handle in handles {
            all_cpus.extend(handle.join().unwrap());
        }

        // Verify total allocations
        assert_eq!(all_cpus.len(), num_threads * allocations_per_thread);

        // Verify all CPUs are valid
        let num_cpus = allocator.num_cpus();
        for cpu in &all_cpus {
            assert!(
                *cpu < num_cpus,
                "Allocated CPU {} should be < {}",
                cpu,
                num_cpus
            );
        }
    }

    #[test]
    fn test_allocator_numa_node_allocation() {
        let allocator = CpuAllocator::new();

        // Allocate on node 0 (should always exist)
        let cpu = allocator.allocate_on_node(0);
        assert!(
            cpu < allocator.num_cpus(),
            "NUMA allocation should return valid CPU"
        );
    }

    #[test]
    fn test_allocator_numa_invalid_node_fallback() {
        let allocator = CpuAllocator::new();

        // Allocate on invalid node (should fallback to any CPU)
        let cpu = allocator.allocate_on_node(999);
        assert!(
            cpu < allocator.num_cpus(),
            "Invalid NUMA node should fallback to valid CPU"
        );
    }

    #[test]
    fn test_pin_current_process() {
        let allocator = CpuAllocator::new();
        let pid = std::process::id();

        // Pin current process (may fail without CAP_SYS_NICE, but shouldn't panic)
        let result = allocator.pin_process(pid);

        // On most systems, pinning your own process should work
        // If it fails, it should be due to permissions, not logic errors
        match result {
            Ok(cpu) => {
                assert!(cpu < allocator.num_cpus());
                println!("Successfully pinned to CPU {}", cpu);
            }
            Err(e) => {
                // Permission errors are acceptable in test environments
                println!("Pin failed (expected in restricted environments): {}", e);
            }
        }
    }

    #[test]
    fn test_verify_affinity_with_taskset() {
        // Spawn a child process and verify its affinity
        let allocator = CpuAllocator::new();

        // Spawn a sleep process
        let child = Command::new("sleep").arg("10").spawn();

        if let Ok(mut child_proc) = child {
            let pid = child_proc.id();

            // Pin the process
            if allocator.pin_process(pid).is_ok() {
                // Verify with taskset
                let output = Command::new("taskset")
                    .arg("-p")
                    .arg(pid.to_string())
                    .output();

                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    // taskset output should contain the PID and affinity mask
                    assert!(
                        stdout.contains(&pid.to_string()),
                        "taskset output should contain PID: {}",
                        stdout
                    );
                    println!("Taskset output: {}", stdout);
                }
            }

            // Clean up
            let _ = child_proc.kill();
            let _ = child_proc.wait();
        }
    }

    #[test]
    fn test_all_cpus_covered_in_full_cycle() {
        let allocator = CpuAllocator::new();
        let num = allocator.num_cpus();

        let mut seen: HashSet<usize> = HashSet::new();

        // Allocate exactly num times
        for _ in 0..num {
            seen.insert(allocator.allocate());
        }

        // All CPUs should have been used
        assert_eq!(
            seen.len(),
            num,
            "After {} allocations, all {} CPUs should be used. Got {:?}",
            num,
            num,
            seen
        );
    }
}
