use clap::Args;
use mmry_core::config::Config;
use mmry_core::stores::export_store_to_json;
use mmry_core::stores::write_export_to_file;
use std::path::PathBuf;

#[derive(Args)]
pub struct ExportCmd {
    /// Output file path (defaults to mmry_export_<timestamp>.json)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub async fn handle(
    cmd: ExportCmd,
    config: &Config,
    store_name: Option<&str>,
) -> anyhow::Result<()> {
    let store = store_name.unwrap_or(&config.stores.default);
    println!("Exporting memories from store '{store}'...");

    let result = export_store_to_json(config, store).await?;

    let output_path = cmd.output.unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("mmry_export_{store}_{timestamp}.json"))
    });

    write_export_to_file(&result, &output_path)?;

    println!();
    println!("Exported {} memories to {}", result.memory_count, output_path.display());

    Ok(())
}
