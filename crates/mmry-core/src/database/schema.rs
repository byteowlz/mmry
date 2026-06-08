// Database schema definitions

pub const INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    content TEXT NOT NULL,
    sparse_embedding BLOB,
    metadata JSON,
    importance INTEGER DEFAULT 5,
    helpful_count INTEGER NOT NULL DEFAULT 0,
    harmful_count INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    category TEXT DEFAULT 'default',
    tags JSON DEFAULT '[]',
    parent_id TEXT,
    chunk_index INTEGER,
    total_chunks INTEGER,
    workspace_id TEXT,
    platform_session_id TEXT,
    harness_session_id TEXT,
    store TEXT NOT NULL DEFAULT 'default'
);

CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    query TEXT NOT NULL,
    returned_ids JSON NOT NULL DEFAULT '[]',
    used_ids JSON,
    result TEXT,
    workspace_id TEXT,
    platform_session_id TEXT,
    harness_session_id TEXT,
    ts DATETIME DEFAULT CURRENT_TIMESTAMP,
    closed_at DATETIME
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_episodes_ts ON episodes(ts DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_workspace ON episodes(workspace_id);
CREATE INDEX IF NOT EXISTS idx_episodes_platform_session ON episodes(platform_session_id);
CREATE INDEX IF NOT EXISTS idx_episodes_closed_at ON episodes(closed_at);
"#;
