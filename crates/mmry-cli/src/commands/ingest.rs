use clap::Parser;
use clap::Subcommand;
use std::io::Read;
use std::io::{self};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use mmry_core::chunking::Chunker;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use mmry_core::embeddings::EmbeddingServiceWrapper;
use mmry_core::memory::Memory;
use mmry_core::memory::MemoryType;
use mmry_core::sparse_embeddings::SparseEmbeddingService;

use notify::Event;
use notify::EventKind;
use notify::RecursiveMode;
use notify::Watcher;
use serde::Deserialize;
use serde::Serialize;
use std::sync::mpsc;
use tokio::process::Command;
use walkdir::WalkDir;

/// Helper to insert a key-value pair into a serde_json::Value (assumed to be an Object)
fn set_metadata(metadata: &mut serde_json::Value, key: &str, value: impl Into<serde_json::Value>) {
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert(key.to_string(), value.into());
    }
}

#[derive(Parser)]
#[command(about = "Ingest files and directories into memories")]
pub struct IngestCmd {
    #[command(subcommand)]
    pub command: IngestCommand,
}

#[derive(Subcommand)]
pub enum IngestCommand {
    /// Ingest a single file or directory
    File(FileIngestOpts),

    /// Watch a directory for changes and ingest new/modified files
    Watch(WatchOpts),

    /// Read content from stdin and create a memory
    Stdin(StdinOpts),
}

#[derive(Parser)]
pub struct FileIngestOpts {
    /// Path to file or directory to ingest
    pub path: PathBuf,

    /// Recursively process directories
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Category for ingested memories
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Tags for ingested memories (comma-separated)
    #[arg(long, short = 't')]
    pub tags: Option<String>,

    /// Memory type (episodic, semantic, procedural)
    #[arg(long = "memory-type", short = 'm')]
    pub memory_type: Option<String>,

    /// File extensions to process (comma-separated, e.g., "md,txt")
    #[arg(long, default_value = "md")]
    pub extensions: String,

    /// Skip files that have already been ingested (based on source_path metadata)
    #[arg(long)]
    pub skip_existing: bool,

    /// Output result as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Dry run - show what would be ingested without actually doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Disable chunking (process entire file as single memory)
    #[arg(long)]
    pub no_chunk: bool,

    /// Maximum tokens per chunk (overrides config)
    #[arg(long)]
    pub max_chunk_tokens: Option<usize>,
}

#[derive(Parser)]
pub struct WatchOpts {
    /// Directory to watch for changes
    pub path: PathBuf,

    /// Category for ingested memories
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Tags for ingested memories (comma-separated)
    #[arg(long, short = 't')]
    pub tags: Option<String>,

    /// Memory type (episodic, semantic, procedural)
    #[arg(long = "memory-type", short = 'm')]
    pub memory_type: Option<String>,

    /// File extensions to process (comma-separated, e.g., "md,txt")
    #[arg(long, default_value = "md")]
    pub extensions: String,

    /// Debounce time in milliseconds
    #[arg(long, default_value_t = 500)]
    pub debounce_ms: u64,

    /// Process existing files on startup
    #[arg(long)]
    pub process_existing: bool,

    /// Output events as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Disable chunking (process entire file as single memory)
    #[arg(long)]
    pub no_chunk: bool,

    /// Maximum tokens per chunk (overrides config)
    #[arg(long)]
    pub max_chunk_tokens: Option<usize>,
}

#[derive(Parser)]
pub struct StdinOpts {
    /// Category for the memory
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Tags for the memory (comma-separated)
    #[arg(long, short = 't')]
    pub tags: Option<String>,

    /// Memory type (episodic, semantic, procedural)
    #[arg(long = "memory-type", short = 'm')]
    pub memory_type: Option<String>,

    /// Title/source identifier for the memory
    #[arg(long)]
    pub title: Option<String>,

    /// Output result as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Disable chunking (process entire input as single memory)
    #[arg(long)]
    pub no_chunk: bool,

    /// Maximum tokens per chunk (overrides config)
    #[arg(long)]
    pub max_chunk_tokens: Option<usize>,
}

