//! `mmry` command-line interface for the append-only memory ledger.

use anyhow::Context as _;
use anyhow::bail;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use mmry_core::AgentCtx;
use mmry_core::MemoryEntry;
use mmry_core::MemoryEvent;
use mmry_core::MemoryFile;
use mmry_core::MemoryType;
use mmry_core::config::Config;
use mmry_core::config::MigrateMode;
use mmry_core::memory_file::parse_expiry;
use mmry_core::memory_file::require_revision;
use mmry_core::repos::Source;
use mmry_core::repos::SourcedHit;
use mmry_core::repos::SourcedMemory;
use mmry_core::repos::{self};
use mmry_core::store::Checkout;
use mmry_core::store::MigrateOptions;
use mmry_core::store::MigrationReport;
use mmry_core::store::MigrationStatus;
use mmry_core::store::Store;
use mmry_core::store::{self};
use serde::Serialize;
use std::fmt::Write as _;
use std::io::IsTerminal;
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mmry",
    version,
    about = "Personal operative memory: general and per-repository ledgers"
)]
struct Cli {
    #[arg(long, global = true, env = "MMRY_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Register the current repository in the central store, or with
    /// --tracked keep its ledger repo-local in .mmry/ (committed with the repo).
    Init {
        #[arg(long)]
        tracked: bool,
    },
    /// Record a memory in the current repository (or general with --general).
    Add(AddArgs),
    /// List memories of the current scope (general + current repository).
    #[command(alias = "ls")]
    List(ListArgs),
    /// Search memories of the current scope.
    Search(SearchArgs),
    /// Replace a memory's content in place (revision + 1).
    Supersede(SupersedeArgs),
    /// Deprecate a memory.
    Rm(RmArgs),
    /// Move repo-local .mmry/mmry.jsonl ledgers into the central store.
    Migrate(MigrateArgs),
    /// Show store, repository mode, and ledger health.
    Doctor,
    /// List known ledgers (general, central repos, tracked repos under roots).
    Repos {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct AddArgs {
    /// Memory text, or '-' to read stdin.
    text: String,
    /// Store as a general (cross-repository) memory.
    #[arg(long)]
    general: bool,
    #[arg(long, value_enum, default_value = "semantic")]
    memory_type: TypeArg,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
    /// Why it matters / how to apply it.
    #[arg(long)]
    why: Option<String>,
    /// Where it was observed (command, issue, URL).
    #[arg(long)]
    source: Option<String>,
    /// Label for machine-only observations.
    #[arg(long)]
    machine: Option<String>,
    /// RFC 3339 timestamp or duration (12h, 30d, 8w).
    #[arg(long)]
    expires: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Clone, ValueEnum)]
enum TypeArg {
    Episodic,
    Semantic,
    Procedural,
}

impl From<TypeArg> for MemoryType {
    fn from(value: TypeArg) -> Self {
        match value {
            TypeArg::Episodic => Self::Episodic,
            TypeArg::Semantic => Self::Semantic,
            TypeArg::Procedural => Self::Procedural,
        }
    }
}

#[derive(Args)]
struct QueryScope {
    /// Only this repository (store name or checkout directory name).
    #[arg(long, conflicts_with_all = ["all", "general"], value_name = "NAME")]
    repo: Option<String>,
    /// Every known ledger.
    #[arg(long, conflicts_with = "general")]
    all: bool,
    /// Only general memories.
    #[arg(long)]
    general: bool,
    #[arg(long, conflicts_with = "plain")]
    json: bool,
    /// Stable tab-separated output with escaped newlines.
    #[arg(long, conflicts_with = "json")]
    plain: bool,
}

#[derive(Args)]
struct ListArgs {
    #[command(flatten)]
    scope: QueryScope,
    /// Include expired memories.
    #[arg(long)]
    include_expired: bool,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    #[command(flatten)]
    scope: QueryScope,
    #[arg(long, default_value_t = 10)]
    limit: usize,
}

#[derive(Args)]
struct SupersedeArgs {
    memory_id: String,
    /// Replacement text, or '-' to read stdin.
    text: String,
    #[arg(long)]
    reason: String,
    /// Fail unless the memory is at this revision.
    #[arg(long)]
    expected_revision: Option<u32>,
    /// New expiry (RFC 3339 or 12h/30d/8w).
    #[arg(long)]
    expires: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RmArgs {
    memory_id: String,
    #[arg(long)]
    reason: Option<String>,
    /// Fail unless the memory is at this revision.
    #[arg(long)]
    expected_revision: Option<u32>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct MigrateArgs {
    /// Every repo-local ledger under the configured roots (default: current repository).
    #[arg(long)]
    all: bool,
    /// Also migrate git-committed ledgers (runs `git rm --cached`; the change is left uncommitted).
    #[arg(long)]
    untrack: bool,
    /// Report what would happen without writing.
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

/// Resolved store plus the repository enclosing the working directory.
struct Env {
    config: Config,
    store: Store,
    checkout: Option<Checkout>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;
    let store = Store::new(config.state_root()?);
    let checkout = Checkout::detect(&std::env::current_dir()?)?;
    let env = Env {
        config,
        store,
        checkout,
    };
    if !matches!(
        cli.command,
        Command::Migrate(_) | Command::Doctor | Command::Repos { .. }
    ) {
        auto_migrate(&env)?;
    }
    match cli.command {
        Command::Init { tracked } => init(&env, tracked),
        Command::Add(args) => add(&env, args),
        Command::List(args) => list(&env, &args),
        Command::Search(args) => search(&env, &args),
        Command::Supersede(args) => supersede(&env, args),
        Command::Rm(args) => remove(&env, &args),
        Command::Migrate(args) => migrate(&env, &args),
        Command::Doctor => doctor(&env),
        Command::Repos { json } => show_repos(&env, json),
    }
}

/// Apply the configured migration policy to the current repository.
fn auto_migrate(env: &Env) -> anyhow::Result<()> {
    let Some(checkout) = &env.checkout else {
        return Ok(());
    };
    if !checkout.needs_migration() {
        return Ok(());
    }
    let local = checkout.local_ledger();
    match env.config.migrate {
        MigrateMode::Off => {
            eprintln!(
                "mmry: warning: {} is not used (central mode, migrate = \"off\"); run `mmry migrate` to move it",
                local.path().display()
            );
            return Ok(());
        }
        MigrateMode::Prompt => {
            if !std::io::stdin().is_terminal() {
                bail!(
                    "{} must be migrated to the central store; run `mmry migrate` (migrate = \"prompt\" and no terminal)",
                    local.path().display()
                );
            }
            eprint!(
                "mmry: move {} into the central store? [y/N] ",
                local.path().display()
            );
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if !answer.trim().eq_ignore_ascii_case("y") {
                bail!(
                    "not migrated; this repository's memories are unavailable until `mmry migrate` runs"
                );
            }
        }
        MigrateMode::Auto => {}
    }
    let report = store::migrate(&env.store, checkout, MigrateOptions::default())?;
    if report.status == MigrationStatus::Conflict {
        bail!(conflict_message(&report));
    }
    eprintln!("mmry: {}", summarize(&report));
    Ok(())
}

fn conflict_message(report: &MigrationReport) -> String {
    format!(
        "cannot migrate {}: {} event id(s) differ from {}: {}; nothing was moved",
        report.source.display(),
        report.conflicts.len(),
        report.central.display(),
        report.conflicts.join(", ")
    )
}

fn summarize(report: &MigrationReport) -> String {
    let verb = if report.dry_run {
        "would migrate"
    } else {
        "migrated"
    };
    match report.status {
        MigrationStatus::Migrated => format!(
            "{verb} {} ({} new, {} duplicate) -> {}{}",
            report.source.display(),
            report.migrated,
            report.skipped_duplicate,
            report.central.display(),
            report
                .backup
                .as_ref()
                .map_or_else(String::new, |b| format!("; backup {}", b.display()))
        ),
        MigrationStatus::NothingToMigrate => {
            format!("{}: nothing to migrate", report.repo_path.display())
        }
        MigrationStatus::TrackedInGit => format!(
            "{}: ledger is committed to git; kept repo-local (use --untrack to move it)",
            report.repo_path.display()
        ),
        MigrationStatus::TrackedMode => format!(
            "{}: tracked mode (.mmry/tracked); kept repo-local",
            report.repo_path.display()
        ),
        MigrationStatus::Conflict => conflict_message(report),
    }
}

fn require_checkout(env: &Env) -> anyhow::Result<&Checkout> {
    env.checkout.as_ref().context(
        "not inside a repository (no .git or .mmry/ in any parent); use --general for general memories",
    )
}

/// Ledger that writes for this scope go to, plus its scope label.
fn write_target(env: &Env, general: bool) -> anyhow::Result<(MemoryFile, String)> {
    if general {
        return Ok((env.store.general(), "general".into()));
    }
    let checkout = require_checkout(env)?;
    let scope = format!("repo:{}", checkout.identity);
    if checkout.tracked().is_some() {
        return Ok((checkout.local_ledger(), scope));
    }
    Ok((env.store.register(checkout)?.ledger(), scope))
}

/// General plus, when inside one, the current repository.
fn current_sources(env: &Env) -> anyhow::Result<Vec<Source>> {
    let mut sources = vec![Source::general(&env.store)];
    if let Some(checkout) = &env.checkout {
        sources.push(Source::for_checkout(&env.store, checkout)?);
    }
    Ok(sources)
}

fn selected_sources(env: &Env, scope: &QueryScope) -> anyhow::Result<Vec<Source>> {
    if scope.general {
        return Ok(vec![Source::general(&env.store)]);
    }
    if !scope.all && scope.repo.is_none() {
        return current_sources(env);
    }
    let all = repos::all_sources(&env.store, &env.config.roots)?;
    match &scope.repo {
        Some(name) => Ok(vec![repos::select_named(&all, name)?]),
        None => Ok(all),
    }
}

fn read_text(text: String) -> anyhow::Result<String> {
    let content = if text == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text.trim().to_owned()
    } else {
        text
    };
    if content.is_empty() {
        bail!("memory text must not be empty");
    }
    Ok(content)
}

fn init(env: &Env, tracked: bool) -> anyhow::Result<()> {
    let path = if tracked {
        let root = env
            .checkout
            .as_ref()
            .map_or_else(std::env::current_dir, |c| Ok(c.root.clone()))?;
        store::init_tracked(&root)?.path().to_path_buf()
    } else if let Some(checkout) = &env.checkout {
        let ledger = env.store.register(checkout)?.ledger();
        ledger.touch()?;
        ledger.path().to_path_buf()
    } else {
        let ledger = env.store.general();
        ledger.touch()?;
        ledger.path().to_path_buf()
    };
    let config_path = mmry_core::config::config_path()?;
    let schema_path = config_path.with_file_name("config.schema.json");
    if let Some(parent) = schema_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&schema_path, Config::schema_json()?)?;
    println!("initialized {}", path.display());
    Ok(())
}

fn add(env: &Env, args: AddArgs) -> anyhow::Result<()> {
    let content = read_text(args.text)?;
    let expires_at = args
        .expires
        .as_deref()
        .map(|text| parse_expiry(text, chrono::Utc::now()))
        .transpose()?;
    let (file, scope) = write_target(env, args.general)?;
    let mut event = MemoryEvent::add(
        content,
        args.memory_type.into(),
        args.tags,
        &AgentCtx::from_env(),
    );
    event.scope = Some(scope);
    event.why = args.why;
    event.source = args.source;
    event.machine = args.machine;
    event.expires_at = expires_at;
    file.append(&event)?;
    if args.json {
        print_json(&event)?;
    } else {
        println!("{}", event.memory_id);
    }
    Ok(())
}

fn list(env: &Env, args: &ListArgs) -> anyhow::Result<()> {
    let sources = selected_sources(env, &args.scope)?;
    let memories = if args.include_expired {
        repos::list_including_expired(&sources)?
    } else {
        repos::list(&sources)?
    };
    if args.scope.json {
        print_json(&memories)?;
    } else if args.scope.plain {
        print!("{}", plain_list(&memories)?);
    } else {
        print!("{}", human_list(&memories)?);
    }
    Ok(())
}

fn search(env: &Env, args: &SearchArgs) -> anyhow::Result<()> {
    let hits = repos::search(
        &selected_sources(env, &args.scope)?,
        &args.query,
        args.limit,
    )?;
    if args.scope.json {
        print_json(&hits)?;
    } else if args.scope.plain {
        print!("{}", plain_search(&hits)?);
    } else {
        print!("{}", human_search(&hits)?);
    }
    Ok(())
}

/// The current-scope ledger holding active memory `memory_id`.
fn ledger_of(env: &Env, memory_id: &str) -> anyhow::Result<MemoryFile> {
    for source in current_sources(env)? {
        let file = source.file();
        if file
            .active_memories()?
            .iter()
            .any(|memory| memory.memory_id == memory_id)
        {
            return Ok(file);
        }
    }
    bail!("memory not found in the current scope (general + current repository): {memory_id}")
}

fn supersede(env: &Env, args: SupersedeArgs) -> anyhow::Result<()> {
    let content = read_text(args.text)?;
    let file = ledger_of(env, &args.memory_id)?;
    let mut event = MemoryEvent::supersede(
        args.memory_id.clone(),
        content,
        args.reason,
        &AgentCtx::from_env(),
    );
    event.expires_at = args
        .expires
        .as_deref()
        .map(|text| parse_expiry(text, chrono::Utc::now()))
        .transpose()?;
    file.append_checked(&event, |active| {
        require_revision(active, &args.memory_id, args.expected_revision)
    })?;
    if args.json {
        print_json(&event)?;
    } else {
        println!("superseded {}", args.memory_id);
    }
    Ok(())
}

fn remove(env: &Env, args: &RmArgs) -> anyhow::Result<()> {
    let file = ledger_of(env, &args.memory_id)?;
    let mut event = MemoryEvent::deprecate(args.memory_id.clone(), &AgentCtx::from_env());
    event.reason.clone_from(&args.reason);
    file.append_checked(&event, |active| {
        require_revision(active, &args.memory_id, args.expected_revision)
    })?;
    if args.json {
        print_json(&event)?;
    } else {
        println!("deprecated {}", args.memory_id);
    }
    Ok(())
}

fn migrate(env: &Env, args: &MigrateArgs) -> anyhow::Result<()> {
    let checkouts = if args.all {
        repos::discover_local(&env.config.roots)?
    } else {
        vec![require_checkout(env)?.clone()]
    };
    let options = MigrateOptions {
        dry_run: args.dry_run,
        untrack: args.untrack,
    };
    let mut reports = Vec::new();
    for checkout in &checkouts {
        reports.push(store::migrate(&env.store, checkout, options)?);
    }
    if args.json {
        print_json(&reports)?;
    } else {
        for report in &reports {
            println!("{}", summarize(report));
        }
    }
    if reports
        .iter()
        .any(|r| r.status == MigrationStatus::Conflict)
    {
        bail!("some ledgers were not migrated because of conflicting event ids");
    }
    Ok(())
}

fn doctor(env: &Env) -> anyhow::Result<()> {
    println!("state root: {}", env.store.root().display());
    report_ledger("general", &env.store.general())?;
    match &env.checkout {
        None => println!("repository: none (outside any repository)"),
        Some(checkout) => {
            println!(
                "repository: {} ({})",
                checkout.root.display(),
                checkout.identity
            );
            let source = Source::for_checkout(&env.store, checkout)?;
            match checkout.tracked() {
                Some(reason) => println!("mode: tracked ({})", serde_json::to_value(reason)?),
                None => println!("mode: central ({})", source.name),
            }
            report_ledger("repo", &source.file())?;
            if checkout.needs_migration() {
                println!(
                    "pending migration: {} (run `mmry migrate`)",
                    checkout.local_ledger().path().display()
                );
            }
        }
    }
    println!("configured roots: {}", env.config.roots.len());
    Ok(())
}

fn report_ledger(label: &str, file: &MemoryFile) -> anyhow::Result<()> {
    if !file.exists() {
        println!("{label} ledger: {} (missing)", file.path().display());
        return Ok(());
    }
    let events = file.read_events()?;
    println!(
        "{label} ledger: {} ({} events, valid)",
        file.path().display(),
        events.len()
    );
    Ok(())
}

fn show_repos(env: &Env, json: bool) -> anyhow::Result<()> {
    let sources = repos::all_sources(&env.store, &env.config.roots)?;
    if json {
        print_json(&sources)?;
    } else {
        for source in sources {
            println!(
                "{}\t{}\t{}\t{}",
                source.name,
                serde_json::to_value(source.storage)?
                    .as_str()
                    .unwrap_or_default(),
                source
                    .repo_path
                    .as_deref()
                    .map_or_else(String::new, |p| p.display().to_string()),
                source.ledger.display()
            );
        }
    }
    Ok(())
}

fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("serialize output")?
    );
    Ok(())
}

