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

    /// Enable auto-start on system boot
    Enable,

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
                    println!("✓ Service started successfully (PID: {pid})");
                    if let Ok(port) = manager.read_port() {
                        println!("  Listening on: 127.0.0.1:{port}");
                    }
                }
                _ => {
                    println!("✗ Service failed to start");
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
                    println!("✓ Service stopped");
                }
                Err(e) => {
                    println!("✗ {e}");
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
                    println!("Failed to stop service: {e}");
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
                            println!("Service did not come back up after {past}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to start service after {past}: {e}");
                    std::process::exit(1);
                }
            }
        }

        ServiceCommands::Status => {
            match manager.status() {
                ServiceStatus::Running { pid } => {
                    println!("Service is running");
                    println!("  PID: {pid}");
                    if let Ok(port) = manager.read_port() {
                        println!("  gRPC port: {port}");
                    }

                    // Show HTTP API port from config if enabled
                    if let Ok(config) = Config::load_with_path(config_path.clone()) {
                        if config.external_api.enable {
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
                }
                ServiceStatus::Stopped => {
                    println!("Service is not running");
                }
                ServiceStatus::Dead => {
                    println!("Service appears to be dead (stale PID file)");
                    println!("Try running 'mmry service stop' to cleanup");
                }
            }
        }

        ServiceCommands::Enable => {
            println!("Auto-start configuration:");
            println!();
            println!("To enable auto-start, add this to your config:");
            println!();
            println!("  ~/.config/mmry/config.toml:");
            println!("  [service]");
            println!("  enabled = true");
            println!("  auto_start = true");
            println!();

            #[cfg(target_os = "linux")]
            {
                println!("For systemd (Linux):");
                println!();
                println!("1. Create ~/.config/systemd/user/mmry-service.service:");
                println!("   [Unit]");
                println!("   Description=mmry embedding service");
                println!("   After=network.target");
                println!();
                println!("   [Service]");
                println!("   Type=simple");
                println!(
                    "   ExecStart={}/mmry-service --foreground",
                    std::env::current_exe()?.parent().unwrap().display()
                );
                println!("   Restart=on-failure");
                println!();
                println!("   [Install]");
                println!("   WantedBy=default.target");
                println!();
                println!("2. Enable and start:");
                println!("   systemctl --user enable mmry-service");
                println!("   systemctl --user start mmry-service");
            }

            #[cfg(target_os = "macos")]
            {
                println!("For launchd (macOS):");
                println!();
                println!("1. Create ~/Library/LaunchAgents/com.mmry.service.plist:");
                println!("   <?xml version=\"1.0\" encoding=\"UTF-8\"?>");
                println!("   <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\"");
                println!("     \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">");
                println!("   <plist version=\"1.0\">");
                println!("   <dict>");
                println!("     <key>Label</key>");
                println!("     <string>com.mmry.service</string>");
                println!("     <key>ProgramArguments</key>");
                println!("     <array>");
                println!(
                    "       <string>{}/mmry-service</string>",
                    std::env::current_exe()?.parent().unwrap().display()
                );
                println!("       <string>--foreground</string>");
                println!("     </array>");
                println!("     <key>RunAtLoad</key>");
                println!("     <true/>");
                println!("     <key>KeepAlive</key>");
                println!("     <true/>");
                println!("   </dict>");
                println!("   </plist>");
                println!();
                println!("2. Load and start:");
                println!("   launchctl load ~/Library/LaunchAgents/com.mmry.service.plist");
            }

            #[cfg(target_os = "windows")]
            {
                println!("For Windows:");
                println!();
                println!("1. Add to startup via Task Scheduler:");
                println!("   - Open Task Scheduler");
                println!("   - Create Basic Task");
                println!("   - Trigger: At log on");
                println!("   - Action: Start a program");
                println!(
                    "   - Program: {}\\mmry-service.exe",
                    std::env::current_exe()?.parent().unwrap().display()
                );
                println!("   - Arguments: --foreground");
                println!();
                println!("Or use the Windows Service wrapper (advanced).");
            }
        }
    }

    Ok(())
}