/// Frontmatter parsed from ingestr-generated markdown files
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct IngestrFrontmatter {
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub source_modified: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub converted_at: Option<String>,
}

/// Result of ingesting a file
#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub path: String,
    pub memory_id: String,
    pub title: Option<String>,
    pub source_path: Option<String>,
    pub content_preview: String,
    pub chunks: usize,
    pub total_tokens_approx: usize,
}

pub async fn handle(
    cmd: IngestCmd,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    match cmd.command {
        IngestCommand::File(opts) => {
            handle_file_ingest(opts, config, db, embeddings, sparse_embeddings).await
        }
        IngestCommand::Watch(opts) => {
            handle_watch(opts, config, db, embeddings, sparse_embeddings).await
        }
        IngestCommand::Stdin(opts) => {
            handle_stdin(opts, config, db, embeddings, sparse_embeddings).await
        }
    }
}

/// Create a chunker with optional overrides
fn create_chunker(config: &Config, no_chunk: bool, max_tokens: Option<usize>) -> Chunker {
    let mut chunking_config = config.chunking.clone();

    if no_chunk {
        chunking_config.enabled = false;
    }

    if let Some(max) = max_tokens {
        chunking_config.max_chunk_tokens = max;
    }

    Chunker::new(chunking_config)
}

/// Estimate token count (rough approximation)
fn estimate_tokens(text: &str) -> usize {
    let word_count = text.split_whitespace().count();
    (word_count as f32 * 1.3).ceil() as usize
}

