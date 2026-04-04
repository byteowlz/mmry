# Issues

## Open

### [mmry-s6v9] Embedding service down: error sending request for url http://localhost:8081/v1/embeddings (P1, bug)

### [mmry-sfxf] Configurable LLM models for extraction/consolidation - support local models (Ollama, LM Studio) and API providers. Configure via config.toml [analyzer] section. (P1, feature)

### [mmry-kj8d] Service hstry integration - watch for new hstry sessions, auto-trigger learning extraction in background. (P1, feature)

### [mmry-w6q5] Learning consolidation (reflector) - periodic LLM pass to merge related learnings, prune superseded ones, compress old entries. Manual via 'mmry consolidate' and automatic in service. (P1, feature)

### [mmry-9sc1] Learning deduplication - embedding similarity to find candidates, then LLM equivalence check before merging. Runs during extraction. (P1, feature)

### [mmry-z5wf] MEMORY.md bidirectional sync - generate human/agent-readable MEMORY.md from learnings store, parse edits back. Per-store file. (P1, feature)

### [mmry-xrbv.10] MCP tools for learnings - GetContext, AddLearning, RecordFeedback, RecordOutcome, ListLearnings, GapAnalysis (P1, feature)

### [mmry-xrbv.4] LLM-based learning extraction pipeline - analyze hstry chat sessions and distill actionable learnings/rules into the learnings store (P1, feature)

### [mmry-xrbv.3] 'mmry context <task>' command - search learnings (not memories) within a store and optionally across stores, return relevant rules + anti-patterns as JSON (P1, feature)

### [mmry-xrbv] Learnings & Context System - distill agent sessions into actionable rules with confidence tracking, inspired by cass-memory (P1, epic)

### [mmry-93vp] Standardize search JSON output for cross-tool integration (P2, feature)
For unified search across hstry/mmry/trx, search results need a common envelope format: { source: 'mmry', source_store, id, title, content, snippet, score, created_at, tags, category, importance, metadata }. Ensure mmry search --json output matches this schema so agntz can merge results from all three tools.

### [mmry-mgcj] Add cross-store search capability (P2, feature)
Each mmry store is isolated. Cannot search across all repos/stores. Add a --all flag to search that queries across all configured stores and merges results. The daemon should support multi-store search natively. This is critical for unified search - a query about 'authentication' might have relevant memories across multiple project stores.

### [mmry-6g12] Add tag filter to search (P2, feature)
Tags exist in the schema (JSON array) and are stored per memory, but search cannot filter by them. Add optional tags parameter to search that filters memories containing any/all specified tags. Expose via CLI --tag flag. Tags are the cross-tool connector for unified search across hstry/mmry/trx.

### [mmry-3gm3] Add date range filters (after/before) to search (P2, feature)
Search has no date range filter. Cannot say 'memories from last week'. Add after/before parameters to SearchService::search_with_options() and ExecuteSearchOptions, apply as WHERE clause on created_at. Expose via CLI --after/--before flags and daemon search API.

### [mmry-rzzd] agntz memory add fails with memory_embeddings dimension mismatch (P2, bug)

### [mmry-8rhe] Research RLM/DSPy.RLM memory systems vs OM (LongMemEval) and assess fit for mmry/observational-memory (P2, task)

### [mmry-a1e9] Remove unused crate dependencies from Cargo.toml files after HMLR removal (rig-core, ort NER models, etc) (P2, chore)

### [mmry-yay2.6] Trust weighting by agent kind - human-added memories/learnings get higher initial trust than agent-extracted ones. Configurable per agent-kind. (P2, feature)

### [mmry-yay2.5] Agent filtering in search and context - --agent flag to filter memories/learnings by source agent, --agent-kind to filter by type (human/coding_agent/review_agent) (P2, feature)

### [mmry-yay2] Agent Identity & Provenance - pass agent/repo/session identity through all mmry interfaces (CLI, MCP, gRPC) so every memory and learning has clear attribution (P2, epic)

### [mmry-xrbv.8] Cross-store context search - 'mmry context <task> --all-stores' to search learnings across multiple stores with store attribution in results (P2, feature)

### [mmry-xrbv.5] Feedback events & outcome recording - record helpful/harmful feedback on learnings, record session outcomes with rule attribution, auto-apply to update scores (P2, feature)

### [mmry-zr20] Add memory type filter to search (P3, feature)
memory_type is indexed (idx_memories_type) but search cannot filter by it. Add optional type parameter to ExecuteSearchOptions. Expose via CLI --type flag (episodic/semantic/procedural etc).

### [mmry-zntt] Add importance threshold filter to search (P3, feature)
Importance currently only boosts scoring but cannot be used as a hard filter. Add optional min_importance parameter to search that excludes memories below the threshold. Useful for 'only show me high-importance memories' queries. Expose via CLI --importance or --min-importance flag.

### [mmry-1ka8] vctr: standalone embedding/reranking server (P3, feature)

