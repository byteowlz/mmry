//! ReasoningService - orchestrates both on-demand and background reasoning
//!
//! Two modes of operation:
//! 1. On-demand: Agent asks a question → system reasons over facts → returns answer
//! 2. Background: Periodic loop derives new inferences from existing facts

use crate::analysis::Analyzer;
use crate::Result;
use sqlx::SqlitePool;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::config::ReasoningConfig;
use super::inference::Inference;
use super::inference::ReasoningAnswer;
use super::questions::ReasoningQuestion;

/// State for the background reasoning loop
#[derive(Debug, Default)]
struct BackgroundState {
    /// Timestamp of last reasoning pass
    last_pass_timestamp: AtomicU64,
    /// Number of facts at last pass (to detect changes)
    facts_at_last_pass: AtomicU64,
    /// Whether a pass is currently running
    pass_in_progress: AtomicBool,
}

/// Simple LRU-ish cache for reasoning answers
struct AnswerCache {
    entries: RwLock<Vec<CacheEntry>>,
    max_entries: usize,
    ttl_seconds: u64,
}

struct CacheEntry {
    question_hash: u64,
    answer: ReasoningAnswer,
    created_at: u64,
}

impl AnswerCache {
    fn new(max_entries: usize, ttl_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            max_entries,
            ttl_seconds,
        }
    }

    async fn get(&self, question: &str) -> Option<ReasoningAnswer> {
        if self.ttl_seconds == 0 {
            return None;
        }

        let hash = Self::hash_question(question);
        let now = Self::now_secs();
        let entries = self.entries.read().await;

        entries
            .iter()
            .find(|e| e.question_hash == hash && (now - e.created_at) < self.ttl_seconds)
            .map(|e| e.answer.clone())
    }

    async fn put(&self, question: &str, answer: ReasoningAnswer) {
        if self.ttl_seconds == 0 {
            return;
        }

        let hash = Self::hash_question(question);
        let now = Self::now_secs();
        let mut entries = self.entries.write().await;

        // Remove expired and matching entries
        entries.retain(|e| (now - e.created_at) < self.ttl_seconds && e.question_hash != hash);

        // Add new entry
        entries.push(CacheEntry {
            question_hash: hash,
            answer,
            created_at: now,
        });

        // Evict oldest if over capacity
        if entries.len() > self.max_entries {
            entries.remove(0);
        }
    }

    fn hash_question(question: &str) -> u64 {
        use std::hash::Hash;
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        question.to_lowercase().hash(&mut hasher);
        hasher.finish()
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// ReasoningService orchestrates reasoning operations
pub struct ReasoningService {
    config: ReasoningConfig,
    pool: SqlitePool,
    analyzer: Arc<dyn Analyzer>,
    background_state: BackgroundState,
    answer_cache: AnswerCache,
}

impl ReasoningService {
    /// Create a new ReasoningService
    pub fn new(config: ReasoningConfig, pool: SqlitePool, analyzer: Arc<dyn Analyzer>) -> Self {
        let answer_cache = AnswerCache::new(100, config.answer_cache_seconds);
        Self {
            config,
            pool,
            analyzer,
            background_state: BackgroundState::default(),
            answer_cache,
        }
    }

    /// Check if reasoning is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    // ─────────────────────────────────────────────────────────────
    // On-demand reasoning (questions)
    // ─────────────────────────────────────────────────────────────

    /// Ask a reasoning question and get an answer
    ///
    /// This is the main entry point for on-demand reasoning.
    /// The system will:
    /// 1. Check cache for recent identical question
    /// 2. Gather relevant facts and inferences
    /// 3. Use the analyzer to reason over them
    /// 4. Return a synthesized answer with reasoning trace
    pub async fn ask(&self, question: ReasoningQuestion) -> Result<ReasoningAnswer> {
        if !self.config.enabled {
            return Ok(ReasoningAnswer::new(
                &question.question,
                "Reasoning module is disabled",
                "No reasoning performed",
                "N/A - reasoning disabled",
            ));
        }

        // Check cache
        if let Some(cached) = self.answer_cache.get(&question.question).await {
            return Ok(cached);
        }

        // Gather relevant facts
        let facts = self.gather_facts_for_question(&question).await?;

        // Gather relevant inferences (if enabled)
        let inferences = if question.include_inferences {
            self.gather_inferences_for_question(&question).await?
        } else {
            Vec::new()
        };

        // Build context and reason
        let answer = self
            .reason_over_evidence(&question, &facts, &inferences)
            .await?;

        // Cache the answer
        self.answer_cache
            .put(&question.question, answer.clone())
            .await;

        Ok(answer)
    }

    /// Convenience method for simple string questions
    pub async fn ask_simple(&self, question: &str) -> Result<ReasoningAnswer> {
        self.ask(ReasoningQuestion::new(question)).await
    }

    async fn gather_facts_for_question(
        &self,
        question: &ReasoningQuestion,
    ) -> Result<Vec<crate::agents::FactRecord>> {
        let limit = question
            .max_facts
            .unwrap_or(self.config.max_facts_for_question) as i64;

        // Use semantic search to find relevant facts
        // For now, use simple listing - TODO: integrate with search
        let facts = crate::database::operations::list_recent_facts(&self.pool, limit).await?;

        Ok(facts)
    }

    async fn gather_inferences_for_question(
        &self,
        _question: &ReasoningQuestion,
    ) -> Result<Vec<Inference>> {
        // TODO: Load inferences from database once schema is added
        Ok(Vec::new())
    }

    async fn reason_over_evidence(
        &self,
        question: &ReasoningQuestion,
        facts: &[crate::agents::FactRecord],
        inferences: &[Inference],
    ) -> Result<ReasoningAnswer> {
        // Build prompt with facts and inferences
        let prompt = self.build_reasoning_prompt(question, facts, inferences);

        // Call analyzer
        let response = self.analyzer.complete(&prompt).await?;

        // Parse response into ReasoningAnswer
        let answer =
            self.parse_reasoning_response(&question.question, &response, facts, inferences);

        Ok(answer)
    }

    fn build_reasoning_prompt(
        &self,
        question: &ReasoningQuestion,
        facts: &[crate::agents::FactRecord],
        inferences: &[Inference],
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str("You are reasoning over a user's memory system to answer a question.\n\n");

        // Add facts
        if !facts.is_empty() {
            prompt.push_str("## Known Facts\n");
            for fact in facts.iter().take(self.config.max_facts_for_question) {
                prompt.push_str(&format!("- {}: {}\n", fact.fact_key, fact.fact_value));
            }
            prompt.push('\n');
        }

        // Add inferences
        if !inferences.is_empty() {
            prompt.push_str("## Previous Inferences\n");
            for inf in inferences
                .iter()
                .take(self.config.max_inferences_for_question)
            {
                prompt.push_str(&format!(
                    "- [{}] {}\n",
                    inf.inference_type.as_str(),
                    inf.conclusion
                ));
            }
            prompt.push('\n');
        }

        // Add context if provided
        if let Some(ctx) = &question.context {
            prompt.push_str(&format!("## Current Context\n{ctx}\n\n"));
        }

        // Add question
        prompt.push_str(&format!("## Question\n{}\n\n", question.question));

        // Instructions
        prompt.push_str(
            r#"## Instructions
Based on the facts and inferences above, answer the question.

Your response MUST be valid JSON with this structure:
{
  "answer": "Your synthesized answer",
  "reasoning": "Step-by-step explanation of how you reached this conclusion",
  "certainty": "Statement about confidence level (e.g., 'Based on 5 consistent observations' or 'Tentative - limited evidence')",
  "supporting_facts": ["fact_key_1", "fact_key_2"]
}

If you cannot answer based on available evidence, say so clearly and explain what information would be needed.
"#,
        );

        prompt
    }

    fn parse_reasoning_response(
        &self,
        question: &str,
        response: &str,
        facts: &[crate::agents::FactRecord],
        _inferences: &[Inference],
    ) -> ReasoningAnswer {
        // Try to parse JSON response
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(response) {
            let answer = json
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or(response);
            let reasoning = json
                .get("reasoning")
                .and_then(|v| v.as_str())
                .unwrap_or("No reasoning trace provided");
            let certainty = json
                .get("certainty")
                .and_then(|v| v.as_str())
                .unwrap_or("Certainty not specified");
            let supporting = json
                .get("supporting_facts")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .filter_map(|key| facts.iter().find(|f| f.fact_key == key))
                        .map(|f| f.id)
                        .collect()
                })
                .unwrap_or_default();

            return ReasoningAnswer::new(question, answer, reasoning, certainty)
                .with_facts(supporting);
        }

        // Fallback: use raw response
        ReasoningAnswer::new(
            question,
            response,
            "Response was not structured - using raw output",
            "Unable to determine certainty from unstructured response",
        )
    }

    // ─────────────────────────────────────────────────────────────
    // Background reasoning loop
    // ─────────────────────────────────────────────────────────────

    /// Check if background reasoning should run
    pub fn should_run_background_pass(&self) -> bool {
        if !self.config.enabled || self.config.background_interval_seconds == 0 {
            return false;
        }

        // Check if pass is already in progress
        if self
            .background_state
            .pass_in_progress
            .load(Ordering::SeqCst)
        {
            return false;
        }

        // Check time since last pass
        let last = self
            .background_state
            .last_pass_timestamp
            .load(Ordering::SeqCst);
        let now = AnswerCache::now_secs();
        let elapsed = now.saturating_sub(last);

        elapsed >= self.config.background_interval_seconds
    }

    /// Run a background reasoning pass
    ///
    /// This method:
    /// 1. Checks if enough new facts exist since last pass
    /// 2. Groups related facts
    /// 3. Derives inductive and abductive inferences
    /// 4. Detects contradictions (if enabled)
    /// 5. Stores new inferences
    pub async fn run_background_pass(&self) -> Result<BackgroundPassResult> {
        if self
            .background_state
            .pass_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(BackgroundPassResult::skipped("Pass already in progress"));
        }

        let result = self.do_background_pass().await;

        // Always mark pass as complete
        self.background_state
            .pass_in_progress
            .store(false, Ordering::SeqCst);
        self.background_state
            .last_pass_timestamp
            .store(AnswerCache::now_secs(), Ordering::SeqCst);

        result
    }

    async fn do_background_pass(&self) -> Result<BackgroundPassResult> {
        // Get current fact count
        let facts = crate::database::operations::list_recent_facts(
            &self.pool, 1000, // Get recent facts for analysis
        )
        .await?;

        let current_count = facts.len() as u64;
        let last_count = self
            .background_state
            .facts_at_last_pass
            .load(Ordering::SeqCst);

        // Check if enough new facts
        let new_facts = current_count.saturating_sub(last_count) as usize;
        if new_facts < self.config.min_new_facts_for_pass {
            return Ok(BackgroundPassResult::skipped(&format!(
                "Not enough new facts ({new_facts} < {})",
                self.config.min_new_facts_for_pass
            )));
        }

        // Update fact count
        self.background_state
            .facts_at_last_pass
            .store(current_count, Ordering::SeqCst);

        // Derive inferences
        let inferences = self.derive_inferences(&facts).await?;

        // Detect contradictions (if enabled)
        let contradictions = if self.config.detect_contradictions {
            self.detect_contradictions(&facts).await?
        } else {
            Vec::new()
        };

        // TODO: Store inferences in database once schema is added

        Ok(BackgroundPassResult {
            skipped: false,
            skip_reason: None,
            facts_processed: facts.len(),
            inferences_derived: inferences.len(),
            contradictions_found: contradictions.len(),
            inferences,
            contradictions,
        })
    }

    async fn derive_inferences(
        &self,
        facts: &[crate::agents::FactRecord],
    ) -> Result<Vec<Inference>> {
        if facts.is_empty() {
            return Ok(Vec::new());
        }

        // Build prompt for inference derivation
        let prompt = self.build_inference_derivation_prompt(facts);

        // Call analyzer
        let response = self.analyzer.complete(&prompt).await?;

        // Parse inferences from response
        let inferences = self.parse_inference_response(&response, facts);

        Ok(inferences)
    }

    fn build_inference_derivation_prompt(&self, facts: &[crate::agents::FactRecord]) -> String {
        let mut prompt = String::new();

        prompt.push_str(
            r#"You are analyzing a set of facts about a user to derive higher-order insights.

## Types of Inferences
1. **Induced** - Generalizations from patterns (e.g., "User consistently uses X, Y, Z → User prefers this category")
2. **Abduced** - Best explanations for behaviors (e.g., "User does X → The best explanation is Y")

## Facts
"#,
        );

        for fact in facts.iter().take(50) {
            prompt.push_str(&format!("- {}: {}\n", fact.fact_key, fact.fact_value));
        }

        prompt.push_str(&format!(
            r#"
## Instructions
Derive up to {} insightful inferences from these facts.
Focus on:
- Patterns that reveal preferences or values
- Explanations for why the user behaves certain ways
- Generalizations that would help predict future behavior

Your response MUST be valid JSON array:
[
  {{
    "type": "induced" or "abduced",
    "conclusion": "The inference statement",
    "reasoning": "How you derived this from the facts",
    "supporting_facts": ["fact_key_1", "fact_key_2"],
    "certainty": "Confidence statement"
  }}
]

Only include inferences you're reasonably confident about. Quality over quantity.
"#,
            self.config.max_inferences_per_pass
        ));

        prompt
    }

    fn parse_inference_response(
        &self,
        response: &str,
        facts: &[crate::agents::FactRecord],
    ) -> Vec<Inference> {
        let mut inferences = Vec::new();

        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(response) {
            for item in arr.into_iter().take(self.config.max_inferences_per_pass) {
                let inf_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("induced");
                let conclusion = item
                    .get("conclusion")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let reasoning = item
                    .get("reasoning")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let certainty = item.get("certainty").and_then(|v| v.as_str());
                let supporting = item
                    .get("supporting_facts")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .filter_map(|key| facts.iter().find(|f| f.fact_key == key))
                            .map(|f| f.id)
                            .collect()
                    })
                    .unwrap_or_default();

                if conclusion.is_empty() {
                    continue;
                }

                let mut inf = match inf_type {
                    "abduced" => Inference::abduced(
                        conclusion.to_string(),
                        supporting,
                        reasoning.to_string(),
                    ),
                    _ => Inference::induced(
                        conclusion.to_string(),
                        supporting,
                        reasoning.to_string(),
                    ),
                };

                if let Some(cert) = certainty {
                    inf = inf.with_certainty(cert);
                }

                inferences.push(inf);
            }
        }

        inferences
    }

    async fn detect_contradictions(
        &self,
        facts: &[crate::agents::FactRecord],
    ) -> Result<Vec<Contradiction>> {
        if facts.len() < 2 {
            return Ok(Vec::new());
        }

        // Build prompt for contradiction detection
        let prompt = self.build_contradiction_detection_prompt(facts);

        // Call analyzer
        let response = self.analyzer.complete(&prompt).await?;

        // Parse contradictions
        let contradictions = self.parse_contradiction_response(&response, facts);

        Ok(contradictions)
    }

    fn build_contradiction_detection_prompt(&self, facts: &[crate::agents::FactRecord]) -> String {
        let mut prompt = String::new();

        prompt.push_str(
            r#"You are analyzing facts about a user to detect any contradictions.

## Facts
"#,
        );

        for (i, fact) in facts.iter().enumerate().take(50) {
            prompt.push_str(&format!(
                "{}. {}: {}\n",
                i + 1,
                fact.fact_key,
                fact.fact_value
            ));
        }

        prompt.push_str(
            r#"
## Instructions
Identify any pairs of facts that contradict each other.
A contradiction is when two facts cannot both be true simultaneously.

Note: Some apparent contradictions may actually be:
- Context-dependent (true in different situations)
- Temporal (preference changed over time)
- Different aspects of the same thing

Your response MUST be valid JSON array (empty array if no contradictions):
[
  {
    "fact_a_index": 1,
    "fact_b_index": 5,
    "explanation": "Why these contradict",
    "resolution_suggestion": "How to resolve (temporal, contextual, or one is incorrect)"
  }
]
"#,
        );

        prompt
    }

    fn parse_contradiction_response(
        &self,
        response: &str,
        facts: &[crate::agents::FactRecord],
    ) -> Vec<Contradiction> {
        let mut contradictions = Vec::new();

        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(response) {
            for item in arr {
                let idx_a = item
                    .get("fact_a_index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize);
                let idx_b = item
                    .get("fact_b_index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize);
                let explanation = item
                    .get("explanation")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let resolution = item
                    .get("resolution_suggestion")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let (Some(a), Some(b)) = (idx_a, idx_b) {
                    // Convert 1-based index to 0-based
                    let a = a.saturating_sub(1);
                    let b = b.saturating_sub(1);

                    if a < facts.len() && b < facts.len() {
                        contradictions.push(Contradiction {
                            fact_a_id: facts[a].id,
                            fact_b_id: facts[b].id,
                            explanation,
                            resolution_suggestion: resolution,
                        });
                    }
                }
            }
        }

        contradictions
    }
}

/// Result of a background reasoning pass
#[derive(Debug, Clone)]
pub struct BackgroundPassResult {
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub facts_processed: usize,
    pub inferences_derived: usize,
    pub contradictions_found: usize,
    pub inferences: Vec<Inference>,
    pub contradictions: Vec<Contradiction>,
}

impl BackgroundPassResult {
    fn skipped(reason: &str) -> Self {
        Self {
            skipped: true,
            skip_reason: Some(reason.to_string()),
            facts_processed: 0,
            inferences_derived: 0,
            contradictions_found: 0,
            inferences: Vec::new(),
            contradictions: Vec::new(),
        }
    }
}

/// A detected contradiction between two facts
#[derive(Debug, Clone)]
pub struct Contradiction {
    pub fact_a_id: uuid::Uuid,
    pub fact_b_id: uuid::Uuid,
    pub explanation: String,
    pub resolution_suggestion: Option<String>,
}
