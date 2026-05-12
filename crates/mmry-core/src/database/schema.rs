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
    helpful_count INTEGER NOT NULL DEFAULT 0,
    harmful_count INTEGER NOT NULL DEFAULT 0,
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
    bridge_block_id TEXT,
    workspace_id TEXT,
    platform_session_id TEXT,
    harness_session_id TEXT
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
    kind TEXT NOT NULL DEFAULT 'guiding',
    category TEXT NOT NULL DEFAULT 'general',
    scope TEXT NOT NULL DEFAULT 'global',
    scope_key TEXT,
    maturity TEXT NOT NULL DEFAULT 'candidate',
    pinned BOOLEAN NOT NULL DEFAULT 0,
    helpful_count INTEGER NOT NULL DEFAULT 0,
    harmful_count INTEGER NOT NULL DEFAULT 0,
    effective_score REAL NOT NULL DEFAULT 0.0,
    agent_id TEXT REFERENCES agents(id),
    source_sessions JSON DEFAULT '[]',
    reasoning TEXT,
    tags JSON DEFAULT '[]',
    metadata JSON DEFAULT '{}',
    embedding BLOB,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS learning_feedback (
    id TEXT PRIMARY KEY,
    learning_id TEXT NOT NULL REFERENCES learnings(id) ON DELETE CASCADE,
    feedback_type TEXT NOT NULL CHECK(feedback_type IN ('helpful', 'harmful')),
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    session_path TEXT,
    reason TEXT,
    agent_id TEXT REFERENCES agents(id)
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
    agent_id TEXT REFERENCES agents(id),
    ts DATETIME DEFAULT CURRENT_TIMESTAMP,
    closed_at DATETIME
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_agent_events_agent ON agent_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_learnings_category ON learnings(category);
CREATE INDEX IF NOT EXISTS idx_learnings_kind ON learnings(kind);
CREATE INDEX IF NOT EXISTS idx_learnings_maturity ON learnings(maturity);
CREATE INDEX IF NOT EXISTS idx_learnings_score ON learnings(effective_score DESC);
CREATE INDEX IF NOT EXISTS idx_learnings_agent ON learnings(agent_id);
CREATE INDEX IF NOT EXISTS idx_learnings_scope ON learnings(scope, scope_key);
CREATE INDEX IF NOT EXISTS idx_learning_feedback_learning ON learning_feedback(learning_id);
CREATE INDEX IF NOT EXISTS idx_learning_feedback_timestamp ON learning_feedback(timestamp);
CREATE INDEX IF NOT EXISTS idx_episodes_ts ON episodes(ts DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_workspace ON episodes(workspace_id);
CREATE INDEX IF NOT EXISTS idx_episodes_platform_session ON episodes(platform_session_id);
CREATE INDEX IF NOT EXISTS idx_episodes_closed_at ON episodes(closed_at);
"#;
