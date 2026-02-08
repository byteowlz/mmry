# Issues

## Open

### [mmry-xrbv.4] LLM-based learning extraction pipeline - analyze hstry chat sessions and distill actionable learnings/rules into the learnings store (P1, feature)

### [mmry-xrbv.3] 'mmry context <task>' command - search learnings (not memories) within a store and optionally across stores, return relevant rules + anti-patterns as JSON (P1, feature)

### [mmry-xrbv.2] Confidence decay & maturity tracking - time-decayed feedback scoring (90-day half-life, 4x harmful multiplier) with maturity transitions (candidate/established/proven/deprecated) (P1, feature)

### [mmry-xrbv.1] Learnings table & data model - store distilled rules/insights separate from raw memories, with category, scope, maturity, and provenance (P1, feature)

### [mmry-xrbv] Learnings & Context System - distill agent sessions into actionable rules with confidence tracking, inspired by cass-memory (P1, epic)

### [mmry-xrbv.10] MCP tools for learnings - GetContext, AddLearning, RecordFeedback, RecordOutcome, ListLearnings, GapAnalysis (P2, feature)

### [mmry-xrbv.8] Cross-store context search - 'mmry context <task> --all-stores' to search learnings across multiple stores with store attribution in results (P2, feature)

### [mmry-xrbv.7] Gap analysis - track learning distribution across categories, identify critical/underrepresented areas, prioritize session analysis for gap-filling (P2, feature)

### [mmry-xrbv.6] Anti-pattern conversion - automatically invert learnings with >50% harmful ratio into PITFALL warnings (P2, feature)

### [mmry-xrbv.5] Feedback events & outcome recording - record helpful/harmful feedback on learnings, record session outcomes with rule attribution, auto-apply to update scores (P2, feature)

### [mmry-jadt.4] Query-time synthesis - reason over retrieved facts to answer queries, not just return raw data (P2, feature)

### [mmry-jadt.3] Contradiction detection and resolution system for conflicting facts (P2, feature)

### [mmry-jadt.2] Reasoning-augmented fact extraction - derive inductive/abductive conclusions from observed facts (P2, feature)

### [mmry-jadt.1] Add inference_type to facts (observed/deduced/induced/abduced) with premise tracking (P2, feature)

### [mmry-xrbv.9] Agent-native onboarding - guided workflow for agents to analyze hstry sessions and build playbook, with progress tracking and resumability across sessions (P3, feature)

### [mmry-jadt.8] Reasoning event types in agent_events - track inference, contradiction, prediction events (P3, feature)

### [mmry-jadt.7] Natural language certainty statements - express confidence as reasoning, not numbers (P3, feature)

### [mmry-jadt.6] Track fact predictive power - importance based on reasoning utility, not arbitrary scores (P3, feature)

### [mmry-jadt.5] Predictions table - store predictive models about user identity with verification loop (P3, feature)

### [mmry-jadt] Memory as Reasoning - treat memory as prediction/reasoning task, not just storage (P3, epic)

