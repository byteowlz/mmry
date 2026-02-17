// Learnings: distilled rules and insights extracted from agent sessions.
//
// Replaces bridge blocks as the procedural memory layer. Each learning is
// an actionable principle (guiding or cautionary) with confidence tracking,
// maturity lifecycle, and agent provenance.
//
// Design influenced by:
// - EvolveR: dual extraction (guiding + cautionary principles), semantic dedup
// - ACE/Copilot: deterministic curation (no LLM in merge step)
// - cass-memory: confidence decay, maturity transitions, feedback events

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

// ── Scoring constants ────────────────────────────────────────────────

/// Default half-life for feedback decay (days).
pub const DEFAULT_DECAY_HALF_LIFE_DAYS: f64 = 90.0;

/// Harmful feedback counts this many times more than helpful.
pub const DEFAULT_HARMFUL_MULTIPLIER: f64 = 4.0;

/// Semantic similarity threshold for deduplication.
pub const DEDUP_SIMILARITY_THRESHOLD: f32 = 0.85;

// ── Enums ────────────────────────────────────────────────────────────

/// Whether a learning is a positive guiding principle or a cautionary warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LearningKind {
    /// "Always X when Y" — a guiding principle extracted from successes.
    #[default]
    Guiding,
    /// "PITFALL: Don't X" — a cautionary principle extracted from failures.
    Cautionary,
}

impl LearningKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Guiding => "guiding",
            Self::Cautionary => "cautionary",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cautionary" | "anti_pattern" | "pitfall" => Self::Cautionary,
            _ => Self::Guiding,
        }
    }
}

/// Scope at which a learning applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LearningScope {
    /// Applies everywhere.
    #[default]
    Global,
    /// Applies to a specific workspace / repo.
    Workspace,
    /// Applies to a programming language.
    Language,
    /// Applies to a framework or library.
    Framework,
    /// Applies to a specific task type.
    Task,
}

impl LearningScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Language => "language",
            Self::Framework => "framework",
            Self::Task => "task",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "workspace" => Self::Workspace,
            "language" => Self::Language,
            "framework" => Self::Framework,
            "task" => Self::Task,
            _ => Self::Global,
        }
    }
}

/// Lifecycle maturity of a learning.  Transitions are deterministic
/// (no LLM call) based on feedback counts and ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    /// Newly extracted, not yet validated.
    #[default]
    Candidate,
    /// ≥3 helpful, <25% harmful.
    Established,
    /// ≥10 helpful, <10% harmful.
    Proven,
    /// >25% harmful ratio (with ≥3 total feedback).
    Deprecated,
}

impl Maturity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Established => "established",
            Self::Proven => "proven",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "established" => Self::Established,
            "proven" => Self::Proven,
            "deprecated" => Self::Deprecated,
            _ => Self::Candidate,
        }
    }
}

// ── Core data types ──────────────────────────────────────────────────

/// A distilled rule or insight extracted from agent sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Learning {
    pub id: Uuid,
    /// The actionable principle text.
    pub content: String,
    /// Guiding (do this) or cautionary (don't do this).
    pub kind: LearningKind,
    /// Category for gap analysis (e.g. "debugging", "testing", "security").
    pub category: String,
    /// Scope of applicability.
    pub scope: LearningScope,
    /// Qualifier within scope (e.g. workspace path, language name).
    pub scope_key: Option<String>,

    // ── Lifecycle ────────────────────────────────────────────────
    pub maturity: Maturity,
    /// True if manually pinned (maturity won't auto-transition).
    pub pinned: bool,

    // ── Feedback counters ────────────────────────────────────────
    pub helpful_count: i32,
    pub harmful_count: i32,
    /// Time-decayed effective score (recomputed on read).
    pub effective_score: f64,

    // ── Provenance ───────────────────────────────────────────────
    /// Agent that extracted this learning.
    pub agent_id: Option<Uuid>,
    /// Source session paths that produced this learning.
    pub source_sessions: Vec<String>,
    /// Human-readable reasoning for why this learning exists.
    pub reasoning: Option<String>,

    // ── Metadata ─────────────────────────────────────────────────
    pub tags: Vec<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Learning {
    pub fn new(
        content: impl Into<String>,
        kind: LearningKind,
        category: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            content: content.into(),
            kind,
            category: category.into(),
            scope: LearningScope::default(),
            scope_key: None,
            maturity: Maturity::default(),
            pinned: false,
            helpful_count: 0,
            harmful_count: 0,
            effective_score: 0.0,
            agent_id: None,
            source_sessions: Vec::new(),
            reasoning: None,
            tags: Vec::new(),
            metadata: Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }
}

