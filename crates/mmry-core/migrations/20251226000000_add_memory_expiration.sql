ALTER TABLE memories ADD COLUMN expires_at DATETIME;
ALTER TABLE memories ADD COLUMN expired_at DATETIME;
CREATE INDEX IF NOT EXISTS idx_memories_expires_at ON memories(expires_at);
CREATE INDEX IF NOT EXISTS idx_memories_expired_at ON memories(expired_at);
