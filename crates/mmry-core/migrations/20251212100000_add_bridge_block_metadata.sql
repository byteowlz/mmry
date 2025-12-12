-- Add open_loops and decisions_made columns to bridge_blocks table
-- These track unresolved questions and key decisions within a conversation topic

-- Add open_loops column (JSON array of strings)
ALTER TABLE bridge_blocks ADD COLUMN open_loops JSON DEFAULT '[]';

-- Add decisions_made column (JSON array of strings)
ALTER TABLE bridge_blocks ADD COLUMN decisions_made JSON DEFAULT '[]';
