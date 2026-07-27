use anyhow::Context;
use clap::Parser;
use dialoguer::theme::ColorfulTheme;
use dialoguer::MultiSelect;
use dialoguer::Select;
use mmry_core::config::Config;
use mmry_core::database::operations;
use mmry_core::database::Database;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser, Clone)]
pub struct FeedbackCmd {
    /// Search session id followed by ratings (+1 -3 +4), or just ratings to use the last session
    #[arg(value_name = "SESSION_OR_RATING")]
    pub args: Vec<String>,

    #[arg(long, help = "Output JSON")]
    pub json: bool,

    #[arg(long, help = "Show feedback statistics")]
    pub stats: bool,

    #[arg(long, help = "Export the benchmark set as JSON")]
    pub export: bool,

    #[arg(long, value_name = "PATH", help = "Import benchmark JSON from a file")]
    pub import: Option<PathBuf>,

    #[arg(long, help = "Read feedback JSON from stdin")]
    pub stdin: bool,

    #[arg(long, help = "Tune search weights using the benchmark set")]
    pub tune: bool,

    #[arg(long, help = "Apply tuned weights to the active config file")]
    pub apply: bool,

    #[arg(short = 'i', long, help = "Interactively pick relevant search results")]
    pub interactive: bool,

    #[arg(long, help = "Use full step-through interactive rating mode")]
    pub full: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSearchSessionState {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedbackRating {
    rank: i64,
    relevant: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedbackInput {
    session: Option<String>,
    ratings: Vec<FeedbackRating>,
    agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
struct FeedbackOutput {
    session: String,
    ratings: Vec<operations::SearchFeedbackRecord>,
}

#[derive(Debug, Clone)]
struct Weights {
    keyword: f32,
    fuzzy: f32,
    vector: f32,
    bm25: f32,
    sparse: f32,
}

impl Weights {
    fn normalize(&mut self) {
        self.keyword = self.keyword.max(0.0);
        self.fuzzy = self.fuzzy.max(0.0);
        self.vector = self.vector.max(0.0);
        self.bm25 = self.bm25.max(0.0);
        self.sparse = self.sparse.max(0.0);

        let sum = self.keyword + self.fuzzy + self.vector + self.bm25 + self.sparse;
        if sum <= f32::EPSILON {
            self.keyword = 1.0;
            self.fuzzy = 0.0;
            self.vector = 0.0;
            self.bm25 = 0.0;
            self.sparse = 0.0;
            return;
        }

        self.keyword /= sum;
        self.fuzzy /= sum;
        self.vector /= sum;
        self.bm25 /= sum;
        self.sparse /= sum;
    }
}

pub async fn handle(cmd: FeedbackCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    if cmd.stats {
        return handle_stats(cmd, db).await;
    }
    if cmd.export {
        return handle_export(cmd, db).await;
    }
    if let Some(path) = cmd.import.clone() {
        return handle_import(cmd, db, &path).await;
    }
    if cmd.tune {
        return handle_tune(cmd, config, db).await;
    }
    if cmd.interactive {
        return handle_interactive(cmd, db).await;
    }
    handle_rate(cmd, db).await
}

pub fn write_last_session(session_id: &str) -> anyhow::Result<()> {
    let path = search_state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&LastSearchSessionState {
        session_id: session_id.to_string(),
    })?;
    std::fs::write(path, content)?;
    Ok(())
}

fn read_last_session() -> anyhow::Result<String> {
    let path = search_state_path()?;
    let content = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "No recent search session found at {}. Pass an explicit session id like `mmry feedback calm-fox +1 -3`.",
            path.display()
        )
    })?;
    let state: LastSearchSessionState = serde_json::from_str(&content)?;
    Ok(state.session_id)
}

fn search_state_path() -> anyhow::Result<PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::state_dir)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("state")
        });
    Ok(base.join("mmry").join("last_search_session.json"))
}

