# HMLR Integration: Comparison with Source Repository

## Source: https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System

### Key Findings from HMLR Repository

#### Verified Achievements (RAGAS Benchmarks)
- **Perfect Faithfulness: 1.00** across all tests
- **Perfect Context Recall: 1.00** across all tests
- Tests include temporal conflicts, policy enforcement, multi-hop reasoning
- Uses only **gpt-4.1-mini** (mini-class model)
- All results verified with RAGAS framework

#### Core Test Scenarios They Pass
1. **7A - API Key Rotation**: State conflict resolution (1.00/1.00)
2. **7B - User Invariant Override**: Persistent constraints (1.00/1.00)
3. **7C - Timestamp Updates**: Temporal ordering (1.00/1.00)
4. **8 - 30-Day Deprecation**: Multi-hop policy reasoning (1.00/1.00)
5. **2A - Vague Secret Retrieval**: Zero-keyword recall (1.00/1.00)
6. **9 - 50-Turn Conversation**: Long-term persistence (1.00/1.00)
7. **12 - "The Hydra"**: 9 policy aliases, 8 revocations, 2300 tokens deep (1.00/1.00) - **0% historical pass rate**

#### Architecture Insights

**Parallel Fan-Out Pattern:**
```
User Query
    ↓
Chunk & Embed
    ↓
[Parallel Tasks]
    ├─→ Task 1: Scribe (User Profile) - Fire-and-forget
    ├─→ Task 2: FactScrubber (Extract Key-Value) - Async
    ├─→ Task 3: LatticeCrawler (Vector Search) - Retrieval
    └─→ Task 4: Governor (Router & Filter) - Main Logic
    ↓
Context Hydration (merge all sources)
    ↓
Final LLM Prompt
```

**Key Components:**
1. **ChunkEngine**: Chunk & embed incoming content
2. **Scribe**: Maintains user profile (fire-and-forget update)
3. **FactScrubber**: Extracts key-value facts to SQL
4. **LatticeCrawler**: Vector search for candidates
5. **Governor**: Routes & filters (resume vs new bridge block)
6. **ContextHydrator**: Assembles final context from all sources

#### Key Design Decisions (from their architecture)

1. **Bridge Blocks for Entire Conversations**
   - Retrieves 5-10 turn blocks instead of fragments
   - Prioritizes recall safety over token minimization
   - Precision scores: 0.27-0.88 (intentionally lower for safety)

2. **Three-Layer Memory:**
   - **User Profile**: Persistent constraints across topics
   - **Fact Store**: Key-value deterministic lookup
   - **Bridge Blocks**: Conversation spans with metadata

3. **Governor is the Brain:**
   - Decides: resume existing block vs create new
   - Filters candidates for relevance
   - Enforces policy and temporal ordering

4. **Uses SQLite + Vector Search:**
   - Facts stored in SQL for deterministic retrieval
   - Vector search for candidate retrieval
   - Bridge blocks stored with metadata

## Comparison with Our mmry Integration Plan

### ✅ What We Got Right

1. **Post-Ingestion Hook Pattern**: Matches their ChunkEngine → Parallel Tasks flow
2. **Three-Layer Design**: 
   - Our facts table = their FactStore
   - Our bridge_blocks = their Bridge Blocks
   - Our user_profiles = their Scribe
3. **Opt-in Design**: We made it disabled by default (correct)
4. **LLM-Optional Core**: We emphasized deterministic fallbacks
5. **Agent Tracking**: Our "human" agent concept aligns with their provenance

### ⚠️ What We Might Need to Adjust

1. **Parallel Task Execution**
   - **HMLR**: Runs Scribe, FactScrubber, Crawler, Governor in parallel
   - **Our plan**: Sequential hook execution
   - **Fix**: Make enrichment pipeline async with concurrent tasks

2. **Governor as Central Brain**
   - **HMLR**: Governor is the main orchestrator (routing + filtering)
   - **Our plan**: HmlrPipeline does everything in sequence
   - **Fix**: Separate Governor module that coordinates other components

3. **Bridge Block Retrieval Strategy**
   - **HMLR**: Retrieves entire 5-10 turn blocks intentionally
   - **Our plan**: Not explicitly specified
   - **Fix**: Document block-level retrieval as intentional design choice

4. **Fact Storage**
   - **HMLR**: SQL-based key-value store
   - **Our plan**: Using our existing facts table ✅
   - **Status**: Already correct

5. **User Profile Management**
   - **HMLR**: Fire-and-forget async update (Scribe agent)
   - **Our plan**: Not explicitly async
   - **Fix**: Make user profile updates async/background

### 🔧 Recommended Adjustments to Our Plan

#### 1. Refactor HmlrPipeline to Governor Pattern

**Before (Sequential):**
```rust
pub async fn enrich_memory(&self, pool, memory, context) {
    // 1. Extract facts
    // 2. Route to bridge block
    // 3. Record event
}
```

