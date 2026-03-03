# Changelog

All notable changes to this project will be documented in this file.

## 0.10.1

### Fixed

- `mmry service restart` / `reload` no longer fails when the service is not running
- Database schema: `bridge_block_id` column now included in INIT_SQL for fresh installs
- Fixed broken `semantic_query_finds_related_memory` test (removed reference to deleted `search_with_embedding` method)
- Fixed legacy schema migration test (`bridge_block_id` index creation order)

## 0.10.0

### Added

- **fastembed 5.11 with new embedding models:**
  - BGE-M3 multilingual (100+ languages, dense + sparse)
  - BGE Chinese models (small/large zh v1.5)
  - Snowflake Arctic Embed family (XS/S/M/M-Long/L)
  - Gemma 300M embedding model
  - CLIP ViT-B/32 text encoder
  - Jina v2 base English
  - all-mpnet-base-v2
  - Paraphrase multilingual mpnet-base-v2
  - BGE-M3 sparse embeddings (new sparse model alongside SPLADE++)
  - Updated ort to 2.0.0-rc.11

- **Service enable/disable (mmry service enable|disable):**
  - `mmry service enable` installs and enables auto-start (systemd user unit on Linux, launchd plist on macOS)
  - `mmry service disable` stops, disables, and removes the service unit
  - `mmry service status` now shows `Auto-start: enabled/disabled`
  - `enable` respects existing unit files (will not overwrite units created by external tools like oqto setup)

### Changed (BREAKING)

- **ExternalApiConfig field rename (mmry-ypv4):**
  - `[external_api] enable` renamed to `enabled`
  - `[external_api] console_enable` renamed to `console_enabled`
  - This aligns with every other config section (`service.enabled`, `embeddings.enabled`, `analyzer.enabled`, etc.)
  - **Action required:** update `config.toml` files and any code that sets these fields (e.g., oqto setup scripts, deploy configs)

### Added**
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

