# Consolidating dual-polarity learnings under temporal decay

**A principled consolidation operator for the mmry system can be constructed by layering four mathematical frameworks**: a semilattice induced by NLI-based textual entailment for subsumption detection, submodular facility-location greedy selection for coverage-preserving compression, bipolar argumentation semantics for polarity-aware consistency, and Bayesian Beta-distribution tracking with discounted updates for staleness. This hybrid deterministic+LLM architecture converges to a **δ/(1-γ) ball** around an ideal fixed point — provably bounded but not exact, due to irreducible LLM non-determinism. The closest existing systems are cass-memory (which shares the 90-day half-life and dual-polarity design) and EvolveR (Laplace-scored guiding/cautionary extraction), but neither provides the formal consolidation operator or decision-theoretic guarantees explored here.

---

## Semilattices exist over embeddings but lattices require careful construction

The algebraic foundation for consolidation rests on whether natural-language learnings admit a lattice structure where join equals generalization and meet equals refinement. The answer is nuanced: **a meet-semilattice is achievable through multiple constructions, but a full lattice over arbitrary natural-language statements remains unproven**.

Plotkin (1970) and Reynolds (1970) showed that first-order term anti-unification always yields a unique least general generalization in **O(n) time**, making the set of terms modulo variable renaming a meet-semilattice. Extending this to natural language, Galitsky (2019) demonstrated anti-unification over parse thickets — graphs of parse trees with discourse relations — enabling structural generalization of NL statements. However, this requires syntactic decomposition unavailable in pure embedding spaces.

Three embedding-space constructions provide lattice-like structure. **Order embeddings** (Vendrov et al., 2016) map concepts to ℝⁿ with the constraint x ⪯ y iff ∀i: xᵢ ≥ yᵢ, forming a vector lattice where meet = componentwise max and join = componentwise min. **Box embeddings** (Vilnis et al., 2018) represent concepts as axis-aligned hyperrectangles, with intersection as meet and bounding box as join, forming a bounded lattice with probabilistic interpretation. **Hyperbolic entailment cones** (Ganea et al., 2018) model hierarchical relations as nested geodesically convex cones with closed-form optimal shape, exploiting hyperbolic space's exponential volume growth to efficiently embed tree-like hierarchies — **5-dimensional Poincaré embeddings match 200-dimensional Euclidean ones** for WordNet hypernymy.

The practical bridge from embeddings to subsumption is NLI-based entailment detection. If sentence A entails B, then B is more general (B ≤ A in information ordering), creating a partial order. State-of-the-art cross-encoder NLI models (DeBERTa-v3-large fine-tuned on MNLI+ANLI+WANLI) achieve **~91% accuracy** but require O(n²) pairwise inference. The efficient pipeline uses bi-encoder embeddings for candidate retrieval followed by cross-encoder verification. MacCartney and Manning's natural logic provides a lightweight symbolic alternative via monotonicity calculus — essentially O(n) per pair — but explicitly lacks formal soundness guarantees.

Formal Concept Analysis (Ganter & Wille, 1999) provides the compression connection: concept lattices over formal contexts identify **minimal generating sets** — irredundant attribute sets from which all other concepts can be derived. For KB compression, this means identifying the smallest subset of learnings from which all others are logically derivable. FCA has been applied to knowledge graph compression (Graux et al., 2021), text mining (Cimiano, 2006), and document clustering, with Poelmans et al. (2013) surveying over 1,072 FCA papers in knowledge discovery.

**What's proven vs. conjectured**: Order embeddings provably form a vector lattice (✓). Box embeddings form a bounded lattice (✓). NLI-induced orderings form a preorder (✓), but whether meets/joins exist for arbitrary NL sentence pairs remains unproven (✗). Whether componentwise lattice operations in embedding space produce linguistically meaningful generalizations is empirically promising but formally uncharacterized (✗).

---

## Submodular greedy gives the best compression guarantees

The consolidation objective — preserve decision value max(relevance × score × maturity) within (1-ε) for all queries — is a **max-type objective**, which standard ε-coreset theory does not directly address. Coresets (Feldman & Langberg, 2011) preserve sum-type objectives within (1±ε), with dimension-independent size bounds of **Ω(k/ε²)** for k-clustering in Euclidean spaces. Cohen-Addad et al. (STOC 2022) proved this bound is tight. Crucially, the curse of dimensionality does not limit coreset size for clustering — a key result for high-dimensional embedding spaces.

