//! Scribe: Maintains user profile via fire-and-forget async updates
//!
//! Extracts persistent user constraints and preferences from conversation
//! and updates the user_profiles table without blocking the main flow.

use super::HmlrContext;
use crate::agents::UserProfileEntry;
use crate::database::operations;
use crate::memory::Memory;
use crate::Result;
use serde_json::Value;
use sqlx::SqlitePool;

/// Scribe maintains user profile with fire-and-forget updates
pub struct Scribe;

impl Scribe {
    /// Create a new Scribe
    pub fn new() -> Self {
        Self
    }

    /// Update user profile based on memory content
    ///
    /// This is a fire-and-forget operation - errors are logged but don't
    /// block the main enrichment flow.
    pub async fn update_profile(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<()> {
        // Get or create user profile
        let mut profile = operations::get_user_profile(pool, context.creator_id)
            .await?
            .unwrap_or_else(|| UserProfileEntry::new(serde_json::json!({})));

        // Ensure profile ID matches creator
        profile.id = context.creator_id;

        // Extract and merge profile updates from memory
        let updates = self.extract_profile_updates(memory);
        if !updates.is_empty() {
            self.merge_profile_updates(&mut profile, updates);
            operations::set_user_profile(pool, &profile).await?;
        }

        Ok(())
    }

    /// Extract profile-relevant information from memory
    fn extract_profile_updates(&self, memory: &Memory) -> Vec<(String, Value)> {
        let mut updates = Vec::new();
        let content = &memory.content;

        // Look for preference patterns
        let preferences = extract_preferences(content);
        for (key, value) in preferences {
            updates.push((format!("preferences.{key}"), Value::String(value)));
        }

        // Look for constraint patterns
        let constraints = extract_constraints(content);
        for (key, value) in constraints {
            updates.push((format!("constraints.{key}"), Value::String(value)));
        }

        // Track topics from memory category/tags
        if !memory.category.is_empty() && memory.category != "default" {
            updates.push((
                "topics".to_string(),
                Value::Array(vec![Value::String(memory.category.clone())]),
            ));
        }

        // Track memory types user tends to create
        updates.push((
            "memory_types_used".to_string(),
            Value::Array(vec![Value::String(format!("{:?}", memory.memory_type))]),
        ));

        updates
    }

    /// Merge updates into the profile JSON
    fn merge_profile_updates(&self, profile: &mut UserProfileEntry, updates: Vec<(String, Value)>) {
        let profile_obj = profile.profile.as_object_mut();
        if profile_obj.is_none() {
            profile.profile = serde_json::json!({});
        }

        if let Some(obj) = profile.profile.as_object_mut() {
            for (key, value) in updates {
                // Handle nested keys (e.g., "preferences.color")
                let parts: Vec<&str> = key.split('.').collect();
                if parts.len() == 1 {
                    // Simple key
                    merge_value(obj, &key, value);
                } else {
                    // Nested key
                    let parent_key = parts[0];
                    let child_key = parts[1..].join(".");

                    // Ensure parent exists
                    if !obj.contains_key(parent_key) {
                        obj.insert(
                            parent_key.to_string(),
                            Value::Object(serde_json::Map::new()),
                        );
                    }

                    if let Some(parent) = obj.get_mut(parent_key) {
                        if let Some(parent_obj) = parent.as_object_mut() {
                            merge_value(parent_obj, &child_key, value);
                        }
                    }
                }
            }
        }

        profile.updated_at = chrono::Utc::now();
    }
}

impl Default for Scribe {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge a value into a JSON object, handling arrays specially
fn merge_value(obj: &mut serde_json::Map<String, Value>, key: &str, value: Value) {
    if let Some(existing) = obj.get_mut(key) {
        // If both are arrays, append
        if let (Some(existing_arr), Some(new_arr)) = (existing.as_array_mut(), value.as_array()) {
            for item in new_arr {
                if !existing_arr.contains(item) {
                    existing_arr.push(item.clone());
                }
            }
            return;
        }
    }

    // Otherwise just set the value
    obj.insert(key.to_string(), value);
}

/// Extract preference patterns from content
fn extract_preferences(content: &str) -> Vec<(String, String)> {
    let mut preferences = Vec::new();
    let content_lower = content.to_lowercase();

    // Patterns like "I prefer X", "I like X", "my favorite X is Y"
    let patterns = [
        ("i prefer ", "preference"),
        ("i like ", "likes"),
        ("i enjoy ", "enjoys"),
        ("i want ", "wants"),
        ("my favorite ", "favorite"),
    ];

    for (pattern, key) in patterns {
        if let Some(pos) = content_lower.find(pattern) {
            let after = &content[pos + pattern.len()..];
            // Take until end of sentence or newline
            let value = after
                .split(&['.', '\n', '!', '?'][..])
                .next()
                .unwrap_or("")
                .trim();
            if !value.is_empty() && value.len() <= 100 {
                preferences.push((key.to_string(), value.to_string()));
            }
        }
    }

    preferences
}

/// Extract constraint patterns from content
fn extract_constraints(content: &str) -> Vec<(String, String)> {
    let mut constraints = Vec::new();
    let content_lower = content.to_lowercase();

    // Patterns like "I don't X", "I can't X", "I won't X", "I always X", "I never X"
    let patterns = [
        ("i don't ", "avoids"),
        ("i can't ", "cannot"),
        ("i won't ", "refuses"),
        ("i always ", "always"),
        ("i never ", "never"),
        ("i must ", "must"),
        ("i have to ", "required"),
        ("i am allergic to ", "allergies"),
        ("i'm allergic to ", "allergies"),
        ("i am ", "identity"),
        ("i'm ", "identity"),
    ];

    for (pattern, key) in patterns {
        if let Some(pos) = content_lower.find(pattern) {
            let after = &content[pos + pattern.len()..];
            let value = after
                .split(&['.', '\n', '!', '?'][..])
                .next()
                .unwrap_or("")
                .trim();
            if !value.is_empty() && value.len() <= 100 {
                constraints.push((key.to_string(), value.to_string()));
            }
        }
    }

    constraints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::memory::MemoryType;
    use tempfile::tempdir;
    use uuid::Uuid;

    async fn setup_test_db() -> anyhow::Result<(tempfile::TempDir, Database)> {
        let temp = tempdir()?;
        let db_path = temp.path().join("test.db");
        let db = Database::init(&db_path, 384).await?;
        Ok((temp, db))
    }

    #[test]
    fn test_extract_preferences() {
        let content = "I prefer dark mode for coding. I like Python over Java.";
        let prefs = extract_preferences(content);

        assert!(prefs.iter().any(|(k, _)| k == "preference"));
        assert!(prefs.iter().any(|(k, _)| k == "likes"));
    }

    #[test]
    fn test_extract_constraints() {
        let content = "I don't eat meat. I am vegetarian. I always use spaces over tabs.";
        let constraints = extract_constraints(content);

        assert!(constraints.iter().any(|(k, _)| k == "avoids"));
        assert!(constraints.iter().any(|(k, _)| k == "identity"));
        assert!(constraints.iter().any(|(k, _)| k == "always"));
    }

    #[test]
    fn test_merge_value_arrays() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "topics".to_string(),
            Value::Array(vec![Value::String("rust".to_string())]),
        );