### [mmry-5avw] ServiceClient should discover actual HTTP port from runtime state, not static config (P3, bug)
When mmry service is started with MMRY__EXTERNAL_API__PORT env override (e.g. 48000), the actual listening port differs from config.toml (8081). ServiceClient.get_api_url() reads from config, so clients like agntz fail to connect. The service should write the actual HTTP port to a state file (like it does for gRPC) or clients should read the env var.

## Closed

- [mmry-h8jz] Add aarch64-unknown-linux-gnu to release CI (Cross.toml + OpenSSL or remove openssl dep) (closed 2026-03-23)
- [mmry-ypv4] ExternalApiConfig uses enable/console_enable instead of enabled/console_enabled - inconsistent with all other config structs (closed 2026-03-03)
- [mmry-rbqc] Clean up mmry-tui/app.rs: remove HMLR/graph/guardrails UI and re-enable in workspace (closed 2026-02-10)
- [mmry-ssfb] Clean up mmry-service/server.rs: remove HMLR endpoints and re-enable in workspace (closed 2026-02-10)
- [mmry-gh9f] Clean up mmry-mcp: remove HMLR tools (bridge_blocks, facts, profile_blocks, context_pack, conversation) and re-enable in workspace (closed 2026-02-10)
- [mmry-3xvs] Remove HMLR system - drop bridge blocks, fact scrubber, scribe, lattice crawler, context hydrator, governor. Keep only learnings table and core search. (closed 2026-02-10)
- [mmry-xrbv.9] Agent-native onboarding - guided workflow for agents to analyze hstry sessions and build playbook, with progress tracking and resumability across sessions (closed 2026-02-10)
- [mmry-yay2.4] Per-agent profiles in Scribe - track what each agent/repo works on, patterns, preferences. Replace single global profile with agent-scoped profiles. (closed 2026-02-10)
- [mmry-xrbv.6] Anti-pattern conversion - automatically invert learnings with >50% harmful ratio into PITFALL warnings (closed 2026-02-10)
- [mmry-xrbv.7] Gap analysis - track learning distribution across categories, identify critical/underrepresented areas, prioritize session analysis for gap-filling (closed 2026-02-10)
- [mmry-jadt.8] Reasoning event types in agent_events - track inference, contradiction, prediction events (closed 2026-02-10)
- [mmry-jadt.7] Natural language certainty statements - express confidence as reasoning, not numbers (closed 2026-02-10)
- [mmry-jadt.6] Track fact predictive power - importance based on reasoning utility, not arbitrary scores (closed 2026-02-10)
- [mmry-jadt.5] Predictions table - store predictive models about user identity with verification loop (closed 2026-02-10)
- [mmry-jadt.4] Query-time synthesis - reason over retrieved facts to answer queries, not just return raw data (closed 2026-02-10)
- [mmry-jadt.3] Contradiction detection and resolution system for conflicting facts (closed 2026-02-10)
- [mmry-jadt.2] Reasoning-augmented fact extraction - derive inductive/abductive conclusions from observed facts (closed 2026-02-10)
- [mmry-jadt.1] Add inference_type to facts (observed/deduced/induced/abduced) with premise tracking (closed 2026-02-10)
- [mmry-jadt] Memory as Reasoning - treat memory as prediction/reasoning task, not just storage (closed 2026-02-10)
- [mmry-xrbv.2] Confidence decay & maturity tracking - time-decayed feedback scoring (90-day half-life, 4x harmful multiplier) with maturity transitions (candidate/established/proven/deprecated) (closed 2026-02-08)
- [mmry-xrbv.1] Learnings table & data model - store distilled rules/insights separate from raw memories, with category, scope, maturity, and provenance (closed 2026-02-08)
- [mmry-yay2.7] Agent provenance in JSON output - all --json output includes agent name, kind, and meta so consumers know who created each memory/learning (closed 2026-02-08)
- [mmry-yay2.8] MMRY_AGENT and MMRY_AGENT_KIND env vars - allow setting agent identity via environment so wrapper scripts (agntz) don't need to pass flags every time (closed 2026-02-08)
- [mmry-yay2.3] Extend AgentRecord with repo, workspace, session_id fields in agent_meta JSON. Support structured metadata beyond just name/kind. (closed 2026-02-08)
- [mmry-yay2.2] Add agent/agent_kind/agent_meta fields to MCP MemoryAddArgs and gRPC SearchRequest/AddRequest so MCP clients can identify themselves (closed 2026-02-08)
- [mmry-yay2.1] Add --agent, --agent-kind, --agent-meta flags to 'mmry add' CLI with 'human' as default. Agent record is get-or-created automatically. (closed 2026-02-08)
- [mmry-kgen] Clean up agents.rs and operations.rs: remove BridgeBlock, FactRecord, UserProfileEntry types and their DB operations (dead code after HMLR removal) (closed )
