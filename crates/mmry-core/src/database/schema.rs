// Database schema definitions

pub const INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB,
    sparse_embedding BLOB,
    metadata JSON,
    importance INTEGER DEFAULT 5,
    expires_at DATETIME,
    expired_at DATETIME,
    source_attribution JSON,
    trust_level REAL DEFAULT 0.5,
    source_reinforcement_score REAL DEFAULT 0.0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    category TEXT DEFAULT 'default',
    tags JSON DEFAULT '[]',
    parent_id TEXT,
    chunk_index INTEGER,
    total_chunks INTEGER,
    chunk_method TEXT,
    bridge_block_id TEXT
);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    description TEXT,
    metadata JSON DEFAULT '{}',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS agent_events (
    id TEXT PRIMARY KEY,
    agent_id TEXT REFERENCES agents(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    status TEXT,
    payload JSON,
    span_id TEXT,
    memory_id TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS learnings (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'general',
    kind TEXT NOT NULL DEFAULT 'pattern',
    source TEXT,
    confidence REAL DEFAULT 0.5,
    maturity TEXT DEFAULT 'candidate',
    score REAL DEFAULT 0.0,
    embedding BLOB,
    agent_id TEXT,
    scope TEXT DEFAULT 'global',
    metadata JSON DEFAULT '{}',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS learning_feedback_events (
    id TEXT PRIMARY KEY,
    learning_id TEXT REFERENCES learnings(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    delta REAL DEFAULT 0.0,
    context TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_agent_events_agent ON agent_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_learnings_category ON learnings(category);
CREATE INDEX IF NOT EXISTS idx_learnings_kind ON learnings(kind);
CREATE INDEX IF NOT EXISTS idx_learnings_maturity ON learnings(maturity);
CREATE INDEX IF NOT EXISTS idx_learnings_score ON learnings(score DESC);
CREATE INDEX IF NOT EXISTS idx_learnings_agent ON learnings(agent_id);
CREATE INDEX IF NOT EXISTS idx_learnings_scope ON learnings(scope);
"#;
