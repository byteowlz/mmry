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
use once_cell::sync::OnceCell;
#[cfg(feature = "ner")]
use tokenizers::Tokenizer;
#[cfg(feature = "ner")]
use tokio::sync::Mutex;

#[cfg(feature = "ner")]
use crate::Error;

/// Default HuggingFace model repository for NER
#[cfg(feature = "ner")]
const DEFAULT_NER_MODEL: &str = "onnx-community/distilbert-NER-ONNX";

/// Entity types from CoNLL-2003 dataset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EntityType {
    /// Person name
    Per,
    /// Location
    Loc,
    /// Organization
    Org,
    /// Miscellaneous
    Misc,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Per => "PER",
            Self::Loc => "LOC",
            Self::Org => "ORG",
            Self::Misc => "MISC",
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A recognized named entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognizedEntity {
    /// The entity text
    pub text: String,
    /// Entity type (PER, LOC, ORG, MISC)
    pub entity_type: EntityType,
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

pub fn list_ner_models() -> Vec<NerModelInfo> {
    vec![
        NerModelInfo {
            code: "onnx-community/distilbert-NER-ONNX",
            description: "DistilBERT NER (66M params, F1: 0.92) - recommended",
        },
        NerModelInfo {
            code: "onnx-community/TinyBERT-finetuned-NER-ONNX",
            description: "TinyBERT NER (14M params, F1: 0.86) - faster, less accurate",
        },
    ]
}

/// Label ID to entity type mapping for CoNLL-2003 NER models
#[cfg(feature = "ner")]
fn label_to_entity_type(label_id: usize) -> Option<(EntityType, bool)> {
    // Standard CoNLL-2003 label mapping:
    // 0: O (outside)
    // 1: B-MISC, 2: I-MISC
    // 3: B-PER, 4: I-PER
    // 5: B-ORG, 6: I-ORG
    // 7: B-LOC, 8: I-LOC
    match label_id {
        1 => Some((EntityType::Misc, true)),  // B-MISC
        2 => Some((EntityType::Misc, false)), // I-MISC
        3 => Some((EntityType::Per, true)),   // B-PER
        4 => Some((EntityType::Per, false)),  // I-PER
        5 => Some((EntityType::Org, true)),   // B-ORG
        6 => Some((EntityType::Org, false)),  // I-ORG
        7 => Some((EntityType::Loc, true)),   // B-LOC
        8 => Some((EntityType::Loc, false)),  // I-LOC
        _ => None,                            // O or unknown
    }
}

#[cfg(feature = "ner")]
mod onnx_backend {
    use super::*;
    use ort::session::Session;
    use ort::value::Value;

    pub struct OnnxNerModel {
        session: Session,
        tokenizer: Tokenizer,
    }

    impl OnnxNerModel {
        pub fn new(model_path: &PathBuf, tokenizer_path: &PathBuf) -> Result<Self> {
            let session = Session::builder()
                .map_err(|e| Error::Ner(format!("Failed to create ONNX session builder: {e}")))?
                .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
                .map_err(|e| Error::Ner(format!("Failed to set optimization level: {e}")))?
                .commit_from_file(model_path)
                .map_err(|e| Error::Ner(format!("Failed to load ONNX model: {e}")))?;

            let tokenizer = Tokenizer::from_file(tokenizer_path)
                .map_err(|e| Error::Ner(format!("Failed to load tokenizer: {e}")))?;

            Ok(Self { session, tokenizer })
        }

        pub fn predict(&mut self, text: &str) -> Result<Vec<RecognizedEntity>> {
            // Tokenize the input
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| Error::Ner(format!("Tokenization failed: {e}")))?;

            let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
            let attention_mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| m as i64)
                .collect();

            let seq_len = input_ids.len();

            // Create input tensors
            let input_ids_array = ndarray::Array2::from_shape_vec((1, seq_len), input_ids.clone())
                .map_err(|e| Error::Ner(format!("Failed to create input_ids array: {e}")))?;

            let attention_mask_array =
                ndarray::Array2::from_shape_vec((1, seq_len), attention_mask).map_err(|e| {
                    Error::Ner(format!("Failed to create attention_mask array: {e}"))
                })?;

            let input_ids_tensor = Value::from_array(input_ids_array)
                .map_err(|e| Error::Ner(format!("Failed to create input_ids tensor: {e}")))?;

            let attention_mask_tensor = Value::from_array(attention_mask_array)
                .map_err(|e| Error::Ner(format!("Failed to create attention_mask tensor: {e}")))?;

            // Run inference
            let inputs = ort::inputs![input_ids_tensor, attention_mask_tensor];
            let outputs = self
                .session
                .run(inputs)
                .map_err(|e| Error::Ner(format!("ONNX inference failed: {e}")))?;

            // Extract logits from output - NER models typically have "logits" as first output
            let logits = &outputs[0];

            let (shape, logits_data) = logits
                .try_extract_tensor::<f32>()
                .map_err(|e| Error::Ner(format!("Failed to extract logits: {e}")))?;

            // Shape is [batch, seq_len, num_labels] -> [1, seq_len, 9]
            let num_labels = shape[2] as usize;

            // Process predictions
            let tokens = encoding.get_tokens();
            let offsets = encoding.get_offsets();
            let mut entities: Vec<RecognizedEntity> = Vec::new();
            let mut current_entity: Option<(String, EntityType, f32, usize, usize)> = None;

            for (idx, token) in tokens.iter().enumerate() {
                // Skip special tokens
                if token == "[CLS]" || token == "[SEP]" || token == "[PAD]" {
                    // Finalize any ongoing entity
                    if let Some((text, entity_type, confidence, start, end)) = current_entity.take()
                    {
                        entities.push(RecognizedEntity {
                            text,
                            entity_type,
                            confidence,
                            start,
                            end,
                        });
                    }
                    continue;
                }

                // Get logits for this token (flat indexing: batch=0, token=idx, label=i)
                let base_idx = idx * num_labels;
                let token_logits: Vec<f32> =
                    (0..num_labels).map(|i| logits_data[base_idx + i]).collect();

                // Apply softmax and get prediction
                let (pred_label, confidence) = softmax_argmax(&token_logits);

                let (offset_start, offset_end) = offsets[idx];
                let is_wordpiece_continuation = token.starts_with("##");

                // Handle wordpiece continuation tokens - always merge with previous entity
                if is_wordpiece_continuation {
                    if let Some((ref mut text, _, ref mut conf, _, ref mut end)) = current_entity {
                        // Append the continuation to current entity
                        if let Some(stripped) = token.strip_prefix("##") {
                            text.push_str(stripped);
                        }
                        *end = offset_end;
                        *conf = (*conf + confidence) / 2.0;
                    }
                    // If no current entity, ignore this wordpiece (shouldn't happen normally)
                    continue;
                }

                if let Some((entity_type, is_beginning)) = label_to_entity_type(pred_label) {
                    if is_beginning {
                        // Finalize previous entity if any
                        if let Some((text, ent_type, conf, start, end)) = current_entity.take() {
                            entities.push(RecognizedEntity {
                                text,
                                entity_type: ent_type,
                                confidence: conf,
                                start,
                                end,
                            });
                        }

                        // Start new entity
                        current_entity = Some((
                            token.to_string(),
                            entity_type,
                            confidence,
                            offset_start,
                            offset_end,
                        ));
                    } else if let Some((
                        ref mut text,
                        ref curr_type,
                        ref mut conf,
                        _,
                        ref mut end,
                    )) = current_entity
                    {
                        // Continue entity if same type
                        if *curr_type == entity_type {
                            text.push(' ');
                            text.push_str(token);
                            *end = offset_end;
                            *conf = (*conf + confidence) / 2.0;
                        } else {
                            // Different type, finalize current and start new
                            let (text, ent_type, conf, start, end) = current_entity.take().unwrap();
                            entities.push(RecognizedEntity {
                                text,
                                entity_type: ent_type,
                                confidence: conf,
                                start,
                                end,
                            });

                            current_entity = Some((
                                token.to_string(),
                                entity_type,
                                confidence,
                                offset_start,
                                offset_end,
                            ));
                        }
                    } else {
                        // I-tag without B-tag, treat as B-tag
                        current_entity = Some((
                            token.to_string(),
                            entity_type,
                            confidence,
                            offset_start,
                            offset_end,
                        ));
                    }
                } else {
                    // O tag - finalize any ongoing entity
                    if let Some((text, entity_type, confidence, start, end)) = current_entity.take()
                    {
                        entities.push(RecognizedEntity {
                            text,
                            entity_type,
                            confidence,
                            start,
                            end,
                        });
                    }
                }
            }

            // Finalize last entity if any
            if let Some((text, entity_type, confidence, start, end)) = current_entity {
                entities.push(RecognizedEntity {
                    text,
                    entity_type,
                    confidence,
                    start,
                    end,
                });
            }

            Ok(entities)
        }
    }

    fn softmax_argmax(logits: &[f32]) -> (usize, f32) {
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits.iter().map(|x| (x - max_logit).exp()).sum();

        let mut max_idx = 0;
        let mut max_prob = 0.0f32;

        for (idx, &logit) in logits.iter().enumerate() {
            let prob = (logit - max_logit).exp() / exp_sum;
            if prob > max_prob {
                max_prob = prob;
                max_idx = idx;
            }
        }

        (max_idx, max_prob)
    }
}

