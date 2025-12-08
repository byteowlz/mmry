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
mod temporal;
mod user_invariant;

pub use multi_hop::*;
pub use temporal::*;
pub use user_invariant::*;

/// Benchmark result tracking
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Name of the benchmark test
    pub name: String,
    /// Whether the test passed
    pub passed: bool,
    /// Faithfulness score (0.0 - 1.0) - does the answer match the facts?
    pub faithfulness: f32,
    /// Context recall (0.0 - 1.0) - were the right memories retrieved?
    pub context_recall: f32,
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
            error: Some(error.to_string()),
            duration_ms,
        }
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

#[derive(Debug, Clone)]
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
            if let Some(err) = &result.error {
                writeln!(f, "    Error: {err}")?;
            }
        }

        Ok(())
    }
}
