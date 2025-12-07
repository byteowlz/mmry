# HMLR Integration Plan: Human + Agent Memory Management

## Problem Statement

HMLR tables (bridge_blocks, facts, agent_events) exist but are dormant. We need to activate them for both human operators and AI agents WITHOUT breaking existing manual memory workflows.

## Design Principles

1. **Zero Breaking Changes**: Existing `mmry add`, TUI operations, and search continue working exactly as before
2. **Opt-in Enhancement**: HMLR features activate via config flags, not by default
3. **Dual-Path Support**: Both humans and agents can create memories and benefit from HMLR
4. **LLM-Optional**: Core HMLR features work deterministically without any model
5. **Graceful Degradation**: When analyzer disabled, fall back to simpler heuristics

## Architecture: Post-Ingestion Hook Pattern

### Core Idea

Instead of modifying the ingestion path, add a **post-ingestion hook** that enriches memories after they're stored:

```
Memory Created (CLI/TUI/Agent)
    ↓
operations::insert_memory()  ← unchanged
    ↓
[NEW] post_ingestion_hook()  ← HMLR enrichment happens here
    ↓
    ├─→ extract_facts() (if analyzer enabled)
    ├─→ assign_to_bridge_block() (routing logic)
    ├─→ record_agent_event() (audit trail)
    └─→ update_user_profile() (optional)
```

### Benefits

- ✅ Existing code paths unchanged
- ✅ Hook can be disabled via config
- ✅ Works for CLI, TUI, agents, and service
- ✅ Easy to A/B test with/without HMLR
- ✅ Can be async/background for performance

## Implementation Phases

### Phase 1: Post-Ingestion Hook Infrastructure

**Config Addition** (crates/mmry-core/src/config.rs):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmlrConfig {
    /// Enable HMLR enrichment pipeline
    pub enabled: bool,
    
    /// Extract facts from memory content
    pub extract_facts: bool,
    
    /// Assign memories to bridge blocks (conversational spans)
    pub bridge_routing: bool,
    
    /// Log all ingestion events for auditability
    pub audit_trail: bool,
    
    /// Auto-create agent record for human operators
    pub track_human_agent: bool,
    
    /// Human operator agent name (used when track_human_agent=true)
    pub human_agent_name: String,
}

impl Default for HmlrConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in
            extract_facts: true,
            bridge_routing: true,
            audit_trail: true,
            track_human_agent: true,
            human_agent_name: "human".to_string(),
        }
    }
}
```

**Hook Interface** (crates/mmry-core/src/hmlr/mod.rs - NEW):

```rust
use crate::agents::{AgentRecord, AgentEvent, BridgeBlock, FactRecord};
use crate::analysis::Analyzer;
use crate::config::HmlrConfig;
use crate::memory::Memory;
use crate::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use uuid::Uuid;

pub struct HmlrContext {
    /// Who created this memory (human or agent)
    pub creator_id: Uuid,
    
    /// Optional: Previous memories in conversation (for routing)
    pub conversation_history: Vec<Memory>,
    
    /// Optional: Query/prompt that led to this memory
    pub query: Option<String>,
}

pub struct HmlrPipeline {
    config: HmlrConfig,
    analyzer: Arc<dyn Analyzer>,
}

impl HmlrPipeline {
    pub fn new(config: HmlrConfig, analyzer: Arc<dyn Analyzer>) -> Self {
        Self { config, analyzer }
    }
    
    /// Main enrichment hook - called after memory insertion
    pub async fn enrich_memory(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: HmlrContext,
    ) -> Result<EnrichmentResult> {
        if !self.config.enabled {
            return Ok(EnrichmentResult::default());
        }
        
        let mut result = EnrichmentResult::default();
        
        // 1. Extract facts (LLM-optional)
        if self.config.extract_facts {
            result.facts = self.extract_facts(pool, memory).await?;
        }
        
        // 2. Route to bridge block (conversational span assignment)
        if self.config.bridge_routing {
            result.bridge_block = self.route_to_bridge_block(
                pool,
                memory,
                &context,
            ).await?;
        }
        
        // 3. Record audit event
        if self.config.audit_trail {
            result.event = Some(self.record_event(
                pool,
                memory,
                &context,
            ).await?);
        }
        
        Ok(result)
    }
    
