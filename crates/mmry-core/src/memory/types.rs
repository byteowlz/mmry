use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::sparse_embeddings::StoredSparseEmbedding;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicMemory {
    pub id: Uuid,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub embedding: Option<Vec<f32>>,
    pub entities: Vec<String>,
    pub tags: Vec<String>,
    pub importance: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: Uuid,
    pub fact: String,
    pub embedding: Vec<f32>,
    pub related_memories: Vec<Uuid>,
    pub confidence: f32,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralMemory {
    pub id: Uuid,
    pub name: String,
    pub steps: Vec<String>,
    pub context: Option<String>,
    pub embedding: Vec<f32>,
    pub tags: Vec<String>,
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
            category,
            tags: Vec::new(),
        }
    }
}
