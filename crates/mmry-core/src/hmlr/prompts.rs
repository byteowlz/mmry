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
//
// Based on Sean-V-Dev's HMLR-Agentic-AI-Memory-System prompts for benchmark-verified performance.

use crate::agents::BridgeBlock;
use crate::agents::FactCategory;
use crate::agents::FactRecord;

/// Generate a prompt for extracting categorized facts from content
///
/// Uses the 4-category system from Sean-V-Dev's HMLR for structured fact extraction:
/// - Definition: Definitions of terms or concepts
/// - Acronym: Acronym expansions
/// - Secret: Credentials, API keys, passwords, tokens
/// - Entity: Relationships between entities
pub fn fact_extraction_prompt(content: &str) -> String {
    format!(
        r#"Extract ONLY hard facts from this message.

CATEGORIES:
1. Definition - Definitions of terms or concepts
2. Acronym - Acronym expansions (e.g., "API = Application Programming Interface")
3. Secret - Credentials, API keys, passwords, tokens
4. Entity - Relationships between entities (e.g., "John is CEO of X")

RULES:
- Ignore general conversation or opinions
- Extract only verifiable, referenceable facts
- For acronyms, include the full expansion
- For secrets, include the key/value pair
- For entities, include the relationship type

MESSAGE:
{content}

Return JSON in this exact format:
{{
  "facts": [
    {{
      "key": "concise identifier (2-4 words)",
      "value": "the fact itself (complete sentence or phrase)",
      "category": "Definition|Acronym|Secret|Entity",
      "evidence_snippet": "exact 10-20 word quote containing the fact"
    }}
  ]
}}

If no facts found, return: {{"facts": []}}

Return ONLY the JSON, no other text:"#
    )
}

/// Parse the LLM response for fact extraction
///
/// Handles multiple formats:
/// 1. New format: {"facts": [...]} with categories
/// 2. Direct array: [...] with categories
/// 3. Markdown-wrapped: ```json {...} ``` or ```json [...] ```
/// 4. Legacy simple format: [{key, value}, ...] without categories
pub fn parse_facts_response(response: &str) -> Vec<FactRecord> {
    // Helper function to parse a fact from a JSON value
    fn parse_fact_value(v: &serde_json::Value) -> Option<FactRecord> {
        let key = v.get("key")?.as_str()?;
        let value = v.get("value")?.as_str()?;
        let category_str = v
            .get("category")
            .and_then(|c| c.as_str())
            .unwrap_or("General");
        let evidence = v.get("evidence_snippet").and_then(|e| e.as_str());

        let mut fact = FactRecord::with_category(key, value, FactCategory::parse(category_str));
        fact.evidence_snippet = evidence.map(String::from);
        Some(fact)
    }

    // Strip markdown code blocks if present (```json ... ``` or ``` ... ```)
    let cleaned = strip_markdown_code_block(response);

    // First try to parse the wrapped format: {"facts": [...]}
    if let Some(obj_start) = cleaned.find('{') {
        if let Some(obj_end) = cleaned.rfind('}') {
            let json_str = &cleaned[obj_start..=obj_end];
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(facts_array) = parsed.get("facts").and_then(|v| v.as_array()) {
                    return facts_array.iter().filter_map(parse_fact_value).collect();
                }
            }
        }
    }

    // Try to parse direct JSON array format: [...]
    // This handles when the LLM returns just the array without wrapping
    if let Some(arr_start) = cleaned.find('[') {
        if let Some(arr_end) = cleaned.rfind(']') {
            let json_str = &cleaned[arr_start..=arr_end];

            // Parse as JSON array
            if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                let facts: Vec<FactRecord> = parsed.iter().filter_map(parse_fact_value).collect();
                if !facts.is_empty() {
                    return facts;
                }
            }
        }
    }

    Vec::new()
}

