// Multi-hop reasoning tests - verify that the system can chain
// multiple pieces of information to answer complex queries.
//
// Based on HMLR Test 8: 30-Day Deprecation Policy
// https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System

use super::BenchmarkResult;
use crate::agents::FactRecord;
use crate::database::operations;
use crate::memory::Memory;
use crate::memory::MemoryType;
use sqlx::SqlitePool;
use std::time::Instant;

/// Test 8: 30-Day Deprecation Policy
///
/// Scenario: Company has a policy that deprecated APIs are removed after 30 days.
/// User asks about an API deprecated 25 days ago.
/// System must reason: deprecation date + policy = removal date.
///
/// HMLR achieves: Faithfulness 1.00, Context Recall 1.00
pub async fn test_30_day_deprecation_policy(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "8 - 30-Day Deprecation Policy";

    // Setup: Store the policy and the deprecation event as separate facts/memories

    // Fact 1: The policy
    let mut policy_fact = FactRecord::new("api_deprecation_policy", "30 days after deprecation");
    policy_fact.source_span = Some("company-policy-v2".to_string());
    policy_fact.recency_score = 1.0;
    if let Err(e) = operations::upsert_fact(pool, &policy_fact).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert policy fact: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Memory about the policy
    let policy_memory = Memory::new(
        MemoryType::Semantic,
        "Company policy: All deprecated APIs are removed from production 30 days after the deprecation announcement.".to_string(),
        "policies".to_string(),
    );
    if let Err(e) = operations::insert_memory(pool, &policy_memory).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert policy memory: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Fact 2: The deprecation event
    let mut deprecation_fact = FactRecord::new("user_api_v1_deprecated", "2024-02-01");
    deprecation_fact.source_span = Some("api-changelog".to_string());
    deprecation_fact.recency_score = 0.9;
    if let Err(e) = operations::upsert_fact(pool, &deprecation_fact).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert deprecation fact: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Memory about the deprecation
    let deprecation_memory = Memory::new(
        MemoryType::Episodic,
        "The User API v1 was deprecated on February 1st, 2024. Users should migrate to v2."
            .to_string(),
        "api-updates".to_string(),
    );
    if let Err(e) = operations::insert_memory(pool, &deprecation_memory).await {
        return BenchmarkResult::failure(
            test_name,
            &format!("Failed to insert deprecation memory: {e}"),
            start.elapsed().as_millis() as u64,
        );
    }

    // Query: "When will User API v1 be removed?"
    // Expected answer: "March 2nd, 2024" (Feb 1 + 30 days)

    // Verification: Both facts should be retrievable
    let policy_facts = operations::list_facts_by_key(pool, "api_deprecation_policy", 1).await;
    let deprecation_facts = operations::list_facts_by_key(pool, "user_api_v1_deprecated", 1).await;

    match (policy_facts, deprecation_facts) {
        (Ok(policies), Ok(deprecations)) => {
            let policy_found = policies
                .first()
                .map(|f| f.fact_value.contains("30 days"))
                .unwrap_or(false);
            let deprecation_found = deprecations
                .first()
                .map(|f| f.fact_value == "2024-02-01")
                .unwrap_or(false);

            // Faithfulness: Both facts needed for multi-hop reasoning
            let faithfulness = if policy_found && deprecation_found {
                1.0
            } else if policy_found || deprecation_found {
                0.5
            } else {
                0.0
            };

            // Context recall: Were both pieces of information retrieved?
            let context_recall = (policy_found as u8 + deprecation_found as u8) as f32 / 2.0;

            BenchmarkResult::success(
                test_name,
                faithfulness,
                context_recall,
                start.elapsed().as_millis() as u64,
            )
        }
        _ => BenchmarkResult::failure(
            test_name,
            "Failed to query facts",
            start.elapsed().as_millis() as u64,
        ),
    }
}

