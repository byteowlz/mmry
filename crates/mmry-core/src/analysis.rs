use crate::agents::BridgeBlock;
use crate::agents::FactRecord;
use crate::config::Config;
#[cfg(feature = "service")]
use crate::hmlr::prompts::expiration_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::fact_extraction_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_expiration_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_facts_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_filtering_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_routing_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_synthesis_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::parse_topic_label_response;
#[cfg(feature = "service")]
use crate::hmlr::prompts::routing_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::synthesis_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::topic_label_prompt;
#[cfg(feature = "service")]
use crate::hmlr::prompts::two_key_filtering_prompt;
use crate::hmlr::prompts::FilteringResult;
use crate::hmlr::prompts::MemoryCandidate;
use crate::hmlr::SynthesisResult;
use crate::Result;
use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
#[cfg(feature = "service")]
use reqwest::Client;
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
use std::sync::atomic::AtomicU64;
#[cfg(feature = "service")]
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(feature = "service")]
use std::time::Duration;
#[cfg(feature = "service")]
use std::time::Instant;
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

    async fn infer_expiration(&self, _content: &str) -> Result<Option<DateTime<Utc>>> {
        Ok(None)
    }

    async fn synthesize_bridge_block(
        &self,
        _block: &BridgeBlock,
        _memories: &[String],
    ) -> Result<Option<SynthesisResult>> {
        Ok(None)
    }

    fn is_noop(&self) -> bool {
        false
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

    async fn infer_expiration(&self, _content: &str) -> Result<Option<DateTime<Utc>>> {
        Ok(None)
    }

    async fn synthesize_bridge_block(
        &self,
        _block: &BridgeBlock,
        _memories: &[String],
    ) -> Result<Option<SynthesisResult>> {
        Ok(None)
    }

    fn is_noop(&self) -> bool {
        true
    }
}

#[cfg(feature = "service")]
#[derive(Debug, Clone)]
struct RigAnalyzer {
    model_name: String,
    client: openai::CompletionsClient,
    retry_count: u32,
    retry_backoff_ms: u64,
}

#[cfg(feature = "service")]
impl RigAnalyzer {
    fn new(
        model_name: String,
        client: openai::CompletionsClient,
        retry_count: u32,
        retry_backoff_ms: u64,
    ) -> Self {
        Self {
            model_name,
            client,
            retry_count,
            retry_backoff_ms,
        }
    }
}

