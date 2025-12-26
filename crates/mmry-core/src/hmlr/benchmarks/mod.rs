// HMLR Benchmarks - RAGAS-style tests for memory system quality
//
// These tests verify that the HMLR memory system can correctly handle
// challenging scenarios that commonly trip up LLM-based memory systems:
// - Temporal conflicts (old vs new information)
// - User invariants (persistent constraints)
// - Multi-hop reasoning (policy chains)
// - Zero-keyword recall (semantic search)
//
// Based on HMLR's benchmark suite: https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System

mod multi_hop;
mod system;
mod temporal;
mod user_invariant;

pub use multi_hop::*;
pub use system::*;
pub use temporal::*;
pub use user_invariant::*;

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalMetrics {
    pub k: usize,
    pub precision_at_k: f32,
    pub recall_at_k: f32,
    pub mrr: f32,
}

/// Benchmark result tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Name of the benchmark test
    pub name: String,
    /// Whether the test passed
    pub passed: bool,
    /// Faithfulness score (0.0 - 1.0) - does the answer match the facts?
    pub faithfulness: f32,
    /// Context recall (0.0 - 1.0) - were the right memories retrieved?
    pub context_recall: f32,
    /// Retrieval metrics when the benchmark returns ranked items
    pub retrieval: Option<RetrievalMetrics>,
    /// Whether secret data leaked in output (must be false under redaction)
    pub secret_leaked: bool,
    /// Stable hash of relevant output (used for determinism checks)
    pub determinism_hash: Option<u64>,
    /// Operation count captured for throughput reporting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_count: Option<u64>,
    /// Optional error message if failed
    pub error: Option<String>,
    /// Execution time in milliseconds
    pub duration_ms: u64,
}

impl BenchmarkResult {
    pub fn success(name: &str, faithfulness: f32, context_recall: f32, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            passed: faithfulness >= 0.9 && context_recall >= 0.9,
            faithfulness,
            context_recall,
            retrieval: None,
            secret_leaked: false,
            determinism_hash: None,
            operation_count: None,
            error: None,
            duration_ms,
        }
    }

    pub fn failure(name: &str, error: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            faithfulness: 0.0,
            context_recall: 0.0,
            retrieval: None,
            secret_leaked: false,
            determinism_hash: None,
            operation_count: None,
            error: Some(error.to_string()),
            duration_ms,
        }
    }

    pub fn with_retrieval(mut self, retrieval: RetrievalMetrics) -> Self {
        self.retrieval = Some(retrieval);
        self
    }

    pub fn with_secret_leak(mut self, secret_leaked: bool) -> Self {
        self.secret_leaked = secret_leaked;
        self
    }

    pub fn with_determinism_hash(mut self, hash: u64) -> Self {
        self.determinism_hash = Some(hash);
        self
    }

    pub fn with_operation_count(mut self, operation_count: u64) -> Self {
        self.operation_count = Some(operation_count);
        self
    }
}

pub fn compute_retrieval_metrics(
    retrieved: &[Uuid],
    relevant: &HashSet<Uuid>,
    k: usize,
) -> RetrievalMetrics {
    let k = k.max(1);
    let relevant_total = relevant.len().max(1) as f32;

    let mut hits_in_top_k = 0usize;
    let mut first_hit_rank: Option<usize> = None;
    for (idx, id) in retrieved.iter().take(k).enumerate() {
        if relevant.contains(id) {
            hits_in_top_k += 1;
            if first_hit_rank.is_none() {
                first_hit_rank = Some(idx + 1);
            }
        }
    }

    let mrr = first_hit_rank.map(|rank| 1.0 / rank as f32).unwrap_or(0.0);
    let precision_at_k = hits_in_top_k as f32 / k as f32;
    let recall_at_k = hits_in_top_k as f32 / relevant_total;

    RetrievalMetrics {
        k,
        precision_at_k,
        recall_at_k,
        mrr,
    }
}

/// Benchmark suite runner
pub struct BenchmarkSuite {
    results: Vec<BenchmarkResult>,
}

impl BenchmarkSuite {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    pub fn add_result(&mut self, result: BenchmarkResult) {
        self.results.push(result);
    }

    pub fn summary(&self) -> BenchmarkSummary {
        let total = self.results.len();
        let passed = self.results.iter().filter(|r| r.passed).count();
        let avg_faithfulness = if total > 0 {
            self.results.iter().map(|r| r.faithfulness).sum::<f32>() / total as f32
        } else {
            0.0
        };
        let avg_recall = if total > 0 {
            self.results.iter().map(|r| r.context_recall).sum::<f32>() / total as f32
        } else {
            0.0
        };

        BenchmarkSummary {
            total_tests: total,
            passed_tests: passed,
            failed_tests: total - passed,
            avg_faithfulness,
            avg_context_recall: avg_recall,
            results: self.results.clone(),
        }
    }
}

impl Default for BenchmarkSuite {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub avg_faithfulness: f32,
    pub avg_context_recall: f32,
    pub results: Vec<BenchmarkResult>,
}

impl std::fmt::Display for BenchmarkSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "HMLR Benchmark Results")?;
        writeln!(f, "=====================")?;
        writeln!(
            f,
            "Passed: {}/{} ({:.1}%)",
            self.passed_tests,
            self.total_tests,
            if self.total_tests > 0 {
                (self.passed_tests as f32 / self.total_tests as f32) * 100.0
            } else {
                0.0
            }
        )?;
        writeln!(f, "Avg Faithfulness: {:.2}", self.avg_faithfulness)?;
        writeln!(f, "Avg Context Recall: {:.2}", self.avg_context_recall)?;
        writeln!(f)?;

        for result in &self.results {
            let status = if result.passed { "PASS" } else { "FAIL" };
            writeln!(
                f,
                "[{}] {} - F:{:.2} R:{:.2} ({}ms)",
                status, result.name, result.faithfulness, result.context_recall, result.duration_ms
            )?;
            if let Some(retrieval) = &result.retrieval {
                writeln!(
                    f,
                    "    retrieval@{} P:{:.2} R:{:.2} MRR:{:.2}",
                    retrieval.k, retrieval.precision_at_k, retrieval.recall_at_k, retrieval.mrr
                )?;
            }
            if let Some(ops) = result.operation_count {
                let seconds = (result.duration_ms as f32 / 1000.0).max(0.001);
                writeln!(f, "    ops={ops} ops_per_sec={:.2}", ops as f32 / seconds)?;
            }
            if result.secret_leaked {
                writeln!(f, "    secret_leaked=true")?;
            }
            if let Some(hash) = result.determinism_hash {
                writeln!(f, "    determinism_hash={hash}")?;
            }
            if let Some(err) = &result.error {
                writeln!(f, "    Error: {err}")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_retrieval_metrics_precision_recall_mrr() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);

        let retrieved = vec![a, b, c];
        let relevant: HashSet<Uuid> = [b].into_iter().collect();

        let metrics = compute_retrieval_metrics(&retrieved, &relevant, 2);

        assert_eq!(metrics.k, 2);
        assert!((metrics.precision_at_k - 0.5).abs() < f32::EPSILON);
        assert!((metrics.recall_at_k - 1.0).abs() < f32::EPSILON);
        assert!((metrics.mrr - 0.5).abs() < f32::EPSILON);
    }
}