/// Strip markdown code blocks from response (```json ... ``` or ``` ... ```)
fn strip_markdown_code_block(response: &str) -> &str {
    let trimmed = response.trim();

    // Check for ```json or ``` at start
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(content) = rest.strip_suffix("```") {
            return content.trim();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(content) = rest.strip_suffix("```") {
            return content.trim();
        }
    }

    response
}

/// Generate a prompt for routing a memory to a bridge block
///
/// Uses Sean-V-Dev's domain continuity framework for intelligent topic routing:
/// - 3 scenarios: Continue LAST ACTIVE, Resume PAUSED, Start NEW
/// - Domain continuity rules (Docker -> Docker Compose = SAME topic)
/// - Semantic context over keywords
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
        r#"You are an intelligent topic routing assistant for a conversational memory system.

EXISTING BRIDGE BLOCKS:
{candidates_str}

NEW MEMORY CONTENT:
---
{content}
---

YOUR TASK:
Analyze the memory and determine which bridge block it belongs to. Use your intelligence to understand the INTENT and SEMANTIC CONTEXT, not just surface-level keywords.

You have 3 possible decisions:
1. **Continue ACTIVE topic** - Memory relates to an ongoing conversation (status="active")
2. **Resume PAUSED topic** - Memory clearly relates to a previous topic (status="paused")
3. **Start NEW topic** - Memory is genuinely about something new/different

DECISION PRINCIPLES:

**Semantic Context Over Keywords:**
- "Let's talk about Docker Compose" while discussing Docker → SAME TOPIC (Docker is the context)
- "Let's talk about hiking" while discussing Docker → NEW TOPIC (completely unrelated)
- Focus on whether the SUBJECT MATTER is the same, not just the exact phrasing

**Domain Continuity - CRITICAL:**
- If the memory is about a SUBTOPIC or COMPONENT of the current domain, it's the SAME conversation
- Example: Docker Containerization → Docker Compose → Docker Volumes → Docker Networks (all Docker, ONE topic)
- Example: Python basics → async/await → threading → decorators (all Python, ONE topic)
- Only create new topic if it's a COMPLETELY DIFFERENT DOMAIN (Docker → cooking, Python → hiking)

**Natural Conversation Flow:**
- Subtopic exploration within a domain → CONTINUE
- Related questions, clarifications, deeper dives → CONTINUE
- "Also...", "What about...", "And..." typically signal continuation

**When in Doubt:**
- STRONGLY prefer CONTINUATION over creating new topics
- Consider the full context: keywords, topic label, not just the memory alone
- Ask yourself: "Is this a different DOMAIN or just a different PART of the same domain?"

Return JSON:
{{
    "chosen_block": "<block_id>" or null,
    "is_new_topic": true/false,
    "rationale": "<explain: 1) What domain is this memory? 2) Does it match any existing block's domain? 3) Why continue/resume/new?>"
}}

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

/// Generate a prompt for creating a topic label for a new bridge block
///
/// The topic label should be concise (3-6 words) and capture the main subject.
pub fn topic_label_prompt(content: &str) -> String {
    format!(
        r#"Generate a short topic label (3-6 words) for this memory content.

CONTENT:
{content}

The topic label should:
- Be concise and descriptive (3-6 words max)
- Capture the main subject/domain
- Use title case (e.g., "Docker Container Setup", "Python API Design")
- NOT include dates, specific values, or personal names unless they are the main topic

Examples:
- "We visited the castle in Altena today" -> "Travel - Altena Castle Visit"
- "Setting up Docker containers for the project" -> "Docker Container Setup"
- "Discussed Python async patterns with the team" -> "Python Async Patterns"
- "My API key for OpenAI is sk-123" -> "OpenAI API Configuration"

Return ONLY the topic label, no other text or formatting:"#
    )
}

