use regex::Regex;
use regex::RegexBuilder;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

use crate::agents::FactRecord;
use crate::config::GuardPattern;
use crate::config::GuardPatternKind;
use crate::config::GuardrailsConfig;
use crate::memory::Memory;
use crate::search::HmlrSearchResult;
use crate::stores::MemoryWithStore;

const REGEX_SIZE_LIMIT_BYTES: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct Guardrails {
    patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone)]
struct CompiledPattern {
    pattern: String,
    kind: GuardPatternKind,
    reason: Option<String>,
    matcher: GuardMatcher,
}

#[derive(Debug, Clone)]
enum GuardMatcher {
    Literal(String),
    Regex(Regex),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardrailsSummary {
    pub blocked_memories: usize,
    pub blocked_facts: usize,
    pub triggered_patterns: Vec<GuardPatternSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardPatternSummary {
    pub pattern: String,
    pub kind: GuardPatternKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GuardrailsAccumulator {
    guardrails: Guardrails,
    matched: Vec<bool>,
    blocked_memories: usize,
    blocked_facts: usize,
}

impl Guardrails {
    pub fn from_config(config: &GuardrailsConfig) -> Self {
        if !config.enabled {
            return Self {
                patterns: Vec::new(),
            };
        }

        if config.max_patterns == 0 || config.max_pattern_length == 0 {
            return Self {
                patterns: Vec::new(),
            };
        }

        let mut patterns = Vec::new();
        for pattern in config.patterns.iter().take(config.max_patterns) {
            match compile_pattern(config, pattern) {
                Ok(compiled) => patterns.push(compiled),
                Err(err) => {
                    tracing::warn!("Guardrails pattern skipped: {err}");
                }
            }
        }

        if config.patterns.len() > config.max_patterns {
            tracing::warn!(
                "Guardrails patterns truncated to max_patterns={}",
                config.max_patterns
            );
        }

        Self { patterns }
    }

    pub fn is_enabled(&self) -> bool {
        !self.patterns.is_empty()
    }

    pub fn validate_pattern(
        config: &GuardrailsConfig,
        pattern: &GuardPattern,
    ) -> Result<(), String> {
        compile_pattern(config, pattern).map(|_| ())
    }

    fn matches_text(&self, text: &str, matched: &mut [bool]) -> bool {
        if text.is_empty() || self.patterns.is_empty() {
            return false;
        }

        let mut lower = None;
        let mut matched_any = false;
        for (idx, pattern) in self.patterns.iter().enumerate() {
            let is_match = match &pattern.matcher {
                GuardMatcher::Literal(lit) => {
                    let lower = lower.get_or_insert_with(|| text.to_lowercase());
                    lower.contains(lit)
                }
                GuardMatcher::Regex(re) => re.is_match(text),
            };
            if is_match {
                matched_any = true;
                if let Some(flag) = matched.get_mut(idx) {
                    *flag = true;
                }
            }
        }

        matched_any
    }

    fn summary(
        &self,
        blocked_memories: usize,
        blocked_facts: usize,
        matched: &[bool],
    ) -> GuardrailsSummary {
        let triggered_patterns = self
            .patterns
            .iter()
            .zip(matched.iter().copied())
            .filter_map(|(pattern, triggered)| {
                if triggered {
                    Some(GuardPatternSummary {
                        pattern: pattern.pattern.clone(),
                        kind: pattern.kind,
                        reason: pattern.reason.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        GuardrailsSummary {
            blocked_memories,
            blocked_facts,
            triggered_patterns,
        }
    }
}

impl GuardrailsAccumulator {
    pub fn new(config: &GuardrailsConfig) -> Self {
        let guardrails = Guardrails::from_config(config);
        let matched = vec![false; guardrails.patterns.len()];
        Self {
            guardrails,
            matched,
            blocked_memories: 0,
            blocked_facts: 0,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.guardrails.is_enabled()
    }

    pub fn summary(&self) -> GuardrailsSummary {
        self.guardrails
            .summary(self.blocked_memories, self.blocked_facts, &self.matched)
    }

    pub fn filter_memories(&mut self, memories: Vec<Memory>) -> Vec<Memory> {
        if !self.guardrails.is_enabled() {
            return memories;
        }

        memories
            .into_iter()
            .filter_map(|memory| {
                if self
                    .guardrails
                    .matches_text(&memory.content, &mut self.matched)
                {
                    self.blocked_memories += 1;
                    None
                } else {
                    Some(memory)
                }
            })
            .collect()
    }

    pub fn filter_memories_with_store(
        &mut self,
        memories: Vec<MemoryWithStore>,
    ) -> Vec<MemoryWithStore> {
        if !self.guardrails.is_enabled() {
            return memories;
        }

        memories
            .into_iter()
            .filter_map(|memory| {
                if self
                    .guardrails
                    .matches_text(&memory.memory.content, &mut self.matched)
                {
                    self.blocked_memories += 1;
                    None
                } else {
                    Some(memory)
                }
            })
            .collect()
    }

    pub fn filter_facts(&mut self, facts: Vec<FactRecord>) -> Vec<FactRecord> {
        if !self.guardrails.is_enabled() {
            return facts;
        }

        facts
            .into_iter()
            .filter_map(|fact| {
                let mut matched = self
                    .guardrails
                    .matches_text(&fact.fact_key, &mut self.matched)
                    || self
                        .guardrails
                        .matches_text(&fact.fact_value, &mut self.matched);
                if let Some(snippet) = fact.evidence_snippet.as_deref() {
                    matched = matched || self.guardrails.matches_text(snippet, &mut self.matched);
                }
                if matched {
                    self.blocked_facts += 1;
                    None
                } else {
                    Some(fact)
                }
            })
            .collect()
    }

    pub fn filter_hmlr_result(&mut self, result: HmlrSearchResult) -> HmlrSearchResult {
        if !self.guardrails.is_enabled() {
            return result;
        }

        let mut result = result;
        let mut kept_memories = Vec::new();
        let mut kept_ids = HashSet::new();

        for memory in result.memories.into_iter() {
            if self
                .guardrails
                .matches_text(&memory.content, &mut self.matched)
            {
                self.blocked_memories += 1;
            } else {
                kept_ids.insert(memory.id);
                kept_memories.push(memory);
            }
        }

        result.memories = kept_memories;

        result.memory_blocks.retain(|id, _| kept_ids.contains(id));
        result.memory_facts.retain(|id, facts| {
            if !kept_ids.contains(id) {
                return false;
            }

            let filtered = self.filter_facts(std::mem::take(facts));
            if filtered.is_empty() {
                false
            } else {
                *facts = filtered;
                true
            }
        });

        result.facts = self.filter_facts(result.facts);

        let mut block_ids = HashSet::new();
        for block_id in result.memory_blocks.values() {
            block_ids.insert(*block_id);
        }
        result
            .bridge_blocks
            .retain(|block| block_ids.contains(&block.block_id));

        result
    }
}

fn compile_pattern(
    config: &GuardrailsConfig,
    pattern: &GuardPattern,
) -> Result<CompiledPattern, String> {
    let trimmed = pattern.pattern.trim();
    if trimmed.is_empty() {
        return Err("pattern cannot be empty".to_string());
    }

    let length = trimmed.chars().count();
    if length > config.max_pattern_length {
        return Err(format!(
            "pattern exceeds max length of {} characters",
            config.max_pattern_length
        ));
    }

    let kind = pattern.kind;
    let reason = pattern.reason.clone();

    let matcher = match kind {
        GuardPatternKind::Literal => GuardMatcher::Literal(trimmed.to_lowercase()),
        GuardPatternKind::Regex => {
            let mut builder = RegexBuilder::new(trimmed);
            builder.size_limit(REGEX_SIZE_LIMIT_BYTES);
            let regex = builder
                .build()
                .map_err(|e| format!("regex compile failed: {e}"))?;
            GuardMatcher::Regex(regex)
        }
    };

    Ok(CompiledPattern {
        pattern: trimmed.to_string(),
        kind,
        reason,
        matcher,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::FactRecord;
    use crate::memory::Memory;
    use crate::memory::MemoryType;
    use uuid::Uuid;

    fn guard_config(patterns: Vec<GuardPattern>) -> GuardrailsConfig {
        GuardrailsConfig {
            enabled: true,
            patterns,
            max_patterns: 10,
            max_pattern_length: 64,
        }
    }

    #[test]
    fn guardrails_filters_memories_and_facts() {
        let patterns = vec![
            GuardPattern {
                pattern: "secret".to_string(),
                kind: GuardPatternKind::Literal,
                reason: Some("hide secrets".to_string()),
            },
            GuardPattern {
                pattern: "\\b\\d{3}-\\d{2}-\\d{4}\\b".to_string(),
                kind: GuardPatternKind::Regex,
                reason: None,
            },
        ];
        let config = guard_config(patterns);
        let mut guard = GuardrailsAccumulator::new(&config);

        let memories = vec![
            Memory::new(
                MemoryType::Semantic,
                "public memory".to_string(),
                "default".to_string(),
            ),
            Memory::new(
                MemoryType::Semantic,
                "secret memory 123-45-6789".to_string(),
                "default".to_string(),
            ),
        ];

        let filtered = guard.filter_memories(memories);
        assert_eq!(filtered.len(), 1);

        let facts = vec![
            FactRecord::new("api_key", "secret"),
            FactRecord::new("project", "alpha"),
        ];
        let filtered_facts = guard.filter_facts(facts);
        assert_eq!(filtered_facts.len(), 1);

        let summary = guard.summary();
        assert_eq!(summary.blocked_memories, 1);
        assert_eq!(summary.blocked_facts, 1);
        assert_eq!(summary.triggered_patterns.len(), 2);
    }

    #[test]
    fn guardrails_skips_overlong_patterns() {
        let config = GuardrailsConfig {
            enabled: true,
            patterns: vec![GuardPattern {
                pattern: "toolong".to_string(),
                kind: GuardPatternKind::Literal,
                reason: None,
            }],
            max_patterns: 10,
            max_pattern_length: 3,
        };
        let mut guard = GuardrailsAccumulator::new(&config);

        let memories = vec![Memory::new(
            MemoryType::Semantic,
            "toolong".to_string(),
            "default".to_string(),
        )];
        let filtered = guard.filter_memories(memories);
        assert_eq!(filtered.len(), 1);
        assert_eq!(guard.summary().blocked_memories, 0);
    }

    #[test]
    fn guardrails_filters_hmlr_result_maps() {
        let config = guard_config(vec![GuardPattern {
            pattern: "drop".to_string(),
            kind: GuardPatternKind::Literal,
            reason: None,
        }]);
        let mut guard = GuardrailsAccumulator::new(&config);

        let kept_id = Uuid::new_v4();
        let dropped_id = Uuid::new_v4();
        let block_id = Uuid::new_v4();

        let mut result = HmlrSearchResult {
            memories: vec![
                Memory {
                    id: kept_id,
                    memory_type: MemoryType::Semantic,
                    content: "keep this".to_string(),
                    embedding: None,
                    sparse_embedding: None,
                    metadata: serde_json::json!({}),
                    importance: 5,
                    expires_at: None,
                    expired_at: None,
                    source_attribution: None,
                    trust_level: 0.5,
                    source_reinforcement_score: 0.0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    category: "default".to_string(),
                    tags: Vec::new(),
                    parent_id: None,
                    chunk_index: None,
                    total_chunks: None,
                    chunk_method: None,
                    bridge_block_id: None,
                },
                Memory {
                    id: dropped_id,
                    memory_type: MemoryType::Semantic,
                    content: "drop this".to_string(),
                    embedding: None,
                    sparse_embedding: None,
                    metadata: serde_json::json!({}),
                    importance: 5,
                    expires_at: None,
                    expired_at: None,
                    source_attribution: None,
                    trust_level: 0.5,
                    source_reinforcement_score: 0.0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    category: "default".to_string(),
                    tags: Vec::new(),
                    parent_id: None,
                    chunk_index: None,
                    total_chunks: None,
                    chunk_method: None,
                    bridge_block_id: None,
                },
            ],
            facts: vec![FactRecord::new("keep", "value")],
            bridge_blocks: vec![crate::agents::BridgeBlock {
                block_id,
                ..crate::agents::BridgeBlock::default()
            }],
            memory_facts: std::collections::HashMap::from([(
                dropped_id,
                vec![FactRecord::new("secret", "drop")],
            )]),
            memory_blocks: std::collections::HashMap::from([(dropped_id, block_id)]),
        };

        result = guard.filter_hmlr_result(result);

        assert_eq!(result.memories.len(), 1);
        assert_eq!(result.memories[0].id, kept_id);
        assert!(result.memory_facts.is_empty());
        assert!(result.memory_blocks.is_empty());
        assert!(result.bridge_blocks.is_empty());
    }
}
