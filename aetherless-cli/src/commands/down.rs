// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! `aether down` command - Stop the orchestrator.
//!
//! Stops a running orchestrator by sending SIGTERM to the process.

use std::fs;
use std::process::Command;

const PID_FILE: &str = "/tmp/aetherless/orchestrator.pid";

pub async fn execute() -> Result<(), Box<dyn std::error::Error>> {
    // Check if PID file exists
    if let Ok(pid_str) = fs::read_to_string(PID_FILE) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            println!("Stopping orchestrator (PID: {})...", pid);

            let status = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();

            match status {
                Ok(s) if s.success() => {
                    // Clean up PID file
                    let _ = fs::remove_file(PID_FILE);
                    println!("✓ Orchestrator stopped");
                }
                Ok(_) => {
                    println!("✗ Failed to stop orchestrator (process may have already exited)");
                    let _ = fs::remove_file(PID_FILE);
                }
                Err(e) => {
                    println!("✗ Error stopping orchestrator: {}", e);
                }
            }
        } else {
            println!("✗ Invalid PID file");
        }
    } else {
        println!("No orchestrator running (PID file not found)");
        println!();
        println!("If you started the orchestrator with --foreground,");
        println!("use Ctrl+C to stop it.");
    }

    Ok(())
}
