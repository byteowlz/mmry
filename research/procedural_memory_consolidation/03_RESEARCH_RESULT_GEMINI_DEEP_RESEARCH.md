# **Optimal Consolidation of Dual-Polarity Procedural Learnings Under Temporal Decay**

## **1\. Executive Summary**

The transition from static retrieval-augmented generation (RAG) to dynamic procedural memory systems represents a pivotal evolution in autonomous agent architecture. Current systems, such as mmry, excel at accumulating experience but suffer from monotonic growth, leading to index saturation, contradictory advice, and "context rot"—a phenomenon where increased context length degrades reasoning performance.1 This report addresses the mathematical and algorithmic definition of a **Consolidation Operator** designed to transform a raw, accumulating log of dual-polarity learnings (guiding principles and cautionary anti-patterns) into a provably minimal, chemically consistent knowledge base.  
Our analysis integrates four distinct theoretical frameworks: **Algebraic Lattice Theory** for semantic generalization via Least General Generalization (LGG); **Quantitative Bipolar Argumentation Frameworks (QBAFs)** for modeling the synergistic interaction between complementary polarity pairs; **Epsilon-Coreset Theory** for ensuring retrieval utility is preserved during compression; and **Recursive Language Models (RLMs)** as the computational engine for execution.  
We propose a formal consolidation operator $C$ that functions as a projection onto a semantic join-semilattice, constrained by a decision-theoretic utility function. We demonstrate that under specific conditions—namely, Lipschitz-continuous update functions and a "surprise-gated" retention mechanism inspired by the TITANS architecture 3—the knowledge base converges to a stable fixed point. Furthermore, we define a "hybrid" implementation strategy where deterministic algorithms handle structural optimization (clustering, decay) while Recursive Language Models manage semantic arbitration (generalization, contradiction resolution) within a Python REPL environment, enabling infinite-context processing without degradation.2  
This framework moves procedural memory from a passive storage paradigm to an active, self-optimizing cognitive process, ensuring that agents do not merely remember their past, but distill wisdom from it.

## ---

**2\. The Challenge of Monotonic Memory Accumulation**

The mmry system operates on a premise of continuous learning, extracting actionable principles from every agent session. While this ensures high recall of specific experiences, it introduces a critical pathology: **Monotonic Accumulation**. In standard RAG systems, adding more documents generally improves performance up to a saturation point. However, in procedural memory, where retrieved items are _instructions_ rather than just facts, redundancy and contradiction are actively harmful.

### **2.1 Context Rot and Decision Boundary Erosion**

Recent research into long-context performance, specifically the analysis of "Context Rot" by Zhang et al. (2025), reveals that the reasoning capabilities of Large Language Models (LLMs) degrade as the relevant information becomes diluted by irrelevant or redundant tokens.1 In the context of mmry, if an agent retrieves 50 learnings to answer a query, and 40 of them are minor variations of "Check auth tokens," the model's attention mechanism is dispersed, often leading to hallucination or failure to adhere to the core principle.  
Furthermore, the decision boundary of the agent—the implicit line separating "safe" actions from "unsafe" ones—becomes fuzzy. A set of specific, overlapping learnings creates a jagged decision boundary that is hard to generalize. A consolidated principle ("Always validate preconditions for shared resources") creates a smooth, robust boundary. The lack of such consolidation prevents the system from achieving **Inference-Time Scaling**, where the model's performance improves with the quality, not just quantity, of its context.2

### **2.2 The Dual-Polarity Conflict**

The unique strength of mmry is its dual-polarity structure:

- **Guiding Principles ($g$):** Prescriptive heuristics (e.g., "Use exponential backoff").
- **Cautionary Anti-patterns ($c$):** Proscriptive constraints (e.g., "PITFALL: Do not use simple sleep").

Without consolidation, these polarities can become decoupled. If a guiding principle $g$ ("Use X") has a high score due to frequent use, but its specific cautionary refinement $c$ ("Don't use X if Y") decays due to rarity, the agent is left with dangerous advice. A naive consolidation approach might view $g$ and $c$ as contradictory and attempt to annihilate them. A rigorous operator must recognize them as **complementary**—together forming a complex rule $g \\wedge c$ (Do X _unless_ Y). This requires a formal argumentation framework to model the "synergy" between disparate polarities.4

## ---

**3\. Algebraic Structure of Procedural Learnings**

