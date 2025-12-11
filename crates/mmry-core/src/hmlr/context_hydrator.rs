//! ContextHydrator: Assembles context from multiple sources
//!
//! Merges context from:
//! - Active bridge blocks (full 5-10 turns)
//! - Inactive blocks (metadata only)
//! - Relevant facts
//! - User profile constraints
//!
//! Implements token budget management and returns HydratedContext.

use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::agents::UserProfileEntry;
use crate::database::operations;
use crate::memory::Memory;
use crate::Result;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Strategy for handling inactive bridge blocks
#[derive(Debug, Clone, Copy, Default)]
pub enum InactiveBlockStrategy {
    /// Return full memories from inactive blocks
    Full,
    /// Return only metadata (summary + keywords)
    #[default]
    MetadataOnly,
    /// Skip inactive blocks entirely
    Skip,
}

/// Options for context hydration
#[derive(Debug, Clone)]
pub struct HydrationOptions {
    /// Maximum tokens for the hydrated context (approximate)
    pub max_tokens: usize,
    /// Maximum memories to include from active block
    pub max_active_memories: usize,
    /// Strategy for inactive blocks
    pub inactive_strategy: InactiveBlockStrategy,
    /// Include user profile in context
    pub include_profile: bool,
    /// Maximum facts to include
    pub max_facts: usize,
}

impl Default for HydrationOptions {
    fn default() -> Self {
        Self {
            max_tokens: 4000,
            max_active_memories: 10,
            inactive_strategy: InactiveBlockStrategy::MetadataOnly,
            include_profile: true,
            max_facts: 20,
        }
    }
}

/// Assembled context from multiple sources
#[derive(Debug, Clone, Default)]
pub struct HydratedContext {
    /// Memories from active bridge block (full content)
    pub active_memories: Vec<Memory>,
    /// Metadata from inactive blocks
    pub inactive_block_metadata: Vec<BlockMetadata>,
    /// Relevant facts
    pub facts: Vec<FactRecord>,
    /// User profile (if available)
    pub user_profile: Option<UserProfileEntry>,
    /// Approximate token count
    pub estimated_tokens: usize,
}

/// Metadata for an inactive bridge block
#[derive(Debug, Clone)]
pub struct BlockMetadata {
    pub block_id: Uuid,
    pub topic_label: Option<String>,
    pub keywords: Vec<String>,
    pub memory_count: usize,
    pub span_id: Option<String>,
}

/// ContextHydrator assembles context from multiple sources
pub struct ContextHydrator {
    options: HydrationOptions,
}

impl ContextHydrator {
    /// Create a new ContextHydrator with default options
    pub fn new() -> Self {
        Self {
            options: HydrationOptions::default(),
        }
    }

    /// Create a ContextHydrator with custom options
    pub fn with_options(options: HydrationOptions) -> Self {
        Self { options }
    }

    /// Hydrate context for a query
    ///
    /// Assembles context from:
    /// 1. Active bridge block (if any)
    /// 2. Inactive blocks (based on strategy)
    /// 3. Relevant facts
    /// 4. User profile
    pub async fn hydrate(
        &self,
        pool: &SqlitePool,
        agent_id: Uuid,
        query: Option<&str>,
    ) -> Result<HydratedContext> {
        let mut context = HydratedContext::default();
        let mut remaining_tokens = self.options.max_tokens;

        // 1. Get active bridge block memories
        let blocks = operations::get_recent_bridge_blocks_for_agent(pool, agent_id, 10).await?;

        for block in &blocks {
            if block.status == Some("active".to_string()) {
                let memories = self.get_block_memories(pool, block).await?;
                for memory in memories.into_iter().take(self.options.max_active_memories) {
                    let tokens = estimate_tokens(&memory.content);
                    if tokens <= remaining_tokens {
                        remaining_tokens -= tokens;
                        context.active_memories.push(memory);
                    }
                }
            } else {
                // Handle inactive blocks based on strategy
                match self.options.inactive_strategy {
                    InactiveBlockStrategy::Full => {
                        let memories = self.get_block_memories(pool, block).await?;
                        for memory in memories {
                            let tokens = estimate_tokens(&memory.content);
                            if tokens <= remaining_tokens {
                                remaining_tokens -= tokens;
                                context.active_memories.push(memory);
                            }
                        }
                    }
                    InactiveBlockStrategy::MetadataOnly => {
                        let metadata = self.extract_block_metadata(pool, block).await?;
                        let tokens = estimate_tokens(&format!("{metadata:?}"));
                        if tokens <= remaining_tokens {
                            remaining_tokens -= tokens;
                            context.inactive_block_metadata.push(metadata);
                        }
                    }
                    InactiveBlockStrategy::Skip => {}
                }
            }
        }

        // 2. Get relevant facts
        if let Some(q) = query {
            let facts = operations::search_facts(pool, q, self.options.max_facts as i64).await?;
            for fact in facts {
                let tokens = estimate_tokens(&format!("{}: {}", fact.fact_key, fact.fact_value));
                if tokens <= remaining_tokens {
                    remaining_tokens -= tokens;
                    context.facts.push(fact);
                }
            }
        } else {
            // Get recent facts if no query
            let facts = operations::list_recent_facts(pool, self.options.max_facts as i64).await?;
            for fact in facts {
                let tokens = estimate_tokens(&format!("{}: {}", fact.fact_key, fact.fact_value));
                if tokens <= remaining_tokens {
                    remaining_tokens -= tokens;
                    context.facts.push(fact);
                }
            }
        }

        // 3. Get user profile
        if self.options.include_profile {
            if let Some(profile) = operations::get_user_profile(pool, agent_id).await? {
                let tokens = estimate_tokens(&profile.profile.to_string());
                if tokens <= remaining_tokens {
                    remaining_tokens -= tokens;
                    context.user_profile = Some(profile);
                }
            }
        }

        context.estimated_tokens = self.options.max_tokens - remaining_tokens;
        Ok(context)
    }

