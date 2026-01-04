// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! Aetherless CLI
//!
//! Command-line interface for the Aetherless serverless platform.

use clap::{Parser, Subcommand};

mod commands;
mod cpu_affinity;
pub mod gateway;
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
    /// Start the orchestrator in the current terminal.
    ///
    /// This command initializes the function registry, starts the Unix socket listener,
    /// and spawns handler processes. It manages the full lifecycle of functions.
    ///
    /// Examples:
    /// ```bash
    /// # Start in foreground
    /// aether up --foreground
    ///
    /// # Start with warm pools enabled
    /// aether up --warm-pool
    /// ```
    Up {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,

        /// Enable CRIU warm pools for sub-15ms cold starts
        #[arg(long)]
        warm_pool: bool,

        /// Enable XDP/eBPF data plane for kernel-bypass networking
        /// Requires root privileges and compiled BPF object
        #[arg(long)]
        xdp: bool,

        /// Network interface for XDP (default: lo)
        #[arg(long, default_value = "lo")]
        xdp_interface: String,
    },

    /// Deploy a function configuration.
    ///
    /// Validates the configuration file syntax and logical correctness (e.g. handler existence).
    /// If validation passes, it provides instructions for starting the orchestrator.
    Deploy {
        /// Path to the function YAML file
        file: String,

        /// Force reload if function already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Show statistics and metrics.
    ///
    /// Displays either a one-time snapshot of system stats or a live TUI dashboard.
    Stats {
        /// Show TUI dashboard instead of one-shot stats
        #[arg(short, long)]
        dashboard: bool,

        /// Watch mode - continuously update stats
        #[arg(short, long)]
        watch: bool,
    },

    /// List registered functions.
    List,

    /// Stop the orchestrator and all running functions.
    Down,

    /// Validate a configuration file without deploying.
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
            xdp,
            xdp_interface,
        } => commands::up::execute(&cli.config, foreground, warm_pool, xdp, &xdp_interface).await,
        Commands::Deploy { file, force } => commands::deploy::execute(&file, force).await,
        Commands::Stats { dashboard, watch } => commands::stats::execute(watch, dashboard).await,
        Commands::List => commands::list::execute(&cli.config).await,
        Commands::Down => commands::down::execute().await,
        Commands::Validate { file } => commands::validate::execute(&file).await,
    }
}
