//! Named Entity Recognition using GLiNER
//!
//! GLiNER is a zero-shot NER model that can extract any entity type you specify.
//! This module provides a service wrapper around the gline-rs inference engine.

#[cfg(feature = "ner")]
compile_error!("NER support is temporarily disabled (gline-rs removed). Use default build without the `ner` feature.");

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::config::NerConfig;
use crate::Result;

#[cfg(feature = "ner")]
use std::env;
#[cfg(feature = "ner")]
use std::fs;
#[cfg(feature = "ner")]
use std::path::PathBuf;
#[cfg(feature = "ner")]
use std::sync::Arc;

#[cfg(feature = "ner")]
use gline_rs::GLiNER;
#[cfg(feature = "ner")]
use gline_rs::Parameters;
#[cfg(feature = "ner")]
use gline_rs::RuntimeParameters;
#[cfg(feature = "ner")]
use gline_rs::SpanMode;
#[cfg(feature = "ner")]
use gline_rs::TextInput;
#[cfg(feature = "ner")]
use once_cell::sync::OnceCell;
#[cfg(feature = "ner")]
use tokio::sync::Mutex;

#[cfg(feature = "ner")]
use crate::Error;

/// Default GLiNER model (multilingual, Apache 2.0)
#[cfg(feature = "ner")]
const DEFAULT_GLINER_MODEL: &str = "urchade/gliner_small-v2.1";

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

#[cfg(feature = "ner")]
type SharedModel = Arc<Mutex<GLiNER<SpanMode>>>;

/// Named Entity Recognition service using GLiNER
pub struct NerService {
    enabled: bool,
    #[cfg(feature = "ner")]
    model_name: String,
    #[cfg(feature = "ner")]
    confidence_threshold: f32,
    #[cfg(feature = "ner")]
    model: OnceCell<SharedModel>,
    #[cfg(feature = "ner")]
    entity_labels: Vec<String>,
}

impl NerService {
    pub fn new(config: &NerConfig) -> Result<Self> {
        #[cfg(not(feature = "ner"))]
        {
            if config.enabled {
                tracing::warn!("NER is enabled in config but the 'ner' feature is not compiled in");
            }
            Ok(Self { enabled: false })
        }

        #[cfg(feature = "ner")]
        {
            if !config.enabled {
                return Ok(Self {
                    enabled: false,
                    model_name: String::new(),
                    confidence_threshold: 0.5,
                    model: OnceCell::new(),
                    entity_labels: Vec::new(),
                });
            }

            Ok(Self {
                enabled: true,
                model_name: config.model.clone(),
                confidence_threshold: config.confidence_threshold,
                model: OnceCell::new(),
                entity_labels: config.labels.clone(),
            })
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(feature = "ner")]
    fn get_model_cache_dir(&self) -> Result<PathBuf> {
        let base = env::var("XDG_CACHE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::cache_dir)
            .unwrap_or_else(|| PathBuf::from("."));

        let cache_dir = base.join("mmry").join("gliner");
        fs::create_dir_all(&cache_dir)?;
        Ok(cache_dir)
    }

    #[cfg(feature = "ner")]
    fn model_repo_to_dir_name(repo: &str) -> String {
        repo.replace('/', "--")
    }

    #[cfg(feature = "ner")]
    fn ensure_model(&self) -> Result<SharedModel> {
        if !self.enabled {
            return Err(Error::Ner("NER service disabled".into()));
        }

        let model_ref = self.model.get_or_try_init(|| -> Result<SharedModel> {
            let cache_dir = self.get_model_cache_dir()?;
            let model_name = if self.model_name.is_empty() {
                DEFAULT_GLINER_MODEL.to_string()
            } else {
                self.model_name.clone()
            };

            let model_dir = cache_dir.join(Self::model_repo_to_dir_name(&model_name));
            let tokenizer_path = model_dir.join("tokenizer.json");
            let model_path = model_dir.join("onnx").join("model.onnx");

            // Check if model is already downloaded
            if !model_path.exists() || !tokenizer_path.exists() {
                tracing::info!(model = %model_name, "Downloading GLiNER model from HuggingFace");
                Self::download_model(&model_name, &model_dir)?;
            }

            tracing::info!(model = %model_name, "Loading GLiNER model");

            let model = GLiNER::<SpanMode>::new(
                Parameters::default(),
                RuntimeParameters::default(),
                &tokenizer_path,
                &model_path,
            )
            .map_err(|e| Error::Ner(format!("Failed to load GLiNER model: {e}")))?;

            Ok(Arc::new(Mutex::new(model)))
        })?;

        Ok(Arc::clone(model_ref))
    }

    #[cfg(feature = "ner")]
    fn download_model(model_name: &str, model_dir: &PathBuf) -> Result<()> {
        // Try ONNX community version first (pre-converted), then fall back to original
        let onnx_model_name = if model_name.starts_with("onnx-community/") {
            model_name.to_string()
        } else {
            format!(
                "onnx-community/{}",
                model_name.split('/').last().unwrap_or(model_name)
            )
        };

        let base_url = format!("https://huggingface.co/{onnx_model_name}/resolve/main");

        let files_to_download = [
            ("onnx/model.onnx", "onnx/model.onnx"),
            ("tokenizer.json", "tokenizer.json"),
            ("config.json", "config.json"),
        ];

        for (remote_path, local_path) in files_to_download {
            let url = format!("{base_url}/{remote_path}");
            let local_file = model_dir.join(local_path);

            if let Some(parent) = local_file.parent() {
                fs::create_dir_all(parent)?;
            }

            tracing::debug!(url = %url, "Downloading file");

            let response = ureq::get(&url)
                .call()
                .map_err(|e| Error::Ner(format!("Failed to download {remote_path}: {e}")))?;

            let mut file = fs::File::create(&local_file)?;
            let mut reader = response.into_reader();
            std::io::copy(&mut reader, &mut file)?;

            tracing::debug!(path = ?local_file, "Downloaded file");
        }

        Ok(())
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
        #[cfg(not(feature = "ner"))]
        {
            let _ = text;
            let _ = labels;
            Ok(Vec::new())
        }

        #[cfg(feature = "ner")]
        {
            if !self.enabled {
                return Ok(Vec::new());
            }

            // Use provided labels or fall back to config labels
            let labels: Vec<&str> = if let Some(l) = labels {
                l.to_vec()
            } else if !self.entity_labels.is_empty() {
                self.entity_labels.iter().map(|s| s.as_str()).collect()
            } else {
                // Default labels if none configured
                vec!["person", "location", "organization", "date", "event"]
            };

            if labels.is_empty() {
                return Ok(Vec::new());
            }

            let model = self.ensure_model()?;
            let guard = model.lock().await;

            let input = TextInput::from_str(&[text], &labels)
                .map_err(|e| Error::Ner(format!("Failed to create input: {e}")))?;

            let output = guard
                .inference(input)
                .map_err(|e| Error::Ner(format!("Inference failed: {e}")))?;

            let mut entities = Vec::new();

            // Process results from first (and only) text
            if let Some(text_entities) = output.get(0) {
                for entity in text_entities {
                    if entity.score >= self.confidence_threshold {
                        entities.push(RecognizedEntity {
                            text: entity.text.clone(),
                            label: entity.label.clone(),
                            confidence: entity.score,
                            start: entity.start,
                            end: entity.end,
                        });
                    }
                }
            }

            Ok(entities)
        }
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
