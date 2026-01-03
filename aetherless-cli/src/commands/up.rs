// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! `aether up` command - Start the orchestrator.
//!
//! Spawns handler processes, creates Unix sockets, and waits for READY signals.
//! With --warm-pool, creates CRIU snapshots for sub-15ms cold starts.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use aetherless_core::{ConfigLoader, FunctionConfig, FunctionRegistry, FunctionState};

use crate::warm_pool::WarmPoolManager;

/// Timeout waiting for READY signal from handler
const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[allow(dead_code)]
struct RunningProcess {
    child: Child,
    config: FunctionConfig,
    pid: u32,
}

pub async fn execute(
    config_path: &str,
    foreground: bool,
    warm_pool_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(config = %config_path, foreground = %foreground, warm_pool = %warm_pool_enabled, "Starting orchestrator");

    // Load and validate configuration - fail fast on invalid config
    let config = ConfigLoader::load_file(config_path)?;

    tracing::info!(
        functions = config.functions.len(),
        "Configuration validated successfully"
    );

    // Create the function registry
    let registry = FunctionRegistry::new_shared();

    // Create socket directory
    let socket_dir = PathBuf::from("/tmp/aetherless");
    if socket_dir.exists() {
        std::fs::remove_dir_all(&socket_dir)?;
    }
    std::fs::create_dir_all(&socket_dir)?;

    // Initialize warm pool manager if enabled
    let snapshot_dir = config.orchestrator.snapshot_dir.clone();
    let restore_timeout = config.orchestrator.restore_timeout_ms;
    let pool_size = config.orchestrator.warm_pool_size;

    let mut warm_pool = if warm_pool_enabled {
        match WarmPoolManager::new(&snapshot_dir, restore_timeout, pool_size) {
            Ok(pool) => {
                println!(
                    "✓ Warm pool enabled (snapshot dir: {})",
                    snapshot_dir.display()
                );
                pool
            }
            Err(e) => {
                println!("⚠ Warm pool unavailable: {} (continuing without)", e);
                WarmPoolManager::disabled()
            }
        }
    } else {
        WarmPoolManager::disabled()
    };

    // Track running processes
    let processes: Arc<Mutex<HashMap<String, RunningProcess>>> =
        Arc::new(Mutex::new(HashMap::new()));

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              AETHERLESS ORCHESTRATOR                         ║");
    if warm_pool.is_enabled() {
        println!("║              [WARM POOL ENABLED]                             ║");
    }
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Spawn all function handlers
    for func_config in &config.functions {
        println!("▶ Spawning function: {}", func_config.id);
        registry.register(func_config.clone())?;

        // Register with warm pool
        if warm_pool.is_enabled() {
            warm_pool.register(func_config.clone()).await;
        }

        // Spawn the handler process with Unix socket handshake
        match spawn_handler_with_socket(func_config, &socket_dir).await {
            Ok((child, pid)) => {
                println!(
                    "  ✓ {} started (PID: {}, Port: {})",
                    func_config.id, pid, func_config.trigger_port
                );

                // Create CRIU snapshot if warm pool enabled
                if warm_pool.is_enabled() {
                    match warm_pool.create_snapshot(&func_config.id, pid).await {
                        Ok(()) => {
                            println!("  ✓ {} snapshot created", func_config.id);
                            registry.transition(&func_config.id, FunctionState::WarmSnapshot)?;
                        }
                        Err(e) => {
                            println!("  ⚠ {} snapshot failed: {} (continuing)", func_config.id, e);
                            registry.transition(&func_config.id, FunctionState::Running)?;
                        }
                    }
                } else {
                    // Update state to Running
                    registry.transition(&func_config.id, FunctionState::Running)?;

                    // Track cold start
                    crate::metrics::COLD_STARTS
                        .with_label_values(&[func_config.id.as_str()])
                        .inc();
                }

                // Track the process
                processes.lock().await.insert(
                    func_config.id.to_string(),
                    RunningProcess {
                        child,
                        config: func_config.clone(),
                        pid,
                    },
                );
            }
            Err(e) => {
                println!("  ✗ {} failed: {}", func_config.id, e);
                tracing::error!(
                    function_id = %func_config.id,
                    error = %e,
                    "Failed to spawn handler"
                );
            }
        }
    }

    let running_count = processes.lock().await.len();

    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!(
        "║ Status: {} functions running                                 ║",
        running_count
    );
    println!("╠══════════════════════════════════════════════════════════════╣");

    for func_config in &config.functions {
        let state = registry
            .get_state(&func_config.id)
            .unwrap_or(FunctionState::Uninitialized);
        let status_icon = if state == FunctionState::Running {
            "●"
        } else {
            "○"
        };
        println!(
            "║ {} {:<20} → http://localhost:{:<5} [{:?}]",
            status_icon,
            func_config.id.as_str(),
            func_config.trigger_port.value(),
            state
        );
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    // Start metrics server
    crate::metrics::start_metrics_server(9090);

    // Start stats writer background task
    let warm_pool = Arc::new(tokio::sync::Mutex::new(warm_pool));

    {
        let warm_pool = warm_pool.clone();
        let registry = registry.clone();
        let processes = processes.clone();

        tokio::spawn(async move {
            loop {
                // Collect stats
                let mut stats = aetherless_core::stats::AetherlessStats {
                    active_instances: processes.lock().await.len(),
                    ..Default::default()
                };

                // Warm pool stats
                let wp = warm_pool.lock().await;
                stats.warm_pool_active = wp.is_enabled();
                // (Could add detailed warm pool stats here if exposed in stats module)

                // Function status
                for (id, state, config) in registry.snapshot() {
                    let memory = config.memory_limit.megabytes();

                    stats.functions.insert(
                        id.clone(),
                        aetherless_core::stats::FunctionStatus {
                            id,
                            state,
                            pid: None, // Could lookup in processes map
                            port: config.trigger_port.value(),
                            memory_mb: memory,
                            restore_count: 0,
                            last_restore_ms: None,
                        },
                    );
                }

                // Write to SHM file for TUI
                // atomic write: write to temp file then rename
                if let Ok(json) = serde_json::to_string(&stats) {
                    let _ = std::fs::write("/dev/shm/aetherless-stats.json", json);
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    }

    if foreground {
        println!();
        println!("Press Ctrl+C to stop...");
        println!();

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;

        println!();
        println!("Shutting down...");
        tracing::info!("Shutting down orchestrator");

        // Kill all child processes
        let mut procs = processes.lock().await;
        for (id, mut proc) in procs.drain() {
            print!("  Stopping {}... ", id);
            let _ = proc.child.kill();
            let _ = proc.child.wait();
            println!("done");
        }

        // Cleanup socket directory
        let _ = std::fs::remove_dir_all(&socket_dir);

        println!();
        println!("Orchestrator stopped.");
    }

    Ok(())
}

/// Spawn a handler process with Unix socket handshake
async fn spawn_handler_with_socket(
    config: &FunctionConfig,
    socket_dir: &Path,
) -> Result<(Child, u32), Box<dyn std::error::Error>> {
    let handler_path = config.handler_path.as_path();
    let socket_path = socket_dir.join(format!("{}.sock", config.id));

    // Remove old socket if exists
    let _ = std::fs::remove_file(&socket_path);

    // Create Unix listener BEFORE spawning the process
    let listener = UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;

    // Determine how to run the handler
    let (program, args): (String, Vec<String>) =
        if handler_path.extension().map(|e| e == "py").unwrap_or(false) {
            // Python script
            (
                "python3".to_string(),
                vec![handler_path.to_string_lossy().to_string()],
            )
        } else {
            // Binary executable
            (handler_path.to_string_lossy().to_string(), vec![])
        };

    // Build environment
    let mut env_vars: HashMap<String, String> = config.environment.clone();
    env_vars.insert(
        "AETHER_SOCKET".to_string(),
        socket_path.to_string_lossy().to_string(),
    );
    env_vars.insert("AETHER_FUNCTION_ID".to_string(), config.id.to_string());
    env_vars.insert(
        "AETHER_TRIGGER_PORT".to_string(),
        config.trigger_port.value().to_string(),
    );

    tracing::debug!(
        program = %program,
        handler = %handler_path.display(),
        socket = %socket_path.display(),
        "Spawning handler"
    );

    // Spawn the process
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .envs(&env_vars)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to spawn '{}': {} (handler_path: {})",
            program,
            e,
            handler_path.display()
        )
    })?;

    let pid = child.id();

    // Wait for READY signal from the handler
    let start = Instant::now();
    let mut ready_received = false;

    while start.elapsed() < READY_TIMEOUT {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;

                let mut buf = [0u8; 16];
                match stream.read(&mut buf) {
                    Ok(n) if n >= 5 => {
                        if &buf[..5] == b"READY" {
                            ready_received = true;
                            tracing::info!(
                                function_id = %config.id,
                                pid = pid,
                                elapsed_ms = start.elapsed().as_millis(),
                                "Handler sent READY signal"
                            );
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection yet, wait a bit
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                return Err(format!("Socket accept error: {}", e).into());
            }
        }
    }

    if !ready_received {
        // Kill the process if it didn't send READY
        let mut child = child;
        let _ = child.kill();
        return Err(format!(
            "Handler did not send READY within {}s",
            READY_TIMEOUT.as_secs()
        )
        .into());
    }

    Ok((child, pid))
}