/// Parse a topic label response from the LLM
pub fn parse_topic_label_response(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // Remove any markdown formatting
    let cleaned = trimmed
        .trim_start_matches(['`', '"', '\''])
        .trim_end_matches(['`', '"', '\''])
        .trim();

    // Validate: should be 1-10 words, not empty
    if cleaned.is_empty() || cleaned.split_whitespace().count() > 10 {
        return None;
    }

    Some(cleaned.to_string())
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

/// Memory candidate for 2-key filtering
#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    /// Index in the original candidate list
    pub index: usize,
    /// Memory content preview
    pub content: String,
    /// Vector similarity score (0.0-1.0)
    pub similarity: f32,
    /// Original query that created this memory (KEY 2)
    pub original_query: Option<String>,
}

/// Result of 2-key filtering
#[derive(Debug, Clone, Default)]
pub struct FilteringResult {
    /// Indices of relevant memories
    pub relevant_indices: Vec<usize>,
    /// Reasoning for filtering decisions
    pub reasoning: Option<String>,
}

/// Generate a prompt for 2-key memory filtering
///
/// Uses Sean-V-Dev's 2-key validation approach:
/// - KEY 1: Vector similarity score (semantic)
/// - KEY 2: Original query text (verbatim or summary)
///
/// This catches false positives where high similarity doesn't mean relevance.
/// Example: "I love Python" vs "I hate Python" = 95% similar but OPPOSITE meaning
pub fn two_key_filtering_prompt(query: &str, candidates: &[MemoryCandidate]) -> String {
    let candidates_text: String = candidates
        .iter()
        .map(|c| {
            let original_query_text = c
                .original_query
                .as_deref()
                .unwrap_or("(unknown original query)");
            format!(
                "[{}] Similarity: {:.2}\n   Original Query: \"{}\"\n   Content: {}...\n",
                c.index,
                c.similarity,
                original_query_text,
                &c.content[..c.content.len().min(300)]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You are a memory filter using 2-key validation.

CURRENT QUERY: "{query}"

MEMORY CANDIDATES:
{candidates_text}

TASK: Select ONLY memories that are truly relevant to the current query.

KEY 1 (Similarity Score): Semantic similarity from embeddings (0.0-1.0)
KEY 2 (Original Query): The actual query that created this memory

IMPORTANT: High similarity does NOT guarantee relevance!
Examples of FALSE POSITIVES to catch:
- "I love Python" vs "I hate Python" = 95% similarity but OPPOSITE meaning
- "Python advantages" vs "Python disadvantages" = High similarity but different intent
- "Meeting scheduled for Monday" vs "Meeting cancelled on Monday" = Same topic, opposite meaning

Use BOTH keys to filter out false positives:
1. Check if similarity is above threshold (0.5)
2. Verify the original query semantically aligns with current query
3. Reject if the INTENT is different even if words are similar

Return JSON:
{{
    "relevant_indices": [0, 2, 5],
    "reasoning": "<brief explanation of why others were filtered out>"
}}

Your response:"#
    )
}

/// Parse the LLM response for 2-key filtering
pub fn parse_filtering_response(response: &str) -> FilteringResult {
    // Try to find JSON object in response
    let json_start = response.find('{');
    let json_end = response.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &response[start..=end];

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            let relevant_indices = parsed
                .get("relevant_indices")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as usize))
                        .collect()
                })
                .unwrap_or_default();

            let reasoning = parsed
                .get("reasoning")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            return FilteringResult {
                relevant_indices,
                reasoning,
            };
        }
    }

    FilteringResult::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_facts_response_new_format() {
        let response = r#"{"facts": [
            {"key": "api_key", "value": "sk-123", "category": "Secret", "evidence_snippet": "My API key is sk-123"},
            {"key": "HMLR", "value": "Hierarchical Memory Lookup & Routing", "category": "Acronym", "evidence_snippet": "HMLR stands for Hierarchical Memory"}
        ]}"#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].fact_key, "api_key");
        assert_eq!(facts[0].fact_value, "sk-123");
        assert_eq!(facts[0].category, FactCategory::Secret);
        assert_eq!(
            facts[0].evidence_snippet,
            Some("My API key is sk-123".to_string())
        );
        assert_eq!(facts[1].fact_key, "HMLR");
        assert_eq!(facts[1].category, FactCategory::Acronym);
    }

    #[test]
    fn test_parse_facts_response_legacy_format() {
        let response =
            r#"[{"key": "api_key", "value": "sk-123"}, {"key": "language", "value": "German"}]"#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].fact_key, "api_key");
        assert_eq!(facts[0].fact_value, "sk-123");
        assert_eq!(facts[0].category, FactCategory::General);
        assert_eq!(facts[1].fact_key, "language");
        assert_eq!(facts[1].fact_value, "German");
    }

    #[test]
    fn test_parse_facts_response_direct_array_with_categories() {
        // This format is returned by some models (e.g., Qwen) that return array directly
        let response = r#"[
            {"key": "API Key Secret", "value": "sk-test123", "category": "Secret", "evidence_snippet": "My API key is sk-test123"},
            {"key": "HMLR Expansion", "value": "Hierarchical Memory Lookup and Routing", "category": "Acronym"},
            {"key": "John CEO Relationship", "value": "John is the CEO of Acme Corp.", "category": "Entity"}
        ]"#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 3);

        assert_eq!(facts[0].fact_key, "API Key Secret");
        assert_eq!(facts[0].fact_value, "sk-test123");
        assert_eq!(facts[0].category, FactCategory::Secret);
        assert_eq!(
            facts[0].evidence_snippet,
            Some("My API key is sk-test123".to_string())
        );

        assert_eq!(facts[1].fact_key, "HMLR Expansion");
        assert_eq!(facts[1].category, FactCategory::Acronym);

        assert_eq!(facts[2].fact_key, "John CEO Relationship");
        assert_eq!(facts[2].category, FactCategory::Entity);
    }

    #[test]
    fn test_parse_facts_response_markdown_wrapped() {
        // This format is returned by Gemma and other models that wrap JSON in markdown
        let response = r#"```json
{
  "facts": [
    {
      "key": "API key",
      "value": "sk-test123",
      "category": "Secret",
      "evidence_snippet": "My API key is sk-test123"
    },
    {
      "key": "HMLR",
      "value": "Hierarchical Memory Lookup and Routing",
      "category": "Acronym",
      "evidence_snippet": "HMLR stands for Hierarchical Memory Lookup and Routing"
    }
  ]
}
```"#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].fact_key, "API key");
        assert_eq!(facts[0].category, FactCategory::Secret);
        assert_eq!(facts[1].fact_key, "HMLR");
        assert_eq!(facts[1].category, FactCategory::Acronym);
    }

    #[test]
    fn test_parse_facts_response_with_surrounding_text() {
        let response = r#"Here are the extracted facts:
{"facts": [{"key": "deadline", "value": "2024-03-15", "category": "Definition", "evidence_snippet": "deadline is March 15th"}]}
That's all I found."#;
        let facts = parse_facts_response(response);

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].fact_key, "deadline");
        assert_eq!(facts[0].category, FactCategory::Definition);
    }

    #[test]
    fn test_parse_facts_response_empty() {
        let response = r#"{"facts": []}"#;
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
        assert!(prompt.contains("CATEGORIES"));
        assert!(prompt.contains("Definition"));
        assert!(prompt.contains("Acronym"));
        assert!(prompt.contains("Secret"));
        assert!(prompt.contains("Entity"));
        assert!(prompt.contains("evidence_snippet"));
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
        assert!(prompt.contains("Domain Continuity"));
        assert!(prompt.contains("SAME TOPIC"));
    }

    #[test]
    fn test_fact_category_parse() {
        assert_eq!(FactCategory::parse("Definition"), FactCategory::Definition);
        assert_eq!(FactCategory::parse("ACRONYM"), FactCategory::Acronym);
        assert_eq!(FactCategory::parse("secret"), FactCategory::Secret);
        assert_eq!(FactCategory::parse("Entity"), FactCategory::Entity);
        assert_eq!(FactCategory::parse("unknown"), FactCategory::General);
    }
}