    async fn extract_facts(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
    ) -> Result<Vec<FactRecord>> {
        // Use analyzer to extract facts
        let facts = self.analyzer.extract_facts(&memory.content)?;
        
        // Persist to database
        for fact in &facts {
            operations::upsert_fact(pool, fact).await?;
        }
        
        Ok(facts)
    }
    
    async fn route_to_bridge_block(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<BridgeBlock> {
        // Get recent bridge blocks for this agent
        let recent_blocks = operations::get_recent_bridge_blocks_for_agent(
            pool,
            context.creator_id,
            5, // Last 5 blocks
        ).await?;
        
        // Decide: resume existing block or start new one
        let routing = if let Some(query) = &context.query {
            self.analyzer.route(query, &recent_blocks)?
        } else {
            // No query = deterministic fallback: create new block
            AnalyzerRouting::new_topic()
        };
        
        let block = if routing.is_new_topic {
            // Create new bridge block
            self.create_bridge_block(pool, memory, context).await?
        } else if let Some(block_id) = routing.chosen_block {
            // Resume existing block
            self.resume_bridge_block(pool, block_id, memory).await?
        } else {
            // Fallback: create new
            self.create_bridge_block(pool, memory, context).await?
        };
        
        Ok(block)
    }
    
    async fn create_bridge_block(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<BridgeBlock> {
        let mut block = BridgeBlock::new();
        block.agent_id = Some(context.creator_id);
        block.span_id = Some(Uuid::new_v4().to_string());
        block.status = Some("active".to_string());
        
        // Extract keywords from memory (simple heuristic or LLM)
        block.keywords = self.extract_keywords(&memory.content);
        
        // Store memory ID in content JSON
        block.content = serde_json::json!({
            "memory_ids": [memory.id],
            "created_from": context.query.clone().unwrap_or_default(),
        });
        
        operations::upsert_bridge_block(pool, &block).await?;
        Ok(block)
    }
    
    async fn resume_bridge_block(
        &self,
        pool: &SqlitePool,
        block_id: Uuid,
        memory: &Memory,
    ) -> Result<BridgeBlock> {
        // Get existing block
        let mut block = operations::get_bridge_block(pool, block_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Bridge block not found"))?;
        
        // Append memory ID to content
        if let Some(obj) = block.content.as_object_mut() {
            if let Some(ids) = obj.get_mut("memory_ids") {
                if let Some(arr) = ids.as_array_mut() {
                    arr.push(serde_json::json!(memory.id));
                }
            }
        }
        
        operations::upsert_bridge_block(pool, &block).await?;
        Ok(block)
    }
    
    async fn record_event(
        &self,
        pool: &SqlitePool,
        memory: &Memory,
        context: &HmlrContext,
    ) -> Result<AgentEvent> {
        let mut event = AgentEvent::new(context.creator_id, "memory_created");
        event.status = Some("success".to_string());
        event.memory_id = Some(memory.id);
        event.payload = serde_json::json!({
            "memory_type": memory.memory_type,
            "category": memory.category,
            "importance": memory.importance,
        });
        
        operations::record_agent_event(pool, &event).await?;
        Ok(event)
    }
    
    fn extract_keywords(&self, content: &str) -> Vec<String> {
        // Simple deterministic keyword extraction
        // TODO: Use analyzer for smarter extraction
        content
            .split_whitespace()
            .filter(|w| w.len() > 5) // Words longer than 5 chars
            .take(5) // Top 5
            .map(|s| s.to_lowercase())
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct EnrichmentResult {
    pub facts: Vec<FactRecord>,
    pub bridge_block: Option<BridgeBlock>,
    pub event: Option<AgentEvent>,
}
```

**Integration Points**:

1. **CLI (`mmry add`)** - crates/mmry-cli/src/commands/add.rs:235 (after insert_memory):
   ```rust
   // After: operations::insert_memory(db.pool(), &memory).await?;
   
   // NEW: HMLR enrichment
   if config.hmlr.enabled {
       let pipeline = HmlrPipeline::new(config.hmlr.clone(), analyzer);
       let context = HmlrContext {
           creator_id: get_or_create_human_agent(db.pool(), &config).await?,
           conversation_history: vec![],
           query: None, // Manual add = no query
       };
       pipeline.enrich_memory(db.pool(), &memory, context).await?;
   }
   ```

2. **TUI (`a` key)** - crates/mmry-tui/src/app.rs:1253 (after insert_memory):
   ```rust
   // After: operations::insert_memory(self.db.pool(), &new_memory).await?;
   
   // NEW: HMLR enrichment
   if self.config.hmlr.enabled {
       let pipeline = HmlrPipeline::new(
           self.config.hmlr.clone(),
           Arc::new(NoOpAnalyzer), // TUI uses no-op for now
       );
       let context = HmlrContext {
           creator_id: self.get_or_create_human_agent().await?,
           conversation_history: vec![],
           query: None,
       };
       pipeline.enrich_memory(self.db.pool(), &new_memory, context).await?;
   }
   ```

3. **Service/Agent API** - crates/mmry-service/src/server.rs:
   ```rust
   // In agent memory creation endpoint
   let context = HmlrContext {
       creator_id: agent.id,
       conversation_history: get_conversation_history(...),
       query: Some(request.query),
   };
   pipeline.enrich_memory(pool, &memory, context).await?;
   ```

### Phase 2: Human Agent Tracking

**Helper Functions** (crates/mmry-core/src/hmlr/mod.rs):

```rust
/// Get or create the "human" agent record for manual operations
pub async fn get_or_create_human_agent(
    pool: &SqlitePool,
    config: &Config,
) -> Result<Uuid> {
    let agent_name = &config.hmlr.human_agent_name;
    
    // Try to find existing
    if let Some(agent) = operations::get_agent_by_name(pool, agent_name).await? {
        return Ok(agent.id);
    }
    
    // Create new
    let mut agent = AgentRecord::new(agent_name, "human_operator");
    agent.description = Some("Manual memory operations via CLI/TUI".to_string());
    
    operations::upsert_agent(pool, &agent).await?;
    Ok(agent.id)
}
```

This allows tracking human-created memories alongside agent-created ones in a unified audit trail.

### Phase 3: Enhanced Search with HMLR

**Search with Bridge Block Context** (crates/mmry-core/src/search/mod.rs):

```rust
pub struct HmlrSearchOptions {
    /// Include facts in results
    pub include_facts: bool,
    
    /// Group results by bridge blocks
    pub group_by_blocks: bool,
    
    /// Hydration strategy for inactive blocks
    pub inactive_block_strategy: InactiveBlockStrategy,
}

pub enum InactiveBlockStrategy {
    /// Return full memories
    Full,
    /// Return only metadata (summary + keywords)
    MetadataOnly,
    /// Skip inactive blocks entirely
    Skip,
}

pub async fn search_with_hmlr(
    pool: &SqlitePool,
    query: &str,
    agent_id: Uuid,
    options: HmlrSearchOptions,
) -> Result<HmlrSearchResult> {
    // 1. Standard hybrid search
    let memories = search_with_options(pool, query, ...).await?;
    
    // 2. Get relevant facts
    let facts = if options.include_facts {
        operations::search_facts(pool, query, 10).await?
    } else {
        vec![]
    };
    
    // 3. Group by bridge blocks if requested
    let blocks = if options.group_by_blocks {
        group_memories_by_bridge_blocks(pool, &memories, agent_id).await?
    } else {
        vec![]
    };
    
    Ok(HmlrSearchResult {
        memories,
        facts,
        bridge_blocks: blocks,
    })
}
```

### Phase 4: TUI Enhancements

**Automatic Enrichment Display**:

When viewing a memory in the TUI, show HMLR enrichments if available:

```
┌─ Memory Details ─────────────────────────────────┐
│ ID: abc123                                       │
│ Type: Episodic                                   │
│ Content: Met with Sarah to discuss...           │
│                                                  │
│ HMLR Enrichments:                               │
│  Bridge Block: #45 (Project Planning)           │
│  Span: 2025-01-15 to present                    │
│  Facts Extracted:                                │
│   • person = Sarah                               │
│   • project = mmry                               │
│   • date = 2025-01-15                           │
│                                                  │
│  Created by: human (via TUI)                    │
└─────────────────────────────────────────────────┘
```

### Phase 5: Background Synthesis (Future)

Run periodic tasks to:
- Condense long bridge blocks into summaries
- Update fact recency scores
- Close inactive bridge blocks
- Generate topic labels using analyzer

This can run via `mmry service` daemon mode.

## Configuration Example

**config.toml**:

```toml
[hmlr]
# Enable HMLR memory enrichment pipeline
enabled = false  # Opt-in, defaults to false

# Extract structured facts from memories
extract_facts = true

# Assign memories to conversational bridge blocks
bridge_routing = true

# Record audit trail of all memory operations
audit_trail = true

# Track human operator as an agent
track_human_agent = true
human_agent_name = "human"

[analyzer]
# LLM-backed analysis (optional, uses NoOp by default)
enabled = false
provider = "rig"
endpoint = "http://localhost:1234/v1"  # LM Studio
model = "qwen/qwen3-coder-30b"
```

## Migration Path

### For Existing Users

1. **No action required**: HMLR disabled by default
2. **Opt-in**: Set `hmlr.enabled = true` in config
3. **Retroactive enrichment** (optional):
   ```bash
   mmry hmlr backfill --limit 1000
   ```
   This runs the enrichment pipeline on existing memories.

### For New Users

Default config ships with `hmlr.enabled = false` but users can enable in setup wizard.

## Testing Strategy

1. **Unit tests**: Test each enrichment function in isolation
2. **Integration tests**: 
   - CLI add with HMLR enabled/disabled
   - TUI add with HMLR enabled/disabled
   - Agent API with conversation history
3. **Regression tests**: Ensure existing workflows unchanged when HMLR disabled
4. **RAGAS-style tests**: 
   - Multi-turn conversation recall
   - Fact consistency across spans
   - Bridge block routing accuracy

## Performance Considerations

1. **Async enrichment**: Run hook in background to not block CLI/TUI
2. **Batch processing**: When adding multiple memories, batch enrichment
3. **Caching**: Cache human agent ID to avoid repeated lookups
4. **Lazy facts**: Only extract facts for important memories (configurable threshold)

## Open Questions

1. **Bridge block lifecycle**: When to auto-close blocks? Time-based? Inactivity?
2. **Fact conflict resolution**: What if analyzer extracts contradictory facts?
3. **Human vs Agent memories**: Should they have different routing strategies?
4. **Multi-user support**: How to handle multiple human operators?

## Success Metrics

- ✅ Zero breaking changes to existing workflows
- ✅ HMLR enrichment completes in <100ms (excluding LLM calls)
- ✅ Fact extraction accuracy >80% when analyzer enabled
- ✅ Bridge block routing improves conversation recall by >30%
- ✅ Audit trail covers 100% of memory operations when enabled

## Next Steps

1. Add `HmlrConfig` to config.rs
2. Implement `HmlrPipeline` core logic
3. Add database operations for agent lookup
4. Wire hooks into CLI add command
5. Wire hooks into TUI add flow
6. Add tests
7. Update documentation
8. Create example config with HMLR enabled