To define the consolidation operator mathematically, we must first establish the algebraic structure of the space $\\mathcal{L}$ containing all possible learnings. We posit that valid procedural learnings form a **Join-Semilattice** ordered by semantic subsumption.

### **3.1 The Subsumption Order ($\\preceq$)**

Let $\\mathcal{L}$ be the set of all valid natural language principles. We define a partial order $\\preceq$ on $\\mathcal{L}$ such that for any two learnings $\\ell\_1, \\ell\_2 \\in \\mathcal{L}$, $\\ell\_1 \\preceq \\ell\_2$ ($ \\ell_2$ subsumes $\\ell\_1$) if and only if:

1. **Scope Inclusion:** The set of contexts in which $\\ell\_2$ is applicable is a superset of the contexts for $\\ell\_1$.
2. **Entailment:** The semantic assertion of $\\ell\_2$ logically entails $\\ell\_1$.

For example:

- $\\ell\_1$: "Use pydantic.BaseModel for request validation in FastAPI endpoints."
- $\\ell\_2$: "Use rigorous schema validation for all API inputs."  
  Here, $\\ell\_1 \\preceq \\ell\_2$. $\\ell\_2$ is the more general principle. It covers FastAPI (and Flask, Django) and pydantic (and marshmallow).

### **3.2 Least General Generalization (LGG) as the Join Operator ($\\sqcup$)**

The consolidation of a set of specific learnings into a single abstract principle is mathematically equivalent to computing their **Least General Generalization (LGG)**. In the semilattice $\\langle \\mathcal{L}, \\preceq \\rangle$, the join of two elements $\\ell\_1 \\sqcup \\ell\_2$ is the unique element $\\ell\_{sup}$ such that:

- $\\ell\_1 \\preceq \\ell\_{sup}$ and $\\ell\_2 \\preceq \\ell\_{sup}$.
- For any other $\\ell'$ that subsumes both, $\\ell\_{sup} \\preceq \\ell'$.

#### **3.2.1 Symbolic vs. Semantic Anti-Unification**

In symbolic logic and automated bug-fixing systems like **Getafix** 5, LGG is computed via **anti-unification**. Given two Abstract Syntax Trees (ASTs), anti-unification produces a pattern that retains common substructures and replaces differing nodes with variables (or "holes").

- Tree 1: call(foo, arg1)
- Tree 2: call(bar, arg1)
- LGG: call(Hole_1, arg1)

For natural language learnings, we must define **Semantic Anti-Unification**. This operates in the embedding space. If $v(\\ell)$ is the vector representation of $\\ell$, the LGG corresponds to finding a vector $v\_{gen}$ that minimizes the distance to the centroid of $v(\\ell\_1)$ and $v(\\ell\_2)$ while maximizing the "coverage" volume.  
Research on **XML Prompting** and logical lattices suggests that arbitrary meets (common specializations) and joins (generalizations) can be formed by synchronized union with conflict resolution.6 In our system, the consolidation operator $C$ iterates through the lattice, identifying clusters of learnings $S \= \\{ \\ell\_1,..., \\ell\_k \\}$ where the "cost" of moving to $\\bigsqcup S$ (loss of specificity) is outweighed by the "gain" (reduction in set size).

### **3.3 The Meet Operator ($\\sqcap$) and Contradiction Resolution**

The **meet** operation $\\ell\_1 \\sqcap \\ell\_2$ represents the greatest lower bound—the specific intersection of two principles.

- **Refinement:** If $\\ell\_1$ is "Use caching" and $\\ell\_2$ is "Avoid stale data," their meet is "Use caching with invalidation strategies." This is a valid refinement.
- **Contradiction:** If $\\ell\_1$ is "Always cache" and $\\ell\_2$ is "Never cache," their meet approximates bottom ($\\bot$), the empty set of valid actions.

The consolidation operator uses $\\sqcap$ to detect conflicts. If $\\ell\_1 \\sqcap \\ell\_2 \\approx \\bot$, the system must invoke a conflict resolution strategy (likely prioritizing the learning with the higher confidence score $S(\\ell)$), or, if they are of opposite polarity, bind them as a bipolar unit.

## ---

**4\. Consolidation as Lossy Compression: Epsilon-Coresets**

We frame the consolidation problem as constructing a **coreset** of the knowledge base. A coreset is a small, weighted subset of a dataset that approximates the original dataset's properties (usually a loss function) within a multiplicative error of $(1 \\pm \\epsilon)$.7

