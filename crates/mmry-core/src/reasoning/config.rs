//! Configuration for the reasoning module

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Configuration for reasoning-based memory access
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ReasoningConfig {
    /// Enable reasoning module (disabled by default)
    pub enabled: bool,

    /// Use same model as analyzer, or specify a different one
    /// If None, uses analyzer.model
    pub model: Option<String>,

    /// Use same endpoint as analyzer, or specify a different one
    /// If None, uses analyzer.endpoint
    pub endpoint: Option<String>,

    // ─────────────────────────────────────────────────────────────
    // Background reasoning loop
    // ─────────────────────────────────────────────────────────────
    /// Interval in seconds for background reasoning loop (0 = disabled)
    /// The loop periodically derives new inferences from existing facts
    pub background_interval_seconds: u64,

    /// Maximum inferences to derive per background reasoning pass
    pub max_inferences_per_pass: usize,

    /// Minimum new facts since last pass to trigger reasoning
    /// Avoids re-processing when nothing has changed
    pub min_new_facts_for_pass: usize,

    /// Run background reasoning only when service is idle
    pub background_only_when_idle: bool,

    // ─────────────────────────────────────────────────────────────
    // Inference quality
    // ─────────────────────────────────────────────────────────────
    /// Confidence threshold for storing derived inferences (0.0-1.0)
    /// Only inferences with reasoning quality above this are persisted
    pub min_confidence_threshold: f32,

    /// Maximum premises to include when deriving new inferences
    pub max_premises: usize,

    // ─────────────────────────────────────────────────────────────
    // Contradiction handling
    // ─────────────────────────────────────────────────────────────
    /// Enable contradiction detection during background passes
    pub detect_contradictions: bool,

    /// Automatically resolve contradictions (vs just flagging them)
    pub auto_resolve_contradictions: bool,

    // ─────────────────────────────────────────────────────────────
    // On-demand reasoning (questions)
    // ─────────────────────────────────────────────────────────────
    /// Maximum facts to consider when answering a question
    pub max_facts_for_question: usize,

    /// Maximum inferences to consider when answering a question
    pub max_inferences_for_question: usize,

    /// Cache reasoning answers for repeated questions (seconds, 0 = no cache)
    pub answer_cache_seconds: u64,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            endpoint: None,
            // Background loop - default 1 hour, 0 to disable
            background_interval_seconds: 3600,
            max_inferences_per_pass: 10,
            min_new_facts_for_pass: 5,
            background_only_when_idle: true,
            // Quality
            min_confidence_threshold: 0.7,
            max_premises: 5,
            // Contradictions
            detect_contradictions: true,
            auto_resolve_contradictions: false,
            // On-demand
            max_facts_for_question: 50,
            max_inferences_for_question: 20,
            answer_cache_seconds: 300, // 5 minutes
        }
    }
}