    /// Get memories referenced in a bridge block
    async fn get_block_memories(
        &self,
        pool: &SqlitePool,
        block: &BridgeBlock,
    ) -> Result<Vec<Memory>> {
        let mut memories = Vec::new();

        // Extract memory IDs from block content
        if let Some(memory_ids) = block.content.get("memory_ids") {
            if let Some(ids) = memory_ids.as_array() {
                for id_val in ids {
                    if let Some(id_str) = id_val.as_str() {
                        if let Ok(id) = Uuid::parse_str(id_str) {
                            if let Some(memory) = operations::get_memory(pool, id).await? {
                                memories.push(memory);
                            }
                        }
                    }
                }
            }
        }

        Ok(memories)
    }

    /// Extract metadata from a bridge block
    async fn extract_block_metadata(
        &self,
        pool: &SqlitePool,
        block: &BridgeBlock,
    ) -> Result<BlockMetadata> {
        let memories = self.get_block_memories(pool, block).await?;

        Ok(BlockMetadata {
            block_id: block.block_id,
            topic_label: block.topic_label.clone(),
            keywords: block.keywords.clone(),
            memory_count: memories.len(),
            span_id: block.span_id.clone(),
        })
    }
}

impl Default for ContextHydrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimate token count for a string (rough approximation)
fn estimate_tokens(text: &str) -> usize {
    // Rough estimate: ~4 characters per token for English text
    text.len() / 4 + 1
}

/// Synthesis result for a bridge block
#[derive(Debug, Clone)]
pub struct SynthesisResult {
    pub block_id: Uuid,
    pub summary: String,
    pub key_points: Vec<String>,
    pub action_items: Vec<String>,
    pub synthesized_at: chrono::DateTime<chrono::Utc>,
}

/// Synthesis options
#[derive(Debug, Clone)]
pub struct SynthesisOptions {
    /// Minimum memories before synthesis is triggered
    pub min_memories_for_synthesis: usize,
    /// Maximum age in hours for block to be eligible for synthesis
    pub max_age_hours: u64,
    /// Only synthesize closed/inactive blocks
    pub only_inactive: bool,
}

impl Default for SynthesisOptions {
    fn default() -> Self {
        Self {
            min_memories_for_synthesis: 5,
            max_age_hours: 24,
            only_inactive: true,
        }
    }
}

impl ContextHydrator {
    /// Find bridge blocks that are eligible for synthesis
    pub async fn find_synthesis_candidates(
        &self,
        pool: &SqlitePool,
        options: &SynthesisOptions,
    ) -> Result<Vec<BridgeBlock>> {
        let blocks = operations::list_bridge_blocks(pool, 100).await?;
        let now = chrono::Utc::now();
        let max_age = chrono::Duration::hours(options.max_age_hours as i64);

        let mut candidates = Vec::new();

        for block in blocks {
            // Skip if we only want inactive and this is active
            if options.only_inactive && block.status == Some("active".to_string()) {
                continue;
            }

            // Skip if block is too new
            if now.signed_duration_since(block.created_at) < max_age {
                continue;
            }

            // Check if block has enough memories
            let memories = self.get_block_memories(pool, &block).await?;
            if memories.len() >= options.min_memories_for_synthesis {
                candidates.push(block);
            }
        }

        Ok(candidates)
    }

