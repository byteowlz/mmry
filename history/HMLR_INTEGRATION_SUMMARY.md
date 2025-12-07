# HMLR Integration: Quick Summary

## The Problem

HMLR tables exist but are dormant. Manual memory creation (CLI `mmry add`, TUI `a` key) doesn't trigger:
- Fact extraction
- Bridge block assignment (conversational spans)
- Agent event logging (audit trail)

## The Solution: Post-Ingestion Hook Pattern

Add enrichment **after** memory insertion, keeping existing code unchanged:

```
Memory Created → insert_memory() → [NEW] post_ingestion_hook() → HMLR enrichment
```

## Key Design Decisions

1. **Opt-in via config**: `hmlr.enabled = false` by default
2. **Works for everyone**: CLI, TUI, agents, and service APIs
3. **Tracks humans too**: Human operators get an agent record ("human")
4. **LLM-optional**: Falls back to heuristics when analyzer disabled
5. **Zero breaking changes**: Existing workflows work exactly as before

## What Gets Added

### New Config Section
```toml
[hmlr]
enabled = false              # Opt-in
extract_facts = true         # Extract structured facts
bridge_routing = true        # Assign to conversational spans
audit_trail = true          # Log all operations
track_human_agent = true    # Track manual operations
human_agent_name = "human"  # Name for human operator agent
```

### Post-Ingestion Enrichment
When a memory is created, the hook:

1. **Extracts facts** (via Analyzer, LLM-optional)
   - Example: "Met with Sarah" → `{person: Sarah, event: meeting}`
   - Stored in `facts` table for deterministic retrieval

2. **Routes to bridge block** (conversational span)
   - Decides: resume existing conversation or start new one
   - Groups related memories into spans
   - Stored in `bridge_blocks` table

3. **Records event** (audit trail)
   - Logs who created memory (human or agent)
   - Tracks success/failure and metadata
   - Stored in `agent_events` table

### Integration Points

**CLI** (after `operations::insert_memory()`):
```rust
if config.hmlr.enabled {
    let context = HmlrContext {
        creator_id: get_or_create_human_agent(db).await?,
        query: None, // manual add
        conversation_history: vec![],
    };
    hmlr_pipeline.enrich_memory(db, &memory, context).await?;
}
```

**TUI** (same pattern after memory insertion)

**Service/Agent API** (include conversation context for routing)

## Benefits

### For Human Operators
- ✅ Automatic fact extraction from notes
- ✅ Memories grouped into conversational topics
- ✅ Audit trail of all operations
- ✅ Better search with fact lookup
- ✅ **No workflow changes required**

### For AI Agents
- ✅ Unified memory system with humans
- ✅ Conversational continuity via bridge blocks
- ✅ Structured fact retrieval
- ✅ Provenance tracking
- ✅ Context-aware routing

### For Developers
- ✅ Zero breaking changes
- ✅ Easy to disable/test
- ✅ Works with existing search
- ✅ Gradual rollout possible
- ✅ LLM usage remains optional

## Example: Manual Memory with HMLR

**Before** (HMLR disabled):
```bash
$ mmry add "Met with Sarah about mmry project"
✓ Added memory: abc123
  Type: Episodic
  Content: Met with Sarah about mmry project
```

**After** (HMLR enabled):
```bash
$ mmry add "Met with Sarah about mmry project"
✓ Added memory: abc123
  Type: Episodic
  Content: Met with Sarah about mmry project
  Facts extracted: 2 (person: Sarah, project: mmry)
  Bridge block: #45 (active conversation)
  Logged by: human
```

## Search Enhancement

**Standard search**:
```bash
$ mmry search "Sarah"
→ Returns matching memories
```

**HMLR-enhanced search**:
```bash
$ mmry search "Sarah" --hmlr
→ Returns matching memories
→ Plus relevant facts: {person: Sarah, role: teammate}
→ Grouped by bridge blocks (conversations)
→ Includes inactive block summaries
```

## Rollout Strategy

### Phase 1: Infrastructure (Week 1)
- Add `HmlrConfig` to config
- Implement `HmlrPipeline` core
- Add hook to CLI `mmry add`
- Basic tests

### Phase 2: TUI Integration (Week 2)
- Add hook to TUI `a` key
- Display HMLR enrichments in detail pane
- Human agent tracking

### Phase 3: Search Enhancement (Week 3)
- Fact-augmented search
- Bridge block grouping
- Context hydration strategies

### Phase 4: Agent API (Week 4)
- Conversation-aware routing
- Agent memory creation
- Multi-turn context

### Phase 5: Background Ops (Future)
- Periodic synthesis
- Block summarization
- Fact recency updates

## Migration

**Existing users**: No action required (disabled by default)

**Enable HMLR**: 
```bash
# Edit config.toml
[hmlr]
enabled = true

# Optionally backfill existing memories
mmry hmlr backfill --limit 1000
```

**New users**: Defaults to disabled, can enable in setup

## Testing

- ✅ Regression: HMLR disabled = existing behavior
- ✅ Integration: HMLR enabled for CLI/TUI/Agent
- ✅ Performance: Enrichment <100ms (excluding LLM)
- ✅ Accuracy: Fact extraction >80% when analyzer enabled

## Questions?

See full plan: `history/HMLR_INTEGRATION_PLAN.md`
