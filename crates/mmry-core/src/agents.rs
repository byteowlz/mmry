use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::database::operations;

/// Lightweight struct for passing agent identity through CLI/MCP/gRPC boundaries.
///
/// Resolved into a full `AgentRecord` via `resolve()`, which get-or-creates
/// the agent in the database by name (matching on both name and kind).
#[derive(Debug, Clone, Default)]
pub struct AgentIdentity {
    /// Agent name (e.g. "claude-code", "pi", "human").  Defaults to "human".
    pub name: Option<String>,
    /// Agent kind (e.g. "human", "coding_agent", "review_agent").  Defaults to "human".
    pub kind: Option<String>,
    /// Free-form JSON metadata (repo, workspace, session_id, …).
    pub meta: Option<Value>,
}

impl AgentIdentity {
    /// Resolve this identity into a persisted `AgentRecord`, creating the
    /// record if it does not already exist.
    ///
    /// Matching is by **name** only; if the agent exists the record is
    /// returned as-is (metadata is merged on creation, not on every lookup).
    pub async fn resolve(&self, pool: &sqlx::SqlitePool) -> crate::Result<AgentRecord> {
        let name = self.name.as_deref().unwrap_or("human");
        let kind = self.kind.as_deref().unwrap_or("human");

        // Fast path: agent already exists
        if let Some(existing) = operations::get_agent_by_name(pool, name).await? {
            return Ok(existing);
        }

        // Slow path: create
        let mut agent = AgentRecord::new(name.to_string(), kind.to_string());
        if let Some(meta) = &self.meta {
            agent.metadata = meta.clone();
        }
        operations::upsert_agent(pool, &agent).await?;
        Ok(agent)
    }