    /// Prepare content for synthesis (to be processed by an analyzer/LLM)
    pub async fn prepare_for_synthesis(
        &self,
        pool: &SqlitePool,
        block: &BridgeBlock,
    ) -> Result<Vec<String>> {
        let memories = self.get_block_memories(pool, block).await?;
        Ok(memories.into_iter().map(|m| m.content).collect())
    }

    /// Update a bridge block with synthesis results
    pub async fn apply_synthesis(
        &self,
        pool: &SqlitePool,
        block: &mut BridgeBlock,
        result: &SynthesisResult,
    ) -> Result<()> {
        // Update block content with synthesis
        if let Some(obj) = block.content.as_object_mut() {
            obj.insert(
                "synthesis".to_string(),
                serde_json::json!({
                    "summary": result.summary,
                    "key_points": result.key_points,
                    "action_items": result.action_items,
                    "synthesized_at": result.synthesized_at.to_rfc3339()
                }),
            );
        }

        // Mark block as closed/synthesized
        block.status = Some("synthesized".to_string());

        // Save updated block
        operations::upsert_bridge_block(pool, block).await?;

        Ok(())
    }

    /// Update fact recency scores (decay older facts)
    pub async fn decay_fact_recency(&self, pool: &SqlitePool, decay_rate: f32) -> Result<usize> {
        let facts = operations::list_recent_facts(pool, 1000).await?;
        let mut updated = 0;

        for mut fact in facts {
            // Apply exponential decay
            let new_recency = fact.recency_score * (1.0 - decay_rate);
            if new_recency < 0.01 {
                // Don't update if already very low
                continue;
            }

            fact.recency_score = new_recency;
            operations::upsert_fact(pool, &fact).await?;
            updated += 1;
        }

        Ok(updated)
    }
}

impl HydratedContext {
    /// Format context as a string for LLM prompts
    pub fn to_prompt_context(&self) -> String {
        let mut parts = Vec::new();

        // User profile
        if let Some(profile) = &self.user_profile {
            parts.push(format!(
                "## User Profile\n{}",
                serde_json::to_string_pretty(&profile.profile).unwrap_or_default()
            ));
        }

        // Facts
        if !self.facts.is_empty() {
            let facts_str: Vec<String> = self
                .facts
                .iter()
                .map(|f| format!("- {}: {}", f.fact_key, f.fact_value))
                .collect();
            parts.push(format!("## Known Facts\n{}", facts_str.join("\n")));
        }

        // Active memories
        if !self.active_memories.is_empty() {
            let memories_str: Vec<String> = self
                .active_memories
                .iter()
                .map(|m| format!("- [{}] {}", m.category, m.content))
                .collect();
            parts.push(format!(
                "## Recent Conversation\n{}",
                memories_str.join("\n")
            ));
        }

        // Inactive block metadata
        if !self.inactive_block_metadata.is_empty() {
            let metadata_str: Vec<String> = self
                .inactive_block_metadata
                .iter()
                .map(|m| {
                    format!(
                        "- {}: {} memories, keywords: {}",
                        m.topic_label.as_deref().unwrap_or("Unknown"),
                        m.memory_count,
                        m.keywords.join(", ")
                    )
                })
                .collect();
            parts.push(format!("## Previous Topics\n{}", metadata_str.join("\n")));
        }

        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::memory::MemoryType;
    use tempfile::tempdir;

    async fn setup_test_db() -> anyhow::Result<(tempfile::TempDir, Database)> {
        let temp = tempdir()?;
        let db_path = temp.path().join("test.db");
        let db = Database::init(&db_path, 384).await?;
        Ok((temp, db))
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("test"), 2); // 4 chars / 4 + 1 = 2
        assert_eq!(estimate_tokens("this is a longer text"), 6); // 21 chars / 4 + 1 = 6
    }

    #[tokio::test]
    async fn test_hydrate_empty() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();

        let context = hydrator.hydrate(db.pool(), Uuid::new_v4(), None).await?;

        assert!(context.active_memories.is_empty());
        assert!(context.facts.is_empty());
        assert!(context.user_profile.is_none());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_hydrate_with_facts() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();
        let agent_id = Uuid::new_v4();

        // Create some facts
        let fact = FactRecord::new("project", "mmry");
        operations::upsert_fact(db.pool(), &fact).await?;

        let context = hydrator
            .hydrate(db.pool(), agent_id, Some("project"))
            .await?;

