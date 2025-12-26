use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::database::operations;
use crate::memory::Memory;
use crate::memory::MemoryType;
use crate::memory::SourceAttribution;
use crate::memory::SourceEntry;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationTurn {
    #[serde(default)]
    pub role: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SummarizePruneOptions {
    pub max_turns: usize,
    pub summary_max_words: usize,
    pub per_turn_max_words: usize,
}

impl Default for SummarizePruneOptions {
    fn default() -> Self {
        Self {
            max_turns: 20,
            summary_max_words: 200,
            per_turn_max_words: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummarizePruneResult {
    pub summary: String,
    pub retained: Vec<ConversationTurn>,
    pub pruned_count: usize,
}

pub fn summarize_and_prune(
    turns: Vec<ConversationTurn>,
    opts: SummarizePruneOptions,
) -> SummarizePruneResult {
    if turns.len() <= opts.max_turns {
        return SummarizePruneResult {
            summary: String::new(),
            retained: turns,
            pruned_count: 0,
        };
    }

    let split_at = turns.len().saturating_sub(opts.max_turns);
    let (pruned, retained) = turns.split_at(split_at);
    let summary = summarize_pruned(pruned, opts.summary_max_words, opts.per_turn_max_words);

    SummarizePruneResult {
        summary,
        retained: retained.to_vec(),
        pruned_count: pruned.len(),
    }
}

fn summarize_pruned(
    pruned: &[ConversationTurn],
    max_words: usize,
    per_turn_max_words: usize,
) -> String {
    if max_words == 0 {
        return String::new();
    }

    let header = "Earlier turns summary:";
    let header_words = header.split_whitespace().count();
    if header_words >= max_words {
        return truncate_words(header.to_string(), max_words);
    }

    let mut out = String::new();
    out.push_str(header);
    out.push('\n');

    let mut used_words = header_words;
    for turn in pruned {
        if used_words >= max_words {
            break;
        }

        let role = turn
            .role
            .as_deref()
            .map(|r| r.trim())
            .filter(|r| !r.is_empty())
            .unwrap_or("turn");

        let snippet = truncate_words(normalize_whitespace(&turn.content), per_turn_max_words);
        if snippet.is_empty() {
            continue;
        }

        let prefix = format!("- [{role}]");
        let prefix_words = prefix.split_whitespace().count();
        let remaining = max_words.saturating_sub(used_words);
        if remaining <= prefix_words {
            break;
        }

        let snippet_budget = remaining.saturating_sub(prefix_words);
        let snippet_truncated = truncate_words(snippet, snippet_budget);
        if snippet_truncated.is_empty() {
            break;
        }

        out.push_str(&prefix);
        out.push(' ');
        out.push_str(&snippet_truncated);
        out.push('\n');
        used_words += prefix_words + snippet_truncated.split_whitespace().count();
    }

    out.trim_end().to_string()
}

fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_words(input: String, max_words: usize) -> String {
    if max_words == 0 {
        return String::new();
    }
    input
        .split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct PersistedSummary {
    pub memory: Memory,
    pub event: AgentEvent,
}

pub async fn persist_summary(
    pool: &SqlitePool,
    agent_id: Uuid,
    span_id: Option<String>,
    summary: String,
    category: String,
    payload: Value,
) -> Result<PersistedSummary> {
    ensure_agent_exists(pool, agent_id).await?;

    let mut memory = Memory::new(MemoryType::Episodic, summary, category);
    memory.source_attribution = Some(SourceAttribution::new(vec![SourceEntry::llm(
        "conversation_summary",
        0.7,
        None,
    )]));
    memory.recompute_trust_metrics();
    operations::insert_memory(pool, &memory).await?;

    let mut event = AgentEvent::new(agent_id, "conversation_summary");
    event.span_id = span_id;
    event.memory_id = Some(memory.id);
    event.payload = payload;
    operations::record_agent_event(pool, &event).await?;

    Ok(PersistedSummary { memory, event })
}

async fn ensure_agent_exists(pool: &SqlitePool, agent_id: Uuid) -> Result<()> {
    if operations::get_agent(pool, agent_id).await?.is_some() {
        return Ok(());
    }

    let mut record = AgentRecord::new("agent", "external");
    record.id = agent_id;
    operations::upsert_agent(pool, &record).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_and_prune_respects_max_turns_and_budget() {
        let turns = (0..6)
            .map(|idx| ConversationTurn {
                role: Some(if idx % 2 == 0 { "user" } else { "assistant" }.to_string()),
                content: format!("turn {idx} has a bunch of words for summarization"),
            })
            .collect::<Vec<_>>();

        let result = summarize_and_prune(
            turns,
            SummarizePruneOptions {
                max_turns: 2,
                summary_max_words: 10,
                per_turn_max_words: 5,
            },
        );

        assert_eq!(result.retained.len(), 2);
        assert_eq!(result.pruned_count, 4);
        assert!(result.summary.split_whitespace().count() <= 10);
    }
}
