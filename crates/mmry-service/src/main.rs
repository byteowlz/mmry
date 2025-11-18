mod server;
mod state;

use anyhow::Result;
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mmry_service=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse arguments
    let args: Vec<String> = std::env::args().collect();
    let foreground = args.contains(&"--foreground".to_string());

    // Get state directory
    let state_dir = get_state_dir()?;
    std::fs::create_dir_all(&state_dir)?;

    let pid_file = state_dir.join("service.pid");
    let port_file = state_dir.join("service.port");

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&pid_file, pid.to_string())?;

    tracing::info!("Starting mmry service (PID: {})", pid);

    // Load configuration
    let config = mmry_core::config::Config::load()?;

    // Start server
    match server::run_server(config, port_file, foreground).await {
        Ok(()) => {
            tracing::info!("Service stopped gracefully");
            std::fs::remove_file(&pid_file).ok();
            Ok(())
        }
        Err(e) => {
            tracing::error!("Service error: {}", e);
            std::fs::remove_file(&pid_file).ok();
            Err(e)
        }
    }
}

fn get_state_dir() -> Result<PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?;

    Ok(base.join("mmry"))
}