async fn handle_file_ingest(
    opts: FileIngestOpts,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    let ctx = IngestContext {
        config,
        db,
        embeddings: &embeddings,
        sparse_embeddings: &sparse_embeddings,
    };

    let extensions: Vec<String> = opts
        .extensions
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();

    let mut ingest_root = opts.path.clone();
    let mut recursive = opts.recursive;
    let mut extensions = extensions;
    let mut _ingestr_output_dir = None;

    if config.ingest.ingestr_enabled && ingest_root.is_dir() && !opts.dry_run {
        let output_dir = IngestrOutputDir::new(config, &ingest_root)?;
        run_ingestr_once(config, &ingest_root, output_dir.path()).await?;

        ingest_root = output_dir.path().to_path_buf();
        recursive = true;
        extensions = vec!["md".to_string()];
        _ingestr_output_dir = Some(output_dir);
    }

    let paths = collect_files(&ingest_root, recursive, &extensions)?;

    if paths.is_empty() {
        if opts.json {
            println!("[]");
        } else {
            println!("No files found matching criteria");
        }
        return Ok(());
    }

    if opts.dry_run {
        if opts.json {
            let items: Vec<_> = paths
                .iter()
                .map(|p| {
                    let content = std::fs::read_to_string(p).unwrap_or_default();
                    let tokens = estimate_tokens(&content);
                    serde_json::json!({
                        "path": p.display().to_string(),
                        "estimated_tokens": tokens,
                        "would_chunk": tokens > config.chunking.max_chunk_tokens && !opts.no_chunk
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        } else {
            println!("Would ingest {} files:", paths.len());
            for path in &paths {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                let tokens = estimate_tokens(&content);
                let chunk_info = if tokens > config.chunking.max_chunk_tokens && !opts.no_chunk {
                    format!(" (will chunk, ~{tokens} tokens)")
                } else {
                    format!(" (~{tokens} tokens)")
                };
                println!("  - {}{}", path.display(), chunk_info);
            }
        }
        return Ok(());
    }

    let mut results = Vec::new();

    for path in paths {
        match ingest_file(&path, &opts, &ctx).await {
            Ok(result) => {
                if !opts.json {
                    let chunk_info = if result.chunks > 1 {
                        format!(" ({} chunks)", result.chunks)
                    } else {
                        String::new()
                    };
                    println!(
                        "+ Ingested: {} -> {}{}",
                        path.display(),
                        result.memory_id,
                        chunk_info
                    );
                }
                results.push(result);
            }
            Err(e) => {
                if opts.json {
                    eprintln!(
                        "{}",
                        serde_json::json!({"error": e.to_string(), "path": path.display().to_string()})
                    );
                } else {
                    eprintln!("! Failed to ingest {}: {e}", path.display());
                }
            }
        }
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        let total_chunks: usize = results.iter().map(|r| r.chunks).sum();
        println!(
            "\nIngested {} files ({} total memories/chunks)",
            results.len(),
            total_chunks
        );
    }

    Ok(())
}

async fn handle_watch(
    opts: WatchOpts,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    let ctx = IngestContext {
        config,
        db,
        embeddings: &embeddings,
        sparse_embeddings: &sparse_embeddings,
    };

    let mut watch_root = opts.path.clone();
    let mut extensions: Vec<String> = opts
        .extensions
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();

    if !opts.path.is_dir() {
        anyhow::bail!("Watch path must be a directory: {}", opts.path.display());
    }

    let mut _ingestr_output_dir = None;
    let mut _ingestr_child = None;

    if config.ingest.ingestr_enabled {
        let output_dir = IngestrOutputDir::new(config, &watch_root)?;
        run_ingestr_once(config, &watch_root, output_dir.path()).await?;

        let mut cmd = Command::new(&config.ingest.ingestr_bin);
        cmd.arg("service")
            .arg("run")
            .arg("--watch-dir")
            .arg(&watch_root)
            .arg("--output-dir")
            .arg(output_dir.path())
            .arg("--disable-index")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        _ingestr_child = Some(cmd.spawn()?);

        watch_root = output_dir.path().to_path_buf();
        extensions = vec!["md".to_string()];
        _ingestr_output_dir = Some(output_dir);
    }

    // Process existing files if requested
    if opts.process_existing {
        let paths = collect_files(&watch_root, true, &extensions)?;
        if !opts.json {
            println!("Processing {} existing files...", paths.len());
        }

        for path in paths {
            let file_opts = FileIngestOpts {
                path: path.clone(),
                recursive: false,
                category: opts.category.clone(),
                tags: opts.tags.clone(),
                memory_type: opts.memory_type.clone(),
                extensions: opts.extensions.clone(),
                skip_existing: false,
                json: opts.json,
                dry_run: false,
                no_chunk: opts.no_chunk,
                max_chunk_tokens: opts.max_chunk_tokens,
            };

            match ingest_file(&path, &file_opts, &ctx).await {
                Ok(result) => {
                    if opts.json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "event": "ingested",
                                "path": path.display().to_string(),
                                "memory_id": result.memory_id,
                                "chunks": result.chunks
                            })
                        );
                    } else {
                        let chunk_info = if result.chunks > 1 {
                            format!(" ({} chunks)", result.chunks)
                        } else {
                            String::new()
                        };
                        println!(
                            "+ Ingested: {} -> {}{}",
                            path.display(),
                            result.memory_id,
                            chunk_info
                        );
                    }
                }
                Err(e) => {
                    if opts.json {
                        eprintln!(
                            "{}",
                            serde_json::json!({"event": "error", "path": path.display().to_string(), "error": e.to_string()})
                        );
                    } else {
                        eprintln!("! Failed: {}: {e}", path.display());
                    }
                }
            }
        }
    }

    if !opts.json {
        println!(
            "Watching {} for changes (press Ctrl+C to stop)...",
            watch_root.display()
        );
    }

    let (tx, rx) = mpsc::channel();
    let debounce = Duration::from_millis(opts.debounce_ms);

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        let _ = tx.send(res);
    })?;

    watcher.watch(&watch_root, RecursiveMode::Recursive)?;

    loop {
        match rx.recv_timeout(debounce) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    if !path.is_file() {
                        continue;
                    }

                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();

                    if !extensions.contains(&ext) {
                        continue;
                    }

                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            let file_opts = FileIngestOpts {
                                path: path.clone(),
                                recursive: false,
                                category: opts.category.clone(),
                                tags: opts.tags.clone(),
                                memory_type: opts.memory_type.clone(),
                                extensions: opts.extensions.clone(),
                                skip_existing: false,
                                json: opts.json,
                                dry_run: false,
                                no_chunk: opts.no_chunk,
                                max_chunk_tokens: opts.max_chunk_tokens,
                            };

                            match futures::executor::block_on(ingest_file(&path, &file_opts, &ctx))
                            {
                                Ok(result) => {
                                    if opts.json {
                                        println!(
                                            "{}",
                                            serde_json::json!({
                                                "event": "ingested",
                                                "path": path.display().to_string(),
                                                "memory_id": result.memory_id,
                                                "chunks": result.chunks
                                            })
                                        );
                                    } else {
                                        let chunk_info = if result.chunks > 1 {
                                            format!(" ({} chunks)", result.chunks)
                                        } else {
                                            String::new()
                                        };
                                        println!(
                                            "+ Ingested: {} -> {}{}",
                                            path.display(),
                                            result.memory_id,
                                            chunk_info
                                        );
                                    }
                                }
                                Err(e) => {
                                    if opts.json {
                                        eprintln!(
                                            "{}",
                                            serde_json::json!({"event": "error", "path": path.display().to_string(), "error": e.to_string()})
                                        );
                                    } else {
                                        eprintln!("! Failed: {}: {e}", path.display());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Err(e)) => {
                if opts.json {
                    eprintln!(
                        "{}",
                        serde_json::json!({"event": "watch_error", "error": e.to_string()})
                    );
                } else {
                    eprintln!("Watch error: {e}");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

async fn handle_stdin(
    opts: StdinOpts,
    config: &Config,
    db: &Database,
    embeddings: Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: Arc<SparseEmbeddingService>,
) -> anyhow::Result<()> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;

    if buffer.trim().is_empty() {
        anyhow::bail!("No content provided via stdin");
    }

    // Parse frontmatter if present
    let (frontmatter, content) = parse_frontmatter(&buffer);

    let title = opts
        .title
        .or_else(|| frontmatter.as_ref().and_then(|fm| fm.title.clone()));
    let source_path = frontmatter.as_ref().and_then(|fm| fm.source_path.clone());

    let memory_type = parse_memory_type(opts.memory_type.as_deref(), content);
    let category = opts
        .category
        .unwrap_or_else(|| config.memory.default_category.clone());

    let tags: Vec<String> = opts
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Create chunker with options
    let chunker = create_chunker(config, opts.no_chunk, opts.max_chunk_tokens);
    let total_tokens = estimate_tokens(content);

    if chunker.needs_chunking(content) {
        // Content needs chunking
        let text_chunks = chunker.chunk_text(content)?;
        let total_chunks = text_chunks.len();

        if !opts.json {
            println!(
                "Content is large (~{total_tokens} tokens), chunking into {total_chunks} pieces"
            );
        }

        // Create parent memory
        let mut parent = Memory::new(memory_type.clone(), content.to_string(), category.clone());
        parent.tags = tags.clone();
        parent.total_chunks = Some(total_chunks as i32);

        // Store source metadata on parent
        if let Some(ref source) = source_path {
            set_metadata(&mut parent.metadata, "source_path", source.clone());
        }
        if let Some(ref t) = title {
            set_metadata(&mut parent.metadata, "title", t.clone());
        }

        // Create chunk memories
        let mut chunk_memories = chunker.create_memory_chunks(&parent, text_chunks);

        // Embed and insert chunks
        for chunk in &mut chunk_memories {
            let embed_text = if config.chunking.embed_metadata {
                let metadata_text = chunker.generate_metadata_text(chunk);
                if !metadata_text.is_empty() {
                    format!("{}\n\n{}", metadata_text, chunk.content)
                } else {
                    chunk.content.clone()
                }
            } else {
                chunk.content.clone()
            };

            {
                let mut emb = embeddings.lock().await;
                if emb.is_enabled() {
                    if let Some(vector) = emb.embed(&embed_text).await? {
                        chunk.embedding = Some(vector);
                    }
                }
            }

            if sparse_embeddings.is_enabled() {
                if let Some(sparse_vec) = sparse_embeddings.embed(&embed_text).await? {
                    chunk.sparse_embedding = Some(sparse_vec.into());
                }
            }

            operations::insert_memory(db.pool(), chunk).await?;
        }

        // Insert parent (without embedding)
        operations::insert_memory(db.pool(), &parent).await?;

        if opts.json {
            let result = IngestResult {
                path: "stdin".to_string(),
                memory_id: parent.id.to_string(),
                title,
                source_path,
                content_preview: parent.content.chars().take(100).collect(),
                chunks: total_chunks,
                total_tokens_approx: total_tokens,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "+ Added chunked memory: {} ({} chunks)",
                parent.id, total_chunks
            );
            if let Some(t) = title {
                println!("  Title: {t}");
            }
        }
    } else {
        // Single memory, no chunking needed
        let mut memory = Memory::new(memory_type, content.to_string(), category);
        memory.tags = tags;

        if let Some(ref source) = source_path {
            set_metadata(&mut memory.metadata, "source_path", source.clone());
        }
        if let Some(ref t) = title {
            set_metadata(&mut memory.metadata, "title", t.clone());
        }

        // Generate embeddings
        {
            let mut emb = embeddings.lock().await;
            if emb.is_enabled() {
                if let Some(vector) = emb.embed(&memory.content).await? {
                    memory.embedding = Some(vector);
                }
            }
        }

        if sparse_embeddings.is_enabled() {
            if let Some(sparse_vec) = sparse_embeddings.embed(&memory.content).await? {
                memory.sparse_embedding = Some(sparse_vec.into());
            }
        }

        operations::insert_memory(db.pool(), &memory).await?;

        // HMLR enrichment

        if opts.json {
            let result = IngestResult {
                path: "stdin".to_string(),
                memory_id: memory.id.to_string(),
                title,
                source_path,
                content_preview: memory.content.chars().take(100).collect(),
                chunks: 1,
                total_tokens_approx: total_tokens,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("+ Added memory: {}", memory.id);
            if let Some(t) = title {
                println!("  Title: {t}");
            }
            println!(
                "  Content: {}...",
                memory.content.chars().take(100).collect::<String>()
            );
        }
    }

    Ok(())
}

struct IngestContext<'a> {
    config: &'a Config,
    db: &'a Database,
    embeddings: &'a Arc<tokio::sync::Mutex<EmbeddingServiceWrapper>>,
    sparse_embeddings: &'a Arc<SparseEmbeddingService>,
}

async fn ingest_file(
    path: &PathBuf,
    opts: &FileIngestOpts,
    ctx: &IngestContext<'_>,
) -> anyhow::Result<IngestResult> {
    let config = ctx.config;
    let db = ctx.db;
    let embeddings = ctx.embeddings;
    let sparse_embeddings = ctx.sparse_embeddings;

    let content = std::fs::read_to_string(path)?;

    if content.trim().is_empty() {
        anyhow::bail!("File is empty");
    }

    // Parse frontmatter if present
    let (frontmatter, body) = parse_frontmatter(&content);

    let title = frontmatter.as_ref().and_then(|fm| fm.title.clone());
    let source_path = frontmatter
        .as_ref()
        .and_then(|fm| fm.source_path.clone())
        .or_else(|| Some(path.display().to_string()));

    let memory_type = parse_memory_type(opts.memory_type.as_deref(), body);
    let category = opts
        .category
        .clone()
        .unwrap_or_else(|| config.memory.default_category.clone());

    let tags: Vec<String> = opts
        .tags
        .as_ref()
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Create chunker with options
    let chunker = create_chunker(config, opts.no_chunk, opts.max_chunk_tokens);
    let total_tokens = estimate_tokens(body);

    if chunker.needs_chunking(body) {
        // Content needs chunking
        let text_chunks = chunker.chunk_text(body)?;
        let total_chunks = text_chunks.len();

        // Create parent memory
        let mut parent = Memory::new(memory_type.clone(), body.to_string(), category.clone());
        parent.tags = tags.clone();
        parent.total_chunks = Some(total_chunks as i32);

        // Store source metadata on parent
        if let Some(ref source) = source_path {
            set_metadata(&mut parent.metadata, "source_path", source.clone());
        }
        if let Some(ref t) = title {
            set_metadata(&mut parent.metadata, "title", t.clone());
        }
        set_metadata(
            &mut parent.metadata,
            "ingested_from",
            path.display().to_string(),
        );

        if let Some(ref fm) = frontmatter {
            if let Some(ref converted_at) = fm.converted_at {
                set_metadata(&mut parent.metadata, "converted_at", converted_at.clone());
            }
            if let Some(ref output_path) = fm.output_path {
                set_metadata(&mut parent.metadata, "markdown_path", output_path.clone());
            }
        }

        // Create chunk memories
        let mut chunk_memories = chunker.create_memory_chunks(&parent, text_chunks);

        // Embed and insert chunks
        for chunk in &mut chunk_memories {
            let embed_text = if config.chunking.embed_metadata {
                let metadata_text = chunker.generate_metadata_text(chunk);
                if !metadata_text.is_empty() {
                    format!("{}\n\n{}", metadata_text, chunk.content)
                } else {
                    chunk.content.clone()
                }
            } else {
                chunk.content.clone()
            };

            {
                let mut emb = embeddings.lock().await;
                if emb.is_enabled() {
                    if let Some(vector) = emb.embed(&embed_text).await? {
                        chunk.embedding = Some(vector);
                    }
                }
            }

            if sparse_embeddings.is_enabled() {
                if let Some(sparse_vec) = sparse_embeddings.embed(&embed_text).await? {
                    chunk.sparse_embedding = Some(sparse_vec.into());
                }
            }

            operations::insert_memory(db.pool(), chunk).await?;
        }

        // Insert parent (without embedding)
        operations::insert_memory(db.pool(), &parent).await?;

        Ok(IngestResult {
            path: path.display().to_string(),
            memory_id: parent.id.to_string(),
            title,
            source_path,
            content_preview: parent.content.chars().take(100).collect(),
            chunks: total_chunks,
            total_tokens_approx: total_tokens,
        })
    } else {
        // Single memory, no chunking needed
        let mut memory = Memory::new(memory_type, body.to_string(), category);
        memory.tags = tags;

        if let Some(ref source) = source_path {
            set_metadata(&mut memory.metadata, "source_path", source.clone());
        }
        if let Some(ref t) = title {
            set_metadata(&mut memory.metadata, "title", t.clone());
        }
        set_metadata(
            &mut memory.metadata,
            "ingested_from",
            path.display().to_string(),
        );

        if let Some(ref fm) = frontmatter {
            if let Some(ref converted_at) = fm.converted_at {
                set_metadata(&mut memory.metadata, "converted_at", converted_at.clone());
            }
            if let Some(ref output_path) = fm.output_path {
                set_metadata(&mut memory.metadata, "markdown_path", output_path.clone());
            }
        }

        // Generate embeddings
        {
            let mut emb = embeddings.lock().await;
            if emb.is_enabled() {
                if let Some(vector) = emb.embed(&memory.content).await? {
                    memory.embedding = Some(vector);
                }
            }
        }

        if sparse_embeddings.is_enabled() {
            if let Some(sparse_vec) = sparse_embeddings.embed(&memory.content).await? {
                memory.sparse_embedding = Some(sparse_vec.into());
            }
        }

        operations::insert_memory(db.pool(), &memory).await?;

        // HMLR enrichment

        Ok(IngestResult {
            path: path.display().to_string(),
            memory_id: memory.id.to_string(),
            title,
            source_path,
            content_preview: memory.content.chars().take(100).collect(),
            chunks: 1,
            total_tokens_approx: total_tokens,
        })
    }
}

fn collect_files(
    path: &PathBuf,
    recursive: bool,
    extensions: &[String],
) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    if path.is_file() {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        if extensions.contains(&ext) {
            paths.push(path.clone());
        }
        return Ok(paths);
    }

    if !path.is_dir() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    let walker = if recursive {
        WalkDir::new(path)
    } else {
        WalkDir::new(path).max_depth(1)
    };

    for entry in walker.into_iter().filter_map(Result::ok) {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }

        let ext = entry_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        if extensions.contains(&ext) {
            paths.push(entry_path.to_path_buf());
        }
    }

    Ok(paths)
}

fn parse_frontmatter(content: &str) -> (Option<IngestrFrontmatter>, &str) {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(idx) = rest.find("\n---\n") {
            let (front, body) = rest.split_at(idx);
            let body = &body["\n---\n".len()..];
            if let Ok(parsed) = serde_yaml::from_str::<IngestrFrontmatter>(front) {
                return (Some(parsed), body.trim());
            }
        }
    }
    (None, content)
}

struct IngestrOutputDir {
    path: PathBuf,
    cleanup: bool,
}

impl IngestrOutputDir {
    fn new(config: &Config, source_dir: &std::path::Path) -> anyhow::Result<Self> {
        let (path, cleanup) = if let Some(base) = &config.ingest.ingestr_output_dir {
            let key = fnv1a64(&source_dir.to_string_lossy());
            (base.join(format!("mmry-ingestr-{key:016x}")), false)
        } else {
            (
                std::env::temp_dir().join(format!("mmry-ingestr-{}", uuid::Uuid::new_v4())),
                true,
            )
        };

        std::fs::create_dir_all(&path)?;
        Ok(Self { path, cleanup })
    }

    fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }
}

