use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::config::Config;
#[cfg(feature = "service")]
use crate::hmlr::prompts::fact_extraction_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_facts_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_filtering_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_topic_label_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::topic_label_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::two_key_filtering_prompt;
use crate::hmlr::prompts::FilteringResult;
use crate::hmlr::prompts::MemoryCandidate;
use crate::Result;
use async_trait::async_trait;
#[cfg(feature = "service")]
use rig::client::CompletionClient;
#[cfg(feature = "service")]
use rig::completion::AssistantContent;
#[cfg(feature = "service")]
use rig::completion::CompletionModel;
#[cfg(feature = "service")]
use rig::completion::CompletionResponse as RigCompletionResponse;
#[cfg(feature = "service")]
use rig::message::Message as RigMessage;
#[cfg(feature = "service")]
use rig::message::UserContent;
#[cfg(feature = "service")]
use rig::one_or_many::OneOrMany;
#[cfg(feature = "service")]
use rig::providers::openai;
#[cfg(feature = "service")]
use serde_json::json;
use std::sync::Arc;
#[cfg(feature = "service")]
use tracing::warn;

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

    /// Generate a concise topic label for a memory/bridge block.
    /// Used when creating new bridge blocks to summarize the conversation topic.
    async fn generate_topic_label(&self, _content: &str) -> Result<Option<String>> {
        // Default: no topic label generation
        Ok(None)
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

    async fn generate_topic_label(&self, _content: &str) -> Result<Option<String>> {
        // NoOp: no topic label generation
        Ok(None)
    }
}

#[cfg(feature = "service")]
#[derive(Debug, Clone)]
struct RigAnalyzer {
    model_name: String,
    client: openai::CompletionsClient,
}

#[cfg(feature = "service")]
impl RigAnalyzer {
    fn new(model_name: String, client: openai::CompletionsClient) -> Self {
        Self { model_name, client }
    }
}

#[cfg(feature = "service")]
#[async_trait]
impl Analyzer for RigAnalyzer {
    async fn extract_facts(&self, content: &str) -> crate::Result<Vec<FactRecord>> {
        let prompt = fact_extraction_prompt(content);

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(prompt)),
            })
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| crate::Error::Service(format!("Rig completion failed: {e}")))?;

        let facts = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => Some(parse_facts_response(&t.text)),
                _ => None,
            })
            .unwrap_or_default();

        Ok(facts)
    }

    async fn route(
        &self,
        query: &str,
        candidates: &[BridgeBlock],
    ) -> crate::Result<AnalyzerRouting> {
        let prompt = "You route user queries to conversation bridge blocks. Reply with JSON: {\"chosen_block\": \"<uuid-or-null>\", \"is_new_topic\": true|false, \"reason\": \"...\"}.";
        let user_payload = json!({
            "query": query,
            "bridge_blocks": candidates.iter().map(|b| {
                json!({
                    "block_id": b.block_id.to_string(),
                    "topic": b.topic_label,
                    "keywords": b.keywords,
                    "status": b.status,
                })
            }).collect::<Vec<_>>()
        })
        .to_string();

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(user_payload)),
            })
            .preamble(prompt.to_string())
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| crate::Error::Service(format!("Rig completion failed: {e}")))?;

        let routing = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => {
                    serde_json::from_str::<serde_json::Value>(&t.text).ok()
                }
                _ => None,
            })
            .and_then(|val| parse_routing_from_content(&val))
            .unwrap_or_else(AnalyzerRouting::new_topic);

        Ok(routing)
    }

    async fn filter_memories(
        &self,
        query: &str,
        candidates: &[MemoryCandidate],
    ) -> crate::Result<FilteringResult> {
        if candidates.is_empty() {
            return Ok(FilteringResult::default());
        }

        let prompt = two_key_filtering_prompt(query, candidates);

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(prompt)),
            })
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| crate::Error::Service(format!("Rig completion failed: {e}")))?;

        let result = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => Some(parse_filtering_response(&t.text)),
                _ => None,
            })
            .unwrap_or_else(|| FilteringResult {
                relevant_indices: candidates.iter().map(|c| c.index).collect(),
                reasoning: None,
            });

        Ok(result)
    }

    async fn generate_topic_label(&self, content: &str) -> crate::Result<Option<String>> {
        let prompt = topic_label_prompt(content);

        tracing::debug!(target: "llm", content_len = content.len(), "Generating topic label");

        let model = self.client.completion_model(self.model_name.clone());
        let request = model
            .completion_request(RigMessage::User {
                content: OneOrMany::one(UserContent::text(prompt)),
            })
            .temperature(0.0)
            .build();

        let response: RigCompletionResponse<_> = model
            .completion(request)
            .await
            .map_err(|e| crate::Error::Service(format!("Rig completion failed: {e}")))?;

        let topic_label = response.choice.iter().find_map(|content| match content {
            AssistantContent::Text(t) => {
                tracing::debug!(target: "llm", raw_response = %t.text, "Topic label response");
                parse_topic_label_response(&t.text)
            }
            _ => None,
        });

        Ok(topic_label)
    }
}

#[cfg(feature = "service")]
fn parse_routing_from_content(content: &serde_json::Value) -> Option<AnalyzerRouting> {
    let chosen_block = content
        .get("chosen_block")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok());
    let is_new_topic = content
        .get("is_new_topic")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let rationale = content
        .get("reason")
        .or_else(|| content.get("rationale"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    Some(AnalyzerRouting {
        chosen_block,
        is_new_topic,
        rationale,
    })
}

#[cfg(feature = "service")]
fn normalize_rig_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let base = if trimmed.ends_with("/chat/completions") {
        trimmed
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
    } else {
        trimmed
    };

    let base = if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    };

    Some(base)
}

#[cfg(feature = "service")]
fn build_rig_analyzer(config: &Config) -> Arc<dyn Analyzer + Send + Sync> {
    if !config.analyzer.enabled {
        return Arc::new(NoOpAnalyzer);
    }

    let Some(base) = config
        .analyzer
        .endpoint
        .as_deref()
        .and_then(normalize_rig_base_url)
    else {
        warn!("Analyzer enabled but no endpoint configured");
        return Arc::new(NoOpAnalyzer);
    };

    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "mmry-local".to_string());

    let builder = openai::CompletionsClient::builder()
        .api_key(&api_key)
        .base_url(&base);

    match builder.build() {
        Ok(client) => {
            let model = config
                .analyzer
                .model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            Arc::new(RigAnalyzer::new(model, client))
        }
        Err(e) => {
            warn!("Failed to build analyzer client: {e}");
            Arc::new(NoOpAnalyzer)
        }
    }
}

pub fn build_analyzer(config: &Config) -> Arc<dyn Analyzer + Send + Sync> {
    #[cfg(feature = "service")]
    {
        build_rig_analyzer(config)
    }

    #[cfg(not(feature = "service"))]
    {
        let _ = config;
        Arc::new(NoOpAnalyzer)
    }
}
