use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactRecord {
    pub id: Uuid,
    pub fact_key: String,
    pub fact_value: String,
    pub source_span: Option<String>,
    pub turn_id: Option<String>,
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
            source_span: None,
            turn_id: None,
            observed_at: Utc::now(),
            recency_score: 1.0,
            metadata: Value::Object(serde_json::Map::new()),
            agent_id: None,
        }
    }
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