fn human_list(items: &[SourcedMemory]) -> Result<String, std::fmt::Error> {
    if items.is_empty() {
        return Ok("No memories found.\n".to_owned());
    }
    let mut output = String::new();
    for item in items {
        write_human_memory(
            &mut output,
            &item.repo,
            item.repo_path.as_deref(),
            &item.memory,
            None,
        )?;
    }
    Ok(output)
}

fn human_search(items: &[SourcedHit]) -> Result<String, std::fmt::Error> {
    if items.is_empty() {
        return Ok("No matching memories.\n".to_owned());
    }
    let mut output = String::new();
    for item in items {
        write_human_memory(
            &mut output,
            &item.repo,
            item.repo_path.as_deref(),
            &item.memory,
            Some(item.score),
        )?;
    }
    Ok(output)
}

fn write_human_memory(
    output: &mut String,
    repo: &str,
    repo_path: Option<&std::path::Path>,
    memory: &MemoryEntry,
    score: Option<usize>,
) -> std::fmt::Result {
    let kind = match memory.memory_type {
        MemoryType::Episodic => "episodic",
        MemoryType::Semantic => "semantic",
        MemoryType::Procedural => "procedural",
    };
    let score = score.map_or_else(String::new, |value| format!("  ·  score {value}"));
    writeln!(
        output,
        "{repo}  ·  {}  ·  {kind}{score}",
        memory.updated_at.format("%Y-%m-%d %H:%M UTC")
    )?;
    match repo_path {
        Some(path) => writeln!(
            output,
            "{}  ·  {}  ·  rev {}",
            path.display(),
            memory.memory_id,
            memory.revision
        )?,
        None => writeln!(output, "{}  ·  rev {}", memory.memory_id, memory.revision)?,
    }
    for line in wrap_content(&memory.content, 96) {
        writeln!(output, "  {line}")?;
    }
    if let Some(why) = &memory.why {
        writeln!(output, "  why: {why}")?;
    }
    if let Some(machine) = &memory.machine {
        writeln!(output, "  machine: {machine}")?;
    }
    if let Some(expires) = memory.expires_at {
        writeln!(
            output,
            "  expires: {}",
            expires.format("%Y-%m-%d %H:%M UTC")
        )?;
    }
    if !memory.tags.is_empty() {
        writeln!(output, "  tags: {}", memory.tags.join(", "))?;
    }
    output.push('\n');
    Ok(())
}

