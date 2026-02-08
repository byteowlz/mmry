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

### 4. Convergence and Minimality

EvolveR's metric score `s(p) = (c_succ + 1) / (c_use + 2)` (Laplace-smoothed success rate) converges to the true success probability as usage grows, by the law of large numbers. Our decay-weighted score does not have this convergence property because old evidence vanishes.

**Question**: Under what conditions does repeated application of the consolidation operator converge to a fixed point (a *minimal knowledge base*)? Specifically:

- Is there a Lyapunov function over the learning set that decreases with each consolidation step?
- Does the fixed point depend on the order of pairwise merges (i.e., is the operator confluent)?
- Can we bound the number of consolidation steps needed to reach ε-optimality?

### 5. Empirical Evaluation Framework

None of the above is useful without measurable impact. The evaluation should measure:

- **Compression ratio**: `|C(L)| / |L|` — how much smaller is the consolidated set?
- **Decision fidelity**: Given a held-out set of tasks, does `mmry context <task>` return equally good learnings from C(L) vs L? (Measured by retrieval recall@k and downstream task success rate.)
- **Latency**: Retrieval over C(L) should be faster due to smaller set.
- **Contradiction rate**: How often does C(L) serve a guiding principle without its protective cautionary refinement?

## Related Work

| System | Consolidation Approach | Limitation |
|--------|----------------------|------------|
| **EvolveR** | Semantic dedup at θ_sim, Laplace-scored quality pruning | No generalization (specific → abstract). Dedup is binary (merge or don't). |
| **cass-memory** | Anti-pattern conversion when harmful > 50%, manual curation | Conversion is polarity flip, not true consolidation. No merge of similar rules. |
| **GitHub Copilot** | Just-in-time verification against live code citations | No consolidation at all — relies on citation freshness. Memory grows unboundedly. |
| **Reflexion** | Sliding window of self-reflections | Fixed window size, no quality-weighted retention. Old reflections fall off regardless of value. |
| **MemGPT/Letta** | Filesystem with agent-driven search | No consolidation — relies on LLM's ability to search effectively. |
| **Mem0** | Structured summarization + conflict resolution | Summarization is lossy without guarantees. Conflict resolution is heuristic. |

None of these systems provide formal guarantees on consolidation quality. The closest work is in **belief revision** (AGM theory) and **knowledge base contraction** (Hansson 1999), but these assume propositional logic — our learnings are natural language with embedding-space semantics.

## Potential Approaches to Explore

1. **ε-coreset construction** over embedding space: Treat each learning as a weighted point in embedding space, construct an ε-coreset that approximates the "coverage" function (max relevance to any query). Well-studied in computational geometry with known polynomial algorithms.

2. **Formal Concept Analysis (FCA)**: Model learnings as objects, categories/scopes as attributes, build the concept lattice, and identify the meet-irreducible elements as the minimal generating set.

3. **Information-theoretic compression**: Define the mutual information between a learning set L and a query distribution Q, then find the minimum-size subset C ⊆ L that preserves (1-ε) of the mutual information. This is the rate-distortion problem.

4. **Bipolar argumentation frameworks**: Model guiding principles as arguments, cautionary principles as attacks, and use Dung's preferred extensions to find the maximal self-consistent learning set. Well-studied with known algorithms for preferred/stable semantics.

5. **Deterministic merge via textual entailment**: Use a lightweight NLI model (not a full LLM) to detect when learning A entails learning B, then keep only A. This is the "subsumption" check. Polynomial in |L|² and fully deterministic.
