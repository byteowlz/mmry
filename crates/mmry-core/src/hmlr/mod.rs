//! HMLR (Hierarchical Memory Ledger with Routing) enrichment pipeline
//!
//! This module implements the HMLR pattern for memory enrichment:
//! - Governor: Central orchestrator that coordinates parallel tasks
//! - FactScrubber: Extracts key-value facts from memory content
//! - Scribe: Maintains user profile (async fire-and-forget)
//! - LatticeCrawler: Finds candidate bridge blocks via search
//! - ContextHydrator: Assembles final context from multiple sources
//! - Benchmarks: RAGAS-style tests for memory system quality

#[cfg(feature = "bench")]
pub mod benchmarks;
mod context_hydrator;
mod fact_scrubber;
mod governor;
mod lattice_crawler;
pub mod prompts;
mod scribe;

pub use context_hydrator::ContextHydrator;
pub use context_hydrator::HydratedContext;
pub use context_hydrator::SynthesisOptions;
pub use context_hydrator::SynthesisResult;
pub use fact_scrubber::FactScrubber;
pub use governor::Governor;
pub use governor::GovernorDecision;
pub use lattice_crawler::LatticeCrawler;
pub use scribe::Scribe;

use crate::agents::AgentEvent;
use crate::agents::AgentRecord;
use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::analysis::Analyzer;
use crate::config::Config;
use crate::config::HmlrConfig;
use crate::database::operations;
use crate::memory::Memory;
use crate::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use uuid::Uuid;

/// Context for HMLR enrichment operations
#[derive(Debug, Clone)]
pub struct HmlrContext {
    /// Who created this memory (human or agent)
    pub creator_id: Uuid,
    /// Optional: Previous memories in conversation (for routing)
    pub conversation_history: Vec<Memory>,
    /// Optional: Query/prompt that led to this memory
    pub query: Option<String>,
}

impl HmlrContext {
    /// Create a new context for a human operator
    pub fn for_human(creator_id: Uuid) -> Self {
        Self {
            creator_id,
            conversation_history: Vec::new(),
            query: None,
        }
    }

    /// Create a new context for an agent with conversation history
    pub fn for_agent(creator_id: Uuid, query: Option<String>, history: Vec<Memory>) -> Self {
        Self {
            creator_id,
            conversation_history: history,
            query,
        }
    }
}

/// Result of HMLR enrichment
#[derive(Debug, Default)]
pub struct EnrichmentResult {
    /// Facts extracted from the memory
    pub facts: Vec<FactRecord>,
    /// Bridge block the memory was assigned to
    pub bridge_block: Option<BridgeBlock>,
    /// Audit event recorded
    pub event: Option<AgentEvent>,
}

/// Main HMLR pipeline that coordinates enrichment
pub struct HmlrPipeline {
    config: HmlrConfig,
    governor: Governor,
}

impl HmlrPipeline {
    /// Create a new HMLR pipeline with the given analyzer
    pub fn new(config: HmlrConfig, analyzer: Arc<dyn Analyzer>) -> Self {
        let governor = Governor::new(config.clone(), analyzer);
        Self { config, governor }
    }

    /// Check if HMLR is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Main enrichment hook - called after memory insertion
    pub async fn enrich_memory(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: HmlrContext,
    ) -> Result<EnrichmentResult> {
        if !self.config.enabled {
            return Ok(EnrichmentResult::default());
        }

        // Use the Governor to process the memory with parallel execution
        let decision = self.governor.process_memory(pool, memory, &context).await?;

        let mut result = EnrichmentResult {
            facts: decision.facts,
            bridge_block: decision.bridge_block,
            event: None,
        };

        // Record audit event if enabled
        if self.config.audit_trail {
            let event = self.record_event(pool, memory, &context, &result).await?;
            result.event = Some(event);
        }

        Ok(result)
    }

    /// Record an audit event for the memory creation
    async fn record_event(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
        enrichment: &EnrichmentResult,
    ) -> Result<AgentEvent> {
        let mut event = AgentEvent::new(context.creator_id, "memory_created");
        event.status = Some("success".to_string());
        event.memory_id = Some(memory.id);

        // Include enrichment details in payload
        event.payload = serde_json::json!({
            "memory_type": format!("{:?}", memory.memory_type),
            "category": memory.category,
            "importance": memory.importance,
            "facts_extracted": enrichment.facts.len(),
            "bridge_block_id": enrichment.bridge_block.as_ref().map(|b| b.block_id.to_string()),
            "had_query": context.query.is_some(),
            "conversation_history_len": context.conversation_history.len(),
        });

        if let Some(block) = &enrichment.bridge_block {
            event.span_id = block.span_id.clone();
        }

        operations::record_agent_event(pool, &event).await?;
        Ok(event)
    }
}

