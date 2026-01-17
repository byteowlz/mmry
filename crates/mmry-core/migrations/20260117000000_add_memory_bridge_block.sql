-- Add bridge_block_id foreign key to memories table for direct memory-to-block relationship
-- This replaces the indirect lookup via content_json.memory_ids

ALTER TABLE memories ADD COLUMN bridge_block_id TEXT REFERENCES bridge_blocks(block_id);

-- Index for efficient lookups by bridge block
CREATE INDEX IF NOT EXISTS idx_memories_bridge_block ON memories(bridge_block_id);

-- Index for finding all memories in a block ordered by time
CREATE INDEX IF NOT EXISTS idx_memories_bridge_block_created ON memories(bridge_block_id, created_at DESC);
