use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use std::path::PathBuf;

use mmry_core::config::Config;
use mmry_core::config::GuardPattern;
use mmry_core::config::GuardPatternKind;
use mmry_core::guardrails::Guardrails;

#[derive(Parser, Clone)]
pub struct GuardCmd {
    #[command(subcommand)]
    command: GuardSubcommand,
}

#[derive(Subcommand, Clone)]
enum GuardSubcommand {
    /// Add a new guardrail pattern
    Add(GuardAddCmd),
    /// List guardrail patterns
    List(GuardListCmd),
    /// Remove a guardrail pattern
    Remove(GuardRemoveCmd),
}

#[derive(Parser, Clone)]
pub struct GuardAddCmd {
    /// Pattern to block (literal by default)
    pub pattern: String,
    #[arg(long, value_enum, default_value_t = GuardPatternKindArg::Literal)]
    pub kind: GuardPatternKindArg,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Parser, Clone)]
pub struct GuardListCmd {
    /// Output guardrails as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser, Clone)]
pub struct GuardRemoveCmd {
    /// Remove by 1-based index (from `mmry guard list`)
    #[arg(long)]
    pub index: Option<usize>,
    /// Remove by exact pattern match
    #[arg(long)]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GuardPatternKindArg {
    Literal,
    Regex,
}

impl From<GuardPatternKindArg> for GuardPatternKind {
    fn from(kind: GuardPatternKindArg) -> Self {
        match kind {
            GuardPatternKindArg::Literal => GuardPatternKind::Literal,
            GuardPatternKindArg::Regex => GuardPatternKind::Regex,
        }
    }
}

pub async fn handle(
    cmd: GuardCmd,
    config: &mut Config,
    config_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    match cmd.command {
        GuardSubcommand::Add(cmd) => handle_add(cmd, config, config_path),
        GuardSubcommand::List(cmd) => handle_list(cmd, config),
        GuardSubcommand::Remove(cmd) => handle_remove(cmd, config, config_path),
    }
}

fn handle_add(
    cmd: GuardAddCmd,
    config: &mut Config,
    config_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let trimmed = cmd.pattern.trim();
    if trimmed.is_empty() {
        anyhow::bail!("pattern cannot be empty");
    }

    if config.guardrails.max_patterns > 0
        && config.guardrails.patterns.len() >= config.guardrails.max_patterns
    {
        anyhow::bail!(
            "guardrails max_patterns limit reached ({})",
            config.guardrails.max_patterns
        );
    }

    let pattern = GuardPattern {
        pattern: trimmed.to_string(),
        kind: cmd.kind.into(),
        reason: cmd.reason,
    };

    Guardrails::validate_pattern(&config.guardrails, &pattern)
        .map_err(|err| anyhow::anyhow!(err))?;

    config.guardrails.enabled = true;
    config.guardrails.patterns.push(pattern);

    save_config(config, config_path)?;
    println!("Guardrail added.");
    Ok(())
}

fn handle_list(cmd: GuardListCmd, config: &Config) -> anyhow::Result<()> {
    if cmd.json {
        let json = serde_json::to_string_pretty(&config.guardrails)?;
        println!("{json}");
        return Ok(());
    }

    if config.guardrails.patterns.is_empty() {
        let status = if config.guardrails.enabled {
            "enabled"
        } else {
            "disabled"
        };
        println!("No guardrails configured ({status}).");
        return Ok(());
    }

    let status = if config.guardrails.enabled {
        "enabled"
    } else {
        "disabled"
    };
    println!(
        "Guardrails ({status}) - max_patterns={}, max_pattern_length={}",
        config.guardrails.max_patterns, config.guardrails.max_pattern_length
    );

    for (idx, pattern) in config.guardrails.patterns.iter().enumerate() {
        let kind = match pattern.kind {
            GuardPatternKind::Literal => "literal",
            GuardPatternKind::Regex => "regex",
        };
        if let Some(reason) = pattern.reason.as_deref() {
            println!("{}. [{}] {} ({reason})", idx + 1, kind, pattern.pattern);
        } else {
            println!("{}. [{}] {}", idx + 1, kind, pattern.pattern);
        }
    }

    Ok(())
}

fn handle_remove(
    cmd: GuardRemoveCmd,
    config: &mut Config,
    config_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let has_index = cmd.index.is_some();
    let has_pattern = cmd.pattern.as_ref().is_some_and(|p| !p.trim().is_empty());

    if has_index == has_pattern {
        anyhow::bail!("Provide exactly one of --index or --pattern");
    }

    if let Some(index) = cmd.index {
        if index == 0 || index > config.guardrails.patterns.len() {
            anyhow::bail!("index out of range");
        }
        config.guardrails.patterns.remove(index - 1);
        if config.guardrails.patterns.is_empty() {
            config.guardrails.enabled = false;
        }
        save_config(config, config_path)?;
        println!("Guardrail removed.");
        return Ok(());
    }

    let pattern = cmd.pattern.unwrap_or_default();
    let trimmed = pattern.trim();
    let before = config.guardrails.patterns.len();
    config.guardrails.patterns.retain(|p| p.pattern != trimmed);
    let removed = before.saturating_sub(config.guardrails.patterns.len());

    if removed == 0 {
        anyhow::bail!("No guardrail matched pattern '{trimmed}'");
    }

    if config.guardrails.patterns.is_empty() {
        config.guardrails.enabled = false;
    }

    save_config(config, config_path)?;
    println!("Removed {removed} guardrail(s).");
    Ok(())
}

fn save_config(config: &Config, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(path) = config_path.as_ref() {
        config.save_to_path(path)?;
    } else {
        config.save()?;
    }
    Ok(())
}
