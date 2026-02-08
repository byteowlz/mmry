//! Question types for reasoning-based memory access
//!
//! Instead of keyword search, agents ask questions like:
//! - "What would the user prefer in this situation?"
//! - "Have we encountered this problem before?"
//! - "What constraints should I be aware of?"

use serde::Deserialize;
use serde::Serialize;

/// Categories of reasoning questions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum QuestionCategory {
    /// Questions about user preferences and values
    /// "What would the user prefer?"
    Preference,

    /// Questions about past experiences and solutions
    /// "Have we seen this before? How did we handle it?"
    Experience,

    /// Questions about constraints and requirements
    /// "What constraints apply here?"
    Constraint,

    /// Questions about identity and characteristics
    /// "What kind of X is the user?"
    Identity,

    /// Questions about predictions and expectations
    /// "What is likely to happen if...?"
    Prediction,

    /// Questions about explanations
    /// "Why does the user do X?"
    Explanation,

    /// General reasoning questions
    #[default]
    General,
}

impl QuestionCategory {
    /// Detect question category from natural language
    pub fn detect(question: &str) -> Self {
        let q = question.to_lowercase();

        // Prediction indicators - check early because "likely" shouldn't match "like"
        if q.contains("likely")
            || q.contains("probably")
            || q.contains("expect")
            || q.contains("predict")
            || q.contains("would happen")
            || q.contains("will happen")
        {
            return Self::Prediction;
        }

        // Preference indicators - use word boundaries to avoid "like" matching "likely"
        if q.contains("prefer")
            || q.contains(" like ")
            || q.contains(" like?")
            || q.contains("want")
            || q.contains("rather")
            || q.contains("favorite")
            || q.contains("choice")
        {
            return Self::Preference;
        }

        // Experience indicators
        if q.contains("before")
            || q.contains("past")
            || q.contains("previously")
            || q.contains("encountered")
            || q.contains("solved")
            || q.contains("handled")
            || q.contains("experience")
        {
            return Self::Experience;
        }

        // Constraint indicators
        if q.contains("constraint")
            || q.contains("requirement")
            || q.contains("must")
            || q.contains("should not")
            || q.contains("avoid")
            || q.contains("limit")
            || q.contains("restriction")
        {
            return Self::Constraint;
        }

        // Identity indicators
        if q.contains("what kind of")
            || q.contains("what type of")
            || q.contains("who is")
            || q.contains("characteristic")
            || q.contains("personality")
        {
            return Self::Identity;
        }

        // Explanation indicators
        if q.contains("why")
            || q.contains("reason")
            || q.contains("because")
            || q.contains("explain")
            || q.contains("motivation")
        {
            return Self::Explanation;
        }

        Self::General
    }

    /// Get example questions for this category
    pub fn examples(&self) -> &'static [&'static str] {
        match self {
            Self::Preference => &[
                "What would the user prefer in this situation?",
                "Does the user like X or Y better?",
                "What's the user's preferred approach to X?",
            ],
            Self::Experience => &[
                "Have we encountered this problem before?",
                "How did we solve similar issues in the past?",
                "What experience do we have with X?",
            ],
            Self::Constraint => &[
                "What constraints should I be aware of?",
                "Are there any requirements I should follow?",
                "What should I avoid doing?",
            ],
            Self::Identity => &[
                "What kind of developer is the user?",
                "What are the user's main characteristics?",
                "How would you describe the user's approach?",
            ],
            Self::Prediction => &[
                "What would the user likely do in this situation?",
                "How would the user probably react to X?",
                "What can we expect if we do X?",
            ],
            Self::Explanation => &[
                "Why does the user prefer X?",
                "What's the reason behind this preference?",
                "Why did the user make this choice?",
            ],
            Self::General => &[
                "What do we know about X?",
                "Tell me about the user's relationship with X.",
            ],
        }
    }
}

/// A reasoning question with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningQuestion {
    /// The question text
    pub question: String,

    /// Detected or specified category
    pub category: QuestionCategory,

    /// Optional context to consider when answering
    pub context: Option<String>,

    /// Maximum facts to consider
    pub max_facts: Option<usize>,

    /// Whether to include inferences in reasoning
    pub include_inferences: bool,
}

impl ReasoningQuestion {
    /// Create a new reasoning question
    pub fn new(question: impl Into<String>) -> Self {
        let question = question.into();
        let category = QuestionCategory::detect(&question);
        Self {
            question,
            category,
            context: None,
            max_facts: None,
            include_inferences: true,
        }
    }

    /// Create with explicit category
    pub fn with_category(question: impl Into<String>, category: QuestionCategory) -> Self {
        Self {
            question: question.into(),
            category,
            context: None,
            max_facts: None,
            include_inferences: true,
        }
    }

    /// Add context
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Set max facts
    pub fn max_facts(mut self, n: usize) -> Self {
        self.max_facts = Some(n);
        self
    }

    /// Disable inference inclusion
    pub fn facts_only(mut self) -> Self {
        self.include_inferences = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_detection() {
        assert_eq!(
            QuestionCategory::detect("What would the user prefer?"),
            QuestionCategory::Preference
        );
        assert_eq!(
            QuestionCategory::detect("Have we seen this before?"),
            QuestionCategory::Experience
        );
        assert_eq!(
            QuestionCategory::detect("What constraints apply?"),
            QuestionCategory::Constraint
        );
        assert_eq!(
            QuestionCategory::detect("What kind of developer is this?"),
            QuestionCategory::Identity
        );
        assert_eq!(
            QuestionCategory::detect("What would likely happen?"),
            QuestionCategory::Prediction
        );
        assert_eq!(
            QuestionCategory::detect("Why does the user do this?"),
            QuestionCategory::Explanation
        );
    }
}
