// Local-LLM prompts for HMLR fact extraction and routing
//
// These prompts are designed to work well with small local models (e.g., Qwen3-Coder-30B)
// and provide deterministic, structured output that can be parsed reliably.
//
// Key design principles:
// 1. Clear, explicit instructions with examples
// 2. JSON output format for reliable parsing
// 3. Few-shot examples to guide the model
// 4. Graceful degradation (empty results are acceptable)

use crate::agents::BridgeBlock;
use crate::agents::FactRecord;

/// Generate a prompt for extracting key-value facts from content
pub fn fact_extraction_prompt(content: &str) -> String {
    format!(
        r#"Extract key-value facts from the following text. Return ONLY a JSON array of objects with "key" and "value" fields. If no facts can be extracted, return an empty array [].

Facts are specific, verifiable pieces of information like:
- Names, dates, numbers, identifiers
- Preferences, settings, configurations
- Relationships, roles, statuses
- Technical details, API keys, versions

Examples:
Input: "My API key is sk-abc123 and I prefer responses in German"
Output: [{{"key": "api_key", "value": "sk-abc123"}}, {{"key": "language_preference", "value": "German"}}]

Input: "The project deadline is March 15th, 2024"
Output: [{{"key": "project_deadline", "value": "2024-03-15"}}]

Input: "Just had a great conversation about the weather"
Output: []

Now extract facts from:
---
{content}
---

Return ONLY the JSON array, no other text:"#
    )
}

/// Parse the LLM response for fact extraction
pub fn parse_facts_response(response: &str) -> Vec<FactRecord> {
    // Try to find JSON array in response
    let json_start = response.find('[');
    let json_end = response.rfind(']');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &response[start..=end];

        // Parse as JSON array
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
            return parsed
                .into_iter()
                .filter_map(|v| {
                    let key = v.get("key")?.as_str()?;
                    let value = v.get("value")?.as_str()?;
                    Some(FactRecord::new(key, value))
                })
                .collect();
        }
    }

    Vec::new()
}

/// Generate a prompt for routing a memory to a bridge block
pub fn routing_prompt(content: &str, candidates: &[BridgeBlock]) -> String {
    let candidates_json: Vec<serde_json::Value> = candidates
        .iter()
        .map(|b| {
            serde_json::json!({
                "block_id": b.block_id.to_string(),
                "topic": b.topic_label,
                "keywords": b.keywords,
                "status": b.status,
            })
        })
        .collect();

    let candidates_str = serde_json::to_string_pretty(&candidates_json).unwrap_or_default();

    format!(
        r#"Decide if the following memory should be added to an existing conversation topic (bridge block) or if it starts a new topic.

Memory content:
---
{content}
---

Existing bridge blocks:
{candidates_str}

Instructions:
1. If the memory fits an existing block's topic/keywords, return the block_id
2. If the memory is a new topic, return null
3. Consider semantic similarity, not just keyword matching

Return ONLY a JSON object with this format:
{{"chosen_block": "<block_id or null>", "is_new_topic": <true/false>, "rationale": "<brief explanation>"}}

Example responses:
- {{"chosen_block": "abc-123", "is_new_topic": false, "rationale": "Matches project discussion topic"}}
- {{"chosen_block": null, "is_new_topic": true, "rationale": "New topic about vacation planning"}}

Your response:"#
    )
}

/// Parse the LLM response for routing decision
pub fn parse_routing_response(response: &str) -> Option<RoutingDecision> {
    // Try to find JSON object in response
    let json_start = response.find('{');
    let json_end = response.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &response[start..=end];

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            let chosen_block = parsed
                .get("chosen_block")
                .and_then(|v| v.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok());

            let is_new_topic = parsed
                .get("is_new_topic")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let rationale = parsed
                .get("rationale")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            return Some(RoutingDecision {
                chosen_block,
                is_new_topic,
                rationale,
            });
        }
    }

    None
}

/// Routing decision from LLM
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub chosen_block: Option<uuid::Uuid>,
    pub is_new_topic: bool,
    pub rationale: Option<String>,
}

impl Default for RoutingDecision {
    fn default() -> Self {
        Self {
            chosen_block: None,
            is_new_topic: true,
            rationale: None,
        }
    }
}

/// Generate a prompt for summarizing a bridge block (for synthesis)
pub fn synthesis_prompt(block: &BridgeBlock, memories_content: &[String]) -> String {
    let memories_str = memories_content
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, c))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"Synthesize the following conversation memories into a concise summary.

Topic: {}
Keywords: {}

Memories:
{}

Instructions:
1. Extract the key points and decisions made
2. Note any action items or follow-ups
3. Keep the summary under 200 words
4. Preserve important facts and dates

Return a JSON object:
{{"summary": "<concise summary>", "key_points": ["<point1>", "<point2>", ...], "action_items": ["<item1>", "<item2>", ...]}}

Your response:"#,
        block.topic_label.as_deref().unwrap_or("Unknown"),
        block.keywords.join(", "),
        memories_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_facts_response_valid() {
        let response =
            r#"[{"key": "api_key", "value": "sk-123"}, {"key": "language", "value": "German"}]"#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].fact_key, "api_key");
        assert_eq!(facts[0].fact_value, "sk-123");
        assert_eq!(facts[1].fact_key, "language");
        assert_eq!(facts[1].fact_value, "German");
    }

    #[test]
    fn test_parse_facts_response_with_surrounding_text() {
        let response = r#"Here are the extracted facts:
[{"key": "deadline", "value": "2024-03-15"}]
That's all I found."#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].fact_key, "deadline");
    }

    #[test]
    fn test_parse_facts_response_empty() {
        let response = "[]";
        let facts = parse_facts_response(response);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_facts_response_invalid() {
        let response = "I couldn't find any facts in the text.";
        let facts = parse_facts_response(response);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_routing_response_existing_block() {
        let response = r#"{"chosen_block": "550e8400-e29b-41d4-a716-446655440000", "is_new_topic": false, "rationale": "Matches project topic"}"#;
        let decision = parse_routing_response(response).unwrap();

        assert!(decision.chosen_block.is_some());
        assert!(!decision.is_new_topic);
        assert_eq!(
            decision.rationale,
            Some("Matches project topic".to_string())
        );
    }

    #[test]
    fn test_parse_routing_response_new_topic() {
        let response = r#"{"chosen_block": null, "is_new_topic": true, "rationale": "New topic"}"#;
        let decision = parse_routing_response(response).unwrap();

        assert!(decision.chosen_block.is_none());
        assert!(decision.is_new_topic);
    }

    #[test]
    fn test_parse_routing_response_invalid() {
        let response = "I'm not sure which block to use.";
        let decision = parse_routing_response(response);
        assert!(decision.is_none());
    }

    #[test]
    fn test_fact_extraction_prompt_generation() {
        let content = "My API key is sk-test123";
        let prompt = fact_extraction_prompt(content);

        assert!(prompt.contains("sk-test123"));
        assert!(prompt.contains("JSON array"));
        assert!(prompt.contains("key"));
        assert!(prompt.contains("value"));
    }

    #[test]
    fn test_routing_prompt_generation() {
        let content = "Let's discuss the API design";
        let mut block = BridgeBlock::new();
        block.topic_label = Some("API Discussion".to_string());
        block.keywords = vec!["api".to_string(), "design".to_string()];

        let prompt = routing_prompt(content, &[block]);

        assert!(prompt.contains("API design"));
        assert!(prompt.contains("API Discussion"));
        assert!(prompt.contains("block_id"));
    }
}
