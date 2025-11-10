use clap::Parser;
use mmry_core::reranker;

#[derive(Parser)]
pub struct RerankersCmd {}

pub async fn handle(_cmd: RerankersCmd) -> anyhow::Result<()> {
    println!("Available reranker models:\n");

    let models = reranker::list_reranker_models();

    for model in &models {
        println!("✓ {}", model.code);
        println!("   {}", model.description);
        println!();
    }

    println!("\nTo use a reranker, set it in ~/.config/mmry/config.toml:");
    println!("[search]");
    println!("rerank_enabled = true");
    println!("rerank_model = \"<model-code>\"");

    Ok(())
}