#[cfg(feature = "ner")]
type SharedModel = Arc<Mutex<onnx_backend::OnnxNerModel>>;

/// Named Entity Recognition service
pub struct NerService {
    enabled: bool,
    #[cfg(feature = "ner")]
    model_name: String,
    #[cfg(feature = "ner")]
    confidence_threshold: f32,
    #[cfg(feature = "ner")]
    model: OnceCell<SharedModel>,
}

impl NerService {
    pub fn new(config: &NerConfig) -> Result<Self> {
        #[cfg(not(feature = "ner"))]
        {
            if config.enabled {
                tracing::warn!("NER is enabled in config but the 'ner' feature is not compiled in");
            }
            return Ok(Self { enabled: false });
        }

        #[cfg(feature = "ner")]
        {
            if !config.enabled {
                return Ok(Self {
                    enabled: false,
                    model_name: String::new(),
                    confidence_threshold: 0.5,
                    model: OnceCell::new(),
                });
            }

            Ok(Self {
                enabled: true,
                model_name: config.model.clone(),
                confidence_threshold: config.confidence_threshold,
                model: OnceCell::new(),
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

        let cache_dir = base.join("mmry").join("ner");
        fs::create_dir_all(&cache_dir)?;
        Ok(cache_dir)
    }

    #[cfg(feature = "ner")]
    fn model_repo_to_dir_name(repo: &str) -> String {
        repo.replace('/', "--")
    }

    #[cfg(feature = "ner")]
    async fn ensure_model(&self) -> Result<SharedModel> {
        if !self.enabled {
            return Err(Error::Ner("NER service disabled".into()));
        }

        let model_ref = self.model.get_or_try_init(|| -> Result<SharedModel> {
            let cache_dir = self.get_model_cache_dir()?;
            let model_name = if self.model_name.is_empty() {
                DEFAULT_NER_MODEL.to_string()
            } else {
                self.model_name.clone()
            };

            let model_dir = cache_dir.join(Self::model_repo_to_dir_name(&model_name));
            let model_path = model_dir.join("onnx").join("model.onnx");
            let tokenizer_path = model_dir.join("tokenizer.json");

            // Check if model is already downloaded
            if !model_path.exists() || !tokenizer_path.exists() {
                tracing::info!(model = %model_name, "Downloading NER model from HuggingFace");

                // Download model files using hf_hub or manual download
                Self::download_model(&model_name, &model_dir)?;
            }

            let model = onnx_backend::OnnxNerModel::new(&model_path, &tokenizer_path)?;
            Ok(Arc::new(Mutex::new(model)))
        })?;

        Ok(Arc::clone(model_ref))
    }

    #[cfg(feature = "ner")]
    fn download_model(model_name: &str, model_dir: &PathBuf) -> Result<()> {
        let base_url = format!("https://huggingface.co/{model_name}/resolve/main");

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

            // Use a simple blocking HTTP client for downloads
            // In production, you might want to use reqwest with async
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

    /// Extract named entities from text
    pub async fn extract(&self, text: &str) -> Result<Vec<RecognizedEntity>> {
        #[cfg(not(feature = "ner"))]
        {
            let _ = text;
            Ok(Vec::new())
        }

        #[cfg(feature = "ner")]
        {
            if !self.enabled {
                return Ok(Vec::new());
            }

            let model = self.ensure_model().await?;
            let entities = {
                let mut guard = model.lock().await;
                guard.predict(text)?
            };

            // Filter by confidence threshold
            let filtered: Vec<RecognizedEntity> = entities
                .into_iter()
                .filter(|e| e.confidence >= self.confidence_threshold)
                .collect();

            Ok(filtered)
        }
    }

    /// Extract and deduplicate entities, returning unique entity names with their types
    pub async fn extract_unique(&self, text: &str) -> Result<HashMap<String, (EntityType, f32)>> {
        let entities = self.extract(text).await?;
        let mut unique: HashMap<String, (EntityType, f32)> = HashMap::new();

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
                .or_insert((entity.entity_type, entity.confidence));
        }

        Ok(unique)
    }
}
