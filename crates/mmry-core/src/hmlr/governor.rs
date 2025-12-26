//! Governor: Central orchestrator for HMLR parallel task execution
//!
//! The Governor coordinates parallel execution of:
//! - FactScrubber: Extract key-value facts
//! - Scribe: Update user profile (fire-and-forget)
//! - LatticeCrawler: Find candidate bridge blocks
//!
//! After parallel tasks complete, Governor makes routing decisions.

use super::fact_scrubber::FactScrubber;
use super::lattice_crawler::LatticeCrawler;
use super::scribe::Scribe;
use super::HmlrContext;
use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::analysis::Analyzer;
use crate::config::HmlrConfig;
use crate::database::operations;
use crate::memory::Memory;
use crate::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use uuid::Uuid;

/// Result of Governor's processing
#[derive(Debug, Default)]
pub struct GovernorDecision {
    /// Facts extracted from the memory
    pub facts: Vec<FactRecord>,
    /// Bridge block the memory was assigned to (new or resumed)
    pub bridge_block: Option<BridgeBlock>,
    /// Whether this is a new topic (true) or continuation (false)
    pub is_new_topic: bool,
    /// Rationale for the routing decision (if available)
    pub rationale: Option<String>,
}

/// Governor orchestrates HMLR parallel tasks and makes routing decisions
pub struct Governor {
    config: HmlrConfig,
    analyzer: Arc<dyn Analyzer>,
    fact_scrubber: FactScrubber,
    scribe: Scribe,
    lattice_crawler: LatticeCrawler,
}

impl Governor {
    /// Create a new Governor with the given analyzer
    pub fn new(config: HmlrConfig, analyzer: Arc<dyn Analyzer>) -> Self {
        Self {
            config: config.clone(),
            analyzer: analyzer.clone(),
            fact_scrubber: FactScrubber::new(analyzer),
            scribe: Scribe::new(),
            lattice_crawler: LatticeCrawler::new(),
        }
    }