/// Generate an embedding for a bridge block based on its topic_label and keywords
/// This should be called after enrichment when the caller has access to the embedding service.
/// The embedding enables semantic routing when LLM routing is unavailable.
pub async fn generate_block_embedding<F, Fut>(
    pool: &SqlitePool,
    block: &BridgeBlock,
    embed_fn: F,
) -> Result<()>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Option<Vec<f32>>>>,
{
    let embed_text = block.embedding_text();
    if embed_text.is_empty() {
        tracing::debug!(block_id = %block.block_id, "Skipping embedding for block with no topic/keywords");
        return Ok(());
    }

    match embed_fn(embed_text).await? {
        Some(embedding) => {
            operations::update_bridge_block_embedding(pool, block.block_id, &embedding).await?;
            tracing::debug!(
                block_id = %block.block_id,
                embedding_dim = embedding.len(),
                "Generated embedding for bridge block"
            );
        }
        None => {
            tracing::debug!(block_id = %block.block_id, "Embedding service disabled, skipping block embedding");
        }
    }

    Ok(())
}

/// Get or create the human agent record for manual operations
pub async fn get_or_create_human_agent(pool: &SqlitePool, config: &Config) -> Result<Uuid> {
    let agent_name = &config.hmlr.human_agent_name;

    // Try to find existing
    if let Some(agent) = operations::get_agent_by_name(pool, agent_name).await? {
        return Ok(agent.id);
    }

    // Create new human agent
    let mut agent = AgentRecord::new(agent_name.clone(), "human_operator".to_string());
    agent.description = Some("Manual memory operations via CLI/TUI".to_string());

    operations::upsert_agent(pool, &agent).await?;
    Ok(agent.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::NoOpAnalyzer;
    use crate::database::Database;
    use crate::memory::MemoryType;
    use tempfile::tempdir;

    async fn setup_test_db() -> anyhow::Result<(tempfile::TempDir, Database)> {
        let temp = tempdir()?;
        let db_path = temp.path().join("test.db");
        let db = Database::init(&db_path, 384).await?;
        Ok((temp, db))
    }

    #[tokio::test]
    async fn test_hmlr_disabled_returns_empty_result() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let config = HmlrConfig {
            enabled: false,
            ..Default::default()
        };

        let pipeline = HmlrPipeline::new(config, Arc::new(NoOpAnalyzer));
        let memory = Memory::new(
            MemoryType::Episodic,
            "Test content".to_string(),
            "test".to_string(),
        );
        let context = HmlrContext::for_human(Uuid::new_v4());

        let result = pipeline.enrich_memory(db.pool(), &memory, context).await?;

        assert!(result.facts.is_empty());
        assert!(result.bridge_block.is_none());
        assert!(result.event.is_none());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_hmlr_enabled_creates_bridge_block() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let config = HmlrConfig {
            enabled: true,
            extract_facts: true,
            bridge_routing: true,
            audit_trail: true,
            track_human_agent: true,
            human_agent_name: "test_human".to_string(),
            synthesis_interval_seconds: 0,
        };

        // Create the human agent first
        let human_id = {
            let agent = AgentRecord::new("test_human", "human_operator");
            operations::upsert_agent(db.pool(), &agent).await?;
            agent.id
        };

        let pipeline = HmlrPipeline::new(config, Arc::new(NoOpAnalyzer));
        let memory = Memory::new(
            MemoryType::Episodic,
            "Met with Sarah about the project".to_string(),
            "work".to_string(),
        );
        let context = HmlrContext::for_human(human_id);

        // Insert memory first (as would happen in real flow)
        operations::insert_memory(db.pool(), &memory).await?;

        let result = pipeline.enrich_memory(db.pool(), &memory, context).await?;

        // Should have created a bridge block (routing is enabled)
        assert!(result.bridge_block.is_some());
        let block = result.bridge_block.unwrap();
        assert_eq!(block.status, Some("active".to_string()));
        assert_eq!(block.agent_id, Some(human_id));

        // Should have recorded an audit event
        assert!(result.event.is_some());
        let event = result.event.unwrap();
        assert_eq!(event.event_type, "memory_created");
        assert_eq!(event.status, Some("success".to_string()));
        assert_eq!(event.memory_id, Some(memory.id));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_or_create_human_agent() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let mut config = Config::default();
        config.hmlr.human_agent_name = "test_human".to_string();

        // First call should create
        let id1 = get_or_create_human_agent(db.pool(), &config).await?;

        // Second call should return same ID
        let id2 = get_or_create_human_agent(db.pool(), &config).await?;

        assert_eq!(id1, id2);

        // Verify agent was created correctly
        let agent = operations::get_agent_by_name(db.pool(), "test_human").await?;
        assert!(agent.is_some());
        let agent = agent.unwrap();
        assert_eq!(agent.name, "test_human");
        assert_eq!(agent.kind, "human_operator");

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_hmlr_context_for_human() {
        let id = Uuid::new_v4();
        let context = HmlrContext::for_human(id);

        assert_eq!(context.creator_id, id);
        assert!(context.conversation_history.is_empty());
        assert!(context.query.is_none());
    }

    #[tokio::test]
    async fn test_hmlr_context_for_agent() {
        let id = Uuid::new_v4();
        let query = Some("What is the project status?".to_string());
        let history = vec![Memory::new(
            MemoryType::Episodic,
            "Previous context".to_string(),
            "test".to_string(),
        )];

        let context = HmlrContext::for_agent(id, query.clone(), history.clone());

        assert_eq!(context.creator_id, id);
        assert_eq!(context.query, query);
        assert_eq!(context.conversation_history.len(), 1);
    }
}
