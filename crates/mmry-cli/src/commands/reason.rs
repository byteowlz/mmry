//! Reasoning commands - query memory through inference rather than search
//!
//! This module provides CLI access to the reasoning system, which derives
//! conclusions from facts rather than just retrieving them.

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mmry_core::config::Config;
use mmry_core::database::Database;
use mmry_core::reasoning::ContradictionRecord;
use mmry_core::reasoning::Inference;
use serde::Serialize;

#[derive(Parser)]
#[command(about = "Reasoning-based memory access (inference over facts)")]
pub struct ReasonCmd {
    #[command(subcommand)]
    command: ReasonCommand,
}

#[derive(Subcommand)]
enum ReasonCommand {
    /// List derived inferences
    Inferences(InferencesCmd),

    /// List detected contradictions
    Contradictions(ContradictionsCmd),

    /// Run a background reasoning pass to derive new inferences
    Derive(DeriveCmd),

    /// Show reasoning audit events
    Events(EventsCmd),
}

#[derive(Parser)]
pub struct InferencesCmd {
    /// Filter by inference type (observed, deduced, induced, abduced)
    #[arg(long, short = 't')]
    pub inference_type: Option<String>,

    /// Filter by category
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Include superseded inferences
    #[arg(long)]
    pub include_superseded: bool,

    /// Maximum number of results
    #[arg(long, short = 'l', default_value_t = 20)]
    pub limit: usize,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,
}

#[derive(Parser)]
pub struct ContradictionsCmd {
    /// Show all contradictions (including resolved)
    #[arg(long)]
    pub all: bool,

    /// Maximum number of results
    #[arg(long, short = 'l', default_value_t = 20)]
    pub limit: usize,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,
}

#[derive(Parser)]
pub struct DeriveCmd {
    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,
}

#[derive(Parser)]
pub struct EventsCmd {
    /// Maximum number of events to show
    #[arg(long, short = 'l', default_value_t = 20)]
    pub limit: usize,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,
}

/// Output format for inferences list
#[derive(Serialize)]
struct InferenceOutput {
    id: String,
    conclusion: String,
    inference_type: String,
    reasoning_trace: String,
    certainty: Option<String>,
    category: Option<String>,
    created_at: String,
    superseded: bool,
}

pub async fn handle(cmd: ReasonCmd, config: &Config, db: &Database) -> Result<()> {
    match cmd.command {
        ReasonCommand::Inferences(args) => handle_inferences(args, db).await,
        ReasonCommand::Contradictions(args) => handle_contradictions(args, db).await,
        ReasonCommand::Derive(args) => handle_derive(args, config, db).await,
        ReasonCommand::Events(args) => handle_events(args, db).await,
    }
}

async fn handle_inferences(args: InferencesCmd, db: &Database) -> Result<()> {
    let inferences = mmry_core::reasoning::operations::list_inferences(
        db.pool(),
        args.limit as i64,
        args.include_superseded,
    )
    .await?;

    // Filter by type if specified
    let filtered: Vec<_> = inferences
        .into_iter()
        .filter(|inf| {
            if let Some(ref t) = args.inference_type {
                inf.inference_type.as_str().eq_ignore_ascii_case(t)
            } else {
                true
            }
        })
        .filter(|inf| {
            if let Some(ref c) = args.category {
                inf.category.as_deref() == Some(c.as_str())
            } else {
                true
            }
        })
        .collect();

    if args.json {
        let output: Vec<InferenceOutput> = filtered.iter().map(inference_to_output).collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if filtered.is_empty() {
        println!("No inferences found.");
        println!();
        println!("Inferences are derived from facts through reasoning.");
        println!("Run `mmry reason derive` to generate inferences from existing facts.");
    } else {
        for inf in &filtered {
            print_inference(inf);
            println!();
        }
        println!("Total: {} inference(s)", filtered.len());
    }

    Ok(())
}

async fn handle_contradictions(args: ContradictionsCmd, db: &Database) -> Result<()> {
    // Currently we only have list_unresolved_contradictions
    // TODO: Add list_all_contradictions when needed
    let contradictions = mmry_core::reasoning::operations::list_unresolved_contradictions(
        db.pool(),
        args.limit as i64,
    )
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&contradictions)?);
    } else if contradictions.is_empty() {
        println!("No unresolved contradictions found.");
    } else {
        for c in &contradictions {
            print_contradiction(c);
            println!();
        }
        println!("Total: {} contradiction(s)", contradictions.len());
    }

    Ok(())
}