For the max-relevance preservation objective, **submodular facility-location maximization** provides the right framework. Define f(S) = 𝔼*q[max*{p∈S} relevance(q,p) · score(p) · maturity(p)], where expectation is over a query distribution. This is a monotone submodular function (sum of pointwise maxima), and the greedy algorithm achieves a **(1 - 1/e) ≈ 63.2% approximation** guarantee in O(nk) time — tight under P ≠ NP by Feige (1998). The stochastic greedy variant (Mirzasoleiman et al., 2015) achieves (1-1/e-ε) in **O(n log(1/ε)) time** — linear in the KB size. Streaming submodular maximization (Badanidiyuru et al., 2014) achieves 1/2-ε with O(k/ε) memory in a single pass, enabling online consolidation.

Rate-distortion theory provides the information-theoretic lower bound on compression quality. Recent work on semantic rate-distortion (Akyol, 2026) derives "semantic waterfilling" solutions establishing that architectural compute limits act as implicit rate constraints. Applied to KBs, the "knee point" of the rate-distortion curve typically achieves **~50% distortion reduction at ~30% of total rate cost**, providing a principled stopping criterion for compression aggressiveness. The MDL principle (Rissanen, 1978) offers a complementary model-selection criterion: choose the coreset size minimizing L(coreset) + L(queries|coreset) — description length of the compressed set plus expected retrieval loss.

The practical consolidation pipeline combines:

- **JL dimensionality reduction** to O(log n/ε²) dimensions preserving inner products within (1±ε)
- **Submodular greedy selection** of k representative learnings with (1-1/e) coverage guarantee
- **Sensitivity sampling** (Braverman et al., 2021) for sum-type objectives requiring (1±ε) guarantees, with coreset size O(t·log t·d/ε²) where t is total sensitivity

---

## Bipolar argumentation models guiding-cautionary pairs with formal consistency

Dung's abstract argumentation (1995) provides the foundation: arguments + attack relation, with preferred extensions corresponding to **maximal consistent subsets** of a knowledge base. Cayrol and Lagasquie-Schiex's Bipolar Argumentation Frameworks (BAFs) extend this with an explicit support relation, directly modeling the guiding/cautionary duality:

- **Guiding principles → support relation (R_sup)**: accepting a guiding principle propagates acceptance to supported conclusions under deductive support semantics
- **Cautionary principles → attack relation (R_att)**: cautionary principles attack arguments they contradict, creating negative interactions
- **Derived interactions**: supported attacks (attack amplified by support chain), mediated attacks (support chain leading to an attacker), creating rich indirect relationships

The computational complexity landscape is well-characterized. **Grounded semantics** — the unique minimal complete extension — is computable in **O(n²)** and provides the most conservative consolidation. **Preferred/stable semantics** are NP-complete for credulous reasoning and Π₂ᴾ-complete for skeptical reasoning, but become tractable for bounded-treewidth, bipartite, and odd-cycle-free frameworks. Practical solvers (ASPARTIX via ASP, ConArg via constraint programming, ArgSemSAT via SAT) handle frameworks with thousands of arguments efficiently.

**Superadditive value** of complementary guiding+cautionary pairs arises naturally in gradual bipolar semantics. In QBAFs (Quantitative Bipolar Argumentation Frameworks), the h-categorizer scoring σ(a) = 1/(1 + Σ σ(attackers)) means a guiding principle that supports an argument *while* its paired cautionary principle attacks competing arguments creates mutual defense — the argument's strength exceeds what either principle alone achieves. Amgoud and Ben-Naim (IJCAI 2018) formalized weighted bipolar evaluation with principles including Franklin balance (equal-strength attackers and supporters cancel out) and resilience (positive basic strength survives any attack). The Shapley-value approach (Amgoud, Ben-Naim & Vesic, 2017) directly quantifies marginal contributions and superadditivity in argument coalitions.

For mmry specifically, the recommended architecture uses **grounded semantics for conservative automated consolidation** (polynomial, unique, deterministic) and **preferred semantics for periodic deeper consistency review** (NP-hard but practical with modern SAT solvers for realistic KB sizes of hundreds to low thousands of learnings).

---