fn wrap_content(content: &str, width: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    for paragraph in content.lines() {
        if paragraph.trim().is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > width {
                wrapped.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        wrapped.push(line);
    }
    wrapped
}

fn plain_list(items: &[SourcedMemory]) -> Result<String, std::fmt::Error> {
    items.iter().try_fold(String::new(), |mut output, item| {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            item.memory.updated_at.to_rfc3339(),
            item.repo,
            item.repo_path
                .as_deref()
                .map_or_else(String::new, |p| p.display().to_string()),
            item.memory.memory_id,
            escape_plain(&item.memory.content)
        )?;
        Ok(output)
    })
}

fn plain_search(items: &[SourcedHit]) -> Result<String, std::fmt::Error> {
    items.iter().try_fold(String::new(), |mut output, item| {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            item.score,
            item.repo,
            item.repo_path
                .as_deref()
                .map_or_else(String::new, |p| p.display().to_string()),
            item.memory.memory_id,
            escape_plain(&item.memory.content)
        )?;
        Ok(output)
    })
}

fn escape_plain(content: &str) -> String {
    content
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str) -> SourcedMemory {
        use mmry_core::repos::SourcedMemory;
        let timestamp = "2026-06-09T18:38:57Z".parse().unwrap();
        SourcedMemory {
            scope: mmry_core::repos::Scope::Repo,
            repo: "oqto_refactor".into(),
            repo_path: Some("/home/wismut/byteowlz/oqto_refactor".into()),
            memory: MemoryEntry {
                memory_id: "mem_123".into(),
                content: content.into(),
                memory_type: MemoryType::Procedural,
                tags: vec!["sandbox".into(), "linux".into()],
                created_at: timestamp,
                updated_at: timestamp,
                revision: 1,
                scope: None,
                why: Some("sandbox differs".into()),
                source: None,
                machine: None,
                expires_at: None,
                metadata: serde_json::json!({}),
                agent_ctx: serde_json::json!({}),
            },
        }
    }

    #[test]
    fn human_output_is_wrapped_and_attributed() {
        let output = human_list(&[item(&"word ".repeat(30))]).unwrap();
        assert!(output.contains("oqto_refactor  ·  2026-06-09 18:38 UTC  ·  procedural"));
        assert!(output.contains("/home/wismut/byteowlz/oqto_refactor  ·  mem_123  ·  rev 1"));
        assert!(output.contains("  why: sandbox differs"));
        assert!(output.contains("  tags: sandbox, linux"));
        assert!(output.lines().all(|line| line.chars().count() <= 98));
    }

    #[test]
    fn plain_output_keeps_one_record_per_line() {
        let output = plain_list(&[item("first line\nsecond\tline")]).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(output.contains("first line\\nsecond\\tline"));
    }
}
