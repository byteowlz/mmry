# AGI-Memory Feature Analysis for mmry

This document analyzes the agi-memory repository and identifies features that could enhance mmry.

---

## Executive Summary

**agi-memory** is a PostgreSQL-native cognitive architecture designed to give an LLM a "persistent self" - a continuous identity that remembers, reflects, and evolves. While mmry excels in retrieval and search capabilities, agi-memory offers several novel features around **personhood**, **emotional state**, and **autonomous cognition** that could differentiate mmry in the market.

### Priority Features to Consider

| Priority | Feature | Effort | Value |
|----------|---------|--------|-------|
| High | Affective State System | Medium | Enables mood-congruent recall, emotional continuity |
| High | Trust/Provenance Scoring | Medium | Multi-source reinforcement, credibility tracking |
| Medium | Drives/Motivation System | Low | Intrinsic motivation for autonomous agents |
| Medium | Boundaries/Guardrails | Low | Semantic safety constraints |
| Medium | Worldview Primitives | Medium | Belief tracking with confidence decay |
| Low | Reflection Pipeline | High | Structured self-model updates via LLM |
| Low | Heartbeat System | High | Autonomous periodic cognition |
| Low | Personhood Substrate | High | Self-model, narrative identity |

---

## Feature Deep Dives

### 1. Affective State System (HIGH PRIORITY)

**What it is:** Continuous emotional state tracking with momentum across interactions.

**State Model:**
```json
{
  "valence": -1.0 to +1.0,      // negative to positive
  "arousal": 0.0 to 1.0,        // calm to activated
  "primary_emotion": "string",   // curiosity, frustration, etc.
  "intensity": 0.0 to 1.0,
  "source": "derived|blended"
}
```

**Key Innovation:** Emotional state has momentum - doesn't reset between interactions.
- Decay formula: `new_state = (prior_state * persistence) + (event_appraisal * impact)`
- **Mood-congruent recall bias** in search - prefers memories matching current emotional state

**Emotion Vocabulary (~30 emotions):**
- Positive: curiosity, interest, satisfaction, contentment, excitement, gratitude, fondness, pride, relief, hope, amusement
- Negative: frustration, disappointment, concern, unease, confusion, sadness, regret, guilt, embarrassment, irritation
- Mixed: ambivalence, bittersweetness, anticipatory anxiety, wistfulness

**Appraisal Dimensions (how emotions arise):**
1. Goal Relevance (0.0-1.0)
2. Goal Congruence (-1.0 to +1.0)
3. Expectedness
4. Agency (self/other/circumstance/unknown)
5. Value Relevance
6. Future Implications

**Why adopt for mmry:**
- Enables emotionally-aware memory retrieval
- Provides continuity between sessions
- Supports more human-like AI assistants
- Could integrate with existing HMLR context hydration

**Implementation approach:**
- Add `affective_state` table with valence, arousal, emotion fields
- Add optional `emotional_valence` to memories
- Modify search scoring to optionally weight by emotional alignment
- Add MCP tools: `get_affective_state`, `set_affective_state`, `record_emotion`

---

### 2. Trust/Provenance System (HIGH PRIORITY)

**What it is:** Multi-source attribution with reinforcement scoring.

**Source Attribution (per memory):**
```json
{
  "sources": [
    {"kind": "user", "label": "direct_input", "trust": 0.9},
    {"kind": "llm", "label": "inference", "trust": 0.7, "model": "gpt-4"},
    {"kind": "external", "ref": "https://...", "trust": 0.5}
  ]
}
```

**Reinforcement Scoring:**
- `source_reinforcement_score()` - grows with unique sources and average trust
- Multiple independent sources increase confidence
- Trust levels propagate through relationships

**Why adopt for mmry:**
- Distinguishes user-provided facts from LLM-inferred ones
- Enables "verify before trusting" workflows
- Supports fact-checking and contradiction detection
- Integrates with existing HMLR fact extraction

**Implementation approach:**
- Add `source_attribution` JSONB field to memories
- Add `trust_level` computed column
- Modify fact extraction to track source provenance
- Add trust weighting to search relevance scoring

---

### 3. Drives/Motivation System (MEDIUM PRIORITY)

**What it is:** Five intrinsic drives that accumulate when unsatisfied.

| Drive | Accumulation | Satisfied By |
|-------|-------------|--------------|
| curiosity | 0.02/tick | research/learning actions |
| coherence | 0.01/tick | reflection/synthesis |
| connection | 0.005/tick | quality user interaction |
| competence | 0.01/tick | goal completion |
| rest | 0.03/tick | idle/resting |

