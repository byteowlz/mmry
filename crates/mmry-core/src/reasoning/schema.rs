//! Database schema for reasoning module
//!
//! Designed for full transparency and traceability:
//! - Every inference links back to its premise facts/inferences
//! - Every reasoning answer links to supporting evidence
//! - Contradictions are tracked with resolution history
//! - All reasoning events are logged for audit

/// SQL schema for the reasoning tables
pub const REASONING_SCHEMA: &str = r#"
-- Inferences derived through reasoning
-- Each inference is traceable back to its premises
CREATE TABLE IF NOT EXISTS inferences (
    id TEXT PRIMARY KEY,
    
    -- The conclusion/insight
    conclusion TEXT NOT NULL,
    
    -- How was this derived? (observed, deduced, induced, abduced)
    inference_type TEXT NOT NULL DEFAULT 'observed',
    
    -- Natural language reasoning trace explaining derivation
    reasoning_trace TEXT NOT NULL,
    
    -- Natural language certainty statement (not a number!)
    -- e.g., "Based on 3 consistent observations over 2 months"
    certainty_statement TEXT,
    
    -- Category for organization (optional)
    category TEXT,
    
    -- Lifecycle
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    superseded BOOLEAN DEFAULT FALSE,
    superseded_by TEXT REFERENCES inferences(id),
    superseded_at DATETIME,
    
    -- Metadata
    metadata JSON DEFAULT '{}'
);

-- Links inferences to their premises (facts or other inferences)
-- This enables full traceability back to original observations
CREATE TABLE IF NOT EXISTS inference_premises (
    id TEXT PRIMARY KEY,
    
    -- The inference that was derived
    inference_id TEXT NOT NULL REFERENCES inferences(id) ON DELETE CASCADE,
    
    -- The premise (can be a fact OR another inference)
    premise_type TEXT NOT NULL, -- 'fact' or 'inference'
    premise_id TEXT NOT NULL,   -- UUID of the fact or inference
    
    -- Order in the reasoning chain (for reconstruction)
    premise_order INTEGER DEFAULT 0,
    
    -- How strongly this premise contributed
    contribution_note TEXT,
    
    UNIQUE(inference_id, premise_type, premise_id)
);

-- Detected contradictions between facts/inferences
CREATE TABLE IF NOT EXISTS contradictions (
    id TEXT PRIMARY KEY,
    
    -- The two contradicting items
    item_a_type TEXT NOT NULL, -- 'fact' or 'inference'
    item_a_id TEXT NOT NULL,
    item_b_type TEXT NOT NULL, -- 'fact' or 'inference'
    item_b_id TEXT NOT NULL,
    
    -- Explanation of why they contradict
    explanation TEXT NOT NULL,
    
    -- Resolution status
    status TEXT DEFAULT 'detected', -- detected, resolved, dismissed
    resolution_type TEXT,           -- temporal, contextual, a_preferred, b_preferred, merged
    resolution_reasoning TEXT,
    resolved_at DATETIME,
    
    -- If merged, the new unified item
    merged_into_type TEXT,
    merged_into_id TEXT,
    
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    
    UNIQUE(item_a_type, item_a_id, item_b_type, item_b_id)
);

-- Cached reasoning answers (for repeated questions)
CREATE TABLE IF NOT EXISTS reasoning_answers (
    id TEXT PRIMARY KEY,
    
    -- The question
    question TEXT NOT NULL,
    question_hash TEXT NOT NULL, -- For fast lookup
    question_category TEXT,
    
    -- The answer
    answer TEXT NOT NULL,
    reasoning_trace TEXT NOT NULL,
    certainty_statement TEXT NOT NULL,
    
    -- Context that was provided (if any)
    context TEXT,
    
    -- Lifecycle
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME,
    
    -- Metadata for debugging
    facts_considered INTEGER DEFAULT 0,
    inferences_considered INTEGER DEFAULT 0,
    
    metadata JSON DEFAULT '{}'
);

