use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agents::BridgeBlock;
use crate::agents::FactCategory;
use crate::agents::FactRecord;
use crate::config::GuardrailsConfig;
use crate::config::SearchMode;
use crate::database::operations;
use crate::guardrails::GuardrailsAccumulator;
use crate::guardrails::GuardrailsSummary;
use crate::profile_blocks::ProfileBlock;
use crate::profile_blocks::ProfileBlocksService;
use crate::search::SearchService;
use crate::stores::MemoryWithStore;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackBudgets {
    pub profile_chars: usize,
    pub memories_chars: usize,
    pub facts_chars: usize,
    pub bridge_blocks_chars: usize,
}

impl Default for ContextPackBudgets {
    fn default() -> Self {
        Self {
            profile_chars: 4_000,
            memories_chars: 12_000,
            facts_chars: 4_000,
            bridge_blocks_chars: 4_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackOptions<'a> {
    pub query: &'a str,
    pub category: Option<&'a str>,
    pub limit: i64,
    pub mode: SearchMode,
    pub rerank: bool,
    pub store: Option<&'a str>,
    pub owner_id: Option<Uuid>,
    pub span_id: Option<&'a str>,
    pub budgets: ContextPackBudgets,
    pub redact_secrets: bool,
    #[serde(default)]
    pub guardrails: GuardrailsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub query: String,
    pub generated_at: DateTime<Utc>,
    pub profile_blocks: Vec<ProfileBlock>,
    pub profile_rendered: String,
    pub memories: Vec<MemoryWithStore>,
    pub facts: Vec<FactRecord>,
    pub bridge_blocks: Vec<BridgeBlock>,
    #[serde(default)]
    pub guardrails: GuardrailsSummary,
    pub redactions: ContextPackRedactions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackRedactions {
    pub redacted_facts: usize,
}

pub async fn build_context_pack(
    pool: &SqlitePool,
    profile_blocks: &ProfileBlocksService,
    search: &SearchService,
    opts: ContextPackOptions<'_>,
) -> Result<ContextPack> {
    let generated_at = Utc::now();
    let mut guard = GuardrailsAccumulator::new(&opts.guardrails);

    let mut searched = search
        .search_with_options(
            opts.query,
            opts.category,
            opts.limit,
            Some(opts.mode),
            Some(opts.rerank),
        )
        .await?;

    for memory in &mut searched {
        memory.embedding = None;
        memory.sparse_embedding = None;
    }

    let store = opts.store.unwrap_or_default();
    let mut memories = searched
        .into_iter()
        .map(|memory| MemoryWithStore {
            memory,
            store: store.to_string(),
        })
        .collect::<Vec<_>>();
    memories = guard.filter_memories_with_store(memories);
    memories = apply_memory_budget(memories, opts.budgets.memories_chars);

    let mut facts = Vec::new();
    for memory in &memories {
        let mut rows = operations::get_facts_for_memory(pool, memory.memory.id, 25).await?;
        facts.append(&mut rows);
    }
    facts.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));

    let mut redacted_facts = 0;
    if opts.redact_secrets {
        let before = facts.len();
        facts.retain(|f| f.category != FactCategory::Secret);
        redacted_facts = before.saturating_sub(facts.len());
    }
    facts = guard.filter_facts(facts);
    facts = apply_facts_budget(facts, opts.budgets.facts_chars);

    let mut blocks: Vec<BridgeBlock> = Vec::new();
    for memory in &memories {
        let events = operations::get_agent_events_for_memory(pool, memory.memory.id, 10).await?;
        for event in events {
            let Some(span_id) = event.span_id.as_deref() else {
                continue;
            };
            if blocks.iter().any(|b| b.span_id.as_deref() == Some(span_id)) {
                continue;
            }
            if let Ok(Some(block)) = operations::get_bridge_block_by_span(pool, span_id).await {
                blocks.push(block);
            }
        }
    }
    if let Some(span_id) = opts.span_id {
        if !blocks.iter().any(|b| b.span_id.as_deref() == Some(span_id)) {
            if let Ok(Some(block)) = operations::get_bridge_block_by_span(pool, span_id).await {
                blocks.push(block);
            }
        }
    }
    blocks = apply_bridge_blocks_budget(blocks, opts.budgets.bridge_blocks_chars);

    let mut profile_blocks_list = Vec::new();
    if let Some(owner_id) = opts.owner_id {
        profile_blocks_list = profile_blocks.list_blocks(pool, owner_id).await?;
        profile_blocks_list = apply_profile_budget(profile_blocks_list, opts.budgets.profile_chars);
    }
    let profile_rendered = profile_blocks.render_for_prompt(&profile_blocks_list);

    Ok(ContextPack {
        query: opts.query.to_string(),
        generated_at,
        profile_blocks: profile_blocks_list,
        profile_rendered,
        memories,
        facts,
        bridge_blocks: blocks,
        guardrails: guard.summary(),
        redactions: ContextPackRedactions { redacted_facts },
    })
}

fn apply_profile_budget(mut blocks: Vec<ProfileBlock>, budget: usize) -> Vec<ProfileBlock> {
    let mut used = 0usize;
    blocks.retain_mut(|b| {
        if used >= budget {
            return false;
        }
        let remaining = budget - used;
        if b.content.chars().count() > remaining {
            b.content = b.content.chars().take(remaining).collect();
            used = budget;
            true
        } else {
            used += b.content.chars().count();
            true
        }
    });
    blocks
}

fn apply_memory_budget(mut memories: Vec<MemoryWithStore>, budget: usize) -> Vec<MemoryWithStore> {
    let mut used = 0usize;
    memories.retain_mut(|m| {
        if used >= budget {
            return false;
        }
        let remaining = budget - used;
        let chars = m.memory.content.chars().count();
        if chars > remaining {
            m.memory.content = m.memory.content.chars().take(remaining).collect();
            used = budget;
            true
        } else {
            used += chars;
            true
        }
    });
    memories
}

fn apply_facts_budget(mut facts: Vec<FactRecord>, budget: usize) -> Vec<FactRecord> {
    let mut used = 0usize;
    facts.retain_mut(|f| {
        if used >= budget {
            return false;
        }
        let rendered = format!("{}: {}", f.fact_key, f.fact_value);
        let chars = rendered.chars().count();
        let remaining = budget - used;
        if chars > remaining {
            let truncated = rendered.chars().take(remaining).collect::<String>();
            let mut parts = truncated.splitn(2, ':');
            f.fact_key = parts.next().unwrap_or_default().to_string();
            f.fact_value = parts.next().unwrap_or_default().trim().to_string();
            used = budget;
            true
        } else {
            used += chars;
            true
        }
    });
    facts
}

fn apply_bridge_blocks_budget(mut blocks: Vec<BridgeBlock>, budget: usize) -> Vec<BridgeBlock> {
    let mut used = 0usize;
    blocks.retain_mut(|b| {
        if used >= budget {
            return false;
        }
        let rendered = serde_json::to_string(&b.content).unwrap_or_default();
        let chars = rendered.chars().count();
        let remaining = budget - used;
        if chars > remaining {
            b.content = serde_json::Value::String(rendered.chars().take(remaining).collect());
            used = budget;
            true
        } else {
            used += chars;
            true
        }
    });
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentEvent;
    use crate::agents::AgentRecord;
    use crate::database::Database;
    use crate::embeddings::EmbeddingServiceWrapper;
    use crate::memory::Memory;
    use crate::reranker::RerankerService;
    use crate::sparse_embeddings::SparseEmbeddingService;

    #[tokio::test]
    async fn context_pack_redacts_secret_facts() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut config = crate::config::Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;

        let db = Database::init_store(&config, None).await?;

        let owner = Uuid::new_v4();
        let mut owner_agent = AgentRecord::new("owner", "human");
        owner_agent.id = owner;
        operations::upsert_agent(db.pool(), &owner_agent).await?;
        let profile = ProfileBlocksService::from_config(&config);
        profile
            .set_block(
                db.pool(),
                owner,
                "persona",
                "You are helpful.".to_string(),
                crate::profile_blocks::ProfileBlockWriteContext {
                    scope: crate::profile_blocks::ProfileBlockScope::Global,
                    actor_id: owner,
                    span_id: None,
                },
            )
            .await?;

        let memory = Memory::new(
            crate::memory::MemoryType::Episodic,
            "hello".into(),
            "default".into(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        let mut event = AgentEvent::new(owner, "fact_extract");
        event.id = Uuid::new_v4();
        event.memory_id = Some(memory.id);
        operations::record_agent_event(db.pool(), &event).await?;

        let mut secret = FactRecord::new("api_key", "secret");
        secret.category = FactCategory::Secret;
        secret.turn_id = Some(event.id.to_string());
        operations::upsert_fact(db.pool(), &secret).await?;

        let embeddings = std::sync::Arc::new(tokio::sync::Mutex::new(
            EmbeddingServiceWrapper::new(&config)?,
        ));
        let sparse = std::sync::Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let search = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            embeddings,
            sparse,
            std::sync::Arc::new(RerankerService::from_config(&config.search)?),
        );

        let pack = build_context_pack(
            db.pool(),
            &profile,
            &search,
            ContextPackOptions {
                query: "hello",
                category: None,
                limit: 5,
                mode: SearchMode::Keyword,
                rerank: false,
                store: Some("test"),
                owner_id: Some(owner),
                span_id: None,
                budgets: ContextPackBudgets {
                    profile_chars: 100,
                    memories_chars: 100,
                    facts_chars: 100,
                    bridge_blocks_chars: 100,
                },
                redact_secrets: true,
                guardrails: config.guardrails.clone(),
            },
        )
        .await?;

        assert_eq!(pack.redactions.redacted_facts, 1);
        assert!(pack.facts.is_empty());
        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn context_pack_applies_guardrails() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut config = crate::config::Config::default();
        config.stores.directory = temp.path().join("stores");
        config.stores.default = "test".to_string();
        config.embeddings.enabled = false;
        config.sparse_embeddings.enabled = false;
        config.guardrails.enabled = true;
        config.guardrails.patterns = vec![crate::config::GuardPattern {
            pattern: "secret".to_string(),
            kind: crate::config::GuardPatternKind::Literal,
            reason: None,
        }];

        let db = Database::init_store(&config, None).await?;

        let agent_id = Uuid::new_v4();
        let mut agent = AgentRecord::new("owner", "human");
        agent.id = agent_id;
        operations::upsert_agent(db.pool(), &agent).await?;

        let public_memory = Memory::new(
            crate::memory::MemoryType::Episodic,
            "public memory".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(db.pool(), &public_memory).await?;

        let secret_memory = Memory::new(
            crate::memory::MemoryType::Episodic,
            "secret memory".to_string(),
            "default".to_string(),
        );
        operations::insert_memory(db.pool(), &secret_memory).await?;

        let mut event = AgentEvent::new(agent_id, "fact_extract");
        event.id = Uuid::new_v4();
        event.memory_id = Some(public_memory.id);
        operations::record_agent_event(db.pool(), &event).await?;

        let mut fact = FactRecord::new("note", "secret value");
        fact.turn_id = Some(event.id.to_string());
        operations::upsert_fact(db.pool(), &fact).await?;

        let profile = ProfileBlocksService::from_config(&config);
        let embeddings = std::sync::Arc::new(tokio::sync::Mutex::new(
            EmbeddingServiceWrapper::new(&config)?,
        ));
        let sparse = std::sync::Arc::new(SparseEmbeddingService::new(&config.sparse_embeddings)?);
        let search = SearchService::new(
            db.pool().clone(),
            config.search.clone(),
            embeddings,
            sparse,
            std::sync::Arc::new(RerankerService::from_config(&config.search)?),
        );

        let pack = build_context_pack(
            db.pool(),
            &profile,
            &search,
            ContextPackOptions {
                query: "memory",
                category: None,
                limit: 5,
                mode: SearchMode::Keyword,
                rerank: false,
                store: Some("test"),
                owner_id: None,
                span_id: None,
                budgets: ContextPackBudgets {
                    profile_chars: 100,
                    memories_chars: 500,
                    facts_chars: 100,
                    bridge_blocks_chars: 100,
                },
                redact_secrets: false,
                guardrails: config.guardrails.clone(),
            },
        )
        .await?;

        assert_eq!(pack.memories.len(), 1);
        assert!(pack.memories[0].memory.content.contains("public"));
        assert!(pack.facts.is_empty());
        assert_eq!(pack.guardrails.blocked_memories, 1);
        assert_eq!(pack.guardrails.blocked_facts, 1);
        assert_eq!(pack.guardrails.triggered_patterns.len(), 1);

        db.close().await;
        Ok(())
    }
}
