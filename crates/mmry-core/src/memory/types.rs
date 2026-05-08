use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
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
    pub expires_at: Option<DateTime<Utc>>,
    pub expired_at: Option<DateTime<Utc>>,
    pub source_attribution: Option<SourceAttribution>,
    pub trust_level: f32,
    pub source_reinforcement_score: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub category: String,
    pub tags: Vec<String>,
    pub parent_id: Option<Uuid>,
    pub chunk_index: Option<i32>,
    pub total_chunks: Option<i32>,
    pub chunk_method: Option<ChunkMethod>,
    /// Bridge block this memory belongs to (for topic/conversation grouping)
    pub bridge_block_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkMethod {
    None,
    Paragraph,
    Sentence,
    Word,
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
        let source_attribution = Some(SourceAttribution::default_user());
        let (trust_level, source_reinforcement_score) = source_attribution
            .as_ref()
            .map(SourceAttribution::compute_metrics)
            .unwrap_or((0.5, 0.0));

        Self {
            id: Uuid::new_v4(),
            memory_type,
            content,
            embedding: None,
            sparse_embedding: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            importance: 5,
            expires_at: None,
            expired_at: None,
            source_attribution,
            trust_level,
            source_reinforcement_score,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            category,
            tags: Vec::new(),
            parent_id: None,
            chunk_index: None,
            total_chunks: None,
            chunk_method: None,
            bridge_block_id: None,
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

    pub fn recompute_trust_metrics(&mut self) {
        let (trust_level, reinforcement) = self
            .source_attribution
            .as_ref()
            .map(SourceAttribution::compute_metrics)
            .unwrap_or((0.5, 0.0));
        self.trust_level = trust_level;
        self.source_reinforcement_score = reinforcement;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    User,
    Llm,
    External,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    pub kind: SourceKind,
    pub label: Option<String>,
    pub trust: f32,
    pub model: Option<String>,
    pub reference: Option<String>,
}

impl SourceEntry {
    pub fn user(label: &str, trust: f32) -> Self {
        Self {
            kind: SourceKind::User,
            label: Some(label.to_string()),
            trust,
            model: None,
            reference: None,
        }
    }

    pub fn llm(label: &str, trust: f32, model: Option<String>) -> Self {
        Self {
            kind: SourceKind::Llm,
            label: Some(label.to_string()),
            trust,
            model,
            reference: None,
        }
    }

    pub fn external(reference: &str, trust: f32) -> Self {
        Self {
            kind: SourceKind::External,
            label: None,
            trust,
            model: None,
            reference: Some(reference.to_string()),
        }
    }

    fn unique_key(&self) -> String {
        format!(
            "{:?}:{:?}:{:?}:{:?}",
            self.kind, self.label, self.model, self.reference
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAttribution {
    pub sources: Vec<SourceEntry>,
}

impl SourceAttribution {
    pub fn new(sources: Vec<SourceEntry>) -> Self {
        Self { sources }
    }

    pub fn default_user() -> Self {
        Self {
            sources: vec![SourceEntry::user("direct_input", 0.9)],
        }
    }

    pub fn add_source(&mut self, source: SourceEntry) -> bool {
        let key = source.unique_key();
        if let Some(existing) = self
            .sources
            .iter_mut()
            .find(|existing| existing.unique_key() == key)
        {
            if source.trust > existing.trust {
                existing.trust = source.trust;
            }
            return false;
        }

        self.sources.push(source);
        true
    }

    pub fn compute_metrics(&self) -> (f32, f32) {
        if self.sources.is_empty() {
            return (0.5, 0.0);
        }

        let avg_trust = self
            .sources
            .iter()
            .map(|source| source.trust.clamp(0.0, 1.0))
            .sum::<f32>()
            / self.sources.len() as f32;

        let mut unique = HashSet::new();
        for source in &self.sources {
            unique.insert(source.unique_key());
        }

        let bonus = (unique.len().saturating_sub(1) as f32) * 0.1;
        let reinforcement = (avg_trust + bonus).clamp(0.0, 1.5);

        (avg_trust.clamp(0.0, 1.0), reinforcement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_attribution_metrics_grow_with_sources() {
        let attribution = SourceAttribution::new(vec![
            SourceEntry::user("direct", 0.8),
            SourceEntry::external("https://example.com", 0.6),
        ]);

        let (trust_level, reinforcement) = attribution.compute_metrics();
        assert!((trust_level - 0.7).abs() < 0.001);
        assert!((reinforcement - 0.8).abs() < 0.001);
    }

    #[test]
    fn add_source_dedupes_by_key_and_updates_trust() {
        let mut attribution = SourceAttribution::new(vec![SourceEntry::user("direct", 0.7)]);
        let added = attribution.add_source(SourceEntry::user("direct", 0.9));

        assert!(!added);
        assert_eq!(attribution.sources.len(), 1);
        assert!((attribution.sources[0].trust - 0.9).abs() < 0.001);
    }
}
