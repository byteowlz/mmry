// Temporal conflict tests - verify that the system correctly handles
// time-ordered information where newer data supersedes older data.
//
// Based on HMLR Test 7A: API Key Rotation
// https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System

use super::BenchmarkResult;
use crate::agents::FactRecord;
use crate::database::operations;
use crate::memory::Memory;
use crate::memory::MemoryType;
use sqlx::SqlitePool;
use std::time::Instant;

/// Test 7A: API Key Rotation
///
/// Scenario: User updates their API key multiple times.
/// The system must return the CURRENT (most recent) key, not older values.
///
/// HMLR achieves: Faithfulness 1.00, Context Recall 1.00
pub async fn test_api_key_rotation(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "7A - API Key Rotation";

    // Setup: Create a sequence of API key updates with timestamps
    let keys = [
        ("2024-01-01", "sk-old-key-001"),
        ("2024-02-01", "sk-mid-key-002"),
        ("2024-03-01", "sk-current-key-003"), // This should be returned
    ];

    // Store as facts with proper temporal ordering
    for (i, (_date, key)) in keys.iter().enumerate() {
        let mut fact = FactRecord::new("api_key", *key);
        fact.source_span = Some(format!("api-key-update-{i}"));
        // Older facts have lower recency scores
        fact.recency_score = (i + 1) as f32 / keys.len() as f32;

        if let Err(e) = operations::upsert_fact(pool, &fact).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert fact: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Also store as memories for full-text search
    for (date, key) in &keys {
        let content = format!("Updated API key on {date}: The new key is {key}");
        let memory = Memory::new(MemoryType::Episodic, content, "credentials".to_string());
        if let Err(e) = operations::insert_memory(pool, &memory).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert memory: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Query: "What is the current API key?"
    // Expected: The system should return "sk-current-key-003"

    // Verification: Search for api_key facts and check the most recent
    match operations::list_facts_by_key(pool, "api_key", 10).await {
        Ok(facts) => {
            if facts.is_empty() {
                return BenchmarkResult::failure(
                    test_name,
                    "No api_key facts found",
                    start.elapsed().as_millis() as u64,
                );
            }

            // Facts are returned in recency order (most recent first)
            let most_recent = &facts[0];

            let faithfulness = if most_recent.fact_value == "sk-current-key-003" {
                1.0
            } else {
                0.0
            };

            // Context recall: were all relevant facts retrievable?
            let context_recall = if facts.len() >= 3 {
                1.0
            } else {
                facts.len() as f32 / 3.0
            };

            BenchmarkResult::success(
                test_name,
                faithfulness,
                context_recall,
                start.elapsed().as_millis() as u64,
            )
        }
        Err(e) => BenchmarkResult::failure(
            test_name,
            &format!("Query failed: {e}"),
            start.elapsed().as_millis() as u64,
        ),
    }
}

/// Test 7C: Timestamp Updates
///
/// Scenario: A project deadline changes multiple times.
/// System must track the current deadline correctly.
pub async fn test_timestamp_updates(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "7C - Timestamp Updates";

    let deadlines = [
        (0.3, "2024-03-15"), // Original
        (0.6, "2024-03-22"), // Extended
        (1.0, "2024-03-29"), // Final (current)
    ];

    for (recency, date) in &deadlines {
        let mut fact = FactRecord::new("project_deadline", *date);
        fact.recency_score = *recency;
        fact.source_span = Some(format!("deadline-update-{recency}"));

        if let Err(e) = operations::upsert_fact(pool, &fact).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert fact: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Verify most recent deadline is returned
    match operations::list_facts_by_key(pool, "project_deadline", 10).await {
        Ok(facts) => {
            if facts.is_empty() {
                return BenchmarkResult::failure(
                    test_name,
                    "No deadline facts found",
                    start.elapsed().as_millis() as u64,
                );
            }

            let most_recent = &facts[0];
            let faithfulness = if most_recent.fact_value == "2024-03-29" {
                1.0
            } else {
                0.0
            };
            let context_recall = if facts.len() >= 3 {
                1.0
            } else {
                facts.len() as f32 / 3.0
            };

            BenchmarkResult::success(
                test_name,
                faithfulness,
                context_recall,
                start.elapsed().as_millis() as u64,
            )
        }
        Err(e) => BenchmarkResult::failure(
            test_name,
            &format!("Query failed: {e}"),
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
    async fn test_api_key_rotation_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_api_key_rotation(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );
        if let Some(err) = &result.error {
            println!("Error: {}", err);
        }

        assert!(
            result.passed,
            "Test should pass with correct temporal ordering"
        );
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }

    #[tokio::test]
    async fn test_timestamp_updates_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_timestamp_updates(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );
        if let Some(err) = &result.error {
            println!("Error: {}", err);
        }

        assert!(
            result.passed,
            "Test should pass with correct temporal ordering"
        );
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }
}
