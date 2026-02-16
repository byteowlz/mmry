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
