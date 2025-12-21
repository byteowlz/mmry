// User invariant tests - verify that persistent user constraints
// are maintained even when contradictory information is encountered.
//
// Based on HMLR Test 7B: User Invariant Override
// https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System

use super::compute_retrieval_metrics;
use super::BenchmarkResult;
use crate::agents::FactRecord;
use crate::agents::UserProfileEntry;
use crate::database::operations;
use crate::memory::Memory;
use crate::memory::MemoryType;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::Instant;
use uuid::Uuid;

/// Test 7B: User Invariant Override
///
/// Scenario: User has a dietary constraint (vegetarian).
/// Later conversation mentions meat dishes, but the constraint must persist.
///
/// HMLR achieves: Faithfulness 1.00, Context Recall 1.00
pub async fn test_user_invariant_override(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "7B - User Invariant Override";

    // Setup: Establish user constraint in profile
    let profile = UserProfileEntry::new(serde_json::json!({
        "dietary": {
            "vegetarian": true,
            "reason": "ethical concerns",
            "since": "2020"
        },
        "preferences": {
            "cuisine": ["Italian", "Thai", "Mexican"]
        }
    }));

    if let Err(e) = operations::set_user_profile(pool, &profile).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to set profile: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Also store as a fact for cross-referencing
    let mut fact = FactRecord::new("dietary_restriction", "vegetarian");
    fact.recency_score = 1.0; // High priority invariant
    let expected_fact_id = fact.id;
    if let Err(e) = operations::upsert_fact(pool, &fact).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert fact: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Add conversation that mentions meat (should NOT override the constraint)
    let distracting_memories = [
        "Had lunch at the new steakhouse downtown. Great ambiance!",
        "My friend ordered the chicken parmigiana, looked amazing.",
        "The restaurant serves both vegetarian and meat options.",
    ];

    for content in &distracting_memories {
        let memory = Memory::new(
            MemoryType::Episodic,
            content.to_string(),
            "dining".to_string(),
        );
        if let Err(e) = operations::insert_memory(pool, &memory).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert memory: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Verification: User profile should still show vegetarian constraint
    match operations::get_user_profile(pool, profile.id).await {
        Ok(Some(retrieved)) => {
            let is_vegetarian = retrieved
                .profile
                .get("dietary")
                .and_then(|d| d.get("vegetarian"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let faithfulness = if is_vegetarian { 1.0 } else { 0.0 };

            // Context recall: check if fact is still correct
            match operations::list_facts_by_key(pool, "dietary_restriction", 10).await {
                Ok(facts) => {
                    let fact_correct = facts
                        .first()
                        .map(|f| f.fact_value == "vegetarian")
                        .unwrap_or(false);
                    let context_recall = if fact_correct { 1.0 } else { 0.5 };

                    let relevant: HashSet<Uuid> = [expected_fact_id].into_iter().collect();
                    let retrieved_ids: Vec<Uuid> = facts.iter().map(|f| f.id).collect();
                    let retrieval = compute_retrieval_metrics(&retrieved_ids, &relevant, 1);

                    let out = BenchmarkResult::success(
                        test_name,
                        faithfulness,
                        context_recall,
                        start.elapsed().as_millis() as u64,
                    );
                    out.with_retrieval(retrieval)
                }
                Err(e) => BenchmarkResult::failure(
                    test_name,
                    &format!("Fact query failed: {e}"),
                    start.elapsed().as_millis() as u64,
                ),
            }
        }
        Ok(None) => BenchmarkResult::failure(
            test_name,
            "Profile not found after retrieval",
            start.elapsed().as_millis() as u64,
        ),
        Err(e) => BenchmarkResult::failure(
            test_name,
            &format!("Profile query failed: {e}"),
            start.elapsed().as_millis() as u64,
        ),
    }
}

/// Test: Language Preference Persistence
///
/// Scenario: User prefers responses in German.
/// System should maintain this preference across sessions.
pub async fn test_language_preference_persistence(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "User Invariant - Language Preference";

    // Setup: User prefers German
    let profile = UserProfileEntry::new(serde_json::json!({
        "language": {
            "preferred": "German",
            "alternatives": ["English"],
            "set_date": "2024-01-15"
        }
    }));

    if let Err(e) = operations::set_user_profile(pool, &profile).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to set profile: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    let mut fact = FactRecord::new("language_preference", "German");
    fact.recency_score = 1.0;
    let expected_fact_id = fact.id;
    if let Err(e) = operations::upsert_fact(pool, &fact).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert fact: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Add some English content (should not override preference)
    let english_memories = [
        "Discussed the project timeline in the team meeting.",
        "Updated the documentation with new API endpoints.",
    ];

    for content in &english_memories {
        let memory = Memory::new(
            MemoryType::Episodic,
            content.to_string(),
            "work".to_string(),
        );
        let _ = operations::insert_memory(pool, &memory).await;
    }

    // Verify preference persists
    match operations::get_user_profile(pool, profile.id).await {
        Ok(Some(retrieved)) => {
            let preferred_lang = retrieved
                .profile
                .get("language")
                .and_then(|l| l.get("preferred"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let faithfulness = if preferred_lang == "German" { 1.0 } else { 0.0 };

            match operations::list_facts_by_key(pool, "language_preference", 10).await {
                Ok(facts) => {
                    let fact_correct = facts
                        .first()
                        .map(|f| f.fact_value == "German")
                        .unwrap_or(false);
                    let context_recall = if fact_correct { 1.0 } else { 0.5 };

                    let relevant: HashSet<Uuid> = [expected_fact_id].into_iter().collect();
                    let retrieved_ids: Vec<Uuid> = facts.iter().map(|f| f.id).collect();
                    let retrieval = compute_retrieval_metrics(&retrieved_ids, &relevant, 1);

                    let out = BenchmarkResult::success(
                        test_name,
                        faithfulness,
                        context_recall,
                        start.elapsed().as_millis() as u64,
                    );
                    out.with_retrieval(retrieval)
                }
                Err(e) => BenchmarkResult::failure(
                    test_name,
                    &format!("Fact query failed: {e}"),
                    start.elapsed().as_millis() as u64,
                ),
            }
        }
        Ok(None) => BenchmarkResult::failure(
            test_name,
            "Profile not found",
            start.elapsed().as_millis() as u64,
        ),
        Err(e) => BenchmarkResult::failure(
            test_name,
            &format!("Profile query failed: {e}"),
            start.elapsed().as_millis() as u64,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::database::Database;

    struct TestContext {
        _temp_dir: tempfile::TempDir,
        pool: SqlitePool,
    }

    async fn setup_test_db() -> TestContext {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.stores.directory = temp_dir.path().to_path_buf();
        config.stores.default = "test".to_string();

        let db = Database::init_store(&config, Some("test")).await.unwrap();
        let pool = db.pool().clone();

        TestContext {
            _temp_dir: temp_dir,
            pool,
        }
    }

    #[tokio::test]
    async fn test_vegetarian_invariant_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_user_invariant_override(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );

        assert!(
            result.passed,
            "User invariant should persist despite distracting content"
        );
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }

    #[tokio::test]
    async fn test_language_preference_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_language_preference_persistence(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );

        assert!(result.passed, "Language preference should persist");
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }
}
