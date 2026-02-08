# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

### Added

- **Agent Identity (mmry-yay2.1–yay2.3, yay2.7, yay2.8):**
  - `--agent`, `--agent-kind`, `--agent-meta` flags on `mmry add` CLI with `human` as default
  - `MMRY_AGENT`, `MMRY_AGENT_KIND`, `MMRY_AGENT_META` environment variables for agent identity
  - `agent`, `agent_kind`, `agent_meta` fields on MCP `mmry.memory.add` tool
  - `AgentIdentity` struct with `resolve(&pool)` for get-or-create by name
  - `AgentRecord` extended with `repo()`, `workspace()`, `session_id()` accessors and `set_meta()`
  - All `--json` output includes agent provenance envelope `{name, kind, meta}`

- **Learnings Data Model (mmry-xrbv.1):**
  - `Learning` struct with dual polarity (`Guiding` / `Cautionary`), category, scope, maturity, provenance
  - `FeedbackEvent` and `FeedbackType` (helpful/harmful) for evidence recording
  - `LearningScope` (global, workspace, language, framework, task)
  - `Maturity` lifecycle: candidate → established → proven → deprecated
  - Schema migration: `learnings` + `learning_feedback` tables with 7 indexes
  - DB operations: `upsert_learning`, `get_learning`, `list_learnings`, `count_learnings`, `count_learnings_by_category`, `delete_learning`, `record_learning_feedback`, `list_learning_feedback`
  - Prompt templates: `learning-extraction-guiding.md` and `learning-extraction-cautionary.md`

- **Confidence Decay & Maturity Tracking (mmry-xrbv.2):**
  - `compute_effective_score()`: time-decayed scoring with 90-day half-life and 4× harmful multiplier
  - `compute_maturity()`: deterministic transitions based on decayed feedback counts and harmful ratios
  - `ScoringConfig` with configurable thresholds for all maturity transitions
  - Pinned learnings bypass automatic maturity transitions

- **Research:**
  - Consolidation research question: optimal merging of dual-polarity learnings under temporal decay
  - Eight sub-questions: algebraic structure, decision-theoretic compression, bipolar scoring, staleness detection & phase-out, feedback ingestion channels, RLM-based recursive consolidation, convergence & minimality, empirical evaluation framework
  - Comparative analysis of EvolveR, cass-memory, GitHub Copilot, Reflexion, MemGPT, Mem0, TITANS, RLM approaches

