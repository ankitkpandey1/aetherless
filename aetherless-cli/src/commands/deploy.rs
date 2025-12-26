//! `aether deploy` command - Deploy function configuration.
//!
//! Validates configuration and provides deployment instructions.

use aetherless_core::ConfigLoader;

pub async fn execute(file: &str, _force: bool) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(file = %file, "Validating function configuration for deployment");

    // Load and validate the function configuration
    let config = ConfigLoader::load_file(file)?;

    println!("✓ Configuration validated successfully");
    println!();
    println!("Functions ready for deployment:");
    for func in &config.functions {
        println!(
            "  • {} (port: {}, memory: {}, handler: {})",
            func.id,
            func.trigger_port,
            func.memory_limit,
            func.handler_path
        );
    }

    println!();
    println!("To start these functions, run:");
    println!();
    println!("  aether -c {} up --foreground", file);
    println!();

    Ok(())
}