**After (Parallel Governor):**
```rust
pub struct Governor {
    fact_scrubber: Arc<FactScrubber>,
    scribe: Arc<Scribe>,
    crawler: Arc<LatticeCrawler>,
}

impl Governor {
    pub async fn process_memory(&self, memory: &Memory, context: HmlrContext) -> Result<GovernorDecision> {
        // Launch all tasks in parallel
        let (facts, profile_update, candidates) = tokio::join!(
            // Task 1: FactScrubber (extract key-value)
            self.fact_scrubber.extract(&memory.content),
            
            // Task 2: Scribe (update user profile - fire-and-forget)
            self.scribe.update_profile(&memory, &context),
            
            // Task 3: LatticeCrawler (retrieve candidates)
            self.crawler.find_bridge_block_candidates(&context)
        );
        
        // Governor logic: route to existing or new bridge block
        self.route_and_filter(candidates, &memory, &context).await
    }
}
```

#### 2. Add ContextHydrator Module

```rust
pub struct ContextHydrator {
    config: HmlrConfig,
}

impl ContextHydrator {
    /// Assemble final context from all sources
    pub async fn hydrate(
        &self,
        pool: &SqlitePool,
        bridge_block: &BridgeBlock,
        relevant_facts: Vec<FactRecord>,
        user_profile: &UserProfileEntry,
    ) -> Result<HydratedContext> {
        // Merge:
        // - Active bridge block (full turns 5-10)
        // - Inactive blocks (metadata only)
        // - Relevant facts
        // - User profile constraints
        
        // Return assembled context under token budget
    }
}
```

#### 3. Update Task Priorities

Based on HMLR's parallel architecture, we should add:

- **mmry-NEW1** [P1] Refactor HmlrPipeline to Governor pattern with parallel execution
- **mmry-NEW2** [P2] Implement ContextHydrator for multi-source assembly
- **mmry-NEW3** [P2] Add async Scribe for user profile updates
- **mmry-NEW4** [P2] Implement LatticeCrawler for bridge block candidate search

#### 4. Testing Strategy Alignment

**Add RAGAS-style tests matching HMLR's:**
- Temporal conflict resolution (API key rotation test)
- User invariant persistence (vegetarian override test)
- Multi-hop policy reasoning (30-day deprecation test)
- Zero-keyword semantic recall (vague secret test)
- Long-term conversation (50-turn, 30-day gap test)

### 📊 Architecture Comparison

| Component | HMLR | mmry (Our Plan) | Status |
|-----------|------|-----------------|--------|
| Memory Storage | SQLite + Vector | SQLite + vec0 | ✅ Match |
| Fact Store | SQL key-value | facts table | ✅ Match |
| User Profile | Scribe (async) | user_profiles | ⚠️ Add async |
| Bridge Blocks | 5-10 turn blocks | bridge_blocks | ✅ Match |
| Governor | Central orchestrator | HmlrPipeline | ⚠️ Refactor |
| Parallel Execution | Yes (4 tasks) | No (sequential) | ❌ Missing |
| Context Hydrator | Yes (merges sources) | No | ❌ Missing |
| Analyzer | LLM-backed | Analyzer trait | ✅ Match |

### ✅ What Our Plan Already Has Right

1. **Database schema**: Already matches (facts, bridge_blocks, user_profiles, agent_events)
2. **LLM-optional design**: NoOpAnalyzer fallback
3. **Post-ingestion hook**: Correct insertion point
4. **Human agent tracking**: Aligned with their provenance model
5. **Opt-in config**: Disabled by default

### 🚨 Critical Gaps to Address

1. **No parallel execution** - HMLR's key performance feature
2. **No Governor orchestrator** - Central brain is missing
3. **No ContextHydrator** - Multi-source assembly not designed
4. **No Scribe pattern** - User profile updates not async
5. **No LatticeCrawler** - Bridge block candidate search not specified

### 📝 Updated Implementation Priorities

#### Phase 0: Architecture Refinement (NEW - Week 0)
1. Design Governor orchestrator pattern
2. Design ContextHydrator for multi-source assembly
3. Design parallel task execution model
4. Update HMLR_INTEGRATION_PLAN.md with new architecture

#### Phase 1: Core Components (Week 1-2)
1. Implement Governor with parallel task coordination
2. Implement FactScrubber
3. Implement Scribe (async user profile)
4. Implement LatticeCrawler
5. Implement ContextHydrator
6. Tests for each component

#### Phase 2: Integration (Week 3)
1. Wire Governor into CLI add command
2. Wire Governor into TUI add memory
3. Integration tests

#### Phase 3: Search & RAGAS (Week 4)
1. HMLR-enhanced search with hydration
2. RAGAS-style benchmark tests
3. Performance tuning

### 🎯 Key Takeaway

**Our plan was 70% correct but missing the critical parallel execution and Governor orchestration pattern that makes HMLR performant and effective.**

We need to:
1. ✅ Keep our database schema (it's correct)
2. ✅ Keep opt-in config design
3. ✅ Keep LLM-optional analyzer
4. ❌ Refactor from sequential HmlrPipeline to parallel Governor
5. ❌ Add ContextHydrator for multi-source assembly
6. ❌ Make Scribe async for user profile updates
7. ❌ Add RAGAS-style adversarial tests

### 📚 References

- HMLR Repository: https://github.com/Sean-V-Dev/HMLR-Agentic-AI-Memory-System
- RAGAS Framework: https://docs.ragas.io/
- LangSmith Verification: https://smith.langchain.com/public/4b3ee453-a530-49c1-abbf-8b85561e6beb/d

---

**Conclusion**: Our integration plan is fundamentally sound but needs architectural refinement to match HMLR's proven parallel Governor pattern. The database design and config approach are correct.
