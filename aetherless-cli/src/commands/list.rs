// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! `aether list` command - List functions from configuration.
//!
//! Lists functions defined in the configuration file.

use aetherless_core::ConfigLoader;

pub async fn execute(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = ConfigLoader::load_file(config_path)?;

    if config.functions.is_empty() {
        println!("No functions defined in configuration.");
        return Ok(());
    }

    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                           CONFIGURED FUNCTIONS                               ║");
    println!("╠═══════════════════╦════════════╦═══════════════════╦═════════════════════════╣");
    println!("║ ID                ║ Port       ║ Memory            ║ Handler                 ║");
    println!("╠═══════════════════╬════════════╬═══════════════════╬═════════════════════════╣");

    for func in &config.functions {
        let handler_display = func
            .handler_path
            .as_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        println!(
            "║ {:<17} ║ {:<10} ║ {:<17} ║ {:<23} ║",
            func.id.as_str(),
            func.trigger_port.value(),
            format!("{}", func.memory_limit),
            handler_display
        );
    }

    println!("╚═══════════════════╩════════════╩═══════════════════╩═════════════════════════╝");
    println!();
    println!("Total: {} function(s)", config.functions.len());

    Ok(())
}
