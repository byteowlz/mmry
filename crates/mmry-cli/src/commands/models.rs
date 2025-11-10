use clap::Parser;
use mmry_core::embeddings;

#[derive(Parser)]
pub struct ModelsCmd {
    #[arg(long, help = "Show all models including quantized variants")]
    all: bool,
}

pub async fn handle(cmd: ModelsCmd) -> anyhow::Result<()> {
    println!("Available embedding models:\n");

    let models = embeddings::list_models();

    for model in &models {
        if !cmd.all && model.code.ends_with("-q") {
            continue;
        }

        println!("✓ {}", model.code);
        println!(
            "   Dimensions: {} | {}",
            model.dimensions, model.description
        );
        println!();
    }

    if !cmd.all {
        println!("Use --all to show quantized model variants");
    }

    println!("\nTo use a model, set it in ~/.config/mmry/config.toml:");
    println!("[embeddings]");
    println!("model = \"<model-code>\"");

    Ok(())
}