#[cfg(feature = "service")]
#[async_trait]
impl Analyzer for RigAnalyzer {
    async fn extract_facts(&self, content: &str) -> crate::Result<Vec<FactRecord>> {
        let prompt = fact_extraction_prompt(content);

        log_llm_prompt("extract_facts", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("extract_facts", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("extract_facts", start.elapsed(), true);

        let facts = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => {
                    log_llm_response("extract_facts", &t.text);
                    Some(parse_facts_response(&t.text))
                }
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
        let prompt = routing_prompt(query, candidates);
        log_llm_prompt("route", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("route", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("route", start.elapsed(), true);

        let routing = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => {
                    log_llm_response("route", &t.text);
                    parse_routing_response(&t.text).map(|decision| AnalyzerRouting {
                        chosen_block: decision.chosen_block,
                        is_new_topic: decision.is_new_topic,
                        rationale: decision.rationale,
                    })
                }
                _ => None,
            })
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

        log_llm_prompt("filter_memories", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("filter_memories", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("filter_memories", start.elapsed(), true);

        let result = response
            .choice
            .iter()
            .find_map(|content| match content {
                AssistantContent::Text(t) => {
                    log_llm_response("filter_memories", &t.text);
                    Some(parse_filtering_response(&t.text))
                }
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

        log_llm_prompt("generate_topic_label", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("generate_topic_label", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("generate_topic_label", start.elapsed(), true);

        let topic_label = response.choice.iter().find_map(|content| match content {
            AssistantContent::Text(t) => {
                log_llm_response("generate_topic_label", &t.text);
                parse_topic_label_response(&t.text)
            }
            _ => None,
        });

        Ok(topic_label)
    }

    async fn infer_expiration(&self, content: &str) -> crate::Result<Option<DateTime<Utc>>> {
        let prompt = expiration_prompt(content);
        log_llm_prompt("infer_expiration", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("infer_expiration", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("infer_expiration", start.elapsed(), true);

        let inferred = response.choice.iter().find_map(|content| match content {
            AssistantContent::Text(t) => {
                log_llm_response("infer_expiration", &t.text);
                parse_expiration_response(&t.text)
            }
            _ => None,
        });

        Ok(inferred)
    }

    async fn synthesize_bridge_block(
        &self,
        block: &BridgeBlock,
        memories: &[String],
    ) -> crate::Result<Option<SynthesisResult>> {
        let prompt = synthesis_prompt(block, memories);
        log_llm_prompt("synthesize_bridge_block", &prompt);
        let start = Instant::now();
        let response: RigCompletionResponse<_> = self
            .completion_with_retry("synthesize_bridge_block", || {
                self.client
                    .completion_model(self.model_name.clone())
                    .completion_request(RigMessage::User {
                        content: OneOrMany::one(UserContent::text(prompt.clone())),
                    })
                    .temperature(0.0)
                    .build()
            })
            .await?;
        log_llm_timing("synthesize_bridge_block", start.elapsed(), true);

        let result = response.choice.iter().find_map(|content| match content {
            AssistantContent::Text(t) => {
                log_llm_response("synthesize_bridge_block", &t.text);
                parse_synthesis_response(&t.text, block.block_id)
            }
            _ => None,
        });

        Ok(result)
    }
}

#[cfg(feature = "service")]
impl RigAnalyzer {
    async fn completion_with_retry<F>(
        &self,
        operation: &str,
        mut build_request: F,
    ) -> crate::Result<RigCompletionResponse<openai::CompletionResponse>>
    where
        F: FnMut() -> rig::completion::CompletionRequest,
    {
        let mut last_err: Option<String> = None;
        let mut start = Instant::now();

        for attempt in 0..=self.retry_count {
            let request = build_request();
            let response = self
                .client
                .completion_model(self.model_name.clone())
                .completion(request)
                .await;

            match response {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    let err_str = e.to_string();
                    last_err = Some(err_str.clone());
                    log_llm_timing(operation, start.elapsed(), false);
                    if attempt < self.retry_count {
                        let backoff = self.retry_backoff_ms.saturating_mul(2_u64.pow(attempt));
                        tracing::warn!(
                            target: "llm",
                            operation,
                            attempt = attempt + 1,
                            backoff_ms = backoff,
                            error = %err_str,
                            "LLM call failed, retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                        start = Instant::now();
                    }
                }
            }
        }

        let attempts = self.retry_count + 1;
        Err(crate::Error::Service(format!(
            "Rig completion failed after {attempts} attempts: {}",
            last_err.unwrap_or_else(|| "unknown error".to_string())
        )))
    }
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
            Arc::new(RigAnalyzer::new(
                model,
                client,
                config.analyzer.retry_count,
                config.analyzer.retry_backoff_ms,
            ))
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

#[cfg(feature = "service")]
pub async fn check_analyzer_health(config: &Config) -> Result<()> {
    if !config.analyzer.enabled {
        return Err(crate::Error::Config(
            "Analyzer is disabled in config".to_string(),
        ));
    }

    let Some(base) = config
        .analyzer
        .endpoint
        .as_deref()
        .and_then(normalize_rig_base_url)
    else {
        return Err(crate::Error::Config(
            "Analyzer enabled but no endpoint configured".to_string(),
        ));
    };

    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "mmry-local".to_string());
    let url = format!("{base}/models");

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| crate::Error::Service(format!("Failed to build HTTP client: {e}")))?;

    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| crate::Error::Service(format!("LLM endpoint unreachable: {e}")))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(crate::Error::Service(format!(
            "LLM endpoint returned status {}",
            response.status()
        )))
    }
}

#[cfg(feature = "service")]
fn llm_debug_enabled() -> bool {
    matches!(
        std::env::var("MMRY_LLM_DEBUG")
            .ok()
            .as_deref()
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[cfg(feature = "service")]
static LLM_REQUESTS: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "service")]
static LLM_FAILURES: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "service")]
fn log_llm_prompt(operation: &str, prompt: &str) {
    if llm_debug_enabled() {
        tracing::debug!(
            target: "llm",
            operation,
            prompt_len = prompt.len(),
            prompt = %prompt,
            "LLM prompt"
        );
    }
}

#[cfg(feature = "service")]
fn log_llm_response(operation: &str, response: &str) {
    if llm_debug_enabled() {
        tracing::debug!(
            target: "llm",
            operation,
            response_len = response.len(),
            raw_response = %response,
            "LLM response"
        );
    }
}

#[cfg(feature = "service")]
fn log_llm_timing(operation: &str, elapsed: std::time::Duration, success: bool) {
    let total = LLM_REQUESTS.fetch_add(1, Ordering::Relaxed) + 1;
    let failures = if success {
        LLM_FAILURES.load(Ordering::Relaxed)
    } else {
        LLM_FAILURES.fetch_add(1, Ordering::Relaxed) + 1
    };
    tracing::info!(
        target: "llm",
        operation,
        elapsed_ms = elapsed.as_millis(),
        success,
        total_requests = total,
        total_failures = failures,
        "LLM call completed"
    );
}
