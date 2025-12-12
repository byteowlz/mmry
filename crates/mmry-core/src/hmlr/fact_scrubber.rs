//! FactScrubber: Extracts key-value facts from memory content
//!
//! Uses the Analyzer trait for LLM-backed extraction when enabled,
//! falls back to heuristics when disabled.

use crate::agents::FactRecord;
use crate::analysis::Analyzer;
use crate::Result;
use std::sync::Arc;

/// FactScrubber extracts structured facts from memory content
pub struct FactScrubber {
    analyzer: Arc<dyn Analyzer>,
}

impl FactScrubber {
    /// Create a new FactScrubber with the given analyzer
    pub fn new(analyzer: Arc<dyn Analyzer>) -> Self {
        Self { analyzer }
    }

    /// Extract facts from content using the analyzer
    ///
    /// When analyzer is enabled (LLM-backed), uses LLM for extraction.
    /// When analyzer is NoOp, falls back to heuristic extraction.
    pub async fn extract(&self, content: &str) -> Result<Vec<FactRecord>> {
        // Try analyzer first (may be NoOp which returns empty)
        let mut facts = self.analyzer.extract_facts(content).await?;

        // If analyzer returned nothing, use heuristic extraction
        if facts.is_empty() {
            facts = self.heuristic_extract(content);
        }

        Ok(facts)
    }

    /// Heuristic-based fact extraction
    ///
    /// Looks for common patterns like:
    /// - "X is Y" statements
    /// - "name: value" pairs
    /// - Person names (capitalized words)
    fn heuristic_extract(&self, content: &str) -> Vec<FactRecord> {
        let mut facts = Vec::new();

        // Look for "X is Y" patterns
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = extract_is_statement(line) {
                facts.push(FactRecord::new(key, value));
            }
        }

        // Look for "key: value" patterns
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = extract_colon_pair(line) {
                // Avoid duplicating "is" statements
                if !facts.iter().any(|f| f.fact_key == key) {
                    facts.push(FactRecord::new(key, value));
                }
            }
        }

        // Extract potential person names (capitalized words that aren't at sentence start)
        let names = extract_person_names(content);
        for name in names {
            if !facts.iter().any(|f| f.fact_value == name) {
                facts.push(FactRecord::new("person", name));
            }
        }

        facts
    }
}

/// Extract "X is Y" statement
fn extract_is_statement(line: &str) -> Option<(String, String)> {
    // Pattern: "Subject is Value" or "Subject are Value"
    let patterns = [" is ", " are "];

    for pattern in patterns {
        if let Some(pos) = line.to_lowercase().find(pattern) {
            let key = line[..pos].trim();
            let value = line[pos + pattern.len()..].trim();

            // Skip if key or value is too short
            if key.len() >= 2 && value.len() >= 2 {
                // Clean up key (remove articles, etc.)
                let key = key
                    .trim_start_matches("The ")
                    .trim_start_matches("the ")
                    .trim_start_matches("A ")
                    .trim_start_matches("a ");

                return Some((key.to_lowercase(), value.to_string()));
            }
        }
    }

    None
}

/// Extract "key: value" pair
fn extract_colon_pair(line: &str) -> Option<(String, String)> {
    // Look for "key: value" or "key = value"
    if let Some(pos) = line.find(':') {
        let key = line[..pos].trim();
        let value = line[pos + 1..].trim();

        if key.len() >= 2
            && key.len() <= 30
            && !value.is_empty()
            && !key.contains(' ')
            && !key.contains('/')
        {
            return Some((key.to_lowercase(), value.to_string()));
        }
    }

    None
}

/// Extract potential person names from text
fn extract_person_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();

    // Split by words and look for capitalized sequences
    let words: Vec<&str> = content.split_whitespace().collect();

    for window in words.windows(2) {
        let first = window[0];
        let second = window[1];

        // Check if both words start with capital letters
        if is_capitalized_word(first) && is_capitalized_word(second) {
            // Exclude common non-name patterns
            if !is_common_phrase_start(first) {
                let name = format!("{first} {second}");
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }

    names
}

/// Check if word starts with capital letter and is otherwise lowercase
fn is_capitalized_word(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_alphabetic());
    if word.is_empty() {
        return false;
    }

    let first = word.chars().next().unwrap();
    let rest = &word[first.len_utf8()..];

    first.is_uppercase()
        && rest.chars().all(|c| c.is_lowercase())
        && word.len() >= 2
        && word.len() <= 20
}

