//! Inference types and structures
//!
//! An inference is a fact derived through reasoning, as opposed to
//! an observed fact extracted directly from content.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// Type of inference - how was this conclusion reached?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum InferenceType {
    /// Directly observed/extracted from content
    #[default]
    Observed,
    /// Deduced - certain conclusion from explicit premises
    /// "If A and B, then C" - C is certain given A and B
    Deduced,
    /// Induced - generalization from patterns
    /// "A1, A2, A3 all have property P, therefore As generally have P"
    Induced,
    /// Abduced - best explanation for observed behavior
    /// "The best explanation for X is Y"
    Abduced,
}

impl InferenceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Deduced => "deduced",
            Self::Induced => "induced",
            Self::Abduced => "abduced",
        }
    }
}

impl std::fmt::Display for InferenceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for InferenceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "observed" => Ok(Self::Observed),
            "deduced" => Ok(Self::Deduced),
            "induced" => Ok(Self::Induced),
            "abduced" => Ok(Self::Abduced),
            _ => Err(format!("Unknown inference type: {s}")),
        }
    }
}

/// An inference - a conclusion derived through reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inference {
    /// Unique identifier
    pub id: Uuid,

    /// The conclusion (what was inferred)
    pub conclusion: String,

    /// How this inference was derived
    pub inference_type: InferenceType,

    /// IDs of facts/inferences used as premises
    pub premise_ids: Vec<Uuid>,

    /// The reasoning trace - natural language explanation of how
    /// the conclusion was reached from the premises
    pub reasoning_trace: String,

    /// Natural language statement of confidence/certainty
    /// e.g., "Based on 3 consistent observations over 2 months"
    pub certainty_statement: Option<String>,

    /// When this inference was created
    pub created_at: DateTime<Utc>,

    /// Optional category for organization
    pub category: Option<String>,

    /// Whether this inference has been superseded by a newer one
    pub superseded: bool,

    /// If superseded, by which inference
    pub superseded_by: Option<Uuid>,
}

impl Inference {
    /// Create a new deduced inference
    pub fn deduced(conclusion: String, premise_ids: Vec<Uuid>, reasoning_trace: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            conclusion,
            inference_type: InferenceType::Deduced,
            premise_ids,
            reasoning_trace,
            certainty_statement: None,
            created_at: Utc::now(),
            category: None,
            superseded: false,
            superseded_by: None,
        }
    }

    /// Create a new induced inference (generalization)
    pub fn induced(conclusion: String, premise_ids: Vec<Uuid>, reasoning_trace: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            conclusion,
            inference_type: InferenceType::Induced,
            premise_ids,
            reasoning_trace,
            certainty_statement: None,
            created_at: Utc::now(),
            category: None,
            superseded: false,
            superseded_by: None,
        }
    }

    /// Create a new abduced inference (best explanation)
    pub fn abduced(conclusion: String, premise_ids: Vec<Uuid>, reasoning_trace: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            conclusion,
            inference_type: InferenceType::Abduced,
            premise_ids,
            reasoning_trace,
            certainty_statement: None,
            created_at: Utc::now(),
            category: None,
            superseded: false,
            superseded_by: None,
        }
    }

    /// Add a certainty statement
    pub fn with_certainty(mut self, statement: impl Into<String>) -> Self {
        self.certainty_statement = Some(statement.into());
        self
    }

    /// Add a category
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// Result of asking a reasoning question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningAnswer {
    /// The question that was asked
    pub question: String,

    /// The synthesized answer
    pub answer: String,

    /// Facts used to derive the answer
    pub supporting_facts: Vec<Uuid>,

    /// Inferences used to derive the answer
    pub supporting_inferences: Vec<Uuid>,

    /// The reasoning trace explaining how the answer was derived
    pub reasoning_trace: String,

    /// Confidence/certainty statement
    pub certainty_statement: String,

    /// When this answer was generated
    pub generated_at: DateTime<Utc>,
}

impl ReasoningAnswer {
    pub fn new(
        question: impl Into<String>,
        answer: impl Into<String>,
        reasoning_trace: impl Into<String>,
        certainty_statement: impl Into<String>,
    ) -> Self {
        Self {
            question: question.into(),
            answer: answer.into(),
            supporting_facts: Vec::new(),
            supporting_inferences: Vec::new(),
            reasoning_trace: reasoning_trace.into(),
            certainty_statement: certainty_statement.into(),
            generated_at: Utc::now(),
        }
    }

    pub fn with_facts(mut self, fact_ids: Vec<Uuid>) -> Self {
        self.supporting_facts = fact_ids;
        self
    }

    pub fn with_inferences(mut self, inference_ids: Vec<Uuid>) -> Self {
        self.supporting_inferences = inference_ids;
        self
    }
}
