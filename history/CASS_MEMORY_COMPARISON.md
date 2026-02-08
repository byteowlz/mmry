# Cass Memory System Comparison and Recommendations for mmry

This document analyzes the [cass memory system](https://github.com/Dicklesworthstone/cass_memory_system) and identifies features that could enhance mmry.

---

## Executive Summary

**cass-memory** implements a sophisticated three-layer cognitive architecture for AI coding agents that transforms scattered session logs into persistent, cross-agent procedural memory. While mmry excels in search technology (multiple modes, sparse embeddings, reranking), cass offers several innovative features around **rule extraction**, **confidence decay**, **cross-agent learning**, and **procedural memory** that could significantly enhance mmry's agent workflow capabilities.

### Priority Features to Consider

| Priority | Feature | Effort | Value | Why Important |
|----------|---------|--------|-------|---------------|
| **High** | Confidence Decay & Maturity Tracking | Medium | High | Prevents stale rules, continuous learning |
| **High** | Feedback Event History | Low | High | Transparent learning, debugging |
| **High** | Rule/Procedural Memory Layer | High | Very High | Converts sessions into actionable rules |
| **Medium** | Agent-Native Onboarding | Medium | High | Zero-cost playbook building |
| **Medium** | Gap Analysis System | Medium | Medium | Balances knowledge coverage |
| **Medium** | Anti-Pattern Conversion | Low | Medium | Learning from mistakes |
| **Medium** | Outcome Recording | Low | Medium | Explicit session feedback |
| **Low** | Trauma Guard | Medium | Low | Safety system for dangerous patterns |
| **Low** | Multi-Source Trust | Medium | Low | Distinguishes fact sources |

---

## Architecture Comparison

### Cass Memory System (Three-Layer)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    EPISODIC MEMORY (cass)                           │
│   Raw session logs from all agents — the "ground truth"             │
│   Claude Code │ Codex │ Cursor │ Aider │ PI │ Gemini │ ChatGPT │ ... │
└───────────────────────────┬─────────────────────────────────────────┘
                            │ cass search
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    WORKING MEMORY (Diary)                           │
│   Structured session summaries bridging raw logs to rules           │
│   accomplishments │ decisions │ challenges │ outcomes               │
└───────────────────────────┬─────────────────────────────────────────┘
                            │ reflect + curate (automated)
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    PROCEDURAL MEMORY (Playbook)                     │
│   Distilled rules with confidence tracking                          │
│   Rules │ Anti-patterns │ Feedback │ Decay                          │
└─────────────────────────────────────────────────────────────────────┘
```

**Key Innovation**: Rules are extracted from sessions, validated against historical evidence, and evolve through feedback.

### mmry (Single-Layer with HMLR Enrichment)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    MEMORIES (SQLite + Embeddings)                   │
│   Episodic │ Semantic │ Procedural                                  │
│   Categories │ Tags │ HMLR Enrichment                              │
│   - Facts                                                        │
│   - Bridge Blocks                                                 │
│   - Agent Attribution                                              │
└─────────────────────────────────────────────────────────────────────┘
```

**Current State**: mmry has excellent search and HMLR enriches memories with facts/bridge blocks, but lacks the rule extraction and confidence tracking layers.

---

## Feature Deep Dives

### 1. Confidence Decay & Maturity Tracking (HIGH PRIORITY)

**What cass does**: Every rule tracks feedback events over time, with scores that decay based on a configurable half-life (default 90 days).

**Data Model**:
```typescript
interface FeedbackEvent {
  id: string;
  type: "helpful" | "harmful";
  timestamp: string;           // ISO 8601
  sessionPath?: string;
  reason?: string;              // Why this feedback
}

interface PlaybookBullet {
  helpfulCount: number;
  harmfulCount: number;
  feedbackEvents: FeedbackEvent[];
  effectiveScore: number;       // Decayed score
  maturity: "candidate" | "established" | "proven" | "deprecated";
}
```

**Decay Formula**:
```typescript
// Feedback decays exponentially
decay = Math.pow(0.5, daysSince / halfLifeDays)

// Effective score (harmful counts 4x)
effectiveScore = decayedHelpful - (4 * decayedHarmful)

// Maturity transitions
candidate (3+ helpful) → established (10+ helpful) → proven
harmfulRatio > 25% → deprecated
```

**Why this matters for mmry**:
1. **Prevents stale knowledge** - A rule helpful in January 2025 loses relevance by February 2026
2. **Continuous validation** - Rules require ongoing evidence to stay active
3. **Quality filtering** - Rules are ranked by effective score, not just helpful count
4. **Anti-pattern conversion** - Rules with >50% harmful ratio become warnings

**Implementation for mmry**:

```sql
-- Add feedback tracking to existing memory/enrichment tables
ALTER TABLE bridge_blocks ADD COLUMN helpful_count INTEGER DEFAULT 0;
ALTER TABLE bridge_blocks ADD COLUMN harmful_count INTEGER DEFAULT 0;
ALTER TABLE bridge_blocks ADD COLUMN effective_score REAL;
ALTER TABLE bridge_blocks ADD COLUMN maturity TEXT DEFAULT 'candidate';
ALTER TABLE bridge_blocks ADD COLUMN pinned BOOLEAN DEFAULT FALSE;

-- New table for detailed feedback history
CREATE TABLE feedback_events (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    bridge_block_id TEXT NOT NULL REFERENCES bridge_blocks(block_id),
    type TEXT NOT NULL CHECK(type IN ('helpful', 'harmful')),
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    session_path TEXT,
    reason TEXT,
    agent_id TEXT REFERENCES agents(id),
    FOREIGN KEY (bridge_block_id) REFERENCES bridge_blocks(block_id)
);

CREATE INDEX idx_feedback_events_block_id ON feedback_events(bridge_block_id);
CREATE INDEX idx_feedback_events_timestamp ON feedback_events(timestamp);
```

**Rust Implementation**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub id: Uuid,
    pub bridge_block_id: Uuid,
    pub feedback_type: FeedbackType,
    pub timestamp: DateTime<Utc>,
    pub session_path: Option<String>,
    pub reason: Option<String>,
    pub agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackType {
    Helpful,
    Harmful,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Maturity {
    Candidate,    // Default for new rules
    Established,  // 3+ helpful, <25% harmful
    Proven,        // 10+ helpful, <10% harmful
    Deprecated,   // >25% harmful or explicit
}

pub struct ScoringConfig {
    pub decay_half_life_days: i64,  // Default 90
    pub harmful_multiplier: f32,    // Default 4.0
    pub min_feedback_for_active: i32, // Default 3
    pub min_helpful_for_proven: i32,  // Default 10
    pub max_harmful_ratio_for_proven: f32, // Default 0.1
}

impl BridgeBlock {
    /// Calculate decayed feedback counts
    pub fn calculate_decayed_scores(&self, config: &ScoringConfig) -> (f32, f32) {
        let now = Utc::now();
        let half_life = Duration::days(config.decay_half_life_days);

        let mut decayed_helpful = 0.0;
        let mut decayed_harmful = 0.0;

        for event in &self.feedback_events {
            let age = now.signed_duration_since(event.timestamp);
            let decay_factor = 0.5_f32.powf(age.num_milliseconds() as f32 / half_life.num_milliseconds() as f32);

            match event.feedback_type {
                FeedbackType::Helpful => decayed_helpful += decay_factor,
                FeedbackType::Harmful => decayed_harmful += decay_factor,
            }
        }

        (decayed_helpful, decayed_harmful)
    }

    /// Calculate effective score with harmful multiplier
    pub fn calculate_effective_score(&self, config: &ScoringConfig) -> f32 {
        let (decayed_helpful, decayed_harmful) = self.calculate_decayed_scores(config);

        // Apply harmful multiplier and maturity multiplier
        let maturity_multiplier = match self.maturity {
            Maturity::Candidate => 0.5,
            Maturity::Established => 1.0,
            Maturity::Proven => 1.5,
            Maturity::Deprecated => 0.0,
        };

        (decayed_helpful - (config.harmful_multiplier * decayed_harmful)) * maturity_multiplier
    }

    /// Determine maturity state based on feedback
    pub fn calculate_maturity(&self, config: &ScoringConfig) -> Maturity {
        if self.pinned {
            return self.maturity.clone();
        }

        let (decayed_helpful, decayed_harmful) = self.calculate_decayed_scores(config);
        let total = decayed_helpful + decayed_harmful;

        if total < config.min_feedback_for_active as f32 - 0.01 {
            return Maturity::Candidate;
        }

        let harmful_ratio = if total > 0.0 {
            decayed_harmful / total
        } else {
            0.0
        };

        // Auto-deprecate if too harmful
        if harmful_ratio > 0.3 && total >= config.min_feedback_for_active as f32 - 0.01 {
            return Maturity::Deprecated;
        }

        // Promote to proven if strong positive signal
        if decayed_helpful >= config.min_helpful_for_proven as f32 - 0.01
            && harmful_ratio < config.max_harmful_ratio_for_proven
        {
            return Maturity::Proven;
        }

        // Otherwise established
        Maturity::Established
    }
}
```

**MCP Tools to Add**:
```typescript
// Record feedback on a bridge block
mcp.registerTool({
  name: "RecordFeedback",
  description: "Record helpful/harmful feedback on a bridge block",
  inputSchema: {
    type: "object",
    properties: {
      blockId: { type: "string" },
      type: { enum: ["helpful", "harmful"] },
      reason: { type: "string" },
      sessionPath: { type: "string" }
    }
  }
});

// Get feedback history for a block
mcp.registerTool({
  name: "GetFeedbackHistory",
  description: "Get all feedback events for a bridge block"
});

// Update effective scores (call after feedback changes)
mcp.registerTool({
  name: "RecalculateScores",
  description: "Recalculate effective scores and maturity for all blocks"
});
```

---

### 2. Rule/Procedural Memory Layer (HIGH PRIORITY)

**What cass does**: Extracts actionable rules from sessions that agents can query before starting tasks.

**The `cm context` command**:
```bash
cm context "implement auth rate limiting" --json
```

**Returns**:
```json
{
  "relevantBullets": [
    {
      "id": "b-8f3a2c",
      "content": "Always check token expiry before other auth debugging",
      "effectiveScore": 8.5,
      "maturity": "proven",
      "category": "debugging",
      "reasoning": "Extracted from 5 successful sessions"
    }
  ],
  "antiPatterns": [
    {
      "id": "b-x7k9p1",
      "content": "PITFALL: Don't cache auth tokens without expiry validation",
      "effectiveScore": 3.2
    }
  ],
  "historySnippets": [
    {
      "source_path": "~/.claude/sessions/session-001.jsonl",
      "agent": "claude",
      "snippet": "Fixed timeout by increasing token refresh interval...",
      "score": 0.87
    }
  ]
}
```

**Why this matters for mmry**:
1. **Actionable knowledge** - Rules are imperative statements ("Always X", "When Y do Z")
2. **Agent-native** - Designed for AI agents to query before tasks
3. **Evidence-backed** - Every rule has reasoning and source sessions
4. **Anti-patterns** - Learning from mistakes creates warnings

**Implementation for mmry**:

**Option A: Extend Bridge Blocks** (Simpler)
- Add `rule_content` field to bridge blocks
- Extract rules from `decisions_made` during HMLR enrichment
- Query bridge blocks for "rule" category

**Option B: Separate Rules Table** (More powerful, cass-style)
```sql
CREATE TABLE rules (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    content TEXT NOT NULL,
    category TEXT NOT NULL,
    scope TEXT CHECK(scope IN ('global', 'workspace', 'language', 'framework', 'task')),
    scope_key TEXT,  -- e.g., workspace path or language name

    -- Classification
    rule_type TEXT CHECK(rule_type IN ('rule', 'anti-pattern')) DEFAULT 'rule',
    kind TEXT CHECK(kind IN ('project_convention', 'stack_pattern', 'workflow_rule', 'anti_pattern')),
    is_negative BOOLEAN DEFAULT FALSE,

    -- Lifecycle
    state TEXT CHECK(state IN ('draft', 'active', 'retired')) DEFAULT 'active',
    maturity TEXT CHECK(maturity IN ('candidate', 'established', 'proven', 'deprecated')) DEFAULT 'candidate',

    -- Feedback
    helpful_count INTEGER DEFAULT 0,
    harmful_count INTEGER DEFAULT 0,
    effective_score REAL,

    -- Provenance
    source_sessions TEXT[],  -- Array of session paths
    source_agents TEXT[],    -- Array of agent names
    reasoning TEXT,

    -- Metadata
    tags TEXT[],
    pinned BOOLEAN DEFAULT FALSE,
    deprecated BOOLEAN DEFAULT FALSE,

    -- Timestamps
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Embeddings for semantic search
CREATE TABLE rule_embeddings (
    rule_id TEXT PRIMARY KEY REFERENCES rules(id),
    embedding BLOB,  -- float32 array
    embedding_dim INTEGER,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Indexing
CREATE INDEX idx_rules_category ON rules(category);
CREATE INDEX idx_rules_effective_score ON rules(effective_score DESC);
CREATE INDEX idx_rules_maturity ON rules(maturity);
```

**Rust Types**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: Uuid,
    pub content: String,
    pub category: String,
    pub scope: RuleScope,
    pub scope_key: Option<String>,

    pub rule_type: RuleType,
    pub kind: RuleKind,
    pub is_negative: bool,

    pub state: RuleState,
    pub maturity: Maturity,

    pub helpful_count: i32,
    pub harmful_count: i32,
    pub effective_score: Option<f32>,

    pub source_sessions: Vec<String>,
    pub source_agents: Vec<String>,
    pub reasoning: Option<String>,

    pub tags: Vec<String>,
    pub pinned: bool,
    pub deprecated: bool,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleScope {
    Global,
    Workspace,
    Language,
    Framework,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleType {
    Rule,
    AntiPattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleKind {
    ProjectConvention,
    StackPattern,
    WorkflowRule,
    AntiPattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleState {
    Draft,
    Active,
    Retired,
}
```

**MCP Tools**:
```typescript
// Query rules relevant to a task (the main command)
mcp.registerTool({
  name: "GetContext",
  description: "Get relevant rules and history for a task",
  inputSchema: {
    type: "object",
    properties: {
      task: { type: "string", description: "Task description" },
      limit: { type: "number", default: 10 },
      includeHistory: { type: "boolean", default: true },
      scope: { type: "string", enum: ["global", "workspace", "language", "framework", "task"] }
    }
  }
});

// Add a rule manually
mcp.registerTool({
  name: "AddRule",
  description: "Add a new rule to the playbook",
  inputSchema: {
    type: "object",
    properties: {
      content: { type: "string" },
      category: { type: "string" },
      reasoning: { type: "string" }
    }
  }
});

// Record outcome of following a rule
mcp.registerTool({
  name: "RecordOutcome",
  description: "Record helpful/harmful outcome for rules used in a session",
  inputSchema: {
    type: "object",
    properties: {
      ruleIds: { type: "array", items: { type: "string" } },
      status: { enum: ["success", "failure", "mixed"] },
      summary: { type: "string" }
    }
  }
});
```

---

### 3. Agent-Native Onboarding (MEDIUM PRIORITY)

**What cass does**: Guides agents through analyzing historical sessions to build a playbook, without requiring external LLM APIs (uses the agent already running).

**The `cm onboard` workflow**:
```bash
# 1. Check status and see recommendations
cm onboard status --json

# 2. Get sessions to analyze (filtered by gaps in playbook)
cm onboard sample --fill-gaps --json

# 3. Read a session with rich context
cm onboard read /path/to/session.jsonl --template --json

# 4. Add extracted rules
cm playbook add "Your rule content" --category "debugging"

# 5. Mark session as processed
cm onboard mark-done /path/to/session.jsonl
```

**Gap Analysis**:
- Tracks rule counts by category (debugging, testing, architecture, etc.)
- Priorities: critical (0 rules), underrepresented (1-2 rules), adequate (3-10 rules), well-covered (11+ rules)
- Suggests sessions most likely to fill gaps

**Template Output** (rich context for extraction):
```json
{
  "metadata": {
    "path": "/path/to/session.jsonl",
    "workspace": "/Users/x/project",
    "messageCount": 127,
    "topicHints": ["debugging", "testing", "git"]
  },
  "context": {
    "relatedRules": [
      {"id": "b-abc123", "content": "...", "similarity": 0.72}
    ],
    "playbookGaps": {
      "critical": ["security", "performance"],
      "underrepresented": ["collaboration"]
    },
    "suggestedFocus": "This session may contain security patterns - you have NO rules in this area!"
  },
  "extractionFormat": {
    "schema": {"content": "string", "category": "string"},
    "categories": ["debugging", "testing", "architecture", ...],
    "examples": [...]
  },
  "sessionContent": "..."
}
```

**Why this matters for mmry**:
1. **Zero additional cost** - Uses the agent already running (e.g., Claude Code)
2. **Guided extraction** - Provides context and examples for better rule extraction
3. **Gap-aware** - Prioritizes underrepresented areas
4. **Progress tracking** - Resumeable across sessions

**Implementation for mmry**:

```sql
-- Onboarding state tracking
CREATE TABLE onboarding_state (
    id TEXT PRIMARY KEY DEFAULT 'current',
    started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    total_sessions_processed INTEGER DEFAULT 0,
    total_rules_extracted INTEGER DEFAULT 0
);

-- Processed sessions tracking
CREATE TABLE processed_sessions (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    session_path TEXT NOT NULL UNIQUE,
    processed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    rules_extracted INTEGER DEFAULT 0,
    workspace TEXT,
    agent TEXT
);

-- Category counts for gap analysis
CREATE TABLE category_stats (
    category TEXT PRIMARY KEY,
    rule_count INTEGER DEFAULT 0,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Session analysis queue (for gap-targeted sampling)
CREATE TABLE session_queue (
    id TEXT PRIMARY KEY,
    session_path TEXT NOT NULL UNIQUE,
    workspace TEXT,
    agent TEXT,
    topic_hints TEXT[],  -- Detected topics
    gap_score REAL DEFAULT 0.0,  -- Priority for gap-filling
    queued_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    processed BOOLEAN DEFAULT FALSE
);
```

**Rust Implementation**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStatus {
    pub started_at: DateTime<Utc>,
    pub last_updated_at: DateTime<Utc>,
    pub total_sessions_processed: i32,
    pub total_rules_extracted: i32,
    pub category_gaps: CategoryGaps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryGaps {
    pub critical: Vec<String>,        // 0 rules
    pub underrepresented: Vec<String>, // 1-2 rules
    pub adequate: Vec<String>,         // 3-10 rules
    pub well_covered: Vec<String>,     // 11+ rules
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateContext {
    pub metadata: SessionMetadata,
    pub context: ExtractionContext,
    pub extraction_format: ExtractionFormat,
    pub session_content: String,
}

pub struct OnboardingManager {
    config: OnboardingConfig,
}

impl OnboardingManager {
    /// Get current onboarding status
    pub async fn get_status(&self, pool: &SqlitePool) -> Result<OnboardingStatus> {
        // Query onboarding_state and category_stats
        // Calculate gaps based on rule counts
    }

    /// Sample sessions prioritized for gap-filling
    pub async fn sample_sessions(&self, pool: &SqlitePool, limit: usize) -> Result<Vec<QueuedSession>> {
        // Get gap categories
        // Generate search queries from category keywords
        // Score sessions against gaps
        // Return unprocessed sessions, sorted by gap_score
    }

    /// Generate template context for session analysis
    pub async fn get_template_context(
        &self,
        pool: &SqlitePool,
        session_path: &str,
    ) -> Result<TemplateContext> {
        // Detect topic hints from session
        // Find related rules via semantic search
        // Calculate playbook gaps
        // Format extraction schema and examples
    }

    /// Mark session as processed
    pub async fn mark_done(
        &self,
        pool: &SqlitePool,
        session_path: &str,
        rules_extracted: i32,
    ) -> Result<()> {
        // Update processed_sessions
        // Update onboarding_state
        // Update category_stats
    }
}
```

**MCP Tools**:
```typescript
mcp.registerTool({
  name: "OnboardingStatus",
  description: "Get onboarding progress and category gaps"
});

mcp.registerTool({
  name: "SampleSessions",
  description: "Get sessions to analyze, prioritized by gap-filling",
  inputSchema: {
    type: "object",
    properties: {
      limit: { type: "number", default: 5 },
      fillGaps: { type: "boolean", default: true },
      workspace: { type: "string" },
      agent: { type: "string" }
    }
  }
});

mcp.registerTool({
  name: "GetSessionTemplate",
  description: "Get session with rich extraction context",
  inputSchema: {
    type: "object",
    properties: {
      sessionPath: { type: "string" },
      includeTemplate: { type: "boolean", default: true }
    }
  }
});

mcp.registerTool({
  name: "MarkSessionProcessed",
  description: "Mark session as processed with rule count"
});
```

---

### 4. Anti-Pattern Conversion (MEDIUM PRIORITY)

**What cass does**: Rules with excessive harmful feedback are automatically inverted into anti-patterns.

**Example**:
```
Original rule: "Cache auth tokens for performance"
    ↓ (3 harmful marks)
Anti-pattern: "PITFALL: Don't cache auth tokens without expiry validation"
```

**Conversion Criteria**:
```typescript
// Invert to anti-pattern if:
if (harmfulRatio > 0.5 && harmfulCount >= 3) {
    convertToAntiPattern(rule);
}
```

**Why this matters for mmry**:
1. **Learning from mistakes** - Bad patterns become warnings
2. **Automatic conversion** - No manual curation needed
3. **Feedback preservation** - Original harmful feedback informs the anti-pattern

**Implementation**:

```rust
impl Rule {
    /// Check if rule should be converted to anti-pattern
    pub fn should_convert_to_anti_pattern(&self) -> bool {
        let total = self.helpful_count + self.harmful_count;
        if total < 3 {
            return false;
        }

        let harmful_ratio = self.harmful_count as f32 / total as f32;
        harmful_ratio > 0.5 && self.harmful_count >= 3
    }

    /// Convert rule to anti-pattern
    pub fn to_anti_pattern(&self) -> Self {
        let inverted_content = self.invert_content();

        Self {
            id: Uuid::new_v4(),
            content: inverted_content,
            rule_type: RuleType::AntiPattern,
            is_negative: true,
            maturity: Maturity::Candidate,
            reasoning: Some(format!(
                "Inverted from harmful rule {}: {}",
                self.id, self.reasoning.as_deref().unwrap_or("no reasoning")
            )),
            helpful_count: 0,
            harmful_count: 0,
            feedback_events: Vec::new(),
            ..self.clone()
        }
    }

    /// Invert rule content to anti-pattern
    fn invert_content(&self) -> String {
        let content = self.content.trim();

        if content.starts_with("Always ") {
            format!("PITFALL: Don't {}", &content[7..].to_lowercase())
        } else if content.starts_with("Never ") {
            format!("RECOMMENDED: {}", &content[6..].to_lowercase())
        } else if content.starts_with("Use ") {
            format!("PITFALL: Avoid using{} without careful consideration",
                    &content[3..].to_lowercase())
        } else {
            format!("PITFALL: Avoid - {}", content)
        }
    }
}
```

---

### 5. Outcome Recording (MEDIUM PRIORITY)

**What cass does**: Agents can explicitly record session outcomes with which rules were followed or violated.

**Data Model**:
```typescript
interface SessionOutcome {
  id: string;
  status: "success" | "failure" | "mixed";
  rulesUsed: string[];      // Bullet IDs that were followed
  rulesViolated?: string[]; // Bullet IDs that were ignored
  task?: string;
  summary?: string;
  sessionPath?: string;
}
```

**Recording Outcomes**:
```bash
# Record success with rules that helped
cm outcome success b-8f3a2c,b-def456 --summary "Fixed auth bug"

# Record failure with rules that were violated
cm outcome failure b-x7k9p1 --summary "Rule led to wrong approach"

# Apply recorded outcomes to playbook (update scores)
cm outcome-apply
```

**Why this matters for mmry**:
1. **Explicit feedback** - Clear signal about what worked/didn't
2. **Rule attribution** - Know which rules contributed to success/failure
3. **Bulk updates** - Apply feedback to multiple rules at once

**Implementation**:

```sql
CREATE TABLE session_outcomes (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    status TEXT NOT NULL CHECK(status IN ('success', 'failure', 'mixed')),
    task TEXT,
    summary TEXT,
    session_path TEXT,

    -- Rules involved
    rules_used TEXT[],      -- UUIDs of helpful rules
    rules_violated TEXT[],  -- UUIDs of harmful rules

    -- State
    applied BOOLEAN DEFAULT FALSE,
    applied_at DATETIME,

    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

**Rust Implementation**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOutcome {
    pub id: Uuid,
    pub status: OutcomeStatus,
    pub task: Option<String>,
    pub summary: Option<String>,
    pub session_path: Option<String>,

    pub rules_used: Vec<Uuid>,
    pub rules_violated: Vec<Uuid>,

    pub applied: bool,
    pub applied_at: Option<DateTime<Utc>>,

    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutcomeStatus {
    Success,
    Failure,
    Mixed,
}

pub struct OutcomeManager {
    config: OutcomeConfig,
}

impl OutcomeManager {
    /// Record a session outcome
    pub async fn record_outcome(
        &self,
        pool: &SqlitePool,
        outcome: SessionOutcome,
    ) -> Result<()> {
        // Insert outcome
        // Queue for application
    }

    /// Apply pending outcomes to update rule scores
    pub async fn apply_outcomes(&self, pool: &SqlitePool) -> Result<usize> {
        // Get unapplied outcomes
        // For each outcome:
        //   - Add helpful feedback for rules_used
        //   - Add harmful feedback for rules_violated
        //   - Update effective scores
        //   - Recalculate maturity
        // Mark as applied
    }
}
```

**MCP Tools**:
```typescript
mcp.registerTool({
  name: "RecordOutcome",
  description: "Record session outcome with rule attribution",
  inputSchema: {
    type: "object",
    properties: {
      status: { enum: ["success", "failure", "mixed"] },
      task: { type: "string" },
      summary: { type: "string" },
      rulesUsed: { type: "array", items: { type: "string" } },
      rulesViolated: { type: "array", items: { type: "string" } }
    }
  }
});

mcp.registerTool({
  name: "ApplyOutcomes",
  description: "Apply pending outcomes to update rule scores"
});
```

---

### 6. Gap Analysis System (MEDIUM PRIORITY)

**What cass does**: Tracks rule distribution across categories and suggests which sessions to analyze to fill gaps.

**Categories** (standard set):
```typescript
const CATEGORIES = [
  "debugging",      // Error resolution, bug fixing, tracing
  "testing",        // Unit tests, mocks, assertions, coverage
  "architecture",   // Design patterns, module structure, abstractions
  "workflow",       // Task management, CI/CD, deployment
  "documentation",  // Comments, READMEs, API docs
  "integration",    // APIs, HTTP, JSON parsing, endpoints
  "collaboration",  // Code review, PRs, team coordination
  "git",            // Version control, branching, merging
  "security",       // Auth, encryption, vulnerability prevention
  "performance",    // Optimization, caching, profiling
];
```

**Status Thresholds**:
- `critical`: 0 rules (high priority)
- `underrepresented`: 1-2 rules (medium priority)
- `adequate`: 3-10 rules (low priority)
- `well-covered`: 11+ rules (no priority)

**Gap-Targeted Sampling Algorithm**:
```
1. Analyze playbook → Identify critical/underrepresented categories
2. Generate search queries from category keywords:
   - "security" → ["security auth token", "security vulnerability", "security encrypt"]
   - "performance" → ["performance optimize", "performance cache", "performance profile"]
3. Search cass with gap-targeted queries
4. Score each session against gaps:
   - Session matches critical category: +3 points
   - Session matches underrepresented category: +2 points
   - Session matches adequate category: +1 point
5. Sort sessions by gap score (highest first)
6. Filter out already-processed sessions
7. Return top N sessions
```

**Why this matters for mmry**:
1. **Balanced knowledge** - Prevents over-specialization in one area
2. **Efficient onboarding** - Focus analysis where it's needed most
3. **Coverage tracking** - Know which areas are strong vs weak

**Implementation**:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub keywords: Vec<String>,
}

pub const DEFAULT_CATEGORIES: &[Category] = &[
    Category {
        name: "debugging".to_string(),
        keywords: vec![
            "debug".to_string(), "error".to_string(), "fix".to_string(),
            "bug".to_string(), "trace".to_string(), "stack".to_string(),
            "exception".to_string(), "panic".to_string(),
        ],
    },
    Category {
        name: "testing".to_string(),
        keywords: vec![
            "test".to_string(), "mock".to_string(), "assert".to_string(),
            "expect".to_string(), "jest".to_string(), "vitest".to_string(),
            "spec".to_string(), "coverage".to_string(),
        ],
    },
    // ... more categories
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapStatus {
    pub category: String,
    pub rule_count: usize,
    pub status: GapPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapPriority {
    Critical,        // 0 rules
    Underrepresented, // 1-2 rules
    Adequate,        // 3-10 rules
    WellCovered,     // 11+ rules
}

impl GapStatus {
    pub fn from_rule_count(category: &str, count: usize) -> Self {
        let status = match count {
            0 => GapPriority::Critical,
            1..=2 => GapPriority::Underrepresented,
            3..=10 => GapPriority::Adequate,
            _ => GapPriority::WellCovered,
        };

        Self {
            category: category.to_string(),
            rule_count: count,
            status,
        }
    }
}

pub struct GapAnalyzer {
    categories: Vec<Category>,
}

impl GapAnalyzer {
    pub fn new() -> Self {
        Self {
            categories: DEFAULT_CATEGORIES.to_vec(),
        }
    }

    /// Analyze current gap status
    pub async fn analyze_gaps(&self, pool: &SqlitePool) -> Result<Vec<GapStatus>> {
        // Count rules per category
        // Determine priority based on counts
    }

    /// Score a memory/session against gap priorities
    pub fn score_against_gaps(&self, content: &str, gaps: &[GapStatus]) -> f32 {
        let content_lower = content.to_lowercase();
        let mut score = 0.0;

        for gap in gaps {
            if gap.status == GapPriority::WellCovered {
                continue;
            }

            let priority_bonus = match gap.status {
                GapPriority::Critical => 3.0,
                GapPriority::Underrepresented => 2.0,
                GapPriority::Adequate => 1.0,
                GapPriority::WellCovered => 0.0,
            };

            let category = self.categories.iter().find(|c| c.name == gap.category);
            if let Some(cat) = category {
                let matches: usize = cat.keywords
                    .iter()
                    .filter(|kw| content_lower.contains(kw))
                    .count();

                if matches > 0 {
                    score += priority_bonus * matches as f32;
                }
            }
        }

        score
    }
}
```

---

### 7. Trauma Guard (LOW PRIORITY)

**What cass does**: Tracks dangerous patterns that should trigger warnings or refusal.

**Data Model**:
```typescript
interface TraumaEntry {
  id: string;
  severity: "CRITICAL" | "FATAL";
  pattern: string;      // Regex pattern
  scope: "global" | "project";
  projectPath?: string;
  status: "active" | "healed";
  description: string;
  occurrences: number;
  lastOccurrence: string;
}
```

**Usage**:
```bash
# List active traumas
cm trauma list

# Add a dangerous pattern
cm trauma add "DROP TABLE" --description "Database table deletion" --severity critical

# Scan sessions for potential traumas
cm trauma scan --days 30
```

**Integration Points**:
- Pre-tool hooks in Claude Code
- Git pre-commit hooks
- MCP tool checks before operations

**Why this matters for mmry**:
1. **Safety layer** - Prevent destructive operations
2. **Pattern learning** - Learn from past mistakes
3. **Configurable** - Project-specific or global rules

**Implementation** (if desired):

```sql
CREATE TABLE trauma_patterns (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    severity TEXT NOT NULL CHECK(severity IN ('CRITICAL', 'FATAL')),
    pattern TEXT NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('global', 'project')),
    project_path TEXT,

    status TEXT NOT NULL CHECK(status IN ('active', 'healed')),
    description TEXT,

    occurrences INTEGER DEFAULT 0,
    last_occurrence DATETIME,

    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

---

## Features mmry Already Has Better

| Feature | mmry | cass | mmry Advantage |
|---------|------|------|----------------|
| **Search Modes** | 6 (keyword, fuzzy, semantic, bm25, sparse, hybrid) | 2 (keyword + semantic) | mmry has 4 more modes |
| **Sparse Embeddings** | SPLADE++ (neural sparse) | None | Better term importance |
| **Reranking** | BGE, Jina models | None | Higher result quality |
| **NER** | GLiNER integration | None | Entity extraction |
| **Chunking** | Cascading strategy | Basic | Better document handling |
| **Performance** | Rust (compiled) | TypeScript (JIT) | mmry is ~10-50x faster |
| **Deployment** | Single binary | Node.js + Bun | Easier distribution |
| **TUI** | Full Yazi-style TUI | None | mmry has interactive UI |
| **Service Mode** | Background daemon with gRPC | None | Fast warm embeddings |
| **Bridge Blocks** | HMLR routing | None | mmry has conversation tracking |

---

## Recommended Implementation Roadmap

### Phase 1: Feedback & Scoring Foundation (1-2 weeks)
**Goal**: Add confidence decay and maturity tracking to bridge blocks

**Tasks**:
1. Add `feedback_events` table
2. Add feedback columns to `bridge_blocks` (helpful_count, harmful_count, effective_score, maturity)
3. Implement decay calculation (90-day half-life, 4x harmful multiplier)
4. Implement maturity transitions (candidate → established → proven → deprecated)
5. Add MCP tools: `RecordFeedback`, `GetFeedbackHistory`, `RecalculateScores`
6. Update HMLR enrichment to auto-generate rules from bridge block decisions

**Success criteria**:
- Can record helpful/harmful feedback on bridge blocks
- Effective scores are calculated correctly with time decay
- Maturity transitions work as expected
- Anti-pattern conversion is implemented

### Phase 2: Rule/Procedural Memory Layer (2-3 weeks)
**Goal**: Extract actionable rules and provide context queries

**Tasks**:
1. Create `rules` table (or extend bridge blocks with rule_content)
2. Create `rule_embeddings` table for semantic search
3. Implement rule extraction from HMLR bridge block decisions
4. Implement `GetContext` MCP tool (main agent command)
5. Add `AddRule` and `RecordOutcome` MCP tools
6. Implement anti-pattern conversion when harmful ratio > 50%

**Success criteria**:
- Rules are extracted from bridge blocks automatically
- `mmry context "<task>"` returns relevant rules and history
- Anti-patterns are generated from harmful rules
- Outcome recording updates rule scores

### Phase 3: Agent-Native Onboarding (1-2 weeks)
**Goal**: Build playbook from historical sessions using the agent

**Tasks**:
1. Create onboarding state tables (`onboarding_state`, `processed_sessions`, `category_stats`, `session_queue`)
2. Implement gap analysis (critical/underrepresented/adequate/well-covered)
3. Implement gap-targeted session sampling
4. Implement template context generation for session analysis
5. Add MCP tools: `OnboardingStatus`, `SampleSessions`, `GetSessionTemplate`, `MarkSessionProcessed`
6. Add topic hint detection from sessions

**Success criteria**:
- Can check onboarding status and see category gaps
- Session sampling prioritizes gap-filling
- Template context provides rich extraction guidance
- Progress persists across sessions

### Phase 4: Integration & Polish (1 week)
**Goal**: Integrate everything and improve UX

**Tasks**:
1. Update CLI commands to support new features
2. Update TUI to show rule feedback and maturity
3. Add `mmry rules` command (list, add, remove, top, stale, why)
4. Add `mmry onboarding` command group
5. Add `mmry outcome` command group
6. Documentation updates (README.md, examples)
7. Integration tests

**Success criteria**:
- All features accessible via CLI and TUI
- Documentation is complete
- Tests pass

---

## Configuration Additions

```toml
# Add to ~/.config/mmry/config.toml

[scoring]
decay_half_life_days = 90      # Feedback decay half-life
harmful_multiplier = 4.0        # Harmful feedback weight
min_feedback_for_active = 3     # Min feedback to consider "active"
min_helpful_for_proven = 10      # Min helpful for "proven" maturity
max_harmful_ratio_for_proven = 0.1  # Max harmful ratio for "proven"

[onboarding]
enabled = true
target_rule_count = 20           # Target for initial playbook
session_sample_limit = 5         # Sessions per sampling round
auto_analyze = false             # Auto-analyze sampled sessions (CLI only)

[rules]
auto_extract_from_bridge_blocks = true
anti_pattern_threshold_harmful = 0.5   # Convert to anti-pattern if >50% harmful
anti_pattern_min_count = 3           # Minimum harmful count
categories = ["debugging", "testing", "architecture", "workflow", "documentation", "integration", "collaboration", "git", "security", "performance"]
```

---

## MCP Tools Summary

**Phase 1 (Feedback & Scoring)**:
- `RecordFeedback` - Record helpful/harmful feedback
- `GetFeedbackHistory` - Get feedback events for a block
- `RecalculateScores` - Update effective scores and maturity

**Phase 2 (Rules & Context)**:
- `GetContext` - Get relevant rules for a task (MAIN COMMAND)
- `AddRule` - Add a new rule manually
- `RecordOutcome` - Record session outcome with rule attribution
- `ApplyOutcomes` - Apply pending outcomes to update scores

**Phase 3 (Onboarding)**:
- `OnboardingStatus` - Get onboarding progress and gaps
- `SampleSessions` - Get sessions to analyze (gap-targeted)
- `GetSessionTemplate` - Get session with extraction context
- `MarkSessionProcessed` - Mark session as done with rule count

**Phase 4 (Rule Management)**:
- `ListRules` - List all rules with filtering
- `GetRule` - Get detailed rule info
- `RemoveRule` - Deprecate/remove a rule
- `TopRules` - Show top N most effective rules
- `StaleRules` - Show rules without recent feedback
- `RuleWhy` - Show why a rule exists (provenance)

---

## CLI Commands Summary

```bash
# === Feedback & Scoring ===
mmry feedback add <block-id> --type helpful --reason "Helped solve auth bug"
mmry feedback history <block-id>
mmry scores recalculate

# === Rules & Context ===
mmry context "implement auth rate limiting"
mmry rules add "Always check token expiry before auth debugging" --category debugging
mmry rules list --category debugging --maturity proven
mmry rules get <rule-id>
mmry rules remove <rule-id> --reason "Superseded by better pattern"
mmry rules top 10
mmry rules stale --days 90
mmry rules why <rule-id>

# === Outcomes ===
mmry outcome success <rule-id>,<rule-id> --summary "Fixed auth bug"
mmry outcome failure <rule-id> --summary "Rule led to wrong approach"
mmry outcomes apply

# === Onboarding ===
mmry onboarding status
mmry onboarding gaps
mmry onboarding sample --fill-gaps --limit 5
mmry onboarding read <session-path> --template
mmry onboarding mark-done <session-path> --rules-extracted 3
mmry onboarding reset
```

---

## Example Workflows

### Agent Workflow (Main Use Case)

```bash
# 1. Before starting a task, get relevant context
mmry context "implement user authentication" --json
# Returns: relevantBullets, antiPatterns, historySnippets

# 2. Work on task, follow rules

# 3. Record outcome when done
mmry outcome success b-8f3a2c --summary "Implemented auth with JWT tokens"
mmry outcomes apply

# Optional: Leave inline feedback during work
// [mmry: helpful b-8f3a2c] - this rule saved me from a rabbit hole
```

### Manual Workflow (Human)

```bash
# 1. Check onboarding status
mmry onboarding status

# 2. Sample sessions to analyze
mmry onboarding sample --fill-gaps --limit 5

# 3. Read session with rich context
mmry onboarding read ~/.claude/sessions/session-001.jsonl --template

# 4. Add extracted rules
mmry rules add "Always run tests before committing" --category testing
mmry rules add "Check token expiry before auth debugging" --category debugging

# 5. Mark session as processed
mmry onboarding mark-done ~/.claude/sessions/session-001.jsonl --rules-extracted 2
```

---

## Conclusion

The cass memory system offers several innovative features that could significantly enhance mmry's agent workflow capabilities:

### High-Value, Medium-Effort Features:
1. **Confidence decay & maturity tracking** - Prevents stale knowledge, continuous learning
2. **Rule/procedural memory layer** - Converts sessions into actionable, queryable knowledge
3. **Feedback event history** - Transparent learning, debugging

### Medium-Value, Medium-Effort Features:
4. **Agent-native onboarding** - Zero-cost playbook building with gap analysis
5. **Anti-pattern conversion** - Learning from mistakes
6. **Outcome recording** - Explicit session feedback with rule attribution

### Lower-Priority Features:
7. **Trauma guard** - Safety system for dangerous patterns
8. **Multi-source trust** - Distinguishes fact sources (less relevant for local-first mmry)

**Recommended approach**: Implement Phases 1-2 first (6-5 weeks total) to get the core value (confidence tracking + rules), then evaluate whether Phases 3-4 (onboarding + polish) are worth the additional effort for your use case.

**Key differentiator**: mmry's superior search technology (6 modes, sparse embeddings, reranking) combined with cass's learning architecture would create a best-of-both-worlds system: fast, accurate retrieval plus continuous, evidence-based knowledge evolution.