## Bayesian staleness with multi-tier protection solves the rare-but-critical problem

AGM belief revision (Alchourrón, Gärdenfors & Makinson, 1985) establishes that **epistemic entrenchment directly corresponds to confidence scoring**: beliefs with higher entrenchment resist contraction, exactly as high-confidence learnings should resist phase-out. The key insight is that temporal decay reduces entrenchment rank (confidence) rather than performing immediate contraction — contraction occurs only when confidence falls below a threshold or an outright contradiction is detected. Kernel contraction (Hansson, 1994) is preferred over partial meet contraction for implementation because it works directly on finite, non-closed belief bases using incision functions — computationally tractable and suitable for iterative belief change.

**Bayesian confidence tracking via Beta distributions** provides the optimal probabilistic framework. Each learning maintains Beta(α, β) parameters updated by positive (α += 1) and negative (β += 1) feedback, with posterior mean α/(α+β) as the confidence estimate. Temporal decay applies the discount factor from Discounted Thompson Sampling (Raj & Kalyani, 2017): **α ← γ·α, β ← γ·β** at each time step, where γ = exp(-ln2/90) for a 90-day half-life. This elegantly ensures that absent feedback, the posterior converges back toward the prior — the learning "forgets" its accumulated evidence. For non-stationary environments, Qi et al. (2023) proved that Discounted TS achieves regret Õ(√(TB_T)) for abruptly changing environments.

TITANS (Behrouz et al., Google Research, 2024) introduces **surprise-gated retention**: the surprise signal ℓ(M*{t-1}; k_t, v_t) = ||M*{t-1}(k_t) - v_t||² determines storage priority. Large prediction errors (surprising inputs) trigger strong memory updates; predictable inputs are mostly ignored. Adapted to symbolic KBs: when a new learning arrives, compute its semantic distance from existing KB predictions — high surprise → high initial entrenchment and slow decay; low surprise → candidate for deduplication. MIRAS (Google Research, 2025) generalizes this, showing all sequence models can be viewed as associative memories with four design choices: memory architecture, attentional bias, retention gate, and optimization algorithm.

The rare-but-critical problem — protecting infrequently-accessed but safety-critical knowledge from decay — requires a **multi-tier decay architecture**:

- **Tier 0 (Immutable)**: Safety-critical and foundational knowledge with zero decay; removal requires explicit human authorization. Analogous to TITANS' persistent memory — learned, input-independent parameters that store task-invariant knowledge
- **Tier 1 (Slow Decay)**: Important operational knowledge with γ ≈ 0.999 (half-life ~1,900 days)
- **Tier 2 (Standard Decay)**: Normal learnings with γ ≈ 0.992 (90-day half-life)
- **Tier 3 (Fast Decay)**: Ephemeral context-specific learnings with γ ≈ 0.9 (half-life ~6.5 days)

HALO (2025) provides an additional mechanism: predicting per-relation half-lives using temporal fact attention, recognizing that different types of knowledge ("which framework version to use" vs. "never store secrets in plaintext") have vastly different natural lifetimes. The First Principles Framework (2026) found that **23% of architectural decisions had stale evidence within two months**, with 86% discovered only during incidents — underscoring the need for proactive staleness detection rather than passive decay.

---

## Thompson Sampling with hierarchical priors solves cold start optimally

For feedback ingestion and cold start, **Thompson Sampling with hierarchical Bayesian priors** is the recommended algorithm. It achieves optimal problem-dependent regret matching the Lai-Robbins lower bound — (1+ε) Σ ln T / d(μᵢ, μ\*) — while naturally handling the cold-start problem through informative priors and remaining robust to delayed/batched feedback.

The cold-start mapping is direct: a newly extracted learning with zero feedback equals a new bandit arm with unknown reward distribution. Three complementary strategies address this. **Empirical Bayes** estimates population-level Beta(α₀, β₀) priors from existing learnings, then uses these for new entries — this implements Laplace smoothing as a special case when α₀ = β₀ = 1, giving the EvolveR formula s(p) = (c_succ + 1)/(c_use + 2). **Warm-Starting** (Oetomo et al., 2023) transfers contextual bandit parameters from related domains, with proven regret bounds showing faster convergence. **Meta-Thompson Sampling** (Kveton et al., 2021) maintains a meta-posterior over task distributions, achieving improved regret scaling across tasks.