async fn handle_interactive(cmd: FeedbackCmd, db: &Database) -> anyhow::Result<()> {
    if cmd.stdin {
        anyhow::bail!("--interactive cannot be combined with --stdin");
    }

    let parsed = parse_feedback_args(&cmd.args)?;
    if !parsed.ratings.is_empty() {
        anyhow::bail!("Do not pass +N/-N ratings with --interactive");
    }

    let session = match parsed.session {
        Some(session) => session,
        None => read_last_session()?,
    };

    let results = operations::get_search_session_results(db.pool(), &session).await?;
    if results.is_empty() {
        anyhow::bail!("No search results found for session '{session}'");
    }

    let stored = if cmd.full {
        handle_interactive_full(db, &session, &results).await?
    } else {
        handle_interactive_pick_relevant(db, &session, &results).await?
    };

    let output = FeedbackOutput {
        session,
        ratings: stored,
    };

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if output.ratings.is_empty() {
        println!("No ratings stored.");
    } else {
        println!(
            "Stored {} rating(s) for search {}",
            output.ratings.len(),
            output.session
        );
    }

    Ok(())
}

async fn handle_interactive_pick_relevant(
    db: &Database,
    session: &str,
    results: &[operations::SearchSessionResultDetail],
) -> anyhow::Result<Vec<operations::SearchFeedbackRecord>> {
    let theme = ColorfulTheme::default();
    let items = results
        .iter()
        .map(|result| {
            format!(
                "{}. {}",
                result.rank,
                result
                    .content
                    .as_deref()
                    .unwrap_or("<memory content unavailable>")
            )
        })
        .collect::<Vec<_>>();
    let defaults = results
        .iter()
        .map(|result| result.relevant.unwrap_or(false))
        .collect::<Vec<_>>();

    let selected = MultiSelect::with_theme(&theme)
        .with_prompt("Pick the relevant results (space to toggle, enter to save)")
        .items(&items)
        .defaults(&defaults)
        .interact()?;

    let mut stored = Vec::with_capacity(selected.len());
    for index in selected {
        let result = &results[index];
        let feedback =
            operations::upsert_search_feedback(db.pool(), session, result.rank, true, None).await?;
        stored.push(feedback);
    }

    Ok(stored)
}

async fn handle_interactive_full(
    db: &Database,
    session: &str,
    results: &[operations::SearchSessionResultDetail],
) -> anyhow::Result<Vec<operations::SearchFeedbackRecord>> {
    let theme = ColorfulTheme::default();
    let mut stored = Vec::new();

    for result in results {
        println!();
        println!("{}. [{}]", result.rank, result.memory_id);
        println!(
            "   {}",
            result
                .content
                .as_deref()
                .unwrap_or("<memory content unavailable>")
        );
        if let Some(existing) = result.relevant {
            println!(
                "   Current rating: {}",
                if existing { "relevant" } else { "irrelevant" }
            );
        }

        let options = ["Relevant", "Irrelevant", "Skip", "Finish now"];
        let default = match result.relevant {
            Some(true) => 0,
            Some(false) => 1,
            None => 2,
        };
        let selection = Select::with_theme(&theme)
            .with_prompt(format!("Rate result {}", result.rank))
            .items(&options)
            .default(default)
            .interact()?;

        match selection {
            0 | 1 => {
                let relevant = selection == 0;
                let feedback = operations::upsert_search_feedback(
                    db.pool(),
                    session,
                    result.rank,
                    relevant,
                    None,
                )
                .await?;
                stored.push(feedback);
            }
            2 => {}
            3 => break,
            _ => unreachable!(),
        }
    }

    Ok(stored)
}

