use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::hmlr::prompts::FilteringResult;
use crate::hmlr::prompts::MemoryCandidate;
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

    /// Filter memory candidates using 2-key validation (similarity + original query).
    /// This catches false positives where high vector similarity doesn't mean relevance.
    /// Example: "I love Python" vs "I hate Python" = 95% similar but OPPOSITE meaning.
    async fn filter_memories(
        &self,
        _query: &str,
        _candidates: &[MemoryCandidate],
    ) -> Result<FilteringResult> {
        // Default: return all candidates (no filtering)
        Ok(FilteringResult {
            relevant_indices: _candidates.iter().map(|c| c.index).collect(),
            reasoning: None,
        })
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

    async fn filter_memories(
        &self,
        _query: &str,
        candidates: &[MemoryCandidate],
    ) -> Result<FilteringResult> {
        // NoOp: return all candidates
        Ok(FilteringResult {
            relevant_indices: candidates.iter().map(|c| c.index).collect(),
            reasoning: None,
        })
    }
}