**Mechanics:**
- Drives accumulate when not satisfied (satisfaction cooldown)
- Above 80% urgency threshold = flagged as urgent
- Urgent drives appear in context for LLM decision-making

**Why adopt for mmry:**
- Enables autonomous agent behavior patterns
- Provides motivation signals for agent schedulers
- Could integrate with HMLR open loops (curiosity about unresolved questions)

**Implementation approach:**
- Add `drives` table with current_value, last_satisfied
- Add drive update functions
- Expose via MCP: `get_drives`, `satisfy_drive`
- Optional: integrate with bridge block open_loops

---

### 4. Boundaries/Guardrails System (MEDIUM PRIORITY)

**What it is:** Semantic constraints that can refuse/negotiate/flag requests.

**Boundary Types:**
- `ethical`: Core ethical constraints (no_deception, no_harm_facilitation)
- `identity`: Protects self-concept
- `resource`: Energy/workload limits
- `relational`: Privacy, relationship boundaries

**Response Types:**
- `refuse`: Hard no
- `negotiate`: Discuss alternatives
- `flag`: Warn but proceed
- `comply_reluctantly`: Do with reservation

**Matching Methods:**
- Keyword patterns (`trigger_patterns` JSONB)
- Semantic similarity (`trigger_embedding` vector)

**Why adopt for mmry:**
- Safety feature for autonomous agents
- Could prevent storing certain content types
- Integrates with existing embedding infrastructure

**Implementation approach:**
- Add `boundaries` table with pattern/embedding matching
- Add boundary check function called during memory operations
- Expose via MCP: `check_boundary`, `list_boundaries`

---

### 5. Worldview Primitives (MEDIUM PRIORITY)

**What it is:** Beliefs that filter perception with confidence tracking.

```sql
worldview_primitives (
  category TEXT,           -- epistemology, ethics, identity, etc.
  belief TEXT,             -- "Knowledge requires evidence"
  confidence FLOAT,        -- 0.0 to 1.0
  emotional_valence FLOAT, -- -1.0 to +1.0
  stability_score FLOAT,   -- how resistant to change
  connected_beliefs UUID[]
)
```

**Key Features:**
- Beliefs can be challenged and updated
- Confidence decays over time without reinforcement
- Connected beliefs form a coherent worldview graph
- Used for worldview alignment scoring

**Why adopt for mmry:**
- Enables perspective-aware retrieval
- Supports user preference learning
- Could improve personalization

**Implementation approach:**
- Add `worldview` table
- Track worldview alignment in search scoring
- Expose via MCP: `get_worldview`, `update_belief`

---

### 6. Reflection Pipeline (LOW PRIORITY)

**What it is:** Structured LLM-driven self-reflection with schema output.

**Reflection Result Schema:**
```json
{
  "insights": [{"content", "confidence", "category"}],
  "identity_updates": [{"aspect_type", "change", "reason"}],
  "worldview_updates": [{"id", "new_confidence", "reason"}],
  "discovered_relationships": [{"from_id", "to_id", "type", "confidence"}],
  "contradictions_noted": [{"memory_a", "memory_b", "resolution"}],
  "self_updates": [{"kind", "concept", "strength", "evidence_memory_id"}]
}
```

**Why lower priority:**
- Requires significant LLM integration
- More relevant for AGI-style applications
- mmry's HMLR already handles some of this

---

### 7. Heartbeat System (LOW PRIORITY)

**What it is:** Periodic autonomous cognition with energy budgeting.

**Heartbeat Cycle (hourly by default):**
1. Initialize: Regenerate energy (10/cycle, max 20)
2. Observe: Environment snapshot
3. Orient: Review goals, gather context
4. Decide: LLM chooses actions within budget
5. Act: Execute chosen actions
6. Record: Create episodic memory

**Action Costs:**
- Free: observe, review_goals, remember
- 1: recall, connect, reprioritize
- 2: reflect, maintain
- 3: brainstorm_goals
- 5: reach_out_user
- 7: reach_out_public

**Why lower priority:**
- Major architectural addition
- More suited for autonomous agents than memory retrieval
- Would require worker infrastructure

---

### 8. Personhood Substrate (LOW PRIORITY)

**What it is:** Self-model graph for AI identity.

**Components:**
- `SelfNode`: Singleton representing the AI's self-concept
- Self-concept edges: `capable_of`, `struggles_with`, `has_trait`, `values`, `has_learned`, `tends_to`, `is_becoming`
- Narrative identity: `LifeChapterNode`, `TurningPointNode`, `NarrativeThreadNode`