/// A single feedback event on a learning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedbackEvent {
    pub id: Uuid,
    pub learning_id: Uuid,
    pub feedback_type: FeedbackType,
    pub timestamp: DateTime<Utc>,
    pub session_path: Option<String>,
    pub reason: Option<String>,
    pub agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackType {
    Helpful,
    Harmful,
}

impl FeedbackType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Helpful => "helpful",
            Self::Harmful => "harmful",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "harmful" => Self::Harmful,
            _ => Self::Helpful,
        }
    }
}

impl FeedbackEvent {
    pub fn new(learning_id: Uuid, feedback_type: FeedbackType) -> Self {
        Self {
            id: Uuid::new_v4(),
            learning_id,
            feedback_type,
            timestamp: Utc::now(),
            session_path: None,
            reason: None,
            agent_id: None,
        }
    }
}

// ── Scoring ──────────────────────────────────────────────────────────

/// Configuration for time-decayed scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub decay_half_life_days: f64,
    pub harmful_multiplier: f64,
    pub min_feedback_for_established: i32,
    pub min_helpful_for_proven: i32,
    pub max_harmful_ratio_for_proven: f64,
    pub max_harmful_ratio_for_established: f64,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            decay_half_life_days: DEFAULT_DECAY_HALF_LIFE_DAYS,
            harmful_multiplier: DEFAULT_HARMFUL_MULTIPLIER,
            min_feedback_for_established: 3,
            min_helpful_for_proven: 10,
            max_harmful_ratio_for_proven: 0.10,
            max_harmful_ratio_for_established: 0.25,
        }
    }
}

/// Compute the time-decayed effective score from feedback events.
pub fn compute_effective_score(events: &[FeedbackEvent], config: &ScoringConfig) -> f64 {
    let now = Utc::now();
    let half_life_ms = config.decay_half_life_days * 24.0 * 3600.0 * 1000.0;

    let mut decayed_helpful = 0.0_f64;
    let mut decayed_harmful = 0.0_f64;

    for event in events {
        let age_ms = now
            .signed_duration_since(event.timestamp)
            .num_milliseconds() as f64;
        let decay = 0.5_f64.powf(age_ms / half_life_ms);

        match event.feedback_type {
            FeedbackType::Helpful => decayed_helpful += decay,
            FeedbackType::Harmful => decayed_harmful += decay,
        }
    }

    decayed_helpful - (config.harmful_multiplier * decayed_harmful)
}