    /// Process a memory with parallel task execution
    ///
    /// Launches FactScrubber, Scribe, and LatticeCrawler in parallel,
    /// then makes routing decisions based on the results.
    pub async fn process_memory(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<GovernorDecision> {
        // Launch parallel tasks using tokio::join!
        let (facts_result, _scribe_result, candidates_result) = tokio::join!(
            // Task 1: FactScrubber - extract key-value facts
            self.run_fact_scrubber(pool, memory),
            // Task 2: Scribe - update user profile (fire-and-forget)
            self.run_scribe(pool, memory, context),
            // Task 3: LatticeCrawler - find candidate bridge blocks
            self.run_lattice_crawler(pool, context)
        );

        // Collect facts (persist them)
        let facts = match facts_result {
            Ok(f) if self.config.extract_facts => f,
            Ok(_) => Vec::new(),
            Err(e) => {
                tracing::warn!("FactScrubber failed: {e}");
                Vec::new()
            }
        };

        // Persist extracted facts
        for fact in &facts {
            if let Err(e) = operations::upsert_fact(pool, fact).await {
                tracing::warn!("Failed to persist fact: {e}");
            }
        }

        // Get candidate bridge blocks
        let candidates = candidates_result.unwrap_or_default();

        // Make routing decision
        if self.config.bridge_routing {
            let (bridge_block, is_new_topic, rationale) = self
                .route_to_bridge_block(pool, memory, context, &candidates)
                .await?;

            Ok(GovernorDecision {
                facts,
                bridge_block: Some(bridge_block),
                is_new_topic,
                rationale,
            })
        } else {
            Ok(GovernorDecision {
                facts,
                bridge_block: None,
                is_new_topic: true,
                rationale: None,
            })
        }
    }

    /// Run FactScrubber to extract facts from memory content
    async fn run_fact_scrubber(
        &self,
        _pool: &SqlitePool,
        memory: &Memory,
    ) -> Result<Vec<FactRecord>> {
        self.fact_scrubber.extract(&memory.content).await
    }

    /// Run Scribe to update user profile (fire-and-forget)
    async fn run_scribe(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<()> {
        // Fire-and-forget: we don't wait for completion or handle errors strictly
        if let Err(e) = self.scribe.update_profile(pool, memory, context).await {
            tracing::debug!("Scribe update failed (non-critical): {e}");
        }
        Ok(())
    }

    /// Run LatticeCrawler to find candidate bridge blocks
    async fn run_lattice_crawler(
        &self,
        pool: &SqlitePool,
        context: &HmlrContext,
    ) -> Result<Vec<BridgeBlock>> {
        self.lattice_crawler
            .find_candidates(pool, context.creator_id, 5)
            .await
    }

    /// Route to bridge block: resume existing or create new
    async fn route_to_bridge_block(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
        candidates: &[BridgeBlock],
    ) -> Result<(BridgeBlock, bool, Option<String>)> {
        if candidates.is_empty() {
            let block = self.create_bridge_block(pool, memory, context).await?;
            return Ok((
                block,
                true,
                Some("No candidates, created new block".to_string()),
            ));
        }

        if self.analyzer.is_noop() {
            // Heuristic fallback when analyzer is disabled
            let active_block = candidates.iter().find(|b| {
                b.status == Some("active".to_string()) && b.agent_id == Some(context.creator_id)
            });

            if let Some(block) = active_block {
                let updated_block = self
                    .resume_bridge_block(pool, block.block_id, memory)
                    .await?;
                return Ok((
                    updated_block,
                    false,
                    Some("Resumed active block (heuristic)".to_string()),
                ));
            }

            let block = self.create_bridge_block(pool, memory, context).await?;
            return Ok((
                block,
                true,
                Some("No active block, created new (heuristic)".to_string()),
            ));
        }

        let routing = self.analyzer.route(&memory.content, candidates).await?;

        if let Some(chosen) = routing.chosen_block {
            let updated_block = self.resume_bridge_block(pool, chosen, memory).await?;
            return Ok((updated_block, false, routing.rationale));
        }

        if !routing.is_new_topic {
            let active_block = candidates.iter().find(|b| {
                b.status == Some("active".to_string()) && b.agent_id == Some(context.creator_id)
            });
            if let Some(block) = active_block {
                let updated_block = self
                    .resume_bridge_block(pool, block.block_id, memory)
                    .await?;
                return Ok((updated_block, false, routing.rationale));
            }
        }

        let block = self.create_bridge_block(pool, memory, context).await?;
        Ok((block, true, routing.rationale))
    }

    /// Create a new bridge block for the memory
    async fn create_bridge_block(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<BridgeBlock> {
        let mut block = BridgeBlock::new();
        block.agent_id = Some(context.creator_id);
        block.span_id = Some(Uuid::new_v4().to_string());
        block.status = Some("active".to_string());

        // Extract keywords from memory content
        block.keywords = extract_keywords(&memory.content);

        // Generate topic label using LLM
        match self.analyzer.generate_topic_label(&memory.content).await {
            Ok(Some(label)) => {
                tracing::debug!(topic_label = %label, "Generated topic label for bridge block");
                block.topic_label = Some(label);
            }
            Ok(None) => {
                tracing::debug!("No topic label generated (analyzer returned None)");
            }
            Err(e) => {
                tracing::warn!("Failed to generate topic label: {e}");
            }
        }

        // Store memory ID in content JSON
        block.content = serde_json::json!({
            "memory_ids": [memory.id.to_string()],
            "created_from": context.query.clone().unwrap_or_default(),
        });

        operations::upsert_bridge_block(pool, &block).await?;
        Ok(block)
    }

    /// Resume an existing bridge block by appending memory
    async fn resume_bridge_block(
        &self,
        pool: &SqlitePool,
        block_id: Uuid,
        memory: &Memory,
    ) -> Result<BridgeBlock> {
        // Get existing block
        let mut block = operations::get_bridge_block(pool, block_id)
            .await?
            .ok_or_else(|| crate::Error::Config("Bridge block not found".to_string()))?;

        // Append memory ID to content
        if let Some(obj) = block.content.as_object_mut() {
            if let Some(ids) = obj.get_mut("memory_ids") {
                if let Some(arr) = ids.as_array_mut() {
                    arr.push(serde_json::json!(memory.id.to_string()));
                }
            } else {
                obj.insert(
                    "memory_ids".to_string(),
                    serde_json::json!([memory.id.to_string()]),
                );
            }
        }

        // Add new keywords
        let new_keywords = extract_keywords(&memory.content);
        for kw in new_keywords {
            if !block.keywords.contains(&kw) {
                block.keywords.push(kw);
            }
        }

        operations::upsert_bridge_block(pool, &block).await?;
        Ok(block)
    }
}

/// Extract keywords from content using simple heuristics
fn extract_keywords(content: &str) -> Vec<String> {
    // Simple word extraction: words longer than 4 chars, lowercase, deduplicated
    let mut words: Vec<String> = content
        .split_whitespace()
        .filter(|w| w.len() > 4)
        .map(|w| {
            w.to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|w| !w.is_empty() && !is_stop_word(w))
        .collect();

    // Deduplicate and take top 10
    words.sort();
    words.dedup();
    words.truncate(10);
    words
}

/// Check if a word is a common stop word
fn is_stop_word(word: &str) -> bool {
    const STOP_WORDS: &[&str] = &[
        "about", "after", "again", "along", "because", "before", "being", "between", "could",
        "does", "doing", "during", "every", "first", "found", "given", "going", "great", "have",
        "having", "their", "there", "these", "thing", "think", "those", "through", "under",
        "until", "using", "wants", "where", "which", "while", "would", "write", "years", "your",
    ];
    STOP_WORDS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentRecord;
    use crate::analysis::AnalyzerRouting;
    use crate::analysis::NoOpAnalyzer;
    use crate::database::Database;
    use crate::memory::MemoryType;
    use async_trait::async_trait;
    use tempfile::tempdir;
    use uuid::Uuid;

    async fn setup_test_db() -> anyhow::Result<(tempfile::TempDir, Database)> {
        let temp = tempdir()?;
        let db_path = temp.path().join("test.db");
        let db = Database::init(&db_path, 384).await?;
        Ok((temp, db))
    }

    /// Create an agent and return its ID
    async fn create_test_agent(pool: &SqlitePool) -> anyhow::Result<Uuid> {
        let agent = AgentRecord::new("test_agent".to_string(), "test".to_string());
        operations::upsert_agent(pool, &agent).await?;
        Ok(agent.id)
    }

    struct FixedRouteAnalyzer {
        chosen: Option<Uuid>,
    }

    #[async_trait]
    impl crate::analysis::Analyzer for FixedRouteAnalyzer {
        async fn extract_facts(&self, _content: &str) -> crate::Result<Vec<FactRecord>> {
            Ok(Vec::new())
        }

        async fn route(
            &self,
            _query: &str,
            _candidates: &[BridgeBlock],
        ) -> crate::Result<AnalyzerRouting> {
            Ok(AnalyzerRouting {
                chosen_block: self.chosen,
                is_new_topic: self.chosen.is_none(),
                rationale: Some("Test routing".to_string()),
            })
        }
    }

    #[tokio::test]
    async fn test_governor_creates_bridge_block() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let config = HmlrConfig {
            enabled: true,
            extract_facts: false,
            bridge_routing: true,
            ..Default::default()
        };

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;

        let governor = Governor::new(config, Arc::new(NoOpAnalyzer));
        let memory = Memory::new(
            MemoryType::Episodic,
            "Meeting with the engineering team".to_string(),
            "work".to_string(),
        );
        let context = HmlrContext::for_human(agent_id);

        // Insert memory first
        operations::insert_memory(db.pool(), &memory).await?;

        let decision = governor
            .process_memory(db.pool(), &memory, &context)
            .await?;

        assert!(decision.bridge_block.is_some());
        assert!(decision.is_new_topic);

        let block = decision.bridge_block.unwrap();
        assert_eq!(block.status, Some("active".to_string()));
        assert!(!block.keywords.is_empty());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_governor_resumes_active_block() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let config = HmlrConfig {
            enabled: true,
            extract_facts: false,
            bridge_routing: true,
            ..Default::default()
        };

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;
        let governor = Governor::new(config, Arc::new(NoOpAnalyzer));

        // Create first memory and bridge block
        let memory1 = Memory::new(
            MemoryType::Episodic,
            "Started the project planning".to_string(),
            "work".to_string(),
        );
        operations::insert_memory(db.pool(), &memory1).await?;

        let context1 =
            HmlrContext::for_agent(agent_id, Some("Planning the project".to_string()), vec![]);
        let decision1 = governor
            .process_memory(db.pool(), &memory1, &context1)
            .await?;

        assert!(decision1.is_new_topic);
        let block1 = decision1.bridge_block.unwrap();

        // Create second memory with same agent and query
        let memory2 = Memory::new(
            MemoryType::Episodic,
            "Continued with milestone definitions".to_string(),
            "work".to_string(),
        );
        operations::insert_memory(db.pool(), &memory2).await?;

        let context2 = HmlrContext::for_agent(
            agent_id,
            Some("Defining milestones".to_string()),
            vec![memory1],
        );
        let decision2 = governor
            .process_memory(db.pool(), &memory2, &context2)
            .await?;

        // Should resume the existing block
        assert!(!decision2.is_new_topic);
        let block2 = decision2.bridge_block.unwrap();
        assert_eq!(block1.block_id, block2.block_id);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_governor_uses_analyzer_routing() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let config = HmlrConfig {
            enabled: true,
            extract_facts: false,
            bridge_routing: true,
            ..Default::default()
        };

        let agent_id = create_test_agent(db.pool()).await?;

        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent_id);
        block.status = Some("active".to_string());
        operations::upsert_bridge_block(db.pool(), &block).await?;

        let analyzer = Arc::new(FixedRouteAnalyzer {
            chosen: Some(block.block_id),
        });
        let governor = Governor::new(config, analyzer);

        let memory = Memory::new(
            MemoryType::Episodic,
            "Routing should use analyzer".to_string(),
            "work".to_string(),
        );
        operations::insert_memory(db.pool(), &memory).await?;

        let context = HmlrContext::for_agent(agent_id, Some("Route this".to_string()), vec![]);
        let decision = governor
            .process_memory(db.pool(), &memory, &context)
            .await?;

        assert!(decision.bridge_block.is_some());
        assert!(!decision.is_new_topic);
        assert_eq!(decision.bridge_block.unwrap().block_id, block.block_id);

        db.close().await;
        Ok(())
    }

    #[test]
    fn test_extract_keywords() {
        let content = "Meeting with the engineering team about project planning";
        let keywords = extract_keywords(content);

        assert!(keywords.contains(&"meeting".to_string()));
        assert!(keywords.contains(&"engineering".to_string()));
        assert!(keywords.contains(&"project".to_string()));
        assert!(keywords.contains(&"planning".to_string()));
        // "with", "the", "about" should be filtered (too short or stop words)
    }

    #[test]
    fn test_extract_keywords_deduplicates() {
        let content = "project project project planning planning";
        let keywords = extract_keywords(content);

        // Should only have one of each
        let project_count = keywords.iter().filter(|k| *k == "project").count();
        let planning_count = keywords.iter().filter(|k| *k == "planning").count();

        assert_eq!(project_count, 1);
        assert_eq!(planning_count, 1);
    }

    #[test]
    fn test_is_stop_word() {
        assert!(is_stop_word("about"));
        assert!(is_stop_word("their"));
        assert!(is_stop_word("would"));
        assert!(!is_stop_word("project"));
        assert!(!is_stop_word("meeting"));
    }
}
