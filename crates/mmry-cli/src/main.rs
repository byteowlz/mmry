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
use mmry_core::MemoryType;
use mmry_core::config::Config;
use mmry_core::config::MigrateMode;
use mmry_core::memory_file::parse_expiry;
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
    /// Central store root (overrides MMRY_STATE_ROOT and `state_root`).
    #[arg(long, global = true, value_name = "PATH")]
    state_root: Option<PathBuf>,
    /// Repo-local ledger handling (overrides MMRY_MIGRATE and `migrate`).
    #[arg(long, global = true, value_name = "MODE")]
    migrate: Option<MigrateMode>,
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
    /// Switch to the central store: find every repo-local .mmry ledger,
    /// show the plan, migrate after confirmation and set `migrate = "auto"`.
    Setup(SetupArgs),
    /// Git sync of the central store (opt-in): commit, pull, push.
    Sync(SyncArgs),
    /// Memories a harness should inject at session start, with the exact text.
    Preview(PreviewArgs),
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
    Doctor(DoctorArgs),
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
    /// Only true on this machine: a label, or '.' for the current machine
    /// (AGENT_CTX_MACHINE_ID, else the host name).
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

#[derive(Args)]
struct SyncArgs {
    #[command(subcommand)]
    action: Option<SyncAction>,
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum SyncAction {
    /// Make the state root a git repository and optionally connect a remote.
    Init {
        /// Remote URL (uses your git credentials; nothing is stored by mmry).
        #[arg(long)]
        remote: Option<String>,
    },
    /// Remote, pending commits, last pull/push, last error.
    Status,
    /// Commit local changes and merge the remote.
    Pull,
    /// Commit local changes and push (pulls once if the push is rejected).
    Push,
}

#[derive(Args)]
struct PreviewArgs {
    /// Directory the session starts in (default: current directory).
    #[arg(long, value_name = "DIR")]
    cwd: Option<PathBuf>,
    /// Token budget for the rendered text (estimated at 4 bytes per token).
    #[arg(long, default_value_t = 1200)]
    max_tokens: usize,
    /// Maximum number of entries.
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Print the JSON contract (default prints only the rendered text).
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct DoctorArgs {
    /// Check every known ledger, not just the current scope.
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SetupArgs {
    /// Directories to scan for .mmry ledgers (default: home). Configured roots are always included.
    #[arg(long, value_name = "PATH")]
    scan: Vec<PathBuf>,
    /// Maximum directory depth of the scan.
    #[arg(long, default_value_t = 6)]
    depth: usize,
    /// Also migrate git-committed ledgers (runs `git rm --cached`; left uncommitted).
    #[arg(long)]
    untrack: bool,
    /// Print the plan without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Do not ask for confirmation (required without a terminal).
    #[arg(short, long)]
    yes: bool,
    #[arg(long)]
    json: bool,
}

/// Resolved store plus the repository enclosing the working directory.
struct Env {
    config_path: PathBuf,
    config: Config,
    store: Store,
    checkout: Option<Checkout>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_path = Config::resolve_path(cli.config.as_deref())?;
    let mut config = Config::load(cli.config.as_deref())?;
    config.apply_env(|key| std::env::var(key).ok())?;
    if let Some(root) = cli.state_root {
        config.state_root = Some(mmry_core::config::expand_tilde(&root)?);
    }
    if let Some(mode) = cli.migrate {
        config.migrate = mode;
    }
    let store = Store::new(config.state_root()?);
    let checkout = Checkout::detect(&std::env::current_dir()?)?;
    let env = Env {
        config_path,
        config,
        store,
        checkout,
    };
    if !matches!(
        cli.command,
        Command::Migrate(_)
            | Command::Setup(_)
            | Command::Preview(_)
            | Command::Doctor(_)
            | Command::Repos { .. }
    ) {
        auto_migrate(&env)?;
    }
    let writes = matches!(
        cli.command,
        Command::Init { .. }
            | Command::Add(_)
            | Command::Supersede(_)
            | Command::Rm(_)
            | Command::Migrate(_)
            | Command::Setup(_)
    );
    let result = match cli.command {
        Command::Init { tracked } => init(&env, tracked),
        Command::Sync(args) => sync(&env, &args),
        Command::Setup(args) => setup(&env, &args),
        Command::Preview(args) => preview(&env, &args),
        Command::Add(args) => add(&env, args),
        Command::List(args) => list(&env, &args),
        Command::Search(args) => search(&env, &args),
        Command::Supersede(args) => supersede(&env, args),
        Command::Rm(args) => remove(&env, &args),
        Command::Migrate(args) => migrate(&env, &args),
        Command::Doctor(args) => doctor(&env, &args),
        Command::Repos { json } => show_repos(&env, json),
    };
    if writes && result.is_ok() {
        auto_commit(&env);
    }
    result
}

fn syncer(env: &Env) -> mmry_core::sync::Sync {
    let machine = mmry_core::agent_ctx::current_machine(&AgentCtx::from_env())
        .unwrap_or_else(|| "unknown".to_owned());
    mmry_core::sync::Sync::new(
        env.store.root(),
        std::time::Duration::from_secs(env.config.sync.timeout_secs),
        machine,
    )
}

/// `sync.auto_commit` / `sync.auto_push` after a successful write. Failures
/// only warn: the write itself is already durable.
fn auto_commit(env: &Env) {
    if !env.config.sync.auto_commit || !mmry_core::sync::is_enabled(env.store.root()) {
        return;
    }
    let sync = syncer(env);
    let result = if env.config.sync.auto_push {
        sync.push().map(|outcome| outcome.error)
    } else {
        sync.commit().map(|_| None)
    };
    match result {
        Ok(None) => {}
        Ok(Some(error)) => {
            eprintln!("mmry: warning: sync: {error} (kept locally; `mmry sync status`)");
        }
        Err(error) => eprintln!("mmry: warning: sync: {error}"),
    }
}

fn sync(env: &Env, args: &SyncArgs) -> anyhow::Result<()> {
    let sync = syncer(env);
    let outcome = match &args.action {
        Some(SyncAction::Status) => {
            let status = sync.status()?;
            if args.json {
                return print_json(&status);
            }
            print_sync_status(&status);
            return Ok(());
        }
        Some(SyncAction::Init { remote }) => sync.init(remote.as_deref())?,
        Some(SyncAction::Pull) => sync.pull()?,
        Some(SyncAction::Push) => sync.push()?,
        None => sync.sync()?,
    };
    if args.json {
        print_json(&outcome)?;
    } else {
        print_sync_status(&outcome.status);
    }
    if let Some(error) = outcome.error {
        bail!("{error} (local changes are kept and committed)");
    }
    Ok(())
}

fn print_sync_status(status: &mmry_core::sync::SyncStatus) {
    if !status.enabled {
        println!("sync: off (run `mmry sync init --remote URL`)");
        return;
    }
    println!("sync: {}", status.root.display());
    println!("remote: {}", status.remote.as_deref().unwrap_or("none"));
    println!(
        "pending commits: {}, behind: {}{}",
        status.pending_commits,
        status.behind,
        if status.uncommitted {
            ", uncommitted changes"
        } else {
            ""
        }
    );
    let at = |time: Option<chrono::DateTime<chrono::Utc>>| {
        time.map_or_else(
            || "never".to_owned(),
            |t| t.format("%Y-%m-%d %H:%M UTC").to_string(),
        )
    };
    println!(
        "last pull: {}, last push: {}",
        at(status.state.last_pull),
        at(status.state.last_push)
    );
    if let Some(error) = &status.state.last_error {
        println!("last error: {error}");
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
                eprintln!(
                    "mmry: warning: {} is not used yet (central mode); run `mmry setup` or `mmry migrate` to move it",
                    local.path().display()
                );
                return Ok(());
            }
            eprint!(
                "mmry: move {} into the central store? [y/N] ",
                local.path().display()
            );
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if !answer.trim().eq_ignore_ascii_case("y") {
                eprintln!(
                    "mmry: not migrated; this repository's local memories stay unused until `mmry migrate` runs"
                );
                return Ok(());
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
fn write_target(env: &Env, general: bool) -> anyhow::Result<(Source, String)> {
    if general {
        return Ok((Source::general(&env.store), "general".into()));
    }
    let checkout = require_checkout(env)?;
    let scope = format!("repo:{}", checkout.identity);
    if checkout.tracked().is_none() {
        env.store.register(checkout)?;
    }
    Ok((Source::for_checkout(&env.store, checkout)?, scope))
}

/// `memory_id` as stored in `source` after a write, in the list/search entry
/// schema. A removed memory is reported as it was before removal.
fn sourced(
    source: &Source,
    memory_id: &str,
    before: Option<MemoryEntry>,
) -> anyhow::Result<SourcedMemory> {
    let memory = source
        .file()
        .active_memories()?
        .into_iter()
        .find(|memory| memory.memory_id == memory_id)
        .or(before)
        .with_context(|| format!("{memory_id} not found after writing"))?;
    Ok(SourcedMemory {
        scope: source.scope,
        repo: source.name.clone(),
        repo_path: source.repo_path.clone(),
        memory,
    })
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
    let (target, scope) = write_target(env, args.general)?;
    let agent = AgentCtx::from_env();
    let machine = match args.machine.as_deref() {
        Some(".") => Some(
            mmry_core::agent_ctx::current_machine(&agent)
                .context("cannot determine this machine; pass --machine NAME")?,
        ),
        _ => args.machine,
    };
    let mut event = MemoryEvent::add(content, args.memory_type.into(), args.tags, &agent);
    event.scope = Some(scope);
    event.why = args.why;
    event.source = args.source;
    event.machine = machine;
    event.expires_at = expires_at;
    target.file().append(&event)?;
    if args.json {
        print_json(&sourced(&target, &event.memory_id, None)?)?;
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
fn ledger_of(env: &Env, memory_id: &str) -> anyhow::Result<Source> {
    for source in current_sources(env)? {
        if source
            .file()
            .active_memories()?
            .iter()
            .any(|memory| memory.memory_id == memory_id)
        {
            return Ok(source);
        }
    }
    bail!("memory not found in the current scope (general + current repository): {memory_id}")
}

fn supersede(env: &Env, args: SupersedeArgs) -> anyhow::Result<()> {
    let content = read_text(args.text)?;
    let target = ledger_of(env, &args.memory_id)?;
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
    target.file().append_edit(event, args.expected_revision)?;
    if args.json {
        print_json(&sourced(&target, &args.memory_id, None)?)?;
    } else {
        println!("superseded {}", args.memory_id);
    }
    Ok(())
}

fn remove(env: &Env, args: &RmArgs) -> anyhow::Result<()> {
    let target = ledger_of(env, &args.memory_id)?;
    let before = target
        .file()
        .active_memories()?
        .into_iter()
        .find(|memory| memory.memory_id == args.memory_id);
    let mut event = MemoryEvent::deprecate(args.memory_id.clone(), &AgentCtx::from_env());
    event.reason.clone_from(&args.reason);
    target.file().append_edit(event, args.expected_revision)?;
    if args.json {
        let mut removed = serde_json::to_value(sourced(&target, &args.memory_id, before)?)?;
        removed["removed"] = true.into();
        print_json(&removed)?;
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

#[derive(Serialize)]
struct SetupReport {
    config: PathBuf,
    state_root: PathBuf,
    dry_run: bool,
    reports: Vec<MigrationReport>,
    unreadable: Vec<PathBuf>,
}

fn setup(env: &Env, args: &SetupArgs) -> anyhow::Result<()> {
    let scan_paths = if args.scan.is_empty() {
        vec![
            mmry_core::paths::home_dir()
                .context("cannot determine the home directory; pass --scan PATH")?,
        ]
    } else {
        args.scan.clone()
    };
    let mut roots: Vec<_> = scan_paths
        .into_iter()
        .map(|path| mmry_core::config::DiscoveryRoot {
            path,
            max_depth: args.depth,
        })
        .collect();
    roots.extend(env.config.roots.iter().cloned());
    let exclude: Vec<_> = std::fs::canonicalize(env.store.root())
        .into_iter()
        .collect();
    let scan = repos::scan_local(&roots, &exclude);
    let plan_options = MigrateOptions {
        dry_run: true,
        untrack: args.untrack,
    };
    let plan = scan
        .found
        .iter()
        .map(|checkout| store::migrate(&env.store, checkout, plan_options))
        .collect::<Result<Vec<_>, _>>()?;
    let pending = plan
        .iter()
        .filter(|r| r.status == MigrationStatus::Migrated)
        .count();

    if !args.json || args.dry_run {
        let report = SetupReport {
            config: env.config_path.clone(),
            state_root: env.store.root().to_path_buf(),
            dry_run: true,
            reports: plan,
            unreadable: scan.unreadable.clone(),
        };
        if args.json {
            return print_json(&report);
        }
        print_setup(&report);
        if args.dry_run {
            return Ok(());
        }
    }
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            bail!("no terminal to confirm; re-run with --yes (or --dry-run to only see the plan)");
        }
        eprint!(
            "Migrate {pending} ledger(s) and set migrate = \"auto\" in {}? [y/N] ",
            env.config_path.display()
        );
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            bail!("aborted; nothing was changed");
        }
    }

    env.store.general().touch()?;
    mmry_core::config::set_config_value(&env.config_path, "migrate", MigrateMode::Auto.as_str())?;
    let options = MigrateOptions {
        dry_run: false,
        untrack: args.untrack,
    };
    let reports = scan
        .found
        .iter()
        .map(|checkout| store::migrate(&env.store, checkout, options))
        .collect::<Result<Vec<_>, _>>()?;
    let report = SetupReport {
        config: env.config_path.clone(),
        state_root: env.store.root().to_path_buf(),
        dry_run: false,
        reports,
        unreadable: scan.unreadable,
    };
    if args.json {
        print_json(&report)?;
    } else {
        println!();
        for item in &report.reports {
            println!("{}", summarize(item));
        }
        println!("config: migrate = \"auto\" in {}", report.config.display());
    }
    if report
        .reports
        .iter()
        .any(|r| r.status == MigrationStatus::Conflict)
    {
        bail!("some ledgers were not migrated because of conflicting event ids; see above");
    }
    Ok(())
}

fn print_setup(report: &SetupReport) {
    println!("central store: {}", report.state_root.display());
    println!("config:        {}", report.config.display());
    if report.reports.is_empty() {
        println!("no repo-local .mmry ledgers found");
    }
    for item in &report.reports {
        println!("  {}", summarize(item));
    }
    if !report.unreadable.is_empty() {
        println!("skipped (unreadable): {}", report.unreadable.len());
        for path in &report.unreadable {
            println!("  {}", path.display());
        }
    }
}

fn preview(env: &Env, args: &PreviewArgs) -> anyhow::Result<()> {
    let detected;
    let checkout = match &args.cwd {
        Some(dir) => {
            detected = Checkout::detect(dir)?;
            detected.as_ref()
        }
        None => env.checkout.as_ref(),
    };
    let mut sources = vec![Source::general(&env.store)];
    let mut warnings = Vec::new();
    if env.config.sync.auto_pull && mmry_core::sync::is_enabled(env.store.root()) {
        match syncer(env).pull() {
            Ok(outcome) => warnings.extend(outcome.error.map(|e| format!("sync pull failed: {e}"))),
            Err(error) => warnings.push(format!("sync pull failed: {error}")),
        }
    }
    if let Some(checkout) = checkout {
        sources.push(Source::for_checkout(&env.store, checkout)?);
        if checkout.needs_migration() {
            warnings.push(format!(
                "{} is not used yet; run `mmry setup` or `mmry migrate` in {}",
                checkout.local_ledger().path().display(),
                checkout.root.display()
            ));
        }
    }
    let machine = mmry_core::agent_ctx::current_machine(&AgentCtx::from_env());
    let budget = mmry_core::preview::Budget {
        max_tokens: args.max_tokens,
        limit: args.limit,
    };
    let preview = mmry_core::preview::build(
        &sources,
        machine.as_deref(),
        budget,
        chrono::Utc::now(),
        warnings,
    )?;
    if args.json {
        print_json(&preview)
    } else {
        print!("{}", preview.rendered);
        for warning in &preview.warnings {
            eprintln!("mmry: warning: {warning}");
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct DoctorReport {
    state_root: PathBuf,
    config: PathBuf,
    migrate: &'static str,
    repository: Option<RepositoryStatus>,
    ledgers: Vec<LedgerStatus>,
    /// True when every ledger is readable and nothing is contested.
    healthy: bool,
}

#[derive(Serialize)]
struct RepositoryStatus {
    root: PathBuf,
    identity: String,
    mode: String,
    pending_migration: Option<PathBuf>,
}

#[derive(Serialize)]
struct LedgerStatus {
    scope: repos::Scope,
    name: String,
    path: PathBuf,
    exists: bool,
    active: usize,
    contested: Vec<String>,
    issues: Vec<mmry_core::memory_file::LedgerIssue>,
}

fn doctor(env: &Env, args: &DoctorArgs) -> anyhow::Result<()> {
    let sources = if args.all {
        repos::all_sources(&env.store, &env.config.roots)?
    } else {
        current_sources(env)?
    };
    let mut ledgers = Vec::new();
    for source in &sources {
        let file = source.file();
        let replay = file.replay()?;
        ledgers.push(LedgerStatus {
            scope: source.scope,
            name: source.name.clone(),
            path: source.ledger.clone(),
            exists: file.exists(),
            active: replay.entries.len(),
            contested: replay
                .entries
                .iter()
                .filter(|entry| entry.contested)
                .map(|entry| entry.memory_id.clone())
                .collect(),
            issues: replay.issues,
        });
    }
    let repository = env.checkout.as_ref().map(|checkout| RepositoryStatus {
        root: checkout.root.clone(),
        identity: checkout.identity.clone(),
        mode: match checkout.tracked() {
            Some(store::TrackedReason::Marker) => "tracked (.mmry/tracked)".into(),
            Some(store::TrackedReason::GitTracked) => "tracked (ledger committed to git)".into(),
            None => "central".into(),
        },
        pending_migration: checkout
            .needs_migration()
            .then(|| checkout.local_ledger().path().to_path_buf()),
    });
    let healthy = ledgers
        .iter()
        .all(|ledger| ledger.contested.is_empty() && ledger.issues.is_empty());
    let report = DoctorReport {
        state_root: env.store.root().to_path_buf(),
        config: env.config_path.clone(),
        migrate: env.config.migrate.as_str(),
        repository,
        ledgers,
        healthy,
    };
    if args.json {
        return print_json(&report);
    }
    println!("state root: {}", report.state_root.display());
    println!(
        "config: {} (migrate = {})",
        report.config.display(),
        report.migrate
    );
    match &report.repository {
        None => println!("repository: none (outside any repository)"),
        Some(repo) => {
            println!("repository: {} ({})", repo.root.display(), repo.identity);
            println!("mode: {}", repo.mode);
            if let Some(path) = &repo.pending_migration {
                println!(
                    "pending migration: {} (run `mmry setup` or `mmry migrate`)",
                    path.display()
                );
            }
        }
    }
    for ledger in &report.ledgers {
        let state = if ledger.exists { "" } else { " (missing)" };
        println!(
            "{} ledger {}: {}{state}, {} active",
            ledger.name,
            ledger.path.display(),
            if ledger.issues.is_empty() {
                "valid"
            } else {
                "DAMAGED"
            },
            ledger.active
        );
        for issue in &ledger.issues {
            println!("  issue: {}", serde_json::to_string(issue)?);
        }
        for id in &ledger.contested {
            println!("  contested: {id} (resolve with `mmry supersede` or `mmry rm`)");
        }
    }
    if !report.healthy {
        bail!("problems found");
    }
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
    let contested = if memory.contested {
        "  ·  CONTESTED (resolve with `mmry supersede` or `mmry rm`)"
    } else {
        ""
    };
    writeln!(
        output,
        "{repo}  ·  {}  ·  {kind}{score}{contested}",
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
                contested: false,
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
