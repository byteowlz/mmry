# Memory as Reasoning: Ideas for mmry

Based on insights from [Plastic Labs' "Memory as Reasoning"](https://blog.plasticlabs.ai/blog/Memory-as-Reasoning) blog post.

## Core Thesis

Traditional memory systems focus on **storage and retrieval** - storing facts with high fidelity and surfacing them when needed. But this is a deterministic, skeuomorphic approach that doesn't leverage what LLMs are actually good at.

The alternative: treat memory as a **reasoning task**. Instead of just storing facts, build models that can predict and reason about identity. LLMs can perform logical reasoning without the cognitive biases, emotional interference, and belief resistance that limit human reasoning.

Key insight: *"The question isn't how best to store your data as it exists for prediction later, but rather how best to reason over it to get the most accurate topological representation of identity."*

---

## Proposed Enhancements

### 1. Inference Layer on Facts

**Current state:** Facts are stored as `fact_key` + `fact_value` pairs, directly extracted from conversations.

**Proposal:** Add a reasoning system that derives new facts from existing ones through logical inference.

Create a distinction between:
- **Observed facts** - directly extracted from user input
- **Deduced facts** - certain conclusions from explicit premises
- **Induced facts** - generalizations from patterns
- **Abduced facts** - best explanations for observed behaviors

Schema additions to `facts` table:
```sql
inference_type TEXT DEFAULT 'observed', -- observed | deduced | induced | abduced
premise_ids JSON DEFAULT '[]',          -- fact IDs this was derived from
reasoning_trace TEXT                    -- the reasoning chain that led to this conclusion
```

This creates a **reasoning tree** that can be traversed and inspected, making the system's conclusions transparent and auditable.

---

### 2. Predictive Profiles / Identity Models

**Current state:** Profile blocks store static preferences and constraints.

**Proposal:** Build predictive models that can answer "what would this user likely do/prefer in situation X?"

New entity: `predictions` table
```sql
CREATE TABLE predictions (
    id TEXT PRIMARY KEY,
    prediction TEXT NOT NULL,           -- "User prefers CLI tools over GUIs"
    confidence_basis TEXT,              -- reasoning in natural language, not a number
    supporting_fact_ids JSON,           -- [uuid] facts used as premises
    context TEXT,                       -- when/where this prediction applies
    verified BOOLEAN,                   -- null = unverified, true/false = outcome known
    verification_outcome TEXT,          -- what actually happened
    verification_date DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

The key innovation: **confidence is expressed in natural language reasoning**, not arbitrary numerical scores. "Based on 3 consistent observations over 2 months" is more useful than "confidence: 0.85".

When predictions are verified against actual behavior, the **surprisal signal** (prediction error) feeds back to improve the model.

---

### 3. Reasoning-Augmented Fact Extraction

**Current state:** HMLR extracts facts directly from conversation content.

**Proposal:** Add a second pass that reasons over extracted facts to generate higher-order insights.

Example reasoning chain:
```
Observed facts:
- "User uses Rust for most projects"
- "User uses Nix for system configuration"  
- "User prefers local-first applications"
- "User self-hosts services when possible"

Induced conclusion:
- "User values control and sovereignty over convenience"

Abduced conclusion:
- "User likely has privacy concerns or works with sensitive data"
```

These derived facts are more valuable for personalization than the raw observations because they capture **why** the user behaves this way, not just **what** they do.

Implementation: Add a `reasoning_pass()` function to the HMLR pipeline that:
1. Groups related facts
2. Prompts an LLM to find patterns (induction) and explanations (abduction)
3. Stores derived facts with full reasoning traces

---

### 4. Contradiction Resolution System

**Current state:** Contradicting facts can coexist without resolution.

**Proposal:** Actively detect and resolve contradictions through reasoning.

The article notes that LLMs handle contradictions without "neural inertia" - they can update beliefs without the resistance humans experience.

New entity: `contradiction_resolutions` table
```sql
CREATE TABLE contradiction_resolutions (
    id TEXT PRIMARY KEY,
    fact_a_id TEXT NOT NULL,
    fact_b_id TEXT NOT NULL,
    resolution_type TEXT,               -- 'a_preferred' | 'b_preferred' | 'context_dependent' | 'merged'
    reasoning TEXT NOT NULL,            -- why this resolution was chosen
    resolved_fact_id TEXT,              -- if merged, the new unified fact
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

Resolution strategies:
- **Temporal**: newer information supersedes older (with exceptions for stable traits)
- **Evidential**: more supporting observations wins
- **Contextual**: both valid in different situations
- **Merged**: synthesize into a more nuanced understanding

Example:
- Fact A: "User prefers dark mode" (observed 2024-01)
- Fact B: "User prefers light mode" (observed 2024-06)
- Resolution: "User switched preference, possibly due to environment change. Current preference is light mode. May be context-dependent (time of day, ambient lighting)."

---

### 5. Query-Time Reasoning / Synthesis

**Current state:** Search returns matching memories and facts.

**Proposal:** Add a synthesis step that reasons over retrieved content to answer the actual query.

New search mode: `--synthesize` or `--reason`

```bash
mmry search "what kind of developer is this user?" --synthesize
```

Instead of returning raw facts, the system:
1. Retrieves relevant facts and memories
2. Reasons over them to answer the specific question
3. Returns a synthesized conclusion with supporting evidence

This is the "infinitely re-composable predictions" idea - the same facts can be combined differently to answer any query.

Implementation:
- Add `SearchService::search_with_synthesis()` 
- Takes retrieved facts + query
- Generates a reasoned response citing specific facts as premises
- Returns both the synthesis and the underlying evidence

---

### 6. Fact Importance via Predictive Power

**Current state:** Facts have `recency_score` that decays over time.

**Proposal:** Track how useful a fact is for successful reasoning and prediction.

New metrics:
```sql
-- additions to facts table
premise_use_count INTEGER DEFAULT 0,    -- how often used in reasoning
prediction_success_count INTEGER DEFAULT 0,  -- predictions using this that were verified true
prediction_failure_count INTEGER DEFAULT 0,  -- predictions using this that were verified false
predictive_power REAL GENERATED ALWAYS AS (
    CASE WHEN (prediction_success_count + prediction_failure_count) > 0 
    THEN CAST(prediction_success_count AS REAL) / (prediction_success_count + prediction_failure_count)
    ELSE NULL END
) STORED
```

Facts that consistently support accurate predictions become more important. Facts that lead to wrong predictions get demoted. This is **evidence-based importance**, not arbitrary scoring.

---

### 7. Natural Language Certainty

**Current state:** Numerical scores like `recency_score`, `trust_level`, `importance`.

**Proposal:** Express uncertainty in natural language alongside or instead of numbers.

```sql
-- additions to facts table
certainty_statement TEXT  -- "Based on 3 consistent observations over 2 months, with no contradicting evidence"
```

Examples:
- "Strongly supported by repeated explicit statements"
- "Inferred from behavior patterns, moderate confidence"
- "Single observation, may not generalize"
- "Previously contradicted, current belief based on more recent evidence"

This is more useful context for downstream reasoning than "confidence: 0.72".

---

### 8. Reasoning Traces for Agent Events

**Current state:** Agent events track what happened (memory created, fact extracted).

**Proposal:** Extend to capture **reasoning events** - when the system reasoned over facts to derive conclusions.

```sql
-- new event_type values
'inference_generated'   -- derived a new fact through reasoning
'contradiction_detected' -- found conflicting facts
'contradiction_resolved' -- resolved a contradiction
'prediction_made'       -- generated a prediction
'prediction_verified'   -- checked a prediction against reality
'synthesis_generated'   -- answered a query through reasoning
```

This creates an audit trail of the system's reasoning, making it transparent and debuggable.

---

## Implementation Priority

1. **Inference types on facts** - Low effort, high value. Just add the column and start categorizing.

2. **Reasoning-augmented extraction** - Medium effort. Add a post-processing step to HMLR.

3. **Contradiction detection** - Medium effort. Can run as background job over existing facts.

4. **Query-time synthesis** - Medium effort. New search mode leveraging existing infrastructure.

5. **Predictions table** - Higher effort. New entity, new extraction logic, verification loop.

6. **Predictive power tracking** - Depends on predictions being implemented first.

---

## Open Questions

- How much reasoning should happen at write time vs query time?
- How to handle reasoning that becomes stale as new facts arrive?
- What's the right granularity for reasoning traces?
- How to expose this to agents in a useful way without overwhelming context?
- Should reasoning be synchronous (blocking) or asynchronous (background job)?

---

## References

- [Memory as Reasoning - Plastic Labs](https://blog.plasticlabs.ai/blog/Memory-as-Reasoning)
- [Chain-of-Thought Prompting](https://arxiv.org/abs/2205.11916)
- [InstructGPT / RLHF](https://arxiv.org/abs/2203.02155)
- [DeepSeek R1](https://github.com/deepseek-ai/DeepSeek-R1/blob/main/DeepSeek_R1.pdf)
