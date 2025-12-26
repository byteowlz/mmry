// Database schema definitions
// Migrations are in crates/mmry-core/migrations/

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
    chunk_method TEXT
);

CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    type TEXT,
    metadata JSON
);

CREATE TABLE IF NOT EXISTS memory_entities (
    memory_id TEXT REFERENCES memories(id) ON DELETE CASCADE,
    entity_id TEXT REFERENCES entities(id) ON DELETE CASCADE,
    PRIMARY KEY (memory_id, entity_id)
);

CREATE TABLE IF NOT EXISTS relationships (
    id TEXT PRIMARY KEY,
    from_entity TEXT REFERENCES entities(id),
    to_entity TEXT REFERENCES entities(id),
    relation_type TEXT,
    strength REAL DEFAULT 1.0
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

CREATE TABLE IF NOT EXISTS bridge_blocks (
    block_id TEXT PRIMARY KEY,
    span_id TEXT,
    topic_label TEXT,
    keywords JSON DEFAULT '[]',
    status TEXT,
    exit_reason TEXT,
    content_json JSON,
    agent_id TEXT REFERENCES agents(id),
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS facts (
    id TEXT PRIMARY KEY,
    fact_key TEXT NOT NULL,
    fact_value TEXT NOT NULL,
    category TEXT DEFAULT 'General',
    evidence_snippet TEXT,
    source_span TEXT,
    turn_id TEXT,
    source_chunk_id TEXT,
    source_paragraph_id TEXT,
    observed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    recency_score REAL DEFAULT 1.0,
    metadata JSON DEFAULT '{}',
    agent_id TEXT REFERENCES agents(id),
    fact_fingerprint TEXT
);

CREATE TABLE IF NOT EXISTS user_profiles (
    id TEXT PRIMARY KEY,
    profile JSON NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_memory_entities_memory ON memory_entities(memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(entity_id);
CREATE INDEX IF NOT EXISTS idx_agent_events_agent ON agent_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_bridge_blocks_span ON bridge_blocks(span_id);
CREATE INDEX IF NOT EXISTS idx_facts_key ON facts(fact_key);
CREATE INDEX IF NOT EXISTS idx_facts_observed ON facts(observed_at DESC);
"#;