### **4.1 Decision Value Preservation**

Let $V(L, q)$ be the value of the knowledge base $L$ for a query $q$:

$$V(L, q) \= \\max\_{\\ell \\in L} \\left( \\text{Relevance}(q, \\ell) \\cdot S(\\ell) \\right)$$  
We seek a consolidated set $C(L)$ such that for all queries $Q$ in the domain distribution:

$$\\left| V(L, q) \- V(C(L), q) \\right| \\le \\epsilon \\cdot V(L, q)$$

### **4.2 Cluster-Pruning and Expected Mutual Information**

Standard random sampling is insufficient for constructing this coreset because rare, high-value learnings (outliers in the embedding space) would be lost. Recent work on **Coreset-based Dual Retrieval (CoDR)** 9 provides a superior method: **Cluster-Pruning**.  
The CoDR framework proves that to maximize **Expected Mutual Information (EMI)** between the retrieved context and the task, one must maximize the semantic diversity of the selected subset. The algorithm is as follows:

1. **Clustering:** Partition the embedding space of $L$ into $k$ clusters $\\{K\_1,..., K\_k\\}$.
2. **Complexity Estimation:** For each cluster, calculate its "complexity" (variance or intrinsic dimension).
3. **Pruning:** Retain samples from clusters proportional to their complexity. Simple, dense clusters (redundant learnings) are heavily pruned or replaced by their centroid (LGG). Complex, sparse clusters (nuanced edge cases) are preserved.

This method guarantees that the decision boundary is preserved. Specifically, the "support vectors" of the knowledge space—those learnings that define the edges of valid behavior—are retained, while the "interior points" (redundant confirmations) are compressed.10

### **4.3 Information-Theoretic Bounds**

The "epsilon" in our $\\epsilon$-lossless compression is bound by the **Lipschitz constant** of the embedding model. If the mapping from text to vector space is $K$-Lipschitz, and we replace a cluster of radius $r$ with its center, the maximum error in relevance scoring is $K \\cdot r$. By setting a strict threshold on the cluster radius (e.g., cosine similarity \> 0.85, as used in mmry), we strictly bound the information loss $\\epsilon$.

## ---

**5\. Interaction Between Polarity and Temporal Decay**

The consolidation of dual-polarity learnings requires handling the interaction between time-decayed scores and semantic opposition. We utilize **Quantitative Bipolar Argumentation Frameworks (QBAFs)** to model this.4

### **5.1 Bipolar Argumentation and Synergy**

In a QBAF, interactions are binary: Attack ($R^-$) or Support ($R^+$).

- **Contradiction:** A guiding learning $g$ and a cautionary learning $c$ act as mutual attackers if they dictate opposite actions in the same context.
- **Complementarity:** If $c$ represents a _necessary exception_ or _boundary condition_ for $g$, it acts as a **Support**.

The user asks for a "bipolar scoring function" where the value of a pair is greater than the sum of its parts. This is supported by the **Synergy** properties observed in recent argumentation literature.12 We define the effective score of a consolidated pair $\\langle g, c \\rangle$ as:

$$S\_{syn}(g, c) \= \\alpha\_{\\text{agg}}(S(g), S(c)) \+ \\beta \\cdot \\mathbb{I}(\\text{Complementary}(g, c))$$  
where $\\mathbb{I}$ is an indicator function for semantic complementarity (e.g., $c$ entails "Exception to $g$").  
This mechanism is crucial for **safety**. If $g$ ("Delete temp files") has a high score, and $c$ ("Don't delete if lock file exists") has a low score due to decay, treating them independently might lead to $c$ being pruned. By binding them into a single node with score $S\_{syn} \> S(g)$, we ensure the safety constraint survives even if the specific feedback for it is old.

### **5.2 Convergence Under Decay: The Contraction Principle**

The scores in mmry decay exponentially: $S\_{t+1} \= S\_t \\cdot \\delta \+ \\Delta$. For the knowledge base to be stable, the score update function must converge to a fixed point. Potyka (2019) proved that in Bipolar Argumentation Frameworks, convergence is guaranteed if the update function satisfies the **Contraction Principle** (Banach Fixed Point Theorem).4 Specifically, the update function $f$ must be a contraction mapping, meaning its Lipschitz constant $\\lambda \< 1$.  
For our decay model, the decay factor $\\delta \\in (0, 1)$ acts as the contraction coefficient.