async fn handle_rate(cmd: FeedbackCmd, db: &Database) -> anyhow::Result<()> {
    let input = if cmd.stdin {
        let mut raw = String::new();
        std::io::stdin().read_to_string(&mut raw)?;
        serde_json::from_str::<FeedbackInput>(&raw)?
    } else {
        parse_feedback_args(&cmd.args)?
    };

    let session = match input.session {
        Some(session) => session,
        None => read_last_session()?,
    };

    if input.ratings.is_empty() {
        anyhow::bail!("No ratings provided. Example: mmry feedback calm-fox +1 -3");
    }

    let mut stored = Vec::with_capacity(input.ratings.len());
    for rating in input.ratings {
        stored.push(
            operations::upsert_search_feedback(
                db.pool(),
                &session,
                rating.rank,
                rating.relevant,
                input.agent_id,
            )
            .await?,
        );
    }

    let output = FeedbackOutput {
        session,
        ratings: stored,
    };

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Stored {} rating(s) for search {}",
            output.ratings.len(),
            output.session
        );
        let summary = output
            .ratings
            .iter()
            .map(|rating| {
                format!(
                    "{}:{}",
                    rating.rank,
                    if rating.relevant {
                        "relevant"
                    } else {
                        "irrelevant"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("{summary}");
    }

    Ok(())
}

async fn handle_stats(cmd: FeedbackCmd, db: &Database) -> anyhow::Result<()> {
    let stats = operations::search_feedback_stats(db.pool()).await?;
    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("Sessions with feedback: {}", stats.sessions_with_feedback);
        println!("Total ratings: {}", stats.total_feedback);
        println!("Relevant: {}", stats.relevant_feedback);
        println!("Irrelevant: {}", stats.irrelevant_feedback);
    }
    Ok(())
}

async fn handle_export(cmd: FeedbackCmd, db: &Database) -> anyhow::Result<()> {
    let rows = operations::export_search_benchmark(db.pool()).await?;
    let json = if cmd.json {
        serde_json::to_string_pretty(&rows)?
    } else {
        serde_json::to_string_pretty(&rows)?
    };
    println!("{json}");
    Ok(())
}

async fn handle_import(cmd: FeedbackCmd, db: &Database, path: &PathBuf) -> anyhow::Result<()> {
    let content = if path.as_os_str() == "-" {
        let mut raw = String::new();
        std::io::stdin().read_to_string(&mut raw)?;
        raw
    } else {
        std::fs::read_to_string(path)?
    };
    let rows: Vec<operations::SearchBenchmarkRow> = serde_json::from_str(&content)?;
    operations::import_search_benchmark(db.pool(), &rows).await?;

    if cmd.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "imported": rows.len() }))?
        );
    } else {
        println!("Imported {} benchmark rows", rows.len());
    }
    Ok(())
}

async fn handle_tune(cmd: FeedbackCmd, config: &Config, db: &Database) -> anyhow::Result<()> {
    let rows = operations::export_search_benchmark(db.pool()).await?;
    if rows.is_empty() {
        anyhow::bail!("No benchmark rows available yet. Rate search results first.");
    }

    let mut weights = Weights {
        keyword: config.search.keyword_weight,
        fuzzy: config.search.fuzzy_weight,
        vector: config.search.vector_weight,
        bm25: config.search.bm25_weight,
        sparse: config.search.sparse_embedding_weight,
    };
    weights.normalize();

    let baseline = benchmark_score(&rows, &weights);
    let mut best_weights = weights.clone();
    let mut best_score = baseline;

    for step in [0.10_f32, 0.05, 0.02, 0.01] {
        let mut improved = true;
        while improved {
            improved = false;
            for idx in 0..5 {
                for delta in [step, -step] {
                    let mut candidate = best_weights.clone();
                    adjust_weight(&mut candidate, idx, delta);
                    candidate.normalize();
                    let candidate_score = benchmark_score(&rows, &candidate);
                    if candidate_score > best_score + 0.0001 {
                        best_score = candidate_score;
                        best_weights = candidate;
                        improved = true;
                    }
                }
            }
        }
    }

    if cmd.apply {
        let mut updated = config.clone();
        updated.search.keyword_weight = best_weights.keyword;
        updated.search.fuzzy_weight = best_weights.fuzzy;
        updated.search.vector_weight = best_weights.vector;
        updated.search.bm25_weight = best_weights.bm25;
        updated.search.sparse_embedding_weight = best_weights.sparse;
        updated.save_to_path(&config_path_from_env_or_default()?)?;
    }

    let output = serde_json::json!({
        "rows": rows.len(),
        "baseline_ndcg_at_10": baseline,
        "tuned_ndcg_at_10": best_score,
        "weights": {
            "keyword": best_weights.keyword,
            "fuzzy": best_weights.fuzzy,
            "vector": best_weights.vector,
            "bm25": best_weights.bm25,
            "sparse": best_weights.sparse,
        },
        "applied": cmd.apply,
    });

    if cmd.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Loaded {} benchmark rows", rows.len());
        println!("Baseline NDCG@10: {:.4}", baseline);
        println!("Tuned NDCG@10: {:.4}", best_score);
        println!(
            "Weights: keyword={:.3} fuzzy={:.3} vector={:.3} bm25={:.3} sparse={:.3}",
            best_weights.keyword,
            best_weights.fuzzy,
            best_weights.vector,
            best_weights.bm25,
            best_weights.sparse
        );
        if cmd.apply {
            println!("Applied tuned weights to config");
        }
    }

    Ok(())
}