-- Links answers to their supporting evidence
-- Enables users to see exactly what informed each answer
CREATE TABLE IF NOT EXISTS answer_evidence (
    id TEXT PRIMARY KEY,
    
    answer_id TEXT NOT NULL REFERENCES reasoning_answers(id) ON DELETE CASCADE,
    
    -- The evidence (fact or inference)
    evidence_type TEXT NOT NULL, -- 'fact' or 'inference'
    evidence_id TEXT NOT NULL,
    
    -- How this evidence contributed
    relevance_note TEXT,
    
    UNIQUE(answer_id, evidence_type, evidence_id)
);

-- Reasoning events for audit trail
-- Every reasoning operation is logged
CREATE TABLE IF NOT EXISTS reasoning_events (
    id TEXT PRIMARY KEY,
    
    -- What happened
    event_type TEXT NOT NULL,
    -- Types:
    -- 'inference_derived' - new inference created
    -- 'inference_superseded' - inference replaced by newer one
    -- 'contradiction_detected' - contradiction found
    -- 'contradiction_resolved' - contradiction resolved
    -- 'question_answered' - reasoning question answered
    -- 'background_pass_started' - background loop started
    -- 'background_pass_completed' - background loop finished
    
    -- References to involved entities
    inference_id TEXT,
    contradiction_id TEXT,
    answer_id TEXT,
    
    -- Human-readable description
    description TEXT NOT NULL,
    
    -- Full details for debugging
    details JSON DEFAULT '{}',
    
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_inferences_type ON inferences(inference_type);
CREATE INDEX IF NOT EXISTS idx_inferences_category ON inferences(category);
CREATE INDEX IF NOT EXISTS idx_inferences_created ON inferences(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_inferences_superseded ON inferences(superseded);

CREATE INDEX IF NOT EXISTS idx_inference_premises_inference ON inference_premises(inference_id);
CREATE INDEX IF NOT EXISTS idx_inference_premises_premise ON inference_premises(premise_type, premise_id);

CREATE INDEX IF NOT EXISTS idx_contradictions_status ON contradictions(status);
CREATE INDEX IF NOT EXISTS idx_contradictions_items ON contradictions(item_a_id, item_b_id);

CREATE INDEX IF NOT EXISTS idx_reasoning_answers_hash ON reasoning_answers(question_hash);
CREATE INDEX IF NOT EXISTS idx_reasoning_answers_expires ON reasoning_answers(expires_at);

CREATE INDEX IF NOT EXISTS idx_answer_evidence_answer ON answer_evidence(answer_id);
CREATE INDEX IF NOT EXISTS idx_answer_evidence_evidence ON answer_evidence(evidence_type, evidence_id);

CREATE INDEX IF NOT EXISTS idx_reasoning_events_type ON reasoning_events(event_type);
CREATE INDEX IF NOT EXISTS idx_reasoning_events_created ON reasoning_events(created_at DESC);
"#;

/// Initialize reasoning tables in the database
pub async fn init_reasoning_schema(pool: &sqlx::SqlitePool) -> crate::Result<()> {
    sqlx::raw_sql(REASONING_SCHEMA).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_schema_is_valid_sql() {
        let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
        init_reasoning_schema(&pool).await.unwrap();

        // Verify tables exist
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND (name LIKE 'inference%' OR name LIKE 'contradiction%' OR name LIKE 'reasoning%' OR name LIKE 'answer%')"
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
        assert!(
            table_names.contains(&"inferences"),
            "Missing 'inferences' table, found: {table_names:?}"
        );
        assert!(
            table_names.contains(&"inference_premises"),
            "Missing 'inference_premises' table, found: {table_names:?}"
        );
        assert!(
            table_names.contains(&"contradictions"),
            "Missing 'contradictions' table, found: {table_names:?}"
        );
        assert!(
            table_names.contains(&"reasoning_answers"),
            "Missing 'reasoning_answers' table, found: {table_names:?}"
        );
        assert!(
            table_names.contains(&"answer_evidence"),
            "Missing 'answer_evidence' table, found: {table_names:?}"
        );
        assert!(
            table_names.contains(&"reasoning_events"),
            "Missing 'reasoning_events' table, found: {table_names:?}"
        );
    }
}
