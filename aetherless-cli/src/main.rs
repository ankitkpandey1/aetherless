// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Aetherless CLI
//!
//! Command-line interface for the Aetherless serverless platform.

use clap::{Parser, Subcommand};

mod commands;
mod metrics;
mod tui;
mod warm_pool;

pub use warm_pool::WarmPoolManager;

/// Aetherless - High-performance serverless function orchestrator
#[derive(Parser)]
#[command(name = "aether")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "aetherless.yaml")]
    pub config: String,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the orchestrator
    Up {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,

        /// Enable CRIU warm pools for sub-15ms cold starts
        #[arg(long)]
        warm_pool: bool,
    },

    /// Deploy a function configuration
    Deploy {
        /// Path to the function YAML file
        file: String,

        /// Force reload if function already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Show statistics and metrics
    Stats {
        /// Show TUI dashboard instead of one-shot stats
        #[arg(short, long)]
        dashboard: bool,

        /// Watch mode - continuously update stats
        #[arg(short, long)]
        watch: bool,
    },

    /// List registered functions
    List,

    /// Stop the orchestrator
    Down,

    /// Validate a configuration file
    Validate {
        /// Path to the configuration file
        file: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt().with_env_filter(log_level).init();

    // Dispatch to command handlers
    match cli.command {
        Commands::Up {
            foreground,
            warm_pool,
        } => commands::up::execute(&cli.config, foreground, warm_pool).await,
        Commands::Deploy { file, force } => commands::deploy::execute(&file, force).await,
        Commands::Stats { dashboard, watch } => commands::stats::execute(watch, dashboard).await,
        Commands::List => commands::list::execute(&cli.config).await,
        Commands::Down => commands::down::execute().await,
        Commands::Validate { file } => commands::validate::execute(&file).await,
    }
}
