// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! `aether up` command - Start the orchestrator.
//!
//! Spawns handler processes, creates Unix sockets, and waits for READY signals.
//! With --warm-pool, creates CRIU snapshots for sub-15ms cold starts.
//! With --autoscaler, dynamically scales function instances based on load.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

use aetherless_core::{ConfigLoader, FunctionConfig, FunctionRegistry, FunctionState};
use aetherless_core::autoscaler::{Autoscaler, ScalingPolicy};

use crate::warm_pool::WarmPoolManager;

/// Timeout waiting for READY signal from handler
const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[allow(dead_code)]
struct RunningProcess {
    child: Child,
    config: FunctionConfig,
    pid: u32,
    instance_id: String,
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

    // Track running processes: Map<FunctionId, Vec<RunningProcess>>
    // We wrap it in Arc<Mutex> to share with stats tasks/main loop
    let processes: Arc<Mutex<HashMap<String, Vec<RunningProcess>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Initialize Autoscaler
    let autoscaler = Autoscaler::new(ScalingPolicy {
        min_replicas: 1,
        max_replicas: 5,         // Hardcoded limit for safety
        target_concurrency: 10.0, // Arbitrary unit of "load"
        ..Default::default()
    });

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              AETHERLESS ORCHESTRATOR                         ║");
    if warm_pool.is_enabled() {
        println!("║              [WARM POOL ENABLED]                             ║");
    }
    println!("║              [AUTOSCALER ENABLED]                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Helper to spawn a single instance
    // Note: This closure captures variables so we can't easily put it in a separate function without passing args
    // We define spawn logic here to be reused.
    // Actually, due to async closure complexity, let's just loop locally.

    // Spawn initial instances (min_replicas = 1)
    for func_config in &config.functions {
        println!("▶ Spawning function: {}", func_config.id);
        registry.register(func_config.clone())?;

        // Register with warm pool
        if warm_pool.is_enabled() {
            warm_pool.register(func_config.clone()).await;
        }

        // Spawn one instance initially
        let instance_id = Uuid::new_v4().to_string();
        match spawn_handler_with_socket(func_config, &socket_dir, &instance_id).await {
            Ok((child, pid)) => {
                println!("  ✓ {} started (PID: {})", func_config.id, pid);
                
                // For initial spawn, we assume it's "Cold" unless restored (not handled perfectly here yet)
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
                    registry.transition(&func_config.id, FunctionState::Running)?;
                    crate::metrics::COLD_STARTS.with_label_values(&[func_config.id.as_str()]).inc();
                }

                processes.lock().await.entry(func_config.id.to_string()).or_default().push(RunningProcess {
                    child,
                    config: func_config.clone(),
                    pid,
                    instance_id,
                });
            }
            Err(e) => {
                println!("  ✗ Failed to start {}: {}", func_config.id, e);
                // Fail hard on initial spawn?
                // return Err(e);
            }
        }
    }

    // Start metrics server
    crate::metrics::start_metrics_server(9090);

    // Setup stats loop
    let processes_stats = processes.clone();
    let registry_stats = registry.clone();
    let warm_pool_stats = Arc::new(tokio::sync::Mutex::new(warm_pool)); 

    // Background stats writer
    tokio::spawn(async move {
        loop {
            let procs = processes_stats.lock().await;
            let mut total_active = 0;
            for list in procs.values() {
                total_active += list.len();
            }
            
            let mut stats = aetherless_core::stats::AetherlessStats {
                active_instances: total_active,
                ..Default::default()
            };
            
            let wp = warm_pool_stats.lock().await;
            stats.warm_pool_active = wp.is_enabled();
            
            for (id, state, config) in registry_stats.snapshot() {
                 stats.functions.insert(id.clone(), aetherless_core::stats::FunctionStatus {
                     id: id.clone(),
                     state,
                     pid: None,
                     port: config.trigger_port.value(),
                     memory_mb: config.memory_limit.megabytes(),
                     restore_count: 0,
                     last_restore_ms: None
                 });
            }
            
            if let Ok(json) = serde_json::to_string(&stats) {
                let _ = std::fs::write("/dev/shm/aetherless-stats.json", json);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    if foreground {
        println!("Press Ctrl+C to stop...");
        
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        
        // Main loop
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                _ = interval.tick() => {
                    // Autoscaler Logic
                    // 1. Read simulated load from file
                    let load: f64 = std::fs::read_to_string("/tmp/aetherless-load")
                        .unwrap_or_default()
                        .trim()
                        .parse()
                        .unwrap_or(0.0);
                    
                    if load > 0.0 {
                        let mut procs_lock = processes.lock().await;
                        // Clone keys to iterate without borrowing lock
                        let func_ids: Vec<String> = procs_lock.keys().cloned().collect();
                        
                        for fid in func_ids {
                            if let Some(instances) = procs_lock.get_mut(&fid) {
                                let current_count = instances.len();
                                let desired = autoscaler.calculate_replicas(current_count, load);
                                
                                if desired > current_count {
                                    let needed = desired - current_count;
                                    tracing::info!("Scaling UP {}: {} -> {} (Load: {})", fid, current_count, desired, load);
                                    
                                    if let Some(first) = instances.first() {
                                        let config = first.config.clone();
                                        // Spawn new instances
                                        for _ in 0..needed {
                                             let instance_id = Uuid::new_v4().to_string();
                                             match spawn_handler_with_socket(&config, &socket_dir, &instance_id).await {
                                                 Ok((child, pid)) => {
                                                     instances.push(RunningProcess {
                                                         child, config: config.clone(), pid, instance_id
                                                     });
                                                 },
                                                 Err(e) => tracing::error!("Scale up failed: {}", e)
                                             }
                                        }
                                    }
                                } else if desired < current_count {
                                    let remove_count = current_count - desired;
                                    tracing::info!("Scaling DOWN {}: {} -> {} (Load: {})", fid, current_count, desired, load);
                                    for _ in 0..remove_count {
                                        if let Some(mut proc) = instances.pop() {
                                            let _ = proc.child.kill();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        println!();
        println!("Shutting down...");
        
        // Kill all
        let mut procs = processes.lock().await;
        for (id, list) in procs.iter_mut() {
            for proc in list.iter_mut() {
                let _ = proc.child.kill();
                let _ = proc.child.wait();
            }
            println!("  Stopped {} instances for {}", list.len(), id);
        }
        
        let _ = std::fs::remove_dir_all(&socket_dir);
        println!("Orchestrator stopped.");
    }

    Ok(())
}

/// Spawn a handler process with Unix socket handshake
async fn spawn_handler_with_socket(
    config: &FunctionConfig,
    socket_dir: &Path,
    instance_id: &str,
) -> Result<(Child, u32), Box<dyn std::error::Error>> {
    let handler_path = config.handler_path.as_path();
    // Unique socket path per instance
    let socket_path = socket_dir.join(format!("{}-{}.sock", config.id, instance_id));

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
    env_vars.insert("AETHER_INSTANCE_ID".to_string(), instance_id.to_string());

    tracing::debug!(
        program = %program,
        handler = %handler_path.display(),
        socket = %socket_path.display(),
        instance = %instance_id,
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
                let _ = child; // drop handle
                return Err(format!("Socket accept error: {}", e).into());
            }
        }
    }

    if !ready_received {
        let mut child = child;
        let _ = child.kill();
        return Err(format!(
            "Handler did not send READY within {}s",
            READY_TIMEOUT.as_secs()
        )
        .into());
    }
    
    // Clean up socket file
    let _ = std::fs::remove_file(&socket_path);

    Ok((child, pid))
}
