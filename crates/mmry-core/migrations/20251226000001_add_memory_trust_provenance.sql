ALTER TABLE memories ADD COLUMN source_attribution JSON;
ALTER TABLE memories ADD COLUMN trust_level REAL DEFAULT 0.5;
ALTER TABLE memories ADD COLUMN source_reinforcement_score REAL DEFAULT 0.0;
CREATE INDEX IF NOT EXISTS idx_memories_trust_level ON memories(trust_level);