For multi-source feedback, the **Dawid-Skene model** (1979) jointly estimates source reliability and true learning quality without requiring ground truth, using EM to infer per-source confusion matrices. The Multiplicative Weights Update framework (Littlestone & Warmuth, 1989) provides an alternative with regret O(√(T ln N)) that down-weights unreliable sources multiplicatively. The composite scoring function becomes:

**Score = Q_posterior(i) × R(source(i)) × Relevance(i, context) × Decay(t)**

where Q_posterior is the Beta posterior mean, R(source) is estimated source reliability, Relevance is embedding similarity to the current task, and Decay is exponential time decay. Non-stationary variants (Discounted UCB, Sliding-Window UCB — Garivier & Moulines, 2011) handle the fact that learning value changes over time, with regret O(√(Υ_T KT ln T)) where Υ_T is the number of change points.

---

## RLM decomposition enables scalable consolidation with bounded convergence

The Recursive Language Models paper (Zhang, Kraska & Khattab, MIT CSAIL, arXiv 2512.24601) introduces an **out-of-core paradigm** treating the LLM context window as "main memory" and the full input as an external environment accessed via REPL-based recursive decomposition. Rather than cramming all learnings into a single context window, RLMs load the KB as a Python variable, let the LLM programmatically examine and partition it, and recursively invoke itself on subsets. This handles inputs **up to 100× beyond model context windows** — tested at 262K tokens. RLM-Qwen3-8B outperforms base Qwen3-8B by 28.3% on average. For KB consolidation, this means hierarchical decomposition: consolidate small groups, then consolidate the consolidations, with no information lost through premature summarization.

The formal properties of LLM-based consolidation operators require careful analysis. **Exact idempotency is impossible** due to GPU non-determinism, sampling stochasticity, and floating-point arithmetic — even at temperature=0, repeated identical queries produce different outputs. **Monotonicity (|output| ≤ |input|) must be enforced programmatically**, not relied upon from the LLM. **Bounded divergence is achievable**: if ||C(x) - C'(x)|| ≤ δ across runs, this composes with contractivity for convergence guarantees.

The **hybrid deterministic+LLM architecture** is the practical synthesis:

- **Deterministic layer** (cheap, provable): exact deduplication via hashing (O(n)), threshold-based merging via embedding similarity, NLI-based subsumption removal, syntactic normalization
- **LLM layer** (expensive, semantic): generalization of similar learnings, conflict resolution between contradictions, abstraction from specific instances to general principles

Convergence theory provides strong guarantees for this hybrid. The **Banach Contraction Mapping Theorem** states that if the consolidation operator C is contractive — d(C(x), C(y)) ≤ γ·d(x,y) for γ < 1 — iterates converge to a unique fixed point at rate γⁿ. With bounded LLM noise δ, the noisy iterates converge to a **δ/(1-γ) ball** around the true fixed point. **Tarski's Fixed-Point Theorem** guarantees that monotone operators on complete lattices have fixed points — applicable when the powerset of learnings ordered by ⊆ forms the lattice and consolidation is monotone. The total learning count |KB| serves as a natural Lyapunov function: if consolidation strictly reduces count (enforceable programmatically), convergence occurs in at most |KB₀| steps.

For steady-state equilibrium, queueing theory gives the balance condition: with extraction rate λ and consolidation rate μ, the KB stabilizes at size **λ/(μ-λ)** when ρ = λ/μ < 1. Adaptive consolidation frequency — triggering more aggressively when KB size exceeds a threshold — ensures μ > λ. For probabilistic term rewriting (Avanzini, Dal Lago, Yamada, 2018), almost-sure termination requires that the probability of reaching a normal form equals 1, with positive AST additionally requiring finite expected derivation length. Robbins-Monro conditions (Σ aₙ = ∞, Σ aₙ² < ∞) provide convergence for stochastic approximation, applicable when consolidation is viewed as noisy gradient descent on a KB quality objective.

---

## How existing systems compare on consolidation and decay

Among the seven systems surveyed, only **two implement explicit dual polarity**: EvolveR (guiding vs. cautionary principles with Laplace-scored quality pruning) and cass-memory (rules vs. auto-inverted anti-patterns with a 4× harmful multiplier). cass-memory's anti-pattern conversion — automatically inverting rules with >60% harmful ratio into active warnings — is the most sophisticated polarity mechanism. The table below captures the key architectural differences:

