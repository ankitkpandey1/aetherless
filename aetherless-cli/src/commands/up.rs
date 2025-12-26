//! `aether up` command - Start the orchestrator.
//!
//! Spawns handler processes, creates Unix sockets, and waits for READY signals.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use aetherless_core::{ConfigLoader, FunctionConfig, FunctionRegistry, FunctionState};

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

    // Create socket directory
    let socket_dir = PathBuf::from("/tmp/aetherless");
    if socket_dir.exists() {
        std::fs::remove_dir_all(&socket_dir)?;
    }
    std::fs::create_dir_all(&socket_dir)?;

    // Track running processes
    let processes: Arc<Mutex<HashMap<String, RunningProcess>>> =
        Arc::new(Mutex::new(HashMap::new()));

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              AETHERLESS ORCHESTRATOR                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Spawn all function handlers
    for func_config in &config.functions {
        println!("▶ Spawning function: {}", func_config.id);
        registry.register(func_config.clone())?;

        // Spawn the handler process with Unix socket handshake
        match spawn_handler_with_socket(func_config, &socket_dir).await {
            Ok((child, pid)) => {
                println!(
                    "  ✓ {} started (PID: {}, Port: {})",
                    func_config.id, pid, func_config.trigger_port
                );

                // Update state to Running
                registry.transition(&func_config.id, FunctionState::Running)?;

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
    println!("║ Status: {} functions running                                 ║", running_count);
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
