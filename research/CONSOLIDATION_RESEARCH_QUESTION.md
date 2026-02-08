# Research Question: Optimal Consolidation of Dual-Polarity Procedural Learnings Under Temporal Decay

## The Question

**Given a procedural memory system that continuously extracts dual-polarity learnings (guiding principles and cautionary anti-patterns) from agent sessions, how should we mathematically define a consolidation operator that merges semantically overlapping learnings into a provably minimal, non-contradictory knowledge base while preserving maximal decision-relevant information — and what are the formal guarantees on convergence, information loss, and retrieval performance of such an operator under exponential temporal decay?**

## Context: Our Current System (mmry)

mmry maintains a `learnings` table where each entry is an actionable principle extracted from agent coding sessions. Each learning has:

- **Dual polarity** (`LearningKind`): either *guiding* ("Always check token expiry before auth debugging") or *cautionary* ("PITFALL: Don't cache auth tokens without expiry validation"). These are extracted by separate prompts inspired by EvolveR's dual-extraction paradigm, which showed that distinguishing guiding from cautionary principles outperforms a single undifferentiated extraction pass.

- **Time-decayed confidence scoring**: Each learning accumulates `FeedbackEvent`s (helpful/harmful) whose contribution decays exponentially with a 90-day half-life. The effective score is:

  ```
  S(ℓ) = Σ_{e ∈ helpful(ℓ)} 0.5^(age(e)/τ) − λ · Σ_{e ∈ harmful(ℓ)} 0.5^(age(e)/τ)
  ```

  where τ = 90 days and λ = 4 (harmful multiplier). This is adapted from cass-memory's decay model.

