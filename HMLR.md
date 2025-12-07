HMLR integration notes
======================

Source reviewed: `https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System` (commit depth 1, CLI entry in `main.py`).

What HMLR does well
- Memory is organized into spans and "Bridge Blocks" stored in SQLite (`daily_ledger`), letting the system resume an active topic verbatim and keep inactive topics as lightweight metadata (see `memory/retrieval/hmlr_hydrator.py`).
- Retrieval is two-stage: hybrid search (vector + keyword) via `LatticeCrawler` → Governor (`memory/retrieval/lattice.py`) that runs three parallel tasks (bridge routing, retrieval filter, fact-store lookup) and decides whether to resume an existing block or start a new one.
- Facts and user profile are maintained separately from free-form turns (`FactScrubber`, `Scribe`); fact lookups are always merged into context.
- Context assembly (Hydrator) enforces a token budget and keeps entire bridge blocks intact to guarantee recall/faithfulness rather than precision trimming.
- Background synthesis/rehydration and eviction managers reduce bloat while keeping long-horizon coherence.

Fit for mmry (Rust, local-first)
- mmry already has rich hybrid search over a SQLite store; HMLR’s value is the routing/governance layer and bridge-block structure for conversational continuity and policy invariants.
- Most components are LLM-driven; make LLM usage optional so the core remains purely local/deterministic, and let agents/tooling call into mmry rather than embedding LLMs into the core binary.
- Bridge Blocks map well to mmry’s existing concept of spans/turns if we add a small ledger table and structured metadata.
- Fact store aligns with mmry’s semantic/procedural tagging; it gives a deterministic path for key-value retrieval without full-context hydration.

Architecture recommendation (LLM optional)
- Keep mmry core fast and LLM-agnostic: all CRUD/search/ranking must work without any model. Add structured agent provenance and fact/profile tables but keep behavior unchanged when no LLM is configured.
- Ship an optional sidecar (e.g., `mmry-agents`) that reuses mmry’s service/store and hosts HMLR-style routing/hydration with local-first models. This avoids pulling heavy deps into the core CLI and keeps startup lightweight.
- Provide a pluggable “analyzer” interface: default no-op, plus an LLM-backed implementation (local model first, remote only if configured). Agents can also just call mmry APIs without the sidecar.
- Use Rig for the sidecar LLM stack to get first-class local model support (Llama.cpp/gguf, GPU when available) with optional remote fallbacks; keep prompts deterministic and auditable.
- LM Studio / local testing path: Rig completion client now accepts an empty/missing API key and only needs an OpenAI-compatible endpoint (e.g., `http://localhost:1234/v1` with `qwen/qwen3-coder-30b`). Analyzer routing calls this directly; when disabled or not configured, the pipeline falls back to deterministic no-op.

Recommended implementation outline (phased)
1) Data model and ingestion
- Add a `bridge_blocks` (or `daily_ledger`) table in mmry’s SQLite schema: `block_id`, `span_id`, `topic_label`, `keywords`, `status`, `created_at`, `exit_reason`, `content_json` (bridge metadata and turn ids). Avoid a parallel “v2” path; evolve existing schema.
- Add tables for agent provenance (`agents`, `agent_events`/`ingestion_jobs`) and deterministic facts/profile storage (`facts`, `user_profile`) so agent-added memories are auditable even without LLMs.
- Extend ingestion to emit both free-form turns and extracted facts. Start with a lightweight Rust fact-extraction prompt via the optional analyzer; when no model is configured, leave facts empty but keep the pipeline working.

2) Retrieval and governance
- Expose a “Governor” pipeline via the optional sidecar: initial candidate search uses mmry’s hybrid retrieval; then an LLM filter scores candidates against query + user profile + fact store results. Return `{routing_decision, filtered_ids, facts}` to callers.
- Implement bridge routing: given query + recent spans, decide `resume span X` vs `new span` and write a bridge block entry. Resume spans hydrate verbatim; inactive spans contribute only metadata (summary + keywords).
- Hydrator: given approved IDs + bridge block list, assemble context under a token budget. Active block => full turns; inactive => metadata placeholders; always append fact hits and user-profile constraints. When no sidecar/LLM is present, hydrate spans deterministically using existing search results only.
- Analyzer integration status: Rig-backed analyzer now wired into `mmry-service` with config-driven `endpoint`/`model`; default is no-op when disabled. Local model smoke test added (env-gated) against `http://localhost:1234/v1` to verify LM Studio path.

3) Lifecycle, synthesis, and evaluation
- Add background synthesis that periodically condenses long spans into bridge-block summaries and updates fact recency. Use mmry’s service mode to schedule this without blocking CLI calls.
- Eviction/rehydration: demote low-usage spans to summaries; rehydrate when the Governor selects them again.
- Port a minimal subset of HMLR’s RAGAS-style tests into mmry’s test suite (Rust integration tests invoking the CLI/service) to validate temporal ordering, cross-topic invariants, and multi-hop retrieval with and without the sidecar enabled.
- Integrate benchmark automation: mirror the HMLR test corpus (RAGAS-style) and wire a reproducible harness that runs locally against mmry+sidecar and mmry-alone modes, reporting faithfulness/recall/precision deltas per change.
- Benchmarks integration plan: track HMLR/RAGAS corpus runs in CI and during dev; add harness that runs both with and without the analyzer enabled to ensure LLM-optional behavior still passes recall/faithfulness targets. Use bd issues to schedule implementation (see mmry-yr5, mmry-d8m).

Implementation notes
- Keep precedence rules consistent with mmry config: flags > env > config file. No emojis in output/README.
- Favor the existing mmry service as the control plane entry: add commands like `mmry service route --query ...` or a Rust API that returns `{routing_decision, contexts, facts}`, and let the sidecar call those APIs.
- Start with deterministic, auditable prompts and log Governor decisions for debugging; store decisions with the bridge block for replayability.
- Focus on recall/faithfulness first (hydrate full active blocks) and layer precision optimizations later (rerank or trim inactive metadata under tight budgets).