/// Check if word is a common phrase start (not a name)
fn is_common_phrase_start(word: &str) -> bool {
    const COMMON_STARTS: &[&str] = &[
        "The", "This", "That", "These", "Those", "There", "When", "Where", "What", "Which", "Why",
        "How", "If", "I", "We", "You", "He", "She", "It", "They", "My", "Your", "Our", "His",
        "Her", "Its", "Their", "In", "On", "At", "To", "For", "With", "By", "From", "As", "But",
        "And", "Or", "Not", "No", "Yes", "All", "Any", "Some", "Many", "Most", "Each", "Every",
        "More", "Less", "Very", "Just", "Also", "Still", "Even", "Only", "Now", "Then", "Here",
    ];

    COMMON_STARTS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::NoOpAnalyzer;

    #[test]
    fn test_extract_is_statement() {
        assert_eq!(
            extract_is_statement("The project is important"),
            Some(("project".to_string(), "important".to_string()))
        );

        assert_eq!(
            extract_is_statement("Sarah is the team lead"),
            Some(("sarah".to_string(), "the team lead".to_string()))
        );

        assert_eq!(
            extract_is_statement("They are working on it"),
            Some(("they".to_string(), "working on it".to_string()))
        );

        // Too short
        assert!(extract_is_statement("X is Y").is_none());
    }

    #[test]
    fn test_extract_colon_pair() {
        assert_eq!(
            extract_colon_pair("name: John Doe"),
            Some(("name".to_string(), "John Doe".to_string()))
        );

        assert_eq!(
            extract_colon_pair("project: Memory System"),
            Some(("project".to_string(), "Memory System".to_string()))
        );

        // Key too long
        assert!(extract_colon_pair("this is a very long key: value").is_none());

        // Key with spaces
        assert!(extract_colon_pair("key with spaces: value").is_none());
    }

    #[test]
    fn test_extract_person_names() {
        let content = "Met with Sarah Johnson about the project. John Smith was also there.";
        let names = extract_person_names(content);

        assert!(names.contains(&"Sarah Johnson".to_string()));
        assert!(names.contains(&"John Smith".to_string()));
    }

    #[test]
    fn test_extract_person_names_excludes_common_phrases() {
        let content = "The Project Manager said it. When This Happens we should act.";
        let names = extract_person_names(content);

        // "The Project" and "When This" should not be extracted
        assert!(!names.contains(&"The Project".to_string()));
        assert!(!names.contains(&"When This".to_string()));
    }

    #[test]
    fn test_is_capitalized_word() {
        assert!(is_capitalized_word("John"));
        assert!(is_capitalized_word("Sarah"));
        assert!(!is_capitalized_word("john"));
        assert!(!is_capitalized_word("JOHN"));
        assert!(!is_capitalized_word("J")); // too short
    }

    #[tokio::test]
    async fn test_fact_scrubber_with_noop() {
        let scrubber = FactScrubber::new(Arc::new(NoOpAnalyzer));
        let content = "Sarah is the project lead. name: John";
        let facts = scrubber.extract(content).await.unwrap();

        // Should find at least the "is" statement
        assert!(facts.iter().any(|f| f.fact_key == "sarah"));
    }

    #[test]
    fn test_heuristic_extract_combined() {
        let scrubber = FactScrubber::new(Arc::new(NoOpAnalyzer));
        let content =
            "Met with Sarah Johnson about the project.\nstatus: ongoing\nThe deadline is Friday.";
        let facts = scrubber.heuristic_extract(content);

        // Should extract various facts
        assert!(facts.iter().any(|f| f.fact_key == "status"));
        assert!(facts.iter().any(|f| f.fact_key == "deadline"));
        assert!(facts.iter().any(|f| f.fact_key == "person"));
    }
}