impl Drop for IngestrOutputDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn fnv1a64(input: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET;
    for b in input.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

async fn run_ingestr_once(
    config: &Config,
    source_dir: &std::path::Path,
    output_dir: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let mut cmd = Command::new(&config.ingest.ingestr_bin);
    cmd.arg("service")
        .arg("run")
        .arg("--once")
        .arg("--watch-dir")
        .arg(source_dir)
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--disable-index")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let timeout = Duration::from_secs(config.ingest.ingestr_timeout_seconds);
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("ingestr timed out after {}s", timeout.as_secs()))??;

    if !output.status.success() {
        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        stdout.truncate(8_000);
        stderr.truncate(8_000);
        anyhow::bail!(
            "ingestr failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            stdout,
            stderr
        );
    }

    Ok(())
}

fn parse_memory_type(explicit: Option<&str>, content: &str) -> MemoryType {
    if let Some(t) = explicit {
        match t.to_lowercase().as_str() {
            "episodic" => return MemoryType::Episodic,
            "semantic" => return MemoryType::Semantic,
            "procedural" => return MemoryType::Procedural,
            _ => {}
        }
    }

    // Simple auto-classification
    let content_lower = content.to_lowercase();
    if content_lower.contains("step")
        || content_lower.contains("how to")
        || content_lower.contains("instructions")
    {
        return MemoryType::Procedural;
    }
    if content_lower.contains(" is ") || content_lower.contains(" are ") {
        return MemoryType::Semantic;
    }
    MemoryType::Episodic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_extracts_ingestr_metadata() {
        let content = r#"---
source_path: /home/user/Documents/report.pdf
output_path: /home/user/markdown/report.md
title: Quarterly Report
converted_at: 2025-01-15T10:30:00Z
---

This is the content of the report.
"#;

        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_some());
        let fm = fm.unwrap();
        assert_eq!(
            fm.source_path,
            Some("/home/user/Documents/report.pdf".to_string())
        );
        assert_eq!(fm.title, Some("Quarterly Report".to_string()));
        assert!(body.starts_with("This is the content"));
    }

    #[test]
    fn parse_frontmatter_handles_missing() {
        let content = "Just plain text content";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_frontmatter_handles_partial_fields() {
        let content = r#"---
title: Only Title
---

Content here.
"#;

        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_some());
        let fm = fm.unwrap();
        assert_eq!(fm.title, Some("Only Title".to_string()));
        assert!(fm.source_path.is_none());
        assert!(body.starts_with("Content here"));
    }

    #[test]
    fn memory_type_classification() {
        assert!(matches!(
            parse_memory_type(Some("procedural"), ""),
            MemoryType::Procedural
        ));
        assert!(matches!(
            parse_memory_type(None, "Step 1: Do this"),
            MemoryType::Procedural
        ));
        assert!(matches!(
            parse_memory_type(None, "The capital is Paris"),
            MemoryType::Semantic
        ));
        assert!(matches!(
            parse_memory_type(None, "Random text here"),
            MemoryType::Episodic
        ));
    }

    #[test]
    fn estimate_tokens_approximation() {
        let text = "This is a simple test sentence with ten words total.";
        let tokens = estimate_tokens(text);
        // 10 words * 1.3 = 13 tokens approximately
        assert!((10..=20).contains(&tokens));
    }
}
