use clap::Args;
use mmry_core::config::Config;

#[derive(Args, Clone)]
pub struct BenchCmd {
    /// Emit JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn handle(_cmd: BenchCmd, _config: &Config) -> anyhow::Result<()> {
    anyhow::bail!("Benchmarks are not yet implemented for the new learnings-based architecture");
}
