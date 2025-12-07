//! LatticeCrawler: Finds candidate bridge blocks for routing decisions
//!
//! Returns recent bridge blocks for the same agent/conversation,
//! used by Governor to decide whether to resume an existing block
//! or create a new one.

use crate::agents::BridgeBlock;
use crate::database::operations;
use crate::Result;
use sqlx::SqlitePool;
use uuid::Uuid;

/// LatticeCrawler finds candidate bridge blocks for routing
pub struct LatticeCrawler;

impl LatticeCrawler {
    /// Create a new LatticeCrawler
    pub fn new() -> Self {
        Self
    }

    /// Find candidate bridge blocks for an agent
    ///
    /// Returns recent blocks that could potentially be resumed.
    pub async fn find_candidates(
        &self,
        pool: &SqlitePool,
        agent_id: Uuid,
        limit: i64,
    ) -> Result<Vec<BridgeBlock>> {
        operations::get_recent_bridge_blocks_for_agent(pool, agent_id, limit).await
    }

    /// Find active blocks for an agent
    pub async fn find_active_blocks(
        &self,
        pool: &SqlitePool,
        agent_id: Uuid,
    ) -> Result<Vec<BridgeBlock>> {
        let all = operations::get_recent_bridge_blocks_for_agent(pool, agent_id, 20).await?;
        Ok(all
            .into_iter()
            .filter(|b| b.status == Some("active".to_string()))
            .collect())
    }

    /// Check if there's a recent active block for continuation
    pub async fn has_active_block(&self, pool: &SqlitePool, agent_id: Uuid) -> Result<bool> {
        let blocks = self.find_active_blocks(pool, agent_id).await?;
        Ok(!blocks.is_empty())
    }

    /// Get the most recent active block for an agent
    pub async fn get_most_recent_active(
        &self,
        pool: &SqlitePool,
        agent_id: Uuid,
    ) -> Result<Option<BridgeBlock>> {
        let blocks = self.find_active_blocks(pool, agent_id).await?;
        Ok(blocks.into_iter().next())
    }
}

impl Default for LatticeCrawler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentRecord;
    use crate::database::Database;
    use tempfile::tempdir;

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

    #[tokio::test]
    async fn test_find_candidates_empty() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let crawler = LatticeCrawler::new();

        let candidates = crawler
            .find_candidates(db.pool(), Uuid::new_v4(), 5)
            .await?;
        assert!(candidates.is_empty());

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_find_candidates_with_blocks() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let crawler = LatticeCrawler::new();

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;

        // Create some bridge blocks
        for i in 0..3 {
            let mut block = BridgeBlock::new();
            block.agent_id = Some(agent_id);
            block.status = Some(if i == 0 { "active" } else { "closed" }.to_string());
            block.topic_label = Some(format!("Topic {i}"));
            operations::upsert_bridge_block(db.pool(), &block).await?;
        }

        let candidates = crawler.find_candidates(db.pool(), agent_id, 5).await?;
        assert_eq!(candidates.len(), 3);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_find_active_blocks() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let crawler = LatticeCrawler::new();

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;

        // Create mixed active/closed blocks
        for i in 0..3 {
            let mut block = BridgeBlock::new();
            block.agent_id = Some(agent_id);
            block.status = Some(if i == 0 { "active" } else { "closed" }.to_string());
            operations::upsert_bridge_block(db.pool(), &block).await?;
        }

        let active = crawler.find_active_blocks(db.pool(), agent_id).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].status, Some("active".to_string()));

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_has_active_block() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let crawler = LatticeCrawler::new();

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;

        // No blocks initially
        assert!(!crawler.has_active_block(db.pool(), agent_id).await?);

        // Add closed block
        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent_id);
        block.status = Some("closed".to_string());
        operations::upsert_bridge_block(db.pool(), &block).await?;

        // Still no active
        assert!(!crawler.has_active_block(db.pool(), agent_id).await?);

        // Add active block
        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent_id);
        block.status = Some("active".to_string());
        operations::upsert_bridge_block(db.pool(), &block).await?;

        // Now has active
        assert!(crawler.has_active_block(db.pool(), agent_id).await?);

        db.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_most_recent_active() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;
        let crawler = LatticeCrawler::new();

        // Create agent first to satisfy foreign key
        let agent_id = create_test_agent(db.pool()).await?;

        // Create an active block
        let mut block = BridgeBlock::new();
        block.agent_id = Some(agent_id);
        block.status = Some("active".to_string());
        block.topic_label = Some("Test Topic".to_string());
        operations::upsert_bridge_block(db.pool(), &block).await?;

        let recent = crawler.get_most_recent_active(db.pool(), agent_id).await?;
        assert!(recent.is_some());
        assert_eq!(recent.unwrap().topic_label, Some("Test Topic".to_string()));

        db.close().await;
        Ok(())
    }
}
