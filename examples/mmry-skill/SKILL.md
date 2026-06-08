---
name: mmry
description: Use mmry as a persistent agent memory layer. Search before answering on a recurring topic, write memories for facts you'll need again, and close the loop with `--using <ids>` so the cited memories rise in future searches. Local-first, no LLM calls inside the store.
---

# mmry — agent memory skill

mmry is a local-first memory store with hybrid retrieval (BM25 + dense + sparse + optional rerank) and an episode-based feedback loop. The CLI is the agent surface.

## When to reach for it

- **Search first.** Before working on something the user has touched before, run `mmry search "<topic>"`. The store is dumb; you are the brain.
- **Write durable facts.** Anything the user states as durable preference, project context, or a non-obvious decision goes in. Skip ephemeral state.
- **Close the loop.** When you actually used a memory in your reply, cite it with `mmry add --using <id> ...`. The retrieval gateway boosts memories that get cited.

Do **not** use mmry for: chat history, scratch state, or anything you can re-derive from the codebase or `git log`.

## Core commands

```bash
# Search — JSON output is the agent contract
mmry search "feedback loop" --json --limit 5

# Search filters
mmry search "auth" --json --tag security --type semantic --min-importance 7

# Add a memory
mmry add "User prefers terse PRs with no trailing summaries." \
  --type semantic --tags style,prefs --importance 8

# Add via JSON (batch supported — pass an array)
echo '{"content":"Episode store ledger lives in episodes table","type":"semantic","tags":["mmry","arch"]}' \
  | mmry add -

# Close the feedback loop: cite the memories that informed your answer
mmry add "Wired the prior into hybrid_score" \
  --type episodic --using <memory-id> [--using <another-id>]

# Or close an explicit episode (when --using shouldn't pick the latest)
mmry add "..." --using <id> --episode <episode-uuid>
```

The default behaviour of `--using <ids>` is to retroactively close the most recent open search episode for the current agent session (within a 30-minute window). That increments `helpful_count` on the cited memories, which the search scorer reads as `feedback_weight * log1p(max(0, helpful - harmful))`. Default `feedback_weight = 0.1`.

## Search JSON shape

```json
{
  "memories": [
    {
      "id": "...",
      "content": "...",
      "memory_type": "semantic",
      "category": "default",
      "tags": ["..."],
      "importance": 7,
      "created_at": "...",
      "updated_at": "...",
      "score": 0.84
    }
  ]
}
```

Pass `--full` to include embeddings and internal fields. Otherwise stick to the standard projection above.

## Stores

Each store is a separate SQLite file under `~/.local/share/mmry/stores/`. Use `-s <name>` (global flag) to target a non-default store. List with `mmry stores list`. Stores isolate domains; episodes do not cross store boundaries.

## AGENT_CTX env contract

mmry reads `AGENT_CTX_*` env vars (defensively — missing/malformed is fine) so episodes and memories carry session/workspace/harness identity:

- `AGENT_CTX_WORKSPACE_ID` — stable id for the working tree / project
- `AGENT_CTX_PLATFORM_SESSION_ID` — outer platform's session id
- `AGENT_CTX_HARNESS_SESSION_ID` — harness/runtime session id
- `AGENT_CTX_VERSION`, `AGENT_CTX_PLATFORM_NAME`, `AGENT_CTX_HARNESS`, `AGENT_CTX_RUN_MODE`, `AGENT_CTX_MODEL`, `AGENT_CTX_REQUEST_ID`, ...

Episodes are scoped by `(workspace_id, platform_session_id, harness_session_id)`, so a fresh `--using` from a different session won't accidentally close an unrelated open episode. Filter searches by these via `--workspace-id`, `--platform-session-id`, `--harness-session-id`.

## Scoring knobs (in `[search]`)

- `mode` — `hybrid` (default), `keyword`, `fuzzy`, `semantic`, `bm25`, `sparse`
- `keyword_weight`, `fuzzy_weight`, `vector_weight`, `bm25_weight`, `sparse_embedding_weight`
- `recency_weight`, `boost_recent`
- `importance_weight`
- `feedback_weight` — episode-feedback prior (set `0.0` to disable the loop)
- `rerank_enabled`, `rerank_top_k`, `rerank_model`

Storage stays dumb. All scoring is a pure function of fields read off the row.

## Anti-patterns

- Don't mirror the conversation into mmry. Save *facts that will outlive this turn*.
- Don't over-tag. Tags are filters, not classification fluff.
- Don't `--using` a memory you didn't actually use. The signal collapses if you cite indiscriminately.
- Don't reach for the MCP server unless the human asked for it. The CLI is the canonical agent surface.
