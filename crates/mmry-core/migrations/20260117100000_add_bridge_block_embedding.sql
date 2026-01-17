-- Add embedding column to bridge_blocks for semantic routing
-- This enables the Governor to match new memories to existing blocks
-- using semantic similarity when LLM routing is unavailable.

ALTER TABLE bridge_blocks ADD COLUMN embedding BLOB;

-- Index for blocks with embeddings (helps filter during routing)
CREATE INDEX IF NOT EXISTS idx_bridge_blocks_has_embedding 
    ON bridge_blocks(block_id) WHERE embedding IS NOT NULL;

-- Index for agent + status lookup used in routing
CREATE INDEX IF NOT EXISTS idx_bridge_blocks_agent_status 
    ON bridge_blocks(agent_id, status);