**Why lower priority:**
- Highly specialized for AGI-style applications
- Philosophically ambitious (designed to "defeat arguments against personhood")
- Not aligned with mmry's core use case

---

## Features mmry Already Has Better

| Feature | mmry | agi-memory |
|---------|------|------------|
| Search modes | 6 hybrid modes | Vector + keyword only |
| Sparse embeddings | SPLADE++ | None |
| Reranking | BGE, Jina models | None |
| NER | GLiNER integration | None |
| Chunking | Cascading strategy | Basic |
| Performance | Rust (fast) | Python + PostgreSQL |
| Deployment | Single binary | Docker compose |
| TUI | Full TUI | None |

---

## Recommended Implementation Roadmap

### Phase 1: Trust & Provenance (1-2 weeks)
1. Add `source_attribution` to memories schema
2. Implement trust scoring functions
3. Add trust weighting to search
4. Expose via MCP tools

### Phase 2: Affective State (2-3 weeks)
1. Add affective state schema
2. Implement emotional momentum
3. Add mood-congruent recall bias (optional search weight)
4. Expose via MCP tools

### Phase 3: Drives & Boundaries (1-2 weeks)
1. Add drives table and update logic
2. Add boundaries table with semantic matching
3. Expose via MCP tools

### Phase 4: Worldview (2 weeks)
1. Add worldview primitives table
2. Implement confidence decay
3. Add worldview alignment to search scoring
4. Expose via MCP tools

---

## Schema Additions Summary

```sql
-- Phase 1: Trust
ALTER TABLE memories ADD COLUMN source_attribution JSONB;
ALTER TABLE memories ADD COLUMN trust_level FLOAT GENERATED ALWAYS AS (...);

-- Phase 2: Affective State
CREATE TABLE affective_state (
    id TEXT PRIMARY KEY DEFAULT 'current',
    valence FLOAT NOT NULL DEFAULT 0.0,
    arousal FLOAT NOT NULL DEFAULT 0.5,
    primary_emotion TEXT,
    intensity FLOAT DEFAULT 0.5,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE emotion_history (
    id TEXT PRIMARY KEY,
    emotion TEXT NOT NULL,
    valence FLOAT,
    arousal FLOAT,
    intensity FLOAT,
    source TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE memories ADD COLUMN emotional_valence FLOAT;

-- Phase 3: Drives & Boundaries
CREATE TABLE drives (
    name TEXT PRIMARY KEY,
    current_value FLOAT DEFAULT 0.0,
    accumulation_rate FLOAT NOT NULL,
    last_satisfied DATETIME,
    satisfaction_cooldown INTEGER DEFAULT 3600
);

CREATE TABLE boundaries (
    id TEXT PRIMARY KEY,
    boundary_type TEXT NOT NULL,
    description TEXT NOT NULL,
    response_type TEXT DEFAULT 'flag',
    trigger_patterns JSON,
    trigger_embedding BLOB,
    active BOOLEAN DEFAULT TRUE
);

-- Phase 4: Worldview
CREATE TABLE worldview (
    id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    belief TEXT NOT NULL,
    confidence FLOAT DEFAULT 0.7,
    emotional_valence FLOAT DEFAULT 0.0,
    stability_score FLOAT DEFAULT 0.5,
    connected_beliefs JSON DEFAULT '[]',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

---

## MCP Tools to Add

```
# Trust/Provenance
get_memory_provenance(memory_id) -> sources
add_memory_source(memory_id, source) -> updated_trust

# Affective State
get_affective_state() -> {valence, arousal, emotion, intensity}
set_affective_state(valence, arousal, emotion) -> state
record_emotion(emotion, source, intensity) -> history_entry

# Drives
get_drives() -> [{name, value, urgency}]
satisfy_drive(name, amount) -> updated_drive

# Boundaries
check_boundary(content) -> {allowed, boundary_hit, response_type}
list_boundaries() -> [boundaries]

# Worldview
get_worldview() -> [{belief, confidence, category}]
update_belief(id, confidence_delta, reason) -> belief
```

---

## Conclusion

agi-memory offers several innovative features around cognitive architecture and personhood. For mmry, the most valuable additions would be:

1. **Trust/Provenance** - Immediately useful for fact verification workflows
2. **Affective State** - Differentiating feature for AI assistant use cases
3. **Drives/Boundaries** - Lightweight additions for autonomous agent support

The more ambitious features (heartbeat, reflection, personhood) could be considered for a future "mmry-cognitive" module if there's demand for AGI-style applications.
