-- Add chunking support to memories table
ALTER TABLE memories ADD COLUMN parent_id TEXT REFERENCES memories(id) ON DELETE CASCADE;
ALTER TABLE memories ADD COLUMN chunk_index INTEGER;
ALTER TABLE memories ADD COLUMN total_chunks INTEGER;
ALTER TABLE memories ADD COLUMN chunk_method TEXT; -- 'none', 'paragraph', 'sentence', 'word'

-- Add index for finding chunks by parent
CREATE INDEX IF NOT EXISTS idx_memories_parent ON memories(parent_id);

-- Add index for chunk ordering
CREATE INDEX IF NOT EXISTS idx_memories_chunk_order ON memories(parent_id, chunk_index) WHERE parent_id IS NOT NULL;