async fn handle_derive(args: DeriveCmd, config: &Config, db: &Database) -> Result<()> {
    // Create analyzer for LLM calls
    let analyzer = mmry_core::analysis::build_analyzer(config);

    // Create reasoning service with owned pool clone
    let service = mmry_core::reasoning::ReasoningService::new(
        config.reasoning.clone(),
        db.pool().clone(),
        analyzer,
    );

    if !service.is_enabled() {
        println!("Reasoning is disabled in config.");
        println!("Enable it by setting reasoning.enabled = true in config.toml");
        return Ok(());
    }

    println!("Running background reasoning pass...");
    let result = service.run_background_pass().await?;

    if args.json {
        #[derive(Serialize)]
        struct DeriveOutput {
            skipped: bool,
            skip_reason: Option<String>,
            facts_processed: usize,
            inferences_derived: usize,
            contradictions_found: usize,
        }

        let output = DeriveOutput {
            skipped: result.skipped,
            skip_reason: result.skip_reason.clone(),
            facts_processed: result.facts_processed,
            inferences_derived: result.inferences_derived,
            contradictions_found: result.contradictions_found,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if result.skipped {
        println!(
            "Pass skipped: {}",
            result.skip_reason.as_deref().unwrap_or("unknown reason")
        );
    } else {
        println!("Background pass complete:");
        println!("  Facts processed: {}", result.facts_processed);
        println!("  Inferences derived: {}", result.inferences_derived);
        println!("  Contradictions found: {}", result.contradictions_found);

        if !result.inferences.is_empty() {
            println!();
            println!("New inferences:");
            for inf in &result.inferences {
                println!("  - [{}] {}", inf.inference_type, inf.conclusion);
            }
        }

        if !result.contradictions.is_empty() {
            println!();
            println!("Contradictions detected:");
            for c in &result.contradictions {
                println!("  - {}", c.explanation);
            }
        }
    }

    Ok(())
}

async fn handle_events(args: EventsCmd, db: &Database) -> Result<()> {
    let events =
        mmry_core::reasoning::operations::list_reasoning_events(db.pool(), args.limit as i64)
            .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else if events.is_empty() {
        println!("No reasoning events found.");
    } else {
        for event in &events {
            println!(
                "[{}] {} - {}",
                event.created_at.format("%Y-%m-%d %H:%M:%S"),
                event.event_type,
                event.description
            );
        }
        println!();
        println!("Total: {} event(s)", events.len());
    }

    Ok(())
}

fn inference_to_output(inf: &Inference) -> InferenceOutput {
    InferenceOutput {
        id: inf.id.to_string(),
        conclusion: inf.conclusion.clone(),
        inference_type: inf.inference_type.as_str().to_string(),
        reasoning_trace: inf.reasoning_trace.clone(),
        certainty: inf.certainty_statement.clone(),
        category: inf.category.clone(),
        created_at: inf.created_at.to_rfc3339(),
        superseded: inf.superseded,
    }
}

fn print_inference(inf: &Inference) {
    let status = if inf.superseded { " (superseded)" } else { "" };
    println!("[{}]{} {}", inf.inference_type, status, inf.conclusion);
    println!("  ID: {}", inf.id);
    if let Some(ref cat) = inf.category {
        println!("  Category: {cat}");
    }
    println!("  Reasoning: {}", inf.reasoning_trace);
    if let Some(ref cert) = inf.certainty_statement {
        println!("  Certainty: {cert}");
    }
}

fn print_contradiction(c: &ContradictionRecord) {
    println!("Contradiction: {}", c.id);
    println!(
        "  {} ({}) vs {} ({})",
        c.item_a_type, c.item_a_id, c.item_b_type, c.item_b_id
    );
    println!("  Explanation: {}", c.explanation);
    println!("  Status: {}", c.status);
    if let Some(ref resolution) = c.resolution_reasoning {
        println!("  Resolution: {resolution}");
    }
}
