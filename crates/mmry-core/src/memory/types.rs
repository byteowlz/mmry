use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::sparse_embeddings::StoredSparseEmbedding;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Episodic,
    Semantic,
    Procedural,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub memory_type: MemoryType,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub sparse_embedding: Option<StoredSparseEmbedding>,
    pub metadata: serde_json::Value,
    pub importance: i32,
    /// Times a search returning this memory was closed with this id in `--using`.
    #[serde(default)]
    pub helpful_count: i64,
    /// Reserved for explicit negative feedback. Not yet populated.
    #[serde(default)]
    pub harmful_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub category: String,
    pub tags: Vec<String>,
    pub parent_id: Option<Uuid>,
    pub chunk_index: Option<i32>,
    pub total_chunks: Option<i32>,
}

impl Memory {
    pub fn new(memory_type: MemoryType, content: String, category: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            memory_type,
            content,
            embedding: None,
            sparse_embedding: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            importance: 5,
            helpful_count: 0,
            harmful_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            category,
            tags: Vec::new(),
            parent_id: None,
            chunk_index: None,
            total_chunks: None,
        }
    }

    pub fn is_chunk(&self) -> bool {
        self.parent_id.is_some()
    }

    pub fn is_parent(&self) -> bool {
        self.total_chunks.is_some() && self.total_chunks.unwrap() > 1
    }

    /// Workspace id captured from `AGENT_CTX_WORKSPACE_ID` at write time.
    pub fn workspace_id(&self) -> Option<&str> {
        self.metadata
            .get("agent_ctx")
            .and_then(|v| v.get("workspace_id"))
            .and_then(serde_json::Value::as_str)
    }

    /// Platform session id from `AGENT_CTX_PLATFORM_SESSION_ID`.
    pub fn platform_session_id(&self) -> Option<&str> {
        self.metadata
            .get("agent_ctx")
            .and_then(|v| v.get("platform_session_id"))
            .and_then(serde_json::Value::as_str)
    }

    /// Harness session id from `AGENT_CTX_HARNESS_SESSION_ID`.
    pub fn harness_session_id(&self) -> Option<&str> {
        self.metadata
            .get("agent_ctx")
            .and_then(|v| v.get("harness_session_id"))
            .and_then(serde_json::Value::as_str)
    }

}
