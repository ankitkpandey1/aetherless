//! `aether list` command - List registered functions.

pub async fn execute() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                           REGISTERED FUNCTIONS                               ║");
    println!("╠═══════════════════╦════════╦════════════╦═══════════════════╦════════════════╣");
    println!("║ ID                ║ State  ║ Port       ║ Memory            ║ Uptime         ║");
    println!("╠═══════════════════╬════════╬════════════╬═══════════════════╬════════════════╣");
    println!("║ (no functions registered)                                                    ║");
    println!("╚═══════════════════╩════════╩════════════╩═══════════════════╩════════════════╝");

    // TODO: Connect to orchestrator and list actual functions

    Ok(())
}
