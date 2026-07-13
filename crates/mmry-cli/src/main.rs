use anyhow::bail;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use mmry_core::config::Config;
use mmry_core::repos::Repository;
use mmry_core::repos::{self};
use mmry_core::AgentCtx;
use mmry_core::MemoryEvent;
use mmry_core::MemoryFile;
use mmry_core::MemoryType;
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mmry",
    version,
    about = "An append-only workspace memory ledger"
)]
struct Cli {
    #[arg(long, global = true, env = "MMRY_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        tracked: bool,
    },
    Add(AddArgs),
    #[command(alias = "ls")]
    List(QueryScope),
    Search(SearchArgs),
    Rm {
        memory_id: String,
        #[arg(long)]
        json: bool,
    },
    Doctor,
    Repos {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct AddArgs {
    text: String,
    #[arg(long, value_enum, default_value = "semantic")]
    memory_type: TypeArg,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
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
    #[arg(long, conflicts_with = "all", value_name = "NAME")]
    repo: Option<String>,
    #[arg(long, conflicts_with = "repo")]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    #[command(flatten)]
    scope: QueryScope,
    #[arg(long, default_value_t = 10)]
    limit: usize,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;
    match cli.command {
        Command::Init { tracked } => init(tracked),
        Command::Add(args) => add(args),
        Command::List(scope) => list(&config, &scope),
        Command::Search(args) => search(&config, &args),
        Command::Rm { memory_id, json } => remove(&memory_id, json),
        Command::Doctor => doctor(&config),
        Command::Repos { json } => show_repos(&config, json),
    }
}

fn init(tracked: bool) -> anyhow::Result<()> {
    let file = MemoryFile::open_current()?;
    file.init(tracked)?;
    let config_path = mmry_core::config::config_path()?;
    let schema_path = config_path.with_file_name("config.schema.json");
    if let Some(parent) = schema_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&schema_path, Config::schema_json()?)?;
    println!("initialized {}", file.path().display());
    Ok(())
}

fn add(args: AddArgs) -> anyhow::Result<()> {
    let content = if args.text == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text.trim().to_owned()
    } else {
        args.text
    };
    if content.is_empty() {
        bail!("memory text must not be empty");
    }
    let file = MemoryFile::open_current()?;
    let event = MemoryEvent::add(
        content,
        args.memory_type.into(),
        args.tags,
        &AgentCtx::from_env(),
    );
    file.append(&event)?;
    if args.json {
        print_json(&event)?;
    } else {
        println!("{}", event.memory_id);
    }
    Ok(())
}

fn selected_repos(config: &Config, scope: &QueryScope) -> anyhow::Result<Vec<Repository>> {
    if !scope.all && scope.repo.is_none() {
        let file = MemoryFile::open_current()?;
        return Ok(vec![repos::repository_for_path(file.root())?]);
    }
    let discovered = repos::discover(&config.roots)?;
    if let Some(name) = &scope.repo {
        Ok(vec![repos::select_named(&discovered, name)?])
    } else {
        Ok(discovered)
    }
}

fn list(config: &Config, scope: &QueryScope) -> anyhow::Result<()> {
    let memories = repos::list(&selected_repos(config, scope)?)?;
    if scope.json {
        print_json(&memories)?;
    } else {
        for item in memories {
            println!(
                "{}\t{}\t{}\t{}",
                item.memory.updated_at.to_rfc3339(),
                item.repo,
                item.memory.memory_id,
                item.memory.content
            );
        }
    }
    Ok(())
}

fn search(config: &Config, args: &SearchArgs) -> anyhow::Result<()> {
    let hits = repos::search(
        &selected_repos(config, &args.scope)?,
        &args.query,
        args.limit,
    )?;
    if args.scope.json {
        print_json(&hits)?;
    } else {
        for hit in hits {
            println!(
                "{}\t{}\t{}\t{}",
                hit.score, hit.repo, hit.memory.memory_id, hit.memory.content
            );
        }
    }
    Ok(())
}

fn remove(memory_id: &str, json: bool) -> anyhow::Result<()> {
    let file = MemoryFile::open_current()?;
    if !file
        .active_memories()?
        .iter()
        .any(|memory| memory.memory_id == memory_id)
    {
        bail!("memory not found: {memory_id}");
    }
    let event = MemoryEvent::deprecate(memory_id.to_owned(), &AgentCtx::from_env());
    file.append(&event)?;
    if json {
        print_json(&event)?;
    } else {
        println!("deprecated {memory_id}");
    }
    Ok(())
}

fn doctor(config: &Config) -> anyhow::Result<()> {
    let file = MemoryFile::open_current()?;
    println!("workspace: {}", file.root().display());
    println!(
        "ledger: {} ({})",
        file.path().display(),
        if file.path().exists() {
            "present"
        } else {
            "missing"
        }
    );
    match file.read_events() {
        Ok(events) => println!("events: {} (valid)", events.len()),
        Err(error) => bail!(error),
    }
    println!("configured roots: {}", config.roots.len());
    Ok(())
}

fn show_repos(config: &Config, json: bool) -> anyhow::Result<()> {
    let repos = repos::discover(&config.roots)?;
    if json {
        print_json(&repos)?;
    } else {
        for repo in repos {
            println!(
                "{}\t{}\texists={}\treadable={}",
                repo.name,
                repo.repo_path.display(),
                repo.exists,
                repo.readable
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
