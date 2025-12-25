use clap::Args;
use mmry_core::config::Config;
use mmry_core::stores::export_all_stores_to_json_with_options;
use mmry_core::stores::export_store_to_json_with_options;
use mmry_core::stores::write_export_to_file;
use std::path::PathBuf;

#[derive(Args)]
pub struct ExportCmd {
    /// Output file path (defaults to mmry_export_<timestamp>.json)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Export all stores (instead of just the current store)
    #[arg(short, long)]
    all: bool,

    /// Exclude HMLR enrichment data (facts, bridge blocks, entities, relationships)
    #[arg(long)]
    no_hmlr: bool,
}

pub async fn handle(
    cmd: ExportCmd,
    config: &Config,
    store_name: Option<&str>,
) -> anyhow::Result<()> {
    let include_hmlr = !cmd.no_hmlr;

    let result = if cmd.all {
        if include_hmlr {
            println!("Exporting memories and HMLR data from all stores...");
        } else {
            println!("Exporting memories from all stores...");
        }
        export_all_stores_to_json_with_options(config, include_hmlr).await?
    } else {
        let store = store_name.unwrap_or(&config.stores.default);
        if include_hmlr {
            println!("Exporting memories and HMLR data from store '{store}'...");
        } else {
            println!("Exporting memories from store '{store}'...");
        }
        export_store_to_json_with_options(config, store, include_hmlr).await?
    };

    let output_path = cmd.output.unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        if cmd.all {
            PathBuf::from(format!("mmry_export_all_{timestamp}.json"))
        } else {
            let store = store_name.unwrap_or(&config.stores.default);
            PathBuf::from(format!("mmry_export_{store}_{timestamp}.json"))
        }
    });

    write_export_to_file(&result, &output_path)?;

    println!();
    let mut summary = format!("Exported {} memories", result.memory_count);
    if let Some(ref hmlr) = result.hmlr {
        summary.push_str(&format!(
            ", {} facts, {} bridge blocks, {} entities, {} relationships",
            hmlr.facts.len(),
            hmlr.bridge_blocks.len(),
            hmlr.entities.len(),
            hmlr.relationships.len()
        ));
    }
    println!("{summary} to {}", output_path.display());

    Ok(())
}