$$\\| f(S) \- f(S') \\| \\le \\delta \\| S \- S' \\|$$  
Since $\\delta \= 0.5^{1/90} \\approx 0.992 \< 1$, the scoring system is formally contractive. This guarantees that for any stream of feedback, the scores of the learnings will converge to a unique steady state, preventing runaway inflation or chaotic oscillation.  
**Trade-off:** There is a known trade-off between convergence guarantees and "open-mindedness" (the ability of the system to change its mind).4 Highly contractive systems (fast decay) are stable but forget too quickly. Our 90-day half-life balances this, but it necessitates a separate mechanism for handling "rare-but-critical" knowledge that would otherwise decay to zero.

## ---

**6\. Staleness Detection: The TITANS Surprise Metric**

Standard exponential decay treats all lack of feedback as evidence of obsolescence. This is incorrect for **Safety Constraints**, which are rarely triggered but permanently valid. To solve this, we adapt the **Surprise-Gated Retention** mechanism from the **TITANS** neural memory architecture.3

### **6.1 Defining Surprise in Procedural Memory**

In TITANS, a memory fragment is retained if it generates a high gradient (surprise) with respect to the current model state. It uses a "surprise metric" based on the gradient of the loss:

$$\\text{Surprise}(x\_t) \= \\| \\nabla\_{\\theta} \\mathcal{L}(x\_t) \\|$$  
Combined with a momentum term, this filters out redundant data while keeping "unexpected" data.  
For mmry, we define the **Semantic Surprise** of a learning $\\ell$:

$$\\text{Surprise}(\\ell) \= 1 \- \\max\_{\\ell' \\in L \\setminus \\{\\ell\\}} \\text{Entailment}(\\ell', \\ell)$$  
If existing learnings (or the base LLM) already entail $\\ell$, its surprise is 0\. If $\\ell$ represents unique, un-entailed knowledge, its surprise is 1\.

### **6.2 The Modified Decay Function**

We propose modifying the decay function to be inversely proportional to criticality and surprise.

$$S\_{eff}(\\ell, t) \= S\_0 \\cdot \\delta^{\\frac{t}{\\omega(\\ell)}}$$  
Where $\\omega(\\ell)$ is the **Criticality Weight**.

$$\\omega(\\ell) \= 1 \+ \\gamma\_1 \\cdot \\mathbb{I}(\\text{is\\\_cautionary}(\\ell)) \+ \\gamma\_2 \\cdot \\text{Surprise}(\\ell)$$  
This ensures that:

1. **Redundant info decays fast:** $\\omega \\approx 1$.
2. **Unique info decays slow:** $\\omega \> 1$.
3. **Critical Safety Rules (Cautionary \+ Unique) decay very slow:** $\\omega \\gg 1$.

### **6.3 Phase-Out Lifecycle: Quarantine and Absorption**

A learning is **Stale** if $S\_{eff} \< \\theta\_{stale}$. However, hard deletion is irreversible. We propose a **Two-Phase Phase-Out**:

1. **Quarantine:** Stale learnings are moved to a Dormant state. They are excluded from standard retrieval (saving tokens) but indexed for "Deep Search."
2. **Absorption:** The consolidation operator scans the Dormant pool. If a dormant learning can be merged into an active generalization (LGG) without reducing the generalization's confidence, it is **absorbed** (its semantics are preserved in the general rule, but the specific entry is deleted).
3. **Pruning:** Only dormant learnings that are _both_ unsurprising and un-mergeable are permanently deleted after a grace period.

## ---

**7\. Algorithmic Implementation: Recursive Language Models (RLMs)**

The theoretical operations described (LGG, Entailment, Coreset construction) are computationally expensive and semantically subtle. Implementing them via simple vector math is insufficient; implementing them via a single massive LLM call is impossible due to context limits.  
**Recursive Language Models (RLMs)** 2 provide the ideal architecture. RLMs treat the context (our learnings table) as an external environment variable and use a Python REPL to programmatically decompose it.

### **7.1 The RLM Consolidation Workflow**

We define the consolidation operator as a recursive program:  
**Step 1: Context-Centric Decomposition**  
The RLM does not load all learnings. It peeks at metadata (clusters, categories) via the REPL.

Python

\# RLM Pseudo-code in REPL  
clusters \= mmry_db.cluster_by_semantic_similarity(threshold=0.85)  
for cluster_id in clusters:  
 process_cluster(cluster_id)

**Step 2: Recursive Generalization (The Sub-Call)**  
For each cluster, the RLM spawns a sub-agent. This agent loads _only_ the learnings in that cluster.

- **Task:** "Find the Least General Generalization (LGG) that subsumes these 5 specific learnings."
- **Execution:** The LLM synthesizes a new text $\\ell\_{gen}$.
- **Verification:** The RLM runs a check: Does $\\ell\_{gen}$ entail all original $\\ell\_i$? If yes, replace cluster with $\\ell\_{gen}$.

**Step 3: Bipolar Pairing (Synergy Check)**  
The RLM searches for complementary pairs across the consolidated set.

- **Query:** "Find pairs where one is Guiding and one is Cautionary, and they refer to the same object."
- **Action:** If Entails(c, Exception(g)) is true, link them as a **Bipolar Unit**.

### **7.2 Feedback Loop and Cold Start**

To drive the scores $S(\\ell)$, the system needs feedback. The RLM enables **Synthetic Feedback Generation** to solve the "Cold Start" problem.  
After an agent completes a task, the RLM analyzes the session transcript (post-hoc).

1. **Retrieval:** Which learnings were relevant?
2. **Outcome Analysis:** Did the agent succeed?
3. **Attribution:** If Success \+ Learning Used $\\to$ PositiveFeedback. If Failure \+ Learning Ignored $\\to$ PositiveFeedback (validation of need). This creates a high-volume signal loop that matures learnings quickly from "Candidate" to "Proven" without requiring manual human tagging.2

## ---

**8\. Formal Guarantees**

### **8.1 Convergence**

**Theorem:** The set of learnings $L\_t$ converges to a fixed point $L^\*$.  
_Proof Sketch:_ The consolidation operator $C$ is a projection onto the lattice $\\mathcal{L}$ that strictly reduces cardinality or increases generality (bounded by the top element $\\top$). The scoring update $f\_S$ is a contraction mapping ($\\lambda \< 1$) due to exponential decay. The "Surprise" filter bounds the rate of new insertions $\\Delta\_{new}$. As $t \\to \\infty$, if the environment stabilizes, $L\_t$ converges to the minimal set of most general hypotheses consistent with observations.

### **8.2 Information Loss**

**Theorem:** The consolidated set $C(L)$ is an $\\epsilon$-coreset.  
_Proof Sketch:_ By using Cluster-Pruning with a radius threshold $\\delta$ (similarity \> 0.85), we bound the displacement of any learning in the embedding space. If the retrieval function is Lipschitz-continuous (which dot-product attention is), the error in retrieval relevance is bounded by $K \\cdot \\delta$.

## ---

**9\. Proposed System Specification**

Based on this analysis, we recommend the following specification for the mmry consolidation system:

| Component          | Mechanism                              | Theoretical Basis                 |
| :----------------- | :------------------------------------- | :-------------------------------- |
| **Data Structure** | Bipolar Units $\\langle g, c \\rangle$ | QBAF Synergy & Duality            |
| **Consolidation**  | RLM-driven LGG Synthesis               | Lattice Theory & Anti-unification |
| **Retention**      | Surprise-Weighted Decay                | TITANS Memory Architecture        |
| **Compression**    | Cluster-Pruning (CoDR)                 | Epsilon-Coreset Theory            |
| **Stability**      | Contraction Mapping                    | Lyapunov Stability Analysis       |

### **9.1 Implementation Roadmap**

1. **Immediate:** Implement **Recursive Language Model (RLM)** infrastructure. Set up a Python REPL environment where an LLM can query the learnings table and execute clustering algorithms (scikit-learn).2
2. **Short-Term:** Deploy the **Synthetic Feedback Loop**. Use the RLM to analyze past sessions and backfill feedback events to mature the current knowledge base.
3. **Mid-Term:** Refine the **Staleness Metric**. Move from pure time-decay to Surprise-weighted decay. Train a small model (or use LLM perplexity) to estimate the "surprise" of a learning.
4. **Long-Term:** Transition to **Bipolar Retrieval**. Modify the vector index to retrieve atomic Pairs $\\langle g, c \\rangle$ rather than individual text chunks, ensuring safety constraints are never decoupled from advice.

This architecture ensures that mmry scales indefinitely. By treating memory not as a static list but as a dynamic lattice subject to continuous algebraic consolidation, we guarantee that the system becomes smarter, faster, and safer as it ages.

#### **Works cited**

1. Recursive Language Models \- RLM \- arXiv, accessed February 8, 2026, [https://arxiv.org/html/2512.24601v1](https://arxiv.org/html/2512.24601v1)
2. Recursive Language Models | Alex L. Zhang, accessed February 8, 2026, [https://alexzhang13.github.io/blog/2025/rlm/](https://alexzhang13.github.io/blog/2025/rlm/)
3. Titans: Learning to Memorize at Test Time \- A Breakthrough in Neural Memory Systems, accessed February 8, 2026, [https://www.shaped.ai/blog/titans-learning-to-memorize-at-test-time-a-breakthrough-in-neural-memory-systems](https://www.shaped.ai/blog/titans-learning-to-memorize-at-test-time-a-breakthrough-in-neural-memory-systems)
4. Extending Modular Semantics for Bipolar Weighted ... \- IFAAMAS, accessed February 8, 2026, [https://aamas.csc.liv.ac.uk/Proceedings/aamas2019/pdfs/p1722.pdf](https://aamas.csc.liv.ac.uk/Proceedings/aamas2019/pdfs/p1722.pdf)
5. (PDF) Getafix: learning to fix bugs automatically \- ResearchGate, accessed February 8, 2026, [https://www.researchgate.net/publication/336453658_Getafix_learning_to_fix_bugs_automatically](https://www.researchgate.net/publication/336453658_Getafix_learning_to_fix_bugs_automatically)
6. XML Prompting as Grammar-Constrained Interaction: Fixed-Point Semantics, Convergence Guarantees, and Human-AI Protocols \- arXiv, accessed February 8, 2026, [https://arxiv.org/html/2509.08182v1](https://arxiv.org/html/2509.08182v1)
7. Knowledge distillation and dataset distillation of large language models: emerging trends, challenges, and future directions \- PMC, accessed February 8, 2026, [https://pmc.ncbi.nlm.nih.gov/articles/PMC12634706/](https://pmc.ncbi.nlm.nih.gov/articles/PMC12634706/)
8. Coresets for the Average Case Error for Finite Query Sets \- MDPI, accessed February 8, 2026, [https://www.mdpi.com/1424-8220/21/19/6689](https://www.mdpi.com/1424-8220/21/19/6689)
9. Efficient and Effective In-context Demonstration Selection with Coreset \- arXiv, accessed February 8, 2026, [https://arxiv.org/pdf/2511.08977](https://arxiv.org/pdf/2511.08977)
10. ECHO: Effective Coreset-Driven Learning via Hierarchical Optimizations \- Informatics Homepages Server, accessed February 8, 2026, [https://homepages.inf.ed.ac.uk/ppatras/pub/icdm25.pdf](https://homepages.inf.ed.ac.uk/ppatras/pub/icdm25.pdf)
11. Applying Attribution Explanations in Truth-Discovery Quantitative Bipolar Argumentation Frameworks \- arXiv, accessed February 8, 2026, [https://arxiv.org/pdf/2409.05831](https://arxiv.org/pdf/2409.05831)
12. Frontiers in Artificial Intelligence and Applications \- DSpace, accessed February 8, 2026, [https://dspace.library.uu.nl/bitstream/handle/1874/415301/9781643681078.pdf?sequence=1](https://dspace.library.uu.nl/bitstream/handle/1874/415301/9781643681078.pdf?sequence=1)
13. Titans \+ MIRAS: Helping AI have long-term memory \- Google Research, accessed February 8, 2026, [https://research.google/blog/titans-miras-helping-ai-have-long-term-memory/](https://research.google/blog/titans-miras-helping-ai-have-long-term-memory/)
14. Titans: Learning to Memorize at Test Time \- arXiv, accessed February 8, 2026, [https://arxiv.org/html/2501.00663v1](https://arxiv.org/html/2501.00663v1)
15. ysz/recursive-llm: Recursive Language Models for unbounded context processing. Process 100k+ tokens with any LLM by storing context as variables instead of prompts. \- GitHub, accessed February 8, 2026, [https://github.com/ysz/recursive-llm](https://github.com/ysz/recursive-llm)