- **Deterministic maturity lifecycle**: candidate → established → proven → deprecated, governed by thresholds on decayed feedback counts and harmful ratios (no LLM call in the transition — per ACE/GitHub Copilot's finding that deterministic curation outperforms LLM-based curation at scale).

- **Semantic deduplication at ingestion**: new learnings with cosine similarity > 0.85 to an existing learning are merged rather than inserted, following EvolveR's two-stage dedup (embedding similarity + semantic equivalence).

- **Category-scoped gap analysis**: learnings are bucketed by category (debugging, testing, security, etc.) to track coverage and prioritize extraction from sessions that fill knowledge gaps.

What the system currently **lacks** is a principled consolidation step — an operation that takes the full set of learnings and produces a *smaller*, *more general*, *equally powerful* set. Right now, learnings accumulate monotonically (modulo dedup and deprecation). There is no mechanism to:

1. Merge two guiding principles that partially overlap into one stronger, more general principle.
2. Annihilate a guiding/cautionary pair that are about the same underlying concept (e.g. "Always validate tokens" and "PITFALL: Don't skip token validation" carry redundant decision-relevant information).
3. Promote a cluster of specific learnings into a single abstract principle (e.g. five "always check X before Y" learnings about different resources → one general "always validate preconditions before operations on shared resources").
4. Prove that the consolidated set preserves the same decision boundary as the original set with respect to any retrieval query.

## Why This Matters

The learnings table is injected into agent prompts at context-retrieval time (`mmry context <task>`). Every learning consumes tokens. A system with 500 learnings where 200 are partially redundant wastes context window budget, dilutes signal, and creates contradictory advice. GitHub Copilot's memory paper showed that just-in-time verification works — but only if the set of memories is compact enough to reason over. EvolveR's key insight is that a *compact, high-quality* experience base outperforms a large noisy one, and that dedup + quality scoring is essential but not sufficient — you also need *abstraction* (specific → general).

## Sub-Questions

### 1. Algebraic Structure of Learnings

Can we define a **semilattice** (or richer algebraic structure) over the space of learnings such that:

- The **join** of two guiding learnings `g₁ ⊔ g₂` produces their least general generalization (the most specific principle that subsumes both)?
- The **meet** of a guiding and cautionary learning about the same concept `g ⊓ c` produces a *refinement* (e.g. "Do X *except* when Y") or annihilates to ⊥ if they fully contradict?
- The ordering `ℓ₁ ≤ ℓ₂` ("ℓ₂ subsumes ℓ₁") is decidable via embedding similarity + entailment, without requiring an LLM call for every pair?

If such a structure exists, consolidation becomes: compute the join of all maximal antichains in the learning poset, weighted by effective score.

### 2. Consolidation as Lossy Compression with Decision-Theoretic Guarantees

Define the **decision value** of a learning set L with respect to a query distribution Q as:

```
V(L, Q) = E_{q ~ Q}[ max_{ℓ ∈ L} relevance(q, ℓ) · S(ℓ) · maturity_weight(ℓ) ]
```

A consolidation operator `C: 2^L → 2^L` is *ε-lossless* if:

```
V(C(L), Q) ≥ (1 - ε) · V(L, Q)   for all Q in some query family
```

**Question**: What is the minimum `|C(L)|` achievable for a given ε, and can we compute C efficiently (polynomial in |L|)? This connects to classical results in set cover and coreset construction — can we adapt coreset theory from computational geometry (where ε-coresets approximate geometric queries) to semantic learning spaces?

### 3. Interaction Between Polarity and Temporal Decay

Our dual-polarity design creates a specific challenge: a guiding principle "Always do X" and a cautionary principle "Never do X in context Y" are not contradictory — they're *complementary* and together form a richer decision rule than either alone. But under temporal decay, if the cautionary principle loses feedback while the guiding principle remains strong, the system may serve dangerous advice.

**Question**: Should consolidation preserve polarity pairs as atomic units? Can we define a **bipolar scoring function** where the value of a guiding principle is *increased* by the presence of a well-supported cautionary refinement (and vice versa), such that:

```
S_bipolar(g, c) > S(g) + S(c)   when g and c are complementary
S_bipolar(g, c) = 0              when g and c fully contradict
```

This would make the consolidation operator polarity-aware: it would never eliminate one half of a complementary pair.

### 4. Staleness Detection and Principled Phase-Out

Our decay model erodes evidence over time but never *removes* a learning. A learning with a single helpful event from 18 months ago has an effective score of `0.5^(540/90) ≈ 0.016` — technically positive, so it survives, yet it carries almost no decision value and occupies space in the retrieval index. The maturity system can only deprecate learnings with a bad harmful ratio; it has no mechanism for deprecating learnings that are simply *forgotten by the world* — never reinforced, never contradicted, just abandoned.

This is a distinct failure mode from low quality. A stale learning may have been excellent advice once, but the codebase evolved, the framework changed, or the team moved on. The system currently cannot distinguish "still valid but untested recently" from "silently obsolete."

**The core question**: How should a procedural memory system identify, quarantine, and phase out stale learnings — and can this be done without false positives on rare-but-valid principles?

#### 4a. Defining Staleness Formally

A learning ℓ is *stale* at time t if it meets some combination of:

- **Evidence desert**: No feedback event (helpful or harmful) within a window W. The current decay function makes old events *weak* but never zero — there is no hard cutoff. Should there be? EvolveR uses a hard pruning threshold (`θ_prune = 0.3` on Laplace-smoothed success rate), which is clean but risks killing a principle that hasn't been *tried* recently, not one that has *failed*.

- **Retrieval neglect**: The learning is never retrieved by `mmry context` queries (it sits in embedding space far from any actual task). This is measurable — we can track retrieval hits per learning — but absence of retrieval isn't proof of obsolescence; it may mean the learning covers a rare edge case that will matter catastrophically when it finally applies.

- **Environmental drift**: The scope key (workspace, framework version, language version) has changed. A learning scoped to `"rust 1.75"` may be stale after `"rust 1.82"` ships. This requires external signals the system currently doesn't ingest.

A formal staleness predicate might combine these:

```
stale(ℓ, t) = (last_feedback_age(ℓ, t) > W_feedback)
            ∧ (retrieval_count(ℓ, [t-W_retrieval, t]) < k_min)
            ∧ (effective_score(ℓ, t) < θ_stale)
```

But what are the right values for W_feedback, W_retrieval, k_min, and θ_stale? These form a multi-dimensional decision boundary. Can we learn them from data, or must they be set by policy?

#### 4b. Phase-Out Strategies

Once a learning is identified as stale, what happens to it? Several strategies exist, each with different tradeoffs:

| Strategy | Description | Pro | Con |
|----------|------------|-----|-----|
| **Hard pruning** | Delete stale learnings (EvolveR's `θ_prune`) | Simple, keeps index small | Irreversible; no recovery if the learning was actually valid |
| **Soft quarantine** | Move to a "dormant" pool, excluded from retrieval but retained in storage | Recoverable; no information loss | Dormant pool grows unboundedly; need a second-stage cleanup |
| **Score floor** | Clamp effective score to 0 when stale; learning remains retrievable only if directly queried | Zero retrieval cost; reversible if new feedback arrives | Pollutes the index with zero-score entries |
| **Tombstoning** | Mark as deprecated with a reason ("stale: no feedback since X") and keep for audit trail | Full provenance; teaches the system *why* things were removed | Never reclaims storage; must filter at query time |
| **Absorption into consolidation** | Stale learnings are preferentially targeted by the consolidation operator — merged into more general active principles before removal | Information is preserved in abstract form; drives consolidation | Requires the consolidation operator to exist first (circular dependency) |

The most promising approach may be a **two-phase lifecycle**: staleness triggers quarantine (soft removal from retrieval), and then the consolidation operator periodically scans the quarantine pool to absorb any residual value into active learnings before final pruning. This connects staleness management to consolidation as two parts of the same knowledge lifecycle.

#### 4c. The Rare-but-Critical Problem

The hardest case is a learning that is rarely relevant but catastrophically important when it is — e.g., "PITFALL: Never run migrations without a backup on the production DB." This learning might go years without feedback because production migrations are rare. Any staleness heuristic based on feedback frequency would kill it.

**Question**: Can we define a **criticality weight** that protects certain learnings from staleness-based phase-out? This might be:

- Derived from the cautionary polarity (cautionary learnings about destructive operations get higher protection)
- Derived from the scope (global learnings are more likely rare-but-critical than task-scoped ones)
- Explicitly set via the `pinned` flag (but this requires human curation, which doesn't scale)
- Inferred from the *content* via a lightweight classifier (e.g., learnings mentioning "production", "data loss", "security", "irreversible" get automatic protection)

Formally, the staleness predicate becomes:

```
stale(ℓ, t) = staleness_signal(ℓ, t) > θ_stale / criticality(ℓ)
```

where `criticality(ℓ) ≥ 1` raises the bar for phase-out. This ensures rare-but-critical learnings require much stronger staleness evidence before removal.

#### 4d. Interaction with Temporal Decay

Our decay model and staleness detection are two overlapping mechanisms that both address the passage of time but at different granularities:

- **Decay** is continuous and score-level: every feedback event weakens gradually, making the learning's *weight* in retrieval ranking decrease over time.
- **Staleness** is discrete and membership-level: at some point, the learning should be *removed* from the active set entirely.

These should compose cleanly. One natural formulation: decay handles the "how much should I trust this?" question (continuous), while staleness handles the "should this even be in the candidate set?" question (binary). The staleness threshold then operates on the *decayed* score, not on raw time — a learning with lots of recent harmful feedback has a low decayed score and should be deprecated (not stale), while a learning with no feedback at all has a near-zero decayed score and should be stale (not deprecated). The maturity lifecycle should distinguish these two exit paths:

```
              helpful
Candidate ──────────→ Established ──────────→ Proven
    │                     │                      │
    │ stale               │ stale                │ stale
    ▼                     ▼                      ▼
 Dormant              Dormant               Dormant (quarantine)
    │                     │                      │
    │ absorbed/pruned     │ absorbed/pruned      │ absorbed/pruned
    ▼                     ▼                      ▼
 Removed              Removed               Removed
    
              harmful ratio
Candidate ──────────→ Deprecated (different exit: quality failure, not staleness)
```

This separates two semantically distinct reasons for leaving the active set: "never confirmed" (staleness) vs "actively disproven" (deprecation). The downstream behavior differs — deprecated learnings might be converted to cautionary principles (as cass-memory suggests), while dormant learnings are silently absorbed or pruned.

### 5. Convergence and Minimality

EvolveR's metric score `s(p) = (c_succ + 1) / (c_use + 2)` (Laplace-smoothed success rate) converges to the true success probability as usage grows, by the law of large numbers. Our decay-weighted score does not have this convergence property because old evidence vanishes.

**Question**: Under what conditions does repeated application of the consolidation operator (including staleness-based phase-out) converge to a fixed point (a *minimal knowledge base*)? Specifically:

- Is there a Lyapunov function over the learning set that decreases with each consolidation step?
- Does the fixed point depend on the order of pairwise merges (i.e., is the operator confluent)?
- Can we bound the number of consolidation steps needed to reach ε-optimality?
- Does the staleness phase-out guarantee that the active set size is bounded (i.e., is there a steady-state equilibrium between extraction rate and phase-out rate)?

### 6. Empirical Evaluation Framework

None of the above is useful without measurable impact. The evaluation should measure:

- **Compression ratio**: `|C(L)| / |L|` — how much smaller is the consolidated set?
- **Decision fidelity**: Given a held-out set of tasks, does `mmry context <task>` return equally good learnings from C(L) vs L? (Measured by retrieval recall@k and downstream task success rate.)
- **Latency**: Retrieval over C(L) should be faster due to smaller set.
- **Contradiction rate**: How often does C(L) serve a guiding principle without its protective cautionary refinement?
- **False positive staleness rate**: How often does the staleness detector quarantine a learning that would have been useful within the next N sessions? (Measured via held-out future session replay.)
- **Rare-critical survival rate**: Of learnings manually tagged as safety-critical, what fraction survives staleness phase-out after 6/12/18 months without feedback?
- **Steady-state size**: Given a constant extraction rate, does `|active(L)|` converge to a bounded value over time, or does it grow without limit?
- **Dormant recovery rate**: Of learnings moved to quarantine, what fraction is later reactivated by new feedback (indicating the quarantine was premature)?

## Related Work

| System | Consolidation Approach | Staleness Handling | Limitation |
|--------|----------------------|-------------------|------------|
| **EvolveR** | Semantic dedup at θ_sim, Laplace-scored quality pruning | Hard prune at `θ_prune = 0.3` on Laplace score | No generalization. Pruning kills untested principles indiscriminately — no distinction between "failed" and "forgotten". |
| **cass-memory** | Anti-pattern conversion when harmful > 50%, manual curation | Confidence decay makes old rules weak; manual `stale` command shows rules without recent feedback | Staleness is surfaced but not acted upon automatically. Conversion is polarity flip, not true consolidation. |
| **GitHub Copilot** | Just-in-time verification against live code citations | Stale memories naturally fail citation verification at read time | No consolidation. Elegant staleness model (freshness = verifiability) but only works for code-grounded memories, not abstract principles. |
| **Reflexion** | Sliding window of self-reflections | FIFO eviction — oldest reflections fall off the window | No quality weighting. Rare-but-critical reflections are evicted purely by age. |
| **MemGPT/Letta** | Filesystem with agent-driven search | No automatic staleness handling; agent may choose to update/delete | No consolidation — relies on LLM's ability to curate. Dormant memories accumulate indefinitely. |
| **Mem0** | Structured summarization + conflict resolution | Conflict resolution may overwrite stale facts | Summarization is lossy without guarantees. No explicit staleness lifecycle. |
| **TITANS** | Neural memory with adaptive weight decay + surprise momentum | Continuous forgetting via gradient-based decay; "surprise" signal retains novel information | Hardware-level (weight space), not applicable to symbolic/textual learnings directly, but the surprise-gated retention is a relevant design pattern. |

None of these systems provide formal guarantees on consolidation quality. The closest work is in **belief revision** (AGM theory) and **knowledge base contraction** (Hansson 1999), but these assume propositional logic — our learnings are natural language with embedding-space semantics.

## Potential Approaches to Explore

1. **ε-coreset construction** over embedding space: Treat each learning as a weighted point in embedding space, construct an ε-coreset that approximates the "coverage" function (max relevance to any query). Well-studied in computational geometry with known polynomial algorithms.

2. **Formal Concept Analysis (FCA)**: Model learnings as objects, categories/scopes as attributes, build the concept lattice, and identify the meet-irreducible elements as the minimal generating set.

3. **Information-theoretic compression**: Define the mutual information between a learning set L and a query distribution Q, then find the minimum-size subset C ⊆ L that preserves (1-ε) of the mutual information. This is the rate-distortion problem.

4. **Bipolar argumentation frameworks**: Model guiding principles as arguments, cautionary principles as attacks, and use Dung's preferred extensions to find the maximal self-consistent learning set. Well-studied with known algorithms for preferred/stable semantics.

5. **Deterministic merge via textual entailment**: Use a lightweight NLI model (not a full LLM) to detect when learning A entails learning B, then keep only A. This is the "subsumption" check. Polynomial in |L|² and fully deterministic.

6. **Surprise-gated retention** (from TITANS/MIRAS): Adapt the neural "surprise" signal to symbolic learnings — a learning's retention priority is proportional to how *unexpected* its retrieval would be given the current active set. A learning that is semantically far from all others (high surprise) is retained even without recent feedback; a learning that is semantically redundant with well-supported neighbors (low surprise) is a prime candidate for staleness-based absorption. This connects staleness detection to the consolidation operator: redundant learnings go stale faster, unique learnings are protected.

7. **Bayesian staleness estimation**: Model each learning's "alive" probability as a Beta distribution updated by feedback events (observation) and time (prior drift). A learning with no observations in W days has its posterior `P(alive | no_feedback, W)` shrink toward a prior that depends on scope and criticality. Phase-out triggers when `P(alive) < θ_alive`. This gives a principled, calibrated staleness probability rather than a hard threshold, and naturally handles the rare-but-critical case (high-criticality prior = slow drift toward staleness).