fn parse_feedback_args(args: &[String]) -> anyhow::Result<FeedbackInput> {
    if args.is_empty() {
        return Ok(FeedbackInput {
            session: None,
            ratings: Vec::new(),
            agent_id: None,
        });
    }

    let (session, rating_args) = if is_rating_token(&args[0]) {
        (None, args)
    } else {
        (Some(args[0].clone()), &args[1..])
    };

    let mut ratings = Vec::new();
    for arg in rating_args {
        ratings.push(parse_rating(arg)?);
    }

    Ok(FeedbackInput {
        session,
        ratings,
        agent_id: None,
    })
}

fn is_rating_token(value: &str) -> bool {
    value.starts_with('+') || value.starts_with('-')
}

fn parse_rating(value: &str) -> anyhow::Result<FeedbackRating> {
    if value.len() < 2 {
        anyhow::bail!("Invalid rating token '{value}'");
    }

    let relevant = match value.chars().next() {
        Some('+') => true,
        Some('-') => false,
        _ => anyhow::bail!("Invalid rating token '{value}'. Use +N or -N."),
    };
    let rank = value[1..]
        .parse::<i64>()
        .with_context(|| format!("Invalid rank in rating token '{value}'"))?;

    if rank <= 0 {
        anyhow::bail!("Rank must be positive in rating token '{value}'");
    }

    Ok(FeedbackRating { rank, relevant })
}

fn adjust_weight(weights: &mut Weights, idx: usize, delta: f32) {
    match idx {
        0 => weights.keyword += delta,
        1 => weights.fuzzy += delta,
        2 => weights.vector += delta,
        3 => weights.bm25 += delta,
        4 => weights.sparse += delta,
        _ => {}
    }
}

fn benchmark_score(rows: &[operations::SearchBenchmarkRow], weights: &Weights) -> f32 {
    let mut grouped: BTreeMap<&str, Vec<&operations::SearchBenchmarkRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(&row.session_id).or_default().push(row);
    }

    let mut total = 0.0_f32;
    let mut count = 0_usize;
    for group in grouped.values() {
        let score = ndcg_at_10(group, weights);
        if score.is_finite() {
            total += score;
            count += 1;
        }
    }

    if count == 0 {
        0.0
    } else {
        total / count as f32
    }
}

fn ndcg_at_10(rows: &[&operations::SearchBenchmarkRow], weights: &Weights) -> f32 {
    let mut scored = rows
        .iter()
        .map(|row| {
            let combined = (row.keyword_score * weights.keyword
                + row.fuzzy_score * weights.fuzzy
                + row.vector_score * weights.vector
                + row.bm25_score * weights.bm25
                + row.sparse_embedding_score * weights.sparse
                + row.recency_boost
                + row.importance_boost)
                * row.trust_multiplier
                + row.reinforcement_boost;
            (combined, row.relevant)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let dcg = scored
        .iter()
        .take(10)
        .enumerate()
        .map(|(idx, (_, relevant))| {
            let rel = if *relevant { 1.0_f32 } else { 0.0_f32 };
            rel / ((idx as f32) + 2.0).log2()
        })
        .sum::<f32>();

    let relevant_count = rows.iter().filter(|row| row.relevant).count();
    if relevant_count == 0 {
        return 0.0;
    }
    let idcg = (0..relevant_count.min(10))
        .map(|idx| 1.0_f32 / ((idx as f32) + 2.0).log2())
        .sum::<f32>();

    if idcg <= f32::EPSILON {
        0.0
    } else {
        dcg / idcg
    }
}

fn config_path_from_env_or_default() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("MMRY_CONFIG") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base.join("mmry").join("config.toml"))
}
