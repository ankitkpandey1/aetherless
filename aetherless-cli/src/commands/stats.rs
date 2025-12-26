//! `aether stats` command - Show orchestrator statistics.
//!
//! Displays runtime metrics when connected to the orchestrator.

use crate::tui;

pub async fn execute(watch: bool, dashboard: bool) -> Result<(), Box<dyn std::error::Error>> {
    if dashboard {
        // Run the TUI dashboard
        tui::run_dashboard().await?;
        return Ok(());
    }

    if watch {
        println!("Stats watch mode requires a running orchestrator.");
        println!();
        println!("Start the orchestrator first:");
        println!("  aether -c config.yaml up --foreground");
        return Ok(());
    }

    // Show static info
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    AETHERLESS STATISTICS                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ To view live statistics, run:                                ║");
    println!("║   aether stats --dashboard                                   ║");
    println!("║                                                              ║");
    println!("║ Or start the orchestrator in foreground mode:                ║");
    println!("║   aether -c config.yaml up --foreground                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(())
}
