//! `aether up` command - Start the orchestrator.

use aetherless_core::{ConfigLoader, FunctionRegistry};

pub async fn execute(
    config_path: &str,
    foreground: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(config = %config_path, foreground = %foreground, "Starting orchestrator");

    // Load and validate configuration - fail fast on invalid config
    let config = ConfigLoader::load_file(config_path)?;

    tracing::info!(
        functions = config.functions.len(),
        "Configuration validated successfully"
    );

    // Create the function registry
    let registry = FunctionRegistry::new_shared();

    // Register all functions from config
    for func_config in config.functions {
        tracing::info!(function_id = %func_config.id, "Registering function");
        registry.register(func_config)?;
    }

    tracing::info!(registered = registry.len(), "All functions registered");

    if foreground {
        tracing::info!("Running in foreground mode. Press Ctrl+C to stop.");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;
        tracing::info!("Shutting down orchestrator");
    } else {
        tracing::info!("Orchestrator started successfully");
    }

    Ok(())
}