/// Deterministically compute the maturity state from feedback events.
pub fn compute_maturity(
    events: &[FeedbackEvent],
    config: &ScoringConfig,
    pinned: bool,
    current: Maturity,
) -> Maturity {
    if pinned {
        return current;
    }

    let now = Utc::now();
    let half_life_ms = config.decay_half_life_days * 24.0 * 3600.0 * 1000.0;

    let mut decayed_helpful = 0.0_f64;
    let mut decayed_harmful = 0.0_f64;

    for event in events {
        let age_ms = now
            .signed_duration_since(event.timestamp)
            .num_milliseconds() as f64;
        let decay = 0.5_f64.powf(age_ms / half_life_ms);

        match event.feedback_type {
            FeedbackType::Helpful => decayed_helpful += decay,
            FeedbackType::Harmful => decayed_harmful += decay,
        }
    }

    let total = decayed_helpful + decayed_harmful;
    if total < f64::from(config.min_feedback_for_established) - 0.01 {
        return Maturity::Candidate;
    }

    let harmful_ratio = if total > 0.0 {
        decayed_harmful / total
    } else {
        0.0
    };

    // Auto-deprecate if too harmful
    if harmful_ratio > config.max_harmful_ratio_for_established {
        return Maturity::Deprecated;
    }

    // Promote to proven
    if decayed_helpful >= f64::from(config.min_helpful_for_proven) - 0.01
        && harmful_ratio < config.max_harmful_ratio_for_proven
    {
        return Maturity::Proven;
    }

    Maturity::Established
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_kind_roundtrip() {
        assert_eq!(LearningKind::parse("guiding"), LearningKind::Guiding);
        assert_eq!(LearningKind::parse("cautionary"), LearningKind::Cautionary);
        assert_eq!(
            LearningKind::parse("anti_pattern"),
            LearningKind::Cautionary
        );
        assert_eq!(LearningKind::parse("pitfall"), LearningKind::Cautionary);
        assert_eq!(LearningKind::parse("unknown"), LearningKind::Guiding);
    }

    #[test]
    fn maturity_roundtrip() {
        assert_eq!(Maturity::parse("candidate"), Maturity::Candidate);
        assert_eq!(Maturity::parse("established"), Maturity::Established);
        assert_eq!(Maturity::parse("proven"), Maturity::Proven);
        assert_eq!(Maturity::parse("deprecated"), Maturity::Deprecated);
    }

    #[test]
    fn effective_score_decays_over_time() {
        let config = ScoringConfig::default();
        let now = Utc::now();

        let recent = FeedbackEvent {
            id: Uuid::new_v4(),
            learning_id: Uuid::new_v4(),
            feedback_type: FeedbackType::Helpful,
            timestamp: now,
            session_path: None,
            reason: None,
            agent_id: None,
        };

        let old = FeedbackEvent {
            id: Uuid::new_v4(),
            learning_id: Uuid::new_v4(),
            feedback_type: FeedbackType::Helpful,
            timestamp: now - chrono::Duration::days(180),
            session_path: None,
            reason: None,
            agent_id: None,
        };

        let score_recent = compute_effective_score(&[recent], &config);
        let score_old = compute_effective_score(&[old], &config);

        assert!(
            score_recent > score_old,
            "Recent feedback should score higher"
        );
        // After 2 half-lives (180 days with 90-day half-life), score ≈ 0.25
        assert!(score_old < 0.3);
    }

    #[test]
    fn harmful_multiplier_reduces_score() {
        let config = ScoringConfig::default();
        let now = Utc::now();
        let lid = Uuid::new_v4();

        let events = vec![
            FeedbackEvent {
                id: Uuid::new_v4(),
                learning_id: lid,
                feedback_type: FeedbackType::Helpful,
                timestamp: now,
                session_path: None,
                reason: None,
                agent_id: None,
            },
            FeedbackEvent {
                id: Uuid::new_v4(),
                learning_id: lid,
                feedback_type: FeedbackType::Harmful,
                timestamp: now,
                session_path: None,
                reason: None,
                agent_id: None,
            },
        ];

        let score = compute_effective_score(&events, &config);
        // 1 helpful - (4 * 1 harmful) = -3
        assert!(score < 0.0, "Harmful multiplier should make score negative");
    }

    #[test]
    fn maturity_transitions() {
        let config = ScoringConfig::default();
        let now = Utc::now();
        let lid = Uuid::new_v4();

        // No feedback → candidate
        assert_eq!(
            compute_maturity(&[], &config, false, Maturity::Candidate),
            Maturity::Candidate
        );

        // 3 helpful → established
        let helpful_3: Vec<FeedbackEvent> = (0..3)
            .map(|_| FeedbackEvent {
                id: Uuid::new_v4(),
                learning_id: lid,
                feedback_type: FeedbackType::Helpful,
                timestamp: now,
                session_path: None,
                reason: None,
                agent_id: None,
            })
            .collect();
        assert_eq!(
            compute_maturity(&helpful_3, &config, false, Maturity::Candidate),
            Maturity::Established
        );

        // 10 helpful → proven
        let helpful_10: Vec<FeedbackEvent> = (0..10)
            .map(|_| FeedbackEvent {
                id: Uuid::new_v4(),
                learning_id: lid,
                feedback_type: FeedbackType::Helpful,
                timestamp: now,
                session_path: None,
                reason: None,
                agent_id: None,
            })
            .collect();
        assert_eq!(
            compute_maturity(&helpful_10, &config, false, Maturity::Established),
            Maturity::Proven
        );

        // High harmful ratio → deprecated
        let mut mixed = helpful_3.clone();
        for _ in 0..3 {
            mixed.push(FeedbackEvent {
                id: Uuid::new_v4(),
                learning_id: lid,
                feedback_type: FeedbackType::Harmful,
                timestamp: now,
                session_path: None,
                reason: None,
                agent_id: None,
            });
        }
        // 3 helpful + 3 harmful = 50% harmful ratio > 25% threshold
        assert_eq!(
            compute_maturity(&mixed, &config, false, Maturity::Established),
            Maturity::Deprecated
        );
    }

    #[test]
    fn pinned_preserves_maturity() {
        let config = ScoringConfig::default();
        assert_eq!(
            compute_maturity(&[], &config, true, Maturity::Proven),
            Maturity::Proven
        );
    }
}
