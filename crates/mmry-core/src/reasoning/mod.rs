//! Reasoning module - Memory as Reasoning
//!
//! This module implements reasoning-based memory access as a complement to search.
//! Instead of just retrieving facts, it derives conclusions through logical inference.
//!
//! ## Two Access Patterns
//!
//! - **Search**: "Find memories/facts matching X" (retrieval) - existing functionality
//! - **Reasoning**: "What can we conclude about X?" (inference) - this module
//!
//! ## Two Modes of Operation
//!
//! 1. **On-demand**: Agent asks a question → system reasons over facts → returns answer
//! 2. **Background**: Periodic loop derives new inferences from existing facts
//!
//! ## Full Traceability
//!
//! Everything is designed for inspection and auditability:
//! - Every inference links back to its premise facts/inferences
//! - Every reasoning answer links to supporting evidence
//! - Contradictions are tracked with resolution history
//! - All reasoning events are logged
//!
//! ## Key Concepts
//!
//! - **Inferences**: Conclusions derived through reasoning (deduced, induced, abduced)
//! - **Questions**: Natural language queries that trigger on-demand reasoning
//! - **Reasoning traces**: Natural language explanation of how conclusions were reached
//! - **Contradictions**: Detected conflicts between facts/inferences with resolution tracking

mod config;
mod inference;
pub mod operations;
mod questions;
mod schema;
mod service;

pub use config::ReasoningConfig;
pub use inference::Inference;
pub use inference::InferenceType;
pub use inference::ReasoningAnswer;
pub use operations::AnswerWithEvidence;
pub use operations::ContradictionRecord;
pub use operations::ReasoningChain;
pub use operations::ReasoningEvent;
pub use questions::QuestionCategory;
pub use questions::ReasoningQuestion;
pub use schema::init_reasoning_schema;
pub use schema::REASONING_SCHEMA;
pub use service::BackgroundPassResult;
pub use service::Contradiction;
pub use service::ReasoningService;
