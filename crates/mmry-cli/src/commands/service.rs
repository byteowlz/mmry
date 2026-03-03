use anyhow::Result;
use clap::Subcommand;
use mmry_core::config::Config;
use mmry_core::service::manager::ServiceManager;
use mmry_core::service::manager::ServiceStatus;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct ServiceCmd {
    #[command(subcommand)]
    command: ServiceCommands,

    #[arg(long, help = "Check analyzer LLM endpoint connectivity")]
    check_llm: bool,
}

#[derive(Subcommand)]
enum ServiceCommands {
    /// Start service in background
    Start,

    /// Run service in foreground (for debugging)
    Run,

    /// Stop the service
    Stop,

    /// Show service status
    Status,

    /// Enable auto-start on system boot (installs systemd user unit / launchd plist)
    Enable,

    /// Disable auto-start and remove the service unit
    Disable,

    /// Reload the service after config changes (stop then start)
    Reload,

    /// Restart the service (stop then start)
    Restart,
}

pub async fn handle(cmd: ServiceCmd, config_path: Option<PathBuf>) -> Result<()> {
    if cmd.check_llm {
        let config = Config::load_with_path(config_path.clone())?;
        if config.analyzer.enabled {
            if let Some(ref endpoint) = config.analyzer.endpoint {
                println!("Analyzer configured: endpoint={endpoint}");
            } else {
                println!("Analyzer enabled but no endpoint configured");
            }
        } else {
            println!("Analyzer is not enabled in config");
        }
    }

    let manager = ServiceManager::new()?;

    match cmd.command {
        ServiceCommands::Start => {
            println!("Starting mmry service...");
            manager.start(false)?;

            // Wait a bit and check status
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            match manager.status() {
                ServiceStatus::Running { pid } => {
                    println!("Service started successfully (PID: {pid})");
                    if let Ok(port) = manager.read_port() {
                        println!("  Listening on: 127.0.0.1:{port}");
                    }
                }
                _ => {
                    println!("Service failed to start");
                    std::process::exit(1);
                }
            }
        }

        ServiceCommands::Run => {
            println!("Running mmry service in foreground...");
            println!("Press Ctrl+C to stop");
            manager.start(true)?;
        }

        ServiceCommands::Stop => {
            println!("Stopping mmry service...");
            match manager.stop() {
                Ok(()) => {
                    println!("Service stopped");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }

        cmd @ (ServiceCommands::Reload | ServiceCommands::Restart) => {
            let is_reload = matches!(cmd, ServiceCommands::Reload);
            let label = if is_reload { "Reloading" } else { "Restarting" };
            let past = if is_reload { "reloaded" } else { "restarted" };
            println!("{label} mmry service...");
            match manager.stop() {
                Ok(()) => {
                    println!("  Service stopped");
                }
                Err(e) => {
                    eprintln!("Failed to stop service: {e}");
                    std::process::exit(1);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            match manager.start(false) {
                Ok(()) => {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    match manager.status() {
                        ServiceStatus::Running { pid } => {
                            println!("Service {past} (PID: {pid})");
                            if let Ok(port) = manager.read_port() {
                                println!("  Listening on: 127.0.0.1:{port}");
                            }
                        }
                        _ => {
                            eprintln!("Service did not come back up after {past}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to start service after {past}: {e}");
                    std::process::exit(1);
                }
            }
        }

        ServiceCommands::Status => {
            let enabled = manager.is_enabled();

            match manager.status() {
                ServiceStatus::Running { pid } => {
                    println!("Service is running");
                    println!("  PID: {pid}");
                    if let Ok(port) = manager.read_port() {
                        println!("  gRPC port: {port}");
                    }

                    // Show HTTP API port from config if enabled
                    if let Ok(config) = Config::load_with_path(config_path.clone()) {
                        if config.external_api.enabled {
                            println!(
                                "  HTTP port: {} ({}:{})",
                                config.external_api.port,
                                config.external_api.host,
                                config.external_api.port
                            );
                        }
                    }

                    // Try to get health info
                    if let Ok(mut client) = mmry_core::service::client::DaemonClient::new() {
                        if client.ping().await.unwrap_or(false) {
                            println!("  Status: Healthy");
                        }
                    }

                    println!(
                        "  Auto-start: {}",
                        if enabled { "enabled" } else { "disabled" }
                    );
                }
                ServiceStatus::Stopped => {
                    println!("Service is not running");
                    println!(
                        "  Auto-start: {}",
                        if enabled { "enabled" } else { "disabled" }
                    );
                }
                ServiceStatus::Dead => {
                    println!("Service appears to be dead (stale PID file)");
                    println!("Try running 'mmry service stop' to cleanup");
                    println!(
                        "  Auto-start: {}",
                        if enabled { "enabled" } else { "disabled" }
                    );
                }
            }
        }

        ServiceCommands::Enable => {
            match manager.enable() {
                Ok(result) => {
                    if result.wrote_unit {
                        println!("Created service unit: {}", result.unit_path);
                    } else {
                        println!("Using existing service unit: {}", result.unit_path);
                    }
                    println!("Auto-start enabled");

                    // If not currently running, offer to start
                    if !manager.is_running() {
                        println!("Start the service with: mmry service start");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to enable service: {e}");
                    std::process::exit(1);
                }
            }
        }

        ServiceCommands::Disable => match manager.disable() {
            Ok(result) => {
                println!("Removed service unit: {}", result.unit_path);
                println!("Auto-start disabled");
            }
            Err(e) => {
                eprintln!("Failed to disable service: {e}");
                std::process::exit(1);
            }
        },
    }

    Ok(())
}
