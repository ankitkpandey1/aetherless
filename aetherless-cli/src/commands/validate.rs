// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! `aether validate` command - Validate configuration file.

use aetherless_core::ConfigLoader;

pub async fn execute(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(file = %file, "Validating configuration");

    match ConfigLoader::load_file(file) {
        Ok(config) => {
            println!("✓ Configuration is valid");
            println!();
            println!("Orchestrator Settings:");
            println!(
                "  SHM Buffer Size:    {} bytes",
                config.orchestrator.shm_buffer_size
            );
            println!(
                "  Warm Pool Size:     {}",
                config.orchestrator.warm_pool_size
            );
            println!(
                "  Restore Timeout:    {}ms",
                config.orchestrator.restore_timeout_ms
            );
            println!(
                "  Snapshot Directory: {}",
                config.orchestrator.snapshot_dir.display()
            );
            println!();
            println!("Functions ({}):", config.functions.len());
            for func in &config.functions {
                println!(
                    "  - {} (port: {}, memory: {}, timeout: {}ms)",
                    func.id, func.trigger_port, func.memory_limit, func.timeout_ms
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("✗ Configuration validation failed:");
            eprintln!("  {}", e);
            std::process::exit(1);
        }
    }
}
