# Discord Agent (cloudflare/awesome-agents) — features to consider for mmry

Source reviewed: `https://github.com/cloudflare/awesome-agents/tree/main/agents/discord-agent`

This note extracts the patterns that feel broadly useful (agent memory + tooling + introspection) and maps them onto mmry’s current shape (local-first memory store + service + HMLR).

## What the Discord agent actually does (useful patterns)

### “Memory blocks” as always-in-context state
- Maintains a small set of persistent, labeled blocks (notably `persona` and `human`) with:
  - `label`, `description`, `value`, `limit`, `lastUpdated`
  - rendering into every system prompt as structured XML-ish text (`renderMemory`)
- Provides explicit editing tools (`memoryInsert`, `memoryReplace`) so the agent can update blocks precisely, producing a tight self-improving loop.

### Rolling context window (summarize + prune)
- Stores full message history in SQLite, but only keeps a fixed-size “active buffer” of message IDs for the LLM.
- When the buffer exceeds a threshold (`MAX_MESSAGES`), it:
  - summarizes the oldest chunk via an LLM call
  - inserts the summary as a synthetic message
  - prunes the old messages from the active buffer while keeping them in storage

### MCP as dynamic tool surface
- Merges a local tool set with tools discovered from connected MCP servers.
- Exposes simple APIs to list/add/remove MCP servers; a small dashboard makes this operable.

### Introspection UI (dashboard)
- A lightweight web console shows:
  - agent identity info
  - persistent memory blocks
  - current context window (buffer)
  - full message history
  - MCP servers + discovered tools
- Also includes an operational control (“Start Gateway”).

## What mmry already has (relevant overlap)

- Persistent storage, rich retrieval/search, and JSON-first CLI output.
- HMLR primitives that look a lot like “agent memory” building blocks:
  - `user_profiles` table (JSON blob) updated by `Scribe`
  - `agent_events` audit trail
  - `bridge_blocks` (topic grouping) including `open_loops` + `decisions_made`
  - facts (`FactRecord`) with a `Secret` category
- A local HTTP service already exposes agent-oriented endpoints:
  - `/v1/agents/route` (returns contexts + bridge blocks + facts + routing decision)
  - `/v1/agents/memories` (create memory; can run HMLR enrichment)
  - `/v1/agents/enrich` (enrich existing memory)

## Gaps vs the Discord agent (opportunities)

- `mmry-mcp` exists but is a placeholder (no real MCP server yet).
- User profile exists internally but isn’t directly inspectable/editable as a first-class concept (CLI/TUI/API).
- No standard “prompt/context pack” output that assembles *exactly what an agent should put in-context* (with size budgeting and redaction).
- No built-in rolling-window summarization helper for conversation context (agents must implement this themselves).
- No lightweight “agent console” view for the service endpoints (TUI is great for memories; less focused on live agent context + profiling).

## Features worth considering for mmry

### 1) Implement `mmry-mcp` as the primary agent integration surface
The Discord agent’s biggest leverage point is “toolability”: once it has reliable tools, the rest of the agent becomes simpler.

For mmry, an MCP server could expose a compact, stable set of operations:
- Search + fetch-by-id for retrieval (with strict, stable JSON schemas).
- Add/update/delete memories (with optional HMLR enrichment controls).
- Read-only introspection: recent bridge blocks, facts, agent events, and user profile.

This makes mmry usable from *any* MCP-capable agent host without coupling to the mmry service HTTP API.

### 2) Promote “user profile” into explicit, editable “blocks”
mmry already has `user_profiles` (JSON) but it’s implicit and shaped by `Scribe`. The Discord agent shows the benefit of:
- a small number of named blocks with limits
- clear rendering semantics (“always inject these”)
- precise edit tools (insert/replace) rather than “rewrite the whole thing”

Concretely: model `persona` + `human` (and optionally a few more) as explicit block entries and support:
- deterministic rendering for prompts/context packs
- safe patch-like edits from agents (with audit events)
- optional redaction policies for anything categorized as secrets

### 3) Add a first-class “context pack” builder
mmry already computes many of the ingredients (contexts, bridge blocks, facts, profile). What’s missing is a single, well-defined output that answers:

“Given a query + optional span/session, what should I put in the LLM context right now?”

Useful properties (borrowed from the Discord agent approach):
- explicit budgets (max chars/tokens per section)
- deterministic structure (so prompts don’t drift)
- optional summary generation or bridge-block synthesis when the pack exceeds budget
- redaction mode for `FactCategory::Secret`

This can exist as:
- an HTTP endpoint on the service (`/v1/agents/context`), and/or
- an MCP prompt/resource so agent hosts can import it directly.

### 4) Provide a rolling-context summarization utility (optional)
mmry doesn’t need to become a chat runtime, but it can make agent hosts easier by offering:
- a “summarize and prune” helper that turns long conversation history into a short summary + recent turns
- a storage format for the summary (memory + bridge block decision/open loop + agent event)

Even a simple “summarize this list of turns into <= N words” endpoint aligns with the Discord agent’s practical approach.

### 5) Lightweight “agent console” for service mode (local-only)
The Discord dashboard isn’t fancy; it’s effective. For mmry-service, a small local-only UI could surface:
- recent `/v1/agents/route` results (contexts chosen, block routing decision)
- agent events timeline
- current user profile blocks
- bridge blocks + open loops/decisions
- basic service health and config flags (HMLR enabled, analyzer enabled)

This could be strictly optional and bound to localhost, fitting mmry’s local-first stance.

## Patterns to copy selectively (if/when relevant)

- Explicit “operations with limits” (e.g., 2k Discord message limit → in mmry’s case: context/prompt budgets).
- Strong separation of “full history” vs “active context” (mmry already does retrieval; this is about packaging).
- Auditability: each agent action that mutates memory emits a structured event (mmry already has `agent_events`; extend that pattern to profile/block edits once they exist).

## Patterns not worth importing directly

- Discord-specific gateway management and Durable Object persistence model (Cloudflare-specific).
- Storing chat transcripts inside mmry just because an agent runtime does; mmry’s value is retrieval + enrichment, not being the chat DB.