| Dimension         | cass-memory           | EvolveR                | Copilot Memory                   | Mem0              | MemGPT/Letta        | Reflexion   | TITANS             |
| ----------------- | --------------------- | ---------------------- | -------------------------------- | ----------------- | ------------------- | ----------- | ------------------ |
| Dual polarity     | ✅ Auto-inversion     | ✅ Guiding/cautionary  | ❌                               | ❌                | ❌                  | ❌          | N/A                |
| Temporal decay    | 90-day half-life      | Score pruning only     | 28-day TTL                       | LRU/decay         | None (agent-driven) | FIFO window | Weight decay       |
| Consolidation     | Deterministic curator | LLM semantic dedup     | JIT verification                 | ADD/UPDATE/DELETE | Agent self-editing  | None        | Gradient updates   |
| Conflict handling | Evidence gate         | Merge under best match | Citation contradiction detection | Conflict Detector | Agent decides       | None        | Gradient overwrite |

**cass-memory is architecturally closest to mmry** — sharing the 90-day half-life, confidence tracking, maturity lifecycle (candidate → established → proven → deprecated), and the critical design decision of a **deterministic curator** (no LLM in the curation loop, preventing context collapse from iterative LLM drift). GitHub Copilot's JIT verification represents an alternative philosophy: "scale by subtraction" — no offline curation at all, pushing verification to runtime via code citation checking. This avoids complex state management but limits the system to citation-verifiable knowledge. Mem0's four-operation classification (ADD/UPDATE/DELETE/NOOP) provides the closest thing to a principled consolidation operator, but lacks formal guarantees and doesn't handle polarity.

---

## Toward a complete consolidation operator for mmry

The research converges on a specific architecture for mmry's consolidation operator, combining provable guarantees with practical polynomial-time algorithms.

**The consolidation pipeline** should execute in three phases. First, the **deterministic layer** performs exact deduplication (O(n) via hashing), NLI-based subsumption removal using a bi-encoder pre-filter followed by cross-encoder verification (~91% accuracy), and threshold-based merging of embeddings within a similarity radius. Second, **submodular greedy selection** identifies the minimum covering set preserving (1-1/e) of decision value via stochastic greedy in O(n log(1/ε)) time. Third, the **LLM layer** performs periodic semantic generalization via RLM-based recursive decomposition, with programmatic enforcement of monotonicity (output count ≤ input count) and convergence monitoring via the |KB| Lyapunov function.

**The scoring system** combines Beta-Bayesian confidence tracking (Beta(α,β) updated by feedback and discounted by γ per time step), source reliability estimation (per-source Beta posteriors or Dawid-Skene confusion matrices), contextual relevance (embedding similarity), and surprise-gated initial entrenchment (high-novelty learnings start with higher α₀). Thompson Sampling with these hierarchical priors provides optimal exploration-exploitation with regret matching the Lai-Robbins lower bound.

**Polarity-aware consistency** uses a Quantitative Bipolar Argumentation Framework where guiding principles create support edges and cautionary principles create attack edges. The grounded extension (O(n²), unique, deterministic) provides the conservative automated consistency check; preferred extensions (NP-hard but practical for realistic KB sizes via SAT solvers) provide deeper periodic review. Complementary guiding+cautionary pairs receive superadditive scores via gradual bipolar semantics.

The system converges to a δ/(1-γ) ball around the ideal consolidated state — where δ bounds LLM per-step divergence and γ is the contraction rate — with steady-state KB size λ/(μ-λ) when consolidation rate exceeds extraction rate. The multi-tier decay architecture protects rare-but-critical knowledge by exempting Tier 0 items from decay entirely, while surprise-gated retention ensures that high-novelty learnings (which are valuable precisely because they are counterintuitive) receive higher initial entrenchment and slower decay rates.

The key open problems remain: proving that NLI-induced orderings satisfy full lattice axioms for arbitrary NL statements, closing the gap between the (1-1/e) submodular guarantee and a tighter (1-ε) bound for max-type objectives, and characterizing probabilistic confluence for LLM-based rewrite rules. Despite these theoretical gaps, the proposed architecture provides polynomial-time algorithms with formal bounds sufficient for practical deployment.

