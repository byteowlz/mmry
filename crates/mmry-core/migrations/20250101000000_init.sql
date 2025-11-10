-- Initial database schema for mmry
CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB,
    metadata TEXT NOT NULL DEFAULT '{}',
    importance INTEGER DEFAULT 5,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    namespace TEXT DEFAULT 'default'
);

CREATE TABLE entities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    type TEXT,
    metadata TEXT
);

CREATE TABLE memory_entities (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    PRIMARY KEY (memory_id, entity_id)
);

CREATE TABLE relationships (
    id TEXT PRIMARY KEY,
    from_entity TEXT REFERENCES entities(id),
    to_entity TEXT REFERENCES entities(id),
    relation_type TEXT,
    strength REAL DEFAULT 1.0
);

-- Indexes for performance
CREATE INDEX idx_memories_type ON memories(type);
CREATE INDEX idx_memories_created ON memories(created_at DESC);
CREATE INDEX idx_memories_importance ON memories(importance DESC);
CREATE INDEX idx_memories_namespace ON memories(namespace);
CREATE INDEX idx_memory_entities_memory ON memory_entities(memory_id);
CREATE INDEX idx_memory_entities_entity ON memory_entities(entity_id);