    /// True when the caller explicitly provided at least a name.
    pub fn is_explicit(&self) -> bool {
        self.name.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRecord {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentRecord {
    pub fn new<S: Into<String>>(name: S, kind: S) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: kind.into(),
            description: None,
            metadata: Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }

    // ── Structured metadata accessors ─────────────────────────────
    // These read/write well-known fields inside the free-form `metadata`
    // JSON object so callers don't need to know the schema.

    /// Repository this agent operates in (e.g. "byteowlz/mmry").
    pub fn repo(&self) -> Option<&str> {
        self.metadata.get("repo").and_then(Value::as_str)
    }

    /// Workspace / project root path.
    pub fn workspace(&self) -> Option<&str> {
        self.metadata.get("workspace").and_then(Value::as_str)
    }

    /// Session identifier (unique per agent invocation).
    pub fn session_id(&self) -> Option<&str> {
        self.metadata.get("session_id").and_then(Value::as_str)
    }

    /// Set a well-known metadata field, creating the object if needed.
    pub fn set_meta(&mut self, key: &str, value: impl Into<Value>) {
        if let Some(obj) = self.metadata.as_object_mut() {
            obj.insert(key.to_string(), value.into());
        } else {
            let mut map = serde_json::Map::new();
            map.insert(key.to_string(), value.into());
            self.metadata = Value::Object(map);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEvent {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub event_type: String,
    pub status: Option<String>,
    pub payload: Value,
    pub span_id: Option<String>,
    pub memory_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentEvent {
    pub fn new<S: Into<String>>(agent_id: Uuid, event_type: S) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            agent_id,
            event_type: event_type.into(),
            status: None,
            payload: Value::Object(serde_json::Map::new()),
            span_id: None,
            memory_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeBlock {
    pub block_id: Uuid,
    pub span_id: Option<String>,
    pub topic_label: Option<String>,
    pub keywords: Vec<String>,
    pub status: Option<String>,
    pub exit_reason: Option<String>,
    pub content: Value,
    pub agent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    /// Unresolved questions or tasks within this conversation topic
    /// Example: ["What is the deployment timeline?", "Need to confirm API rate limits"]
    #[serde(default)]
    pub open_loops: Vec<String>,
    /// Key decisions made during this conversation topic
    /// Example: ["Use PostgreSQL for the database", "Deploy to AWS us-east-1"]
    #[serde(default)]
    pub decisions_made: Vec<String>,
    /// Embedding vector for semantic matching during routing
    /// Generated from topic_label + keywords for similarity search
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

impl Default for BridgeBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeBlock {
    pub fn new() -> Self {
        Self {
            block_id: Uuid::new_v4(),
            span_id: None,
            topic_label: None,
            keywords: Vec::new(),
            status: None,
            exit_reason: None,
            content: Value::Object(serde_json::Map::new()),
            agent_id: None,
            created_at: Utc::now(),
            open_loops: Vec::new(),
            decisions_made: Vec::new(),
            embedding: None,
        }
    }

    /// Generate text representation for embedding
    /// Combines topic_label, keywords, and any summary for semantic matching
    pub fn embedding_text(&self) -> String {
        let mut parts = Vec::new();

        if let Some(label) = &self.topic_label {
            parts.push(label.clone());
        }

        if !self.keywords.is_empty() {
            parts.push(self.keywords.join(" "));
        }

        // Extract summary from content if present
        if let Some(summary) = self.content.get("summary").and_then(|v| v.as_str()) {
            parts.push(summary.to_string());
        }

        parts.join(". ")
    }

    /// Add an open loop (unresolved question/task) to this block
    pub fn add_open_loop<S: Into<String>>(&mut self, question: S) {
        let q = question.into();
        if !self.open_loops.contains(&q) {
            self.open_loops.push(q);
        }
    }

    /// Close an open loop (mark question/task as resolved)
    pub fn close_open_loop(&mut self, question: &str) {
        self.open_loops.retain(|q| q != question);
    }

    /// Record a decision made during this conversation
    pub fn add_decision<S: Into<String>>(&mut self, decision: S) {
        let d = decision.into();
        if !self.decisions_made.contains(&d) {
            self.decisions_made.push(d);
        }
    }
}

/// Category of extracted fact for better organization and retrieval
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum FactCategory {
    /// Definitions of terms or concepts
    Definition,
    /// Acronym expansions (e.g., "API = Application Programming Interface")
    Acronym,
    /// Credentials, API keys, passwords, tokens
    Secret,
    /// Relationships between entities (e.g., "John is CEO of X")
    Entity,
    /// Generic fact that doesn't fit other categories
    #[default]
    General,
}

impl FactCategory {
    /// Parse category from string (case-insensitive)
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "definition" => Self::Definition,
            "acronym" => Self::Acronym,
            "secret" => Self::Secret,
            "entity" => Self::Entity,
            _ => Self::General,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Definition => "Definition",
            Self::Acronym => "Acronym",
            Self::Secret => "Secret",
            Self::Entity => "Entity",
            Self::General => "General",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactRecord {
    pub id: Uuid,
    pub fact_key: String,
    pub fact_value: String,
    /// Category of the fact (Definition, Acronym, Secret, Entity, General)
    pub category: FactCategory,
    /// 10-20 word snippet of context around the fact for provenance
    pub evidence_snippet: Option<String>,
    pub source_span: Option<String>,
    pub turn_id: Option<String>,
    /// Source chunk ID for sentence-level provenance
    pub source_chunk_id: Option<String>,
    /// Source paragraph chunk ID for broader context
    pub source_paragraph_id: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub recency_score: f32,
    pub metadata: Value,
    pub agent_id: Option<Uuid>,
}

impl FactRecord {
    pub fn new<K: Into<String>, V: Into<String>>(key: K, value: V) -> Self {
        Self {
            id: Uuid::new_v4(),
            fact_key: key.into(),
            fact_value: value.into(),
            category: FactCategory::General,
            evidence_snippet: None,
            source_span: None,
            turn_id: None,
            source_chunk_id: None,
            source_paragraph_id: None,
            observed_at: Utc::now(),
            recency_score: 1.0,
            metadata: Value::Object(serde_json::Map::new()),
            agent_id: None,
        }
    }

    /// Create a fact with a specific category
    pub fn with_category<K: Into<String>, V: Into<String>>(
        key: K,
        value: V,
        category: FactCategory,
    ) -> Self {
        let mut fact = Self::new(key, value);
        fact.category = category;
        fact
    }

    pub fn fingerprint(&self) -> String {
        fact_fingerprint(
            self.category,
            &self.fact_key,
            &self.fact_value,
            self.agent_id,
        )
    }
}

pub(crate) fn fact_fingerprint(
    category: FactCategory,
    fact_key: &str,
    fact_value: &str,
    agent_id: Option<Uuid>,
) -> String {
    let category = category.as_str().to_ascii_lowercase();
    let key = normalize_fingerprint_component(fact_key);
    let value = normalize_fingerprint_component(fact_value);
    let agent = agent_id.map(|id| id.to_string()).unwrap_or_default();

    format!("{category}|{agent}|{key}|{value}")
}

fn normalize_fingerprint_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_space = false;

    for ch in input.trim().chars() {
        let is_space = ch.is_whitespace();
        if is_space {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
            }
            last_was_space = true;
            continue;
        }

        last_was_space = false;

        for lower in ch.to_lowercase() {
            if lower.is_alphanumeric() || matches!(lower, '-' | '_' | '.' | ':' | '/' | '@' | '=') {
                out.push(lower);
            } else if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
        }
    }

    out.trim()
        .trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\''))
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserProfileEntry {
    pub id: Uuid,
    pub profile: Value,
    pub updated_at: DateTime<Utc>,
}

impl UserProfileEntry {
    pub fn new(profile: Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            profile,
            updated_at: Utc::now(),
        }
    }
}