        merge_value(
            &mut obj,
            "topics",
            Value::Array(vec![Value::String("python".to_string())]),
        );

        let topics = obj.get("topics").unwrap().as_array().unwrap();
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&Value::String("rust".to_string())));
        assert!(topics.contains(&Value::String("python".to_string())));
    }

    #[test]
    fn test_merge_value_arrays_no_duplicates() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "topics".to_string(),
            Value::Array(vec![Value::String("rust".to_string())]),
        );

        merge_value(
            &mut obj,
            "topics",
            Value::Array(vec![Value::String("rust".to_string())]),
        );

        let topics = obj.get("topics").unwrap().as_array().unwrap();
        assert_eq!(topics.len(), 1); // No duplicate
    }

    #[tokio::test]
    async fn test_scribe_update_profile() -> anyhow::Result<()> {
        let (_temp, db) = setup_test_db().await?;

        let scribe = Scribe::new();
        let memory = Memory::new(
            MemoryType::Semantic,
            "I prefer dark mode for all my tools".to_string(),
            "preferences".to_string(),
        );
        let context = HmlrContext {
            creator_id: Uuid::new_v4(),
            conversation_history: vec![],
            query: None,
        };

        scribe.update_profile(db.pool(), &memory, &context).await?;

        // Check profile was created
        let profile = operations::get_user_profile(db.pool(), context.creator_id).await?;
        assert!(profile.is_some());

        let profile = profile.unwrap();
        assert!(profile.profile.get("preferences").is_some());

        db.close().await;
        Ok(())
    }

    #[test]
    fn test_scribe_extract_profile_updates() {
        let scribe = Scribe::new();
        let memory = Memory::new(
            MemoryType::Semantic,
            "I prefer dark mode".to_string(),
            "work".to_string(),
        );

        let updates = scribe.extract_profile_updates(&memory);

        // Should have preference and topic updates
        assert!(!updates.is_empty());
        assert!(updates.iter().any(|(k, _)| k.starts_with("preferences")));
        assert!(updates.iter().any(|(k, _)| k == "topics"));
    }
}