        assert!(context.facts.iter().any(|f| f.fact_value == "mmry"));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_hydrate_with_profile() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();
        let agent_id = Uuid::new_v4();

        // Create user profile
        let mut profile = UserProfileEntry::new(serde_json::json!({
            "preferences": {"theme": "dark"}
        }));
        profile.id = agent_id;
        operations::set_user_profile(db.pool(), &profile).await?;

        let context = hydrator.hydrate(db.pool(), agent_id, None).await?;

        assert!(context.user_profile.is_some());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_hydrate_with_bridge_block() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();

        // Create agent first to satisfy foreign key
        let agent = crate::agents::AgentRecord::new("test_agent".to_string(), "test".to_string());
        operations::upsert_agent(db.pool(), &agent).await?;
        let agent_id = agent.id;

        // Create memory
        let memory = Memory::new(
            MemoryType::Episodic,
            "Test memory content".to_string(),
            "test".to_string(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        // Create active bridge block referencing memory
        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent_id);
        block.status = Some("active".to_string());
        block.content = serde_json::json!({
            "memory_ids": [memory.id.to_string()]
        });
        operations::upsert_bridge_block(db.pool(), &block).await?;

        let context = hydrator.hydrate(db.pool(), agent_id, None).await?;

        assert!(!context.active_memories.is_empty());
        assert_eq!(context.active_memories[0].id, memory.id);

        db.close().await;
        Ok(())
    }

    #[test]
    fn test_hydrated_context_to_prompt() {
        let mut context = HydratedContext::default();
        context.facts.push(FactRecord::new("project", "mmry"));
        context.active_memories.push(Memory::new(
            MemoryType::Episodic,
            "Working on the project".to_string(),
            "work".to_string(),
        ));

        let prompt = context.to_prompt_context();
        assert!(prompt.contains("Known Facts"));
        assert!(prompt.contains("project: mmry"));
        assert!(prompt.contains("Recent Conversation"));
        assert!(prompt.contains("Working on the project"));
    }

    #[test]
    fn test_hydration_options_default() {
        let options = HydrationOptions::default();
        assert_eq!(options.max_tokens, 4000);
        assert_eq!(options.max_active_memories, 10);
        assert!(options.include_profile);
    }

    #[test]
    fn test_synthesis_options_default() {
        let options = SynthesisOptions::default();
        assert_eq!(options.min_memories_for_synthesis, 5);
        assert_eq!(options.max_age_hours, 24);
        assert!(options.only_inactive);
    }

    #[tokio::test]
    async fn test_find_synthesis_candidates_empty() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();
        let options = SynthesisOptions::default();

        let candidates = hydrator
            .find_synthesis_candidates(db.pool(), &options)
            .await?;
        assert!(candidates.is_empty());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_decay_fact_recency() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();

        // Create fact with high recency
        let mut fact = FactRecord::new("test_key", "test_value");
        fact.recency_score = 1.0;
        operations::upsert_fact(db.pool(), &fact).await?;

        // Apply decay
        let updated = hydrator.decay_fact_recency(db.pool(), 0.1).await?;
        assert_eq!(updated, 1);

        // Check new recency
        let facts = operations::list_facts_by_key(db.pool(), "test_key", 1).await?;
        assert!(!facts.is_empty());
        assert!(facts[0].recency_score < 1.0);
        assert!(facts[0].recency_score > 0.8); // 1.0 * 0.9 = 0.9

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_apply_synthesis() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let hydrator = ContextHydrator::new();

        // Create agent first for FK constraint
        let agent = crate::agents::AgentRecord::new("synth_agent", "test");
        operations::upsert_agent(db.pool(), &agent).await?;

        // Create a bridge block
        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent.id);
        block.status = Some("active".to_string());
        block.topic_label = Some("Test Topic".to_string());
        operations::upsert_bridge_block(db.pool(), &block).await?;

        // Apply synthesis
        let result = SynthesisResult {
            block_id: block.block_id,
            summary: "This was a discussion about testing".to_string(),
            key_points: vec!["Point 1".to_string(), "Point 2".to_string()],
            action_items: vec!["Follow up".to_string()],
            synthesized_at: chrono::Utc::now(),
        };

        hydrator
            .apply_synthesis(db.pool(), &mut block, &result)
            .await?;

        // Verify block was updated
        let updated_block = operations::get_bridge_block(db.pool(), block.block_id)
            .await?
            .unwrap();
        assert_eq!(updated_block.status, Some("synthesized".to_string()));
        assert!(updated_block.content.get("synthesis").is_some());

        db.close().await;
        Ok(())
    }
}
