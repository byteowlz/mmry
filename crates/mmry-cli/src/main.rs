use anyhow::bail;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use mmry_core::config::Config;
use mmry_core::repos::Repository;
use mmry_core::repos::RepositoryMemory;
use mmry_core::repos::RepositorySearchHit;
use mmry_core::repos::{self};
use mmry_core::AgentCtx;
use mmry_core::MemoryEntry;
use mmry_core::MemoryEvent;
use mmry_core::MemoryFile;
use mmry_core::MemoryType;
use serde::Serialize;
use std::fmt::Write as _;
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
    #[arg(long, conflicts_with = "plain")]
    json: bool,
    /// Stable tab-separated output with escaped newlines.
    #[arg(long, conflicts_with = "json")]
    plain: bool,
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
    } else if scope.plain {
        print!("{}", plain_list(&memories));
    } else {
        print!("{}", human_list(&memories));
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
    } else if args.scope.plain {
        print!("{}", plain_search(&hits));
    } else {
        print!("{}", human_search(&hits));
    }
    Ok(())
}

fn human_list(items: &[RepositoryMemory]) -> String {
    if items.is_empty() {
        return "No memories found.\n".to_owned();
    }
    let mut output = String::new();
    for item in items {
        write_human_memory(&mut output, &item.repo, &item.repo_path, &item.memory, None);
    }
    output
}

fn human_search(items: &[RepositorySearchHit]) -> String {
    if items.is_empty() {
        return "No matching memories.\n".to_owned();
    }
    let mut output = String::new();
    for item in items {
        write_human_memory(
            &mut output,
            &item.repo,
            &item.repo_path,
            &item.memory,
            Some(item.score),
        );
    }
    output
}

fn write_human_memory(
    output: &mut String,
    repo: &str,
    repo_path: &std::path::Path,
    memory: &MemoryEntry,
    score: Option<usize>,
) {
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
    )
    .expect("writing to a String cannot fail");
    writeln!(output, "{}  ·  {}", repo_path.display(), memory.memory_id)
        .expect("writing to a String cannot fail");
    for line in wrap_content(&memory.content, 96) {
        writeln!(output, "  {line}").expect("writing to a String cannot fail");
    }
    if !memory.tags.is_empty() {
        writeln!(output, "  tags: {}", memory.tags.join(", "))
            .expect("writing to a String cannot fail");
    }
    output.push('\n');
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

fn plain_list(items: &[RepositoryMemory]) -> String {
    items.iter().fold(String::new(), |mut output, item| {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            item.memory.updated_at.to_rfc3339(),
            item.repo,
            item.repo_path.display(),
            item.memory.memory_id,
            escape_plain(&item.memory.content)
        )
        .expect("writing to a String cannot fail");
        output
    })
}

fn plain_search(items: &[RepositorySearchHit]) -> String {
    items.iter().fold(String::new(), |mut output, item| {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            item.score,
            item.repo,
            item.repo_path.display(),
            item.memory.memory_id,
            escape_plain(&item.memory.content)
        )
        .expect("writing to a String cannot fail");
        output
    })
}

fn escape_plain(content: &str) -> String {
    content
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str) -> RepositoryMemory {
        let timestamp = "2026-06-09T18:38:57Z".parse().unwrap();
        RepositoryMemory {
            repo: "oqto_refactor".into(),
            repo_path: "/home/wismut/byteowlz/oqto_refactor".into(),
            memory: MemoryEntry {
                memory_id: "mem_123".into(),
                content: content.into(),
                memory_type: MemoryType::Procedural,
                tags: vec!["sandbox".into(), "linux".into()],
                created_at: timestamp,
                updated_at: timestamp,
                metadata: serde_json::json!({}),
                agent_ctx: serde_json::json!({}),
            },
        }
    }

    #[test]
    fn human_output_is_wrapped_and_attributed() {
        let output = human_list(&[item(&"word ".repeat(30))]);
        assert!(output.contains("oqto_refactor  ·  2026-06-09 18:38 UTC  ·  procedural"));
        assert!(output.contains("/home/wismut/byteowlz/oqto_refactor  ·  mem_123"));
        assert!(output.contains("  tags: sandbox, linux"));
        assert!(output.lines().all(|line| line.chars().count() <= 98));
    }

    #[test]
    fn plain_output_keeps_one_record_per_line() {
        let output = plain_list(&[item("first line\nsecond\tline")]);
        assert_eq!(output.lines().count(), 1);
        assert!(output.contains("first line\\nsecond\\tline"));
    }
}
