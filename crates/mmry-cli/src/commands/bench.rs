use clap::Args;
use mmry_core::config::Config;
use mmry_core::config::SearchMode;
use mmry_core::hmlr::benchmarks::run_system_benchmarks;
use mmry_core::hmlr::benchmarks::BenchmarkSummary;
use mmry_core::hmlr::benchmarks::SystemBenchmarkOptions;
use serde::Serialize;
use sysinfo::System;

#[derive(Args, Clone)]
pub struct BenchCmd {
    /// Random seed for deterministic scenarios
    #[arg(long, default_value_t = 0)]
    pub seed: u64,

    /// Number of documents to ingest for the perf run
    #[arg(long, default_value_t = 200)]
    pub ingest_docs: usize,

    /// Approximate token count per benchmark document
    #[arg(long, default_value_t = 800)]
    pub ingest_doc_tokens: usize,

    /// Number of documents to seed for usage benchmarks
    #[arg(long, default_value_t = 400)]
    pub usage_docs: usize,

    /// Number of search queries to execute
    #[arg(long, default_value_t = 50)]
    pub usage_queries: usize,

    /// Number of context packs to build
    #[arg(long, default_value_t = 25)]
    pub usage_context_packs: usize,

    /// Emit JSON output
    #[arg(long)]
    pub json: bool,

    /// Remove existing benchmark stores before running
    #[arg(long)]
    pub reset: bool,

    /// Skip confirmation prompts (use with caution)
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Serialize)]
struct BenchReport {
    version: String,
    seed: u64,
    generated_at: chrono::DateTime<chrono::Utc>,
    config: BenchConfigSummary,
    hardware: BenchHardware,
    summary: BenchmarkSummary,
}

#[derive(Debug, Serialize)]
struct BenchConfigSummary {
    stores_directory: String,
    search_mode: SearchMode,
    rerank_enabled: bool,
    embeddings_model: String,
    embeddings_backend: String,
    embeddings_dimension: usize,
    embeddings_batch_size: usize,
    sparse_enabled: bool,
    ingest_docs: usize,
    ingest_doc_tokens: usize,
    usage_docs: usize,
    usage_queries: usize,
    usage_context_packs: usize,
}

#[derive(Debug, Serialize)]
struct BenchHardware {
    os: String,
    arch: String,
    cpu_brand: Option<String>,
    cpu_cores: usize,
    memory_total_mb: u64,
}

pub async fn handle(cmd: BenchCmd, config: &Config) -> anyhow::Result<()> {
    if !config.embeddings.enabled {
        anyhow::bail!("Embeddings are disabled. Enable them in the benchmark config.");
    }

    if config.embeddings.model.trim().is_empty() {
        anyhow::bail!("Embeddings model is empty. Set embeddings.model in the benchmark config.");
    }

    if cmd.reset {
        reset_bench_stores(config, cmd.yes)?;
    }

    let opts = SystemBenchmarkOptions {
        seed: cmd.seed,
        retrieval_k: 5,
        search_mode: config.search.mode,
        rerank: config.search.rerank_enabled,
        include_perf: true,
        ingest_docs: cmd.ingest_docs,
        ingest_doc_tokens: cmd.ingest_doc_tokens,
        usage_docs: cmd.usage_docs,
        usage_queries: cmd.usage_queries,
        usage_context_packs: cmd.usage_context_packs,
    };

    let summary = run_system_benchmarks(config, opts).await?;
    let report = BenchReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        seed: cmd.seed,
        generated_at: chrono::Utc::now(),
        config: BenchConfigSummary {
            stores_directory: config.stores.directory.display().to_string(),
            search_mode: config.search.mode,
            rerank_enabled: config.search.rerank_enabled,
            embeddings_model: config.embeddings.model.clone(),
            embeddings_backend: config.embeddings.backend.clone(),
            embeddings_dimension: config.embeddings.dimension,
            embeddings_batch_size: config.embeddings.batch_size,
            sparse_enabled: config.sparse_embeddings.enabled,
            ingest_docs: cmd.ingest_docs,
            ingest_doc_tokens: cmd.ingest_doc_tokens,
            usage_docs: cmd.usage_docs,
            usage_queries: cmd.usage_queries,
            usage_context_packs: cmd.usage_context_packs,
        },
        hardware: current_hardware(),
        summary,
    };

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report.summary);
        println!(
            "\nHardware: {} {} ({} cores, {} MB RAM)",
            report.hardware.os,
            report.hardware.arch,
            report.hardware.cpu_cores,
            report.hardware.memory_total_mb
        );
        if let Some(brand) = report.hardware.cpu_brand.as_deref() {
            println!("CPU: {brand}");
        }
        println!(
            "Embeddings: {} ({}, dim {}, batch {})",
            report.config.embeddings_model,
            report.config.embeddings_backend,
            report.config.embeddings_dimension,
            report.config.embeddings_batch_size
        );
        println!("Stores: {}", report.config.stores_directory);
    }

    Ok(())
}

fn reset_bench_stores(config: &Config, yes: bool) -> anyhow::Result<()> {
    let dir = &config.stores.directory;
    if !dir.exists() {
        return Ok(());
    }

    let mut targets = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("bench-") || !file_name.ends_with(".db") {
            continue;
        }
        targets.push(path);
    }

    if targets.is_empty() {
        return Ok(());
    }

    if !yes {
        println!(
            "About to delete {} benchmark store(s) in {}. Continue? [y/N]",
            targets.len(),
            dir.display()
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    for path in targets {
        remove_store_files(&path)?;
    }

    Ok(())
}

fn remove_store_files(path: &std::path::Path) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let path_str = path.to_string_lossy();
    let wal_path = std::path::PathBuf::from(format!("{path_str}-wal"));
    let shm_path = std::path::PathBuf::from(format!("{path_str}-shm"));
    if wal_path.exists() {
        std::fs::remove_file(wal_path)?;
    }
    if shm_path.exists() {
        std::fs::remove_file(shm_path)?;
    }
    Ok(())
}

fn current_hardware() -> BenchHardware {
    let mut system = System::new_all();
    system.refresh_all();

    let cpu_brand = system.cpus().first().map(|cpu| cpu.brand().to_string());
    let cpu_cores = system.cpus().len();
    let memory_total_mb = system.total_memory() / 1024;

    BenchHardware {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_brand,
        cpu_cores,
        memory_total_mb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reset_bench_stores_removes_bench_files_only() -> anyhow::Result<()> {
        let temp = tempdir()?;
        let bench_db = temp.path().join("bench-test.db");
        let bench_wal = temp.path().join("bench-test.db-wal");
        let bench_shm = temp.path().join("bench-test.db-shm");
        let other_db = temp.path().join("other.db");

        std::fs::write(&bench_db, "bench")?;
        std::fs::write(&bench_wal, "bench")?;
        std::fs::write(&bench_shm, "bench")?;
        std::fs::write(&other_db, "other")?;

        let mut config = Config::default();
        config.stores.directory = temp.path().to_path_buf();

        reset_bench_stores(&config, true)?;

        assert!(!bench_db.exists());
        assert!(!bench_wal.exists());
        assert!(!bench_shm.exists());
        assert!(other_db.exists());

        Ok(())
    }
}