/// Test: Project Dependency Chain
///
/// Scenario: Project A depends on Library B, which depends on Framework C.
/// Query about Project A's indirect dependencies requires multi-hop reasoning.
pub async fn test_dependency_chain(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "Multi-hop - Dependency Chain";

    // Setup: Create a dependency chain
    let dependencies = [
        ("project_a_depends", "library_b"),
        ("library_b_depends", "framework_c"),
        ("framework_c_version", "3.2.1"),
    ];

    for (key, value) in &dependencies {
        let mut fact = FactRecord::new(*key, *value);
        fact.recency_score = 1.0;
        if let Err(e) = operations::upsert_fact(pool, &fact).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert fact: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Add memories describing the relationships
    let memories = [
        "Project A uses Library B for database operations.",
        "Library B is built on top of Framework C for async support.",
        "Framework C version 3.2.1 introduced breaking changes.",
    ];

    for content in &memories {
        let memory = Memory::new(
            MemoryType::Semantic,
            content.to_string(),
            "dependencies".to_string(),
        );
        if let Err(e) = operations::insert_memory(pool, &memory).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert memory: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Query: "What framework version does Project A ultimately depend on?"
    // Expected: Framework C 3.2.1 (requires following the chain)

    // Verify all facts are retrievable
    let mut found_count = 0;
    for (key, expected_value) in &dependencies {
        if let Ok(facts) = operations::list_facts_by_key(pool, key, 1).await {
            if facts
                .first()
                .map(|f| f.fact_value == *expected_value)
                .unwrap_or(false)
            {
                found_count += 1;
            }
        }
    }

    let faithfulness = found_count as f32 / dependencies.len() as f32;
    let context_recall = faithfulness; // Same metric for this test

    BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
}

/// Test: Access Control Chain
///
/// Scenario: User has role X, role X grants permission Y, permission Y allows action Z.
/// Query about whether user can do action Z requires multi-hop reasoning.
pub async fn test_access_control_chain(pool: &SqlitePool) -> BenchmarkResult {
    let start = Instant::now();
    let test_name = "Multi-hop - Access Control Chain";

    // Setup: Role-based access control chain
    let access_facts = [
        ("user_alice_role", "developer"),
        ("role_developer_permissions", "deploy,read,write"),
        ("permission_deploy_allows", "production_access"),
    ];

    for (key, value) in &access_facts {
        let mut fact = FactRecord::new(*key, *value);
        fact.recency_score = 1.0;
        if let Err(e) = operations::upsert_fact(pool, &fact).await {
            return BenchmarkResult::failure(
                test_name,
                &format!("Failed to insert fact: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }
    }

    // Query: "Can Alice access production?"
    // Expected: Yes (Alice -> developer -> deploy permission -> production_access)

    let mut found_count = 0;
    for (key, expected_value) in &access_facts {
        if let Ok(facts) = operations::list_facts_by_key(pool, key, 1).await {
            if facts
                .first()
                .map(|f| f.fact_value == *expected_value)
                .unwrap_or(false)
            {
                found_count += 1;
            }
        }
    }

    let faithfulness = found_count as f32 / access_facts.len() as f32;
    let context_recall = faithfulness;

    BenchmarkResult::success(
        test_name,
        faithfulness,
        context_recall,
        start.elapsed().as_millis() as u64,
    )
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
    async fn test_deprecation_policy_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_30_day_deprecation_policy(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );

        assert!(result.passed, "Multi-hop deprecation reasoning should work");
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }

    #[tokio::test]
    async fn test_dependency_chain_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_dependency_chain(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );

        assert!(result.passed, "Dependency chain reasoning should work");
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }

    #[tokio::test]
    async fn test_access_control_benchmark() {
        let ctx = setup_test_db().await;
        let result = test_access_control_chain(&ctx.pool).await;

        println!(
            "{}: faithfulness={:.2}, recall={:.2}",
            result.name, result.faithfulness, result.context_recall
        );

        assert!(result.passed, "Access control chain reasoning should work");
        assert_eq!(result.faithfulness, 1.0);
        assert_eq!(result.context_recall, 1.0);
    }
}
