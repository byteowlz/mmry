# Changelog

All notable changes to this project will be documented in this file.

## 0.14.1

### Changed

- The default store moves to `$XDG_DATA_HOME/mmry` (`~/.local/share/mmry`): memories are user data. Existing stores: `mv ~/.local/state/mmry/{general,repos,local,.git,.gitattributes,.gitignore} ~/.local/share/mmry/` (or set `state_root`).
- Sync only tracks `general/`, `repos/` and its rule files (allowlist `.gitignore`).

### Fixed

- `mmry sync` no longer gets stuck on machine-local files committed by 0.14.0 (e.g. `service.pid`/`service.port` left by the removed daemon): they are untracked on the next sync, add/add conflicts in them and in `.gitignore` are resolved, and local copies are kept.
- A ledger one machine removed (duplicate directory merged) while another still appended to it no longer blocks sync: the changed file is kept and merged again on the next write.

## 0.14.0

Breaking: memories move from repo-local `.mmry/mmry.jsonl` to a per-user central store. Run `mmry setup --dry-run`, then `mmry setup`. Until then repo-local ledgers are not read (`migrate = "prompt"` asks once on a terminal, otherwise warns).

### Added

- Central store (`~/.local/state/mmry`) with `general/` and one `repos/<name>--<id>/` ledger per repository, identified by git root commit; `mmry init --tracked` keeps a repo-local ledger.
- `mmry setup` and `mmry migrate` (verified merge by event id, backup of the old file); `MMRY_STATE_ROOT`/`MMRY_MIGRATE` and `--state-root`/`--migrate`.
- `mmry supersede` with `--expected-revision`; `--expires`, `--why`, `--source`, `--machine` (`.` = this machine) on `add`.
- Contested memories for concurrent edits from two machines; `mmry doctor [--all] [--json]` reports them and damaged lines.
- `mmry preview [--json]`: deterministic session-start selection and the exact text to inject; published schemas in `examples/`.
- `mmry sync init|status|pull|push`: opt-in git sync of the store, with optional `[sync]` auto pull/commit/push.
- `mmry cleanup propose|apply`: reviewed removal of duplicates, near-duplicates and expired memories.
- AGENT_CTX v2 provenance fields are recorded with each event.

### Changed

- `add`, `supersede` and `rm` print the stored entry with `--json`.
- The repository now follows the byteowlz repository standard (`just check-all`, strict clippy, schema generation).

## 0.12.0

### Changed

- Default CLI storage is now the lean workspace-local append-only memory file at `.mmry/mmry.jsonl`.
- `mmry init` now initializes the JSONL memory file and gitignores it by default; use `mmry init --tracked` to track it.
- Legacy SQLite/indexed behavior moved behind `--indexed` for `init`, `add`, `list`/`ls`, `search`, and `rm`.
- Renamed the new core JSONL API to `MemoryFile`, `MemoryEntry`, `MemoryEvent`, and `MemoryEventType`.
- `mmry rm` now appends a `memory.deprecate` event in default mode instead of physically deleting data.

### Added

- `mmry-core::MemoryFile::open_workspace` for embedding workspace-local memory directly in tools such as oqto runner.
- Standalone migration script: `scripts/migrate_legacy_mmry_to_jsonl.py`.
- Migration filtering for bulky hstry/session/chunk imports by default.
- `list` alias for `ls` in the lean CLI.

### Notes

Release notes are generated from this changelog section. For GitHub releases, use the `0.12.0` section as the release body after pushing tag `v0.12.0`.

## 0.10.2

### Added

- `mmry stores copy <from> <to>` - copy all content between stores (alias: `cp`)
- `mmry stores move <from> <to>` - move all content between stores (alias: `mv`)
- Conflict resolution via `--on-conflict skip|overwrite|fail` (default: skip)
- Auto-creates destination store if it doesn't exist
- JSON output with `--json` flag

### Fixed

- INIT_SQL learnings table schema now matches current structure (was using stale column names)

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

