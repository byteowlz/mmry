use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::Result;
use async_trait::async_trait;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AnalyzerRouting {
    pub chosen_block: Option<uuid::Uuid>,
    pub is_new_topic: bool,
    pub rationale: Option<String>,
}

impl AnalyzerRouting {
    pub fn new_topic() -> Self {
        Self {
            chosen_block: None,
            is_new_topic: true,
            rationale: None,
        }
    }
}

/// Analyzer trait for extracting facts and routing queries.
/// All methods are async to support both synchronous and async implementations.
#[async_trait]
pub trait Analyzer: Send + Sync {
    /// Extract structured facts from free-form content. Implementations may use LLMs,
    /// heuristics, or rule-based logic. Must be safe to call even when no model is present.
    async fn extract_facts(&self, content: &str) -> Result<Vec<FactRecord>>;

    /// Route a query against candidate bridge blocks. Implementations should prefer
    /// deterministic behavior and only use LLMs when configured.
    async fn route(&self, _query: &str, _candidates: &[BridgeBlock]) -> Result<AnalyzerRouting> {
        Ok(AnalyzerRouting::new_topic())
    }
}

#[derive(Debug, Default)]
pub struct NoOpAnalyzer;

#[async_trait]
impl Analyzer for NoOpAnalyzer {
    async fn extract_facts(&self, _content: &str) -> Result<Vec<FactRecord>> {
        Ok(Vec::new())
    }

    async fn route(&self, _query: &str, _candidates: &[BridgeBlock]) -> Result<AnalyzerRouting> {
        Ok(AnalyzerRouting::new_topic())
    }
}
