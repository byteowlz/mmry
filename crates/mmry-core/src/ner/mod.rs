//! Named Entity Recognition (currently disabled)

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::config::NerConfig;
use crate::Result;

/// A recognized named entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognizedEntity {
    /// The entity text
    pub text: String,
    /// Entity type/label (user-defined, e.g., "person", "company", "technology")
    pub label: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Start character offset in original text
    pub start: usize,
    /// End character offset in original text
    pub end: usize,
}

/// NER model information
#[derive(Debug, Clone)]
pub struct NerModelInfo {
    pub code: &'static str,
    pub description: &'static str,
}

/// List available GLiNER models
pub fn list_ner_models() -> Vec<NerModelInfo> {
    vec![
        NerModelInfo {
            code: "urchade/gliner_small-v2.1",
            description: "GLiNER Small v2.1 (166M params) - fast, English",
        },
        NerModelInfo {
            code: "urchade/gliner_medium-v2.1",
            description: "GLiNER Medium v2.1 (209M params) - balanced, English",
        },
        NerModelInfo {
            code: "urchade/gliner_large-v2.1",
            description: "GLiNER Large v2.1 (459M params) - accurate, English",
        },
        NerModelInfo {
            code: "urchade/gliner_multi-v2.1",
            description: "GLiNER Multi v2.1 (209M params) - multilingual, recommended",
        },
    ]
}

/// Named Entity Recognition service (no-op)
pub struct NerService {
    enabled: bool,
}

impl NerService {
    pub fn new(config: &NerConfig) -> Result<Self> {
        if config.enabled {
            tracing::warn!("NER is enabled in config but NER support is currently disabled");
        }
        Ok(Self { enabled: false })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Extract named entities from text using specified labels
    ///
    /// # Arguments
    /// * `text` - The text to extract entities from
    /// * `labels` - Optional custom labels. If None, uses default labels from config.
    ///
    /// # Example
    /// ```ignore
    /// let entities = ner.extract("John works at Google", Some(&["person", "company"])).await?;
    /// // Returns: [("John", "person"), ("Google", "company")]
    /// ```
    pub async fn extract(
        &self,
        text: &str,
        labels: Option<&[&str]>,
    ) -> Result<Vec<RecognizedEntity>> {
        let _ = text;
        let _ = labels;
        Ok(Vec::new())
    }

    /// Extract and deduplicate entities, returning unique entity names with their labels
    pub async fn extract_unique(
        &self,
        text: &str,
        labels: Option<&[&str]>,
    ) -> Result<HashMap<String, (String, f32)>> {
        let entities = self.extract(text, labels).await?;
        let mut unique: HashMap<String, (String, f32)> = HashMap::new();

        for entity in entities {
            let normalized = entity.text.trim().to_string();
            if normalized.is_empty() {
                continue;
            }

            unique
                .entry(normalized)
                .and_modify(|(_, conf)| {
                    // Keep highest confidence
                    if entity.confidence > *conf {
                        *conf = entity.confidence;
                    }
                })
                .or_insert((entity.label, entity.confidence));
        }

        Ok(unique)
    }
}
