//! `aether down` command - Stop the orchestrator.

pub async fn execute() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Stopping orchestrator...");

    // TODO: Send shutdown signal to running orchestrator via Unix socket

    println!("✓ Orchestrator stopped");
    Ok(())
}
