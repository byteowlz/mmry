# Optimal Consolidation of Dual-Polarity Procedural Learnings Under Temporal Decay

## Algebraic Structure for Procedural Learnings

To design a principled consolidation operator, we can endow the space of **procedural learnings** with a lattice-like structure. Each learning $\ell$ is either a **guiding principle** or a **cautionary principle** (dual polarity). We define a **subsumption order** $\ell\_1 \le \ell\_2$ meaning "learning $\ell\_2$ subsumes $\ell\_1$" if following $\ell\_2$ in all situations would implicitly follow $\ell\_1$ as well. In practice, we can approximate $\ell\_1 \le \ell\_2$ by a combination of high embedding similarity _and_ a textual entailment relation (using a lightweight NLI model) – i.e. if $\ell\_2$ semantically entails $\ell\_1$【7†L294-L301】【30†L53-L58】. This order is reflexive and transitive (a preorder; mod out by semantic equivalence to get a partial order).

Within this structure, we can define **meet** and **join** operations for consolidation:

- **Join ($\bigsqcup$)** of two _guiding_ learnings $g\_1$ and $g\_2$: produce their **least general generalization** – the most specific guiding principle that covers both cases【30†L53-L58】. Intuitively, this merged principle should hold in all scenarios where $g\_1$ or $g\_2$ applied, but nothing more general. For example, joining “Always validate API tokens” with “Always validate file permissions” could yield “Always validate credentials and permissions before performing sensitive operations.” The existence and uniqueness of a least general generalization (LGG) is a well-studied concept in inductive logic and anti-unification【30†L53-L58】, ensuring that $\bigsqcup$ yields the **least** abstraction that subsumes both learnings.

- **Meet ($\bigsqcap$)** of a guiding and a cautionary learning on the same concept: if guiding $g$ says "Always do X" and cautionary $c$ says "Never do X in context Y," their meet is a **refined principle** like "Do X except when Y.” In other words, $\bigsqcap$ incorporates the exception from the cautionary into the guiding. If instead $g$ and $c$ fully contradict (e.g. "Always do X" vs "Never do X"), then their meet is the bottom element $\perp$ (an inconsistency flag), effectively _annihilating_ the pair because they cannot coexist without contradiction. In practice, detecting such direct polarity conflicts can be done via entailment checks as well (does one negate the other?). If a guiding and cautionary are merely complementary (one warns against the exact pitfall the other encourages avoiding), we might not merge them but mark them as a **linked pair** in the knowledge base.

This structure forms a **semilattice**: guiding principles are ordered by specificity (more general principles subsume more specific ones), and every pair has a join (an LGG). The lattice is _not_ distributive in general (since combining cautionary exceptions doesn’t distribute neatly), but it gives us a partial order of learnings. Consolidation can then be framed as computing the **join of all learnings in an antichain of the poset** – effectively finding the set of maximal general principles that cover the ground knowledge.

**Decidability:** Because we avoid heavy LLM calls per pair, we use a combination of embedding-based similarity and a trained entailment classifier to decide $\ell\_1 \le \ell\_2$. Modern NLI models can serve as a fast oracle for “does principle B logically imply principle A?”, enabling us to build this ordering automatically. We already use a similar approach for deduplication: new extractions with cosine similarity > 0.85 trigger a semantic equivalence check【7†L294-L301】. We can extend that to _subsumption_ checks (not just equivalence) by fine-tuning the classifier to recognize implication vs mere similarity. This way, the partial order and meets/joins can be computed without an expensive LLM-in-the-loop for each comparison.

## Consolidation as Lossy Compression with Guarantees

Think of the full set of learnings $L$ as a knowledge base we want to **compress** without losing decision-making power. We define the **decision value** of a set $L$ with respect to a distribution of queries $Q$ as:

$$V(L, Q) ;=; \mathbb{E}*{q \sim Q}\Big\[\max*{\ell \in L} \text{relevance}(q,\ell);\cdot;S(\ell);\cdot;\text{maturity\_weight}(\ell)\Big],$$

where $\text{relevance}(q,\ell)$ is how well learning $\ell$ addresses query $q$, $S(\ell)$ is its decayed confidence score, and maturity weight prioritizes established/proven learnings over raw candidates. Intuitively, $V(L,Q)$ measures how useful the learning set is for the kind of queries we expect (higher if for each query there’s a highly relevant, high-confidence learning available).

A consolidation operator $C: 2^L \to 2^L$ is **$\varepsilon$-lossless** if for all reasonable query distributions $Q$ in a target family, the compressed set retains at least a $(1-\varepsilon)$ fraction of the decision value:

$$V(C(L), Q) ;\ge; (1 - \varepsilon),V(L, Q).$$

This parallels the idea of an **$\varepsilon$-coreset** in computational geometry, where a small weighted subset of points approximates some query function over the full set with bounded error【19†L147-L156】. Here, queries are semantic task descriptions and the “loss function” is whether the best applicable principle remains nearly as relevant and high-scoring after compression. The goal is to make $|C(L)|$ as small as possible for a given tolerance $\varepsilon$.

**Theoretical Bounds:** In the worst case, finding the minimum subset that is $\varepsilon$-lossless is NP-hard (related to set cover and hitting set problems). However, we can leverage known results for approximation. For example, if we treat each learning as "covering" a region of query-space (its embedding neighborhood) with a certain weight, the consolidation task is akin to finding a minimal-weight subset cover for those regions such that all high-probability queries are still covered. Greedy algorithms or coreset algorithms can give approximation guarantees. Recent advances in coreset theory show that for certain query families (e.g. linear queries or nearest-neighbor queries in high-dimensional spaces), one can compute small summaries _independent of the full set size_【19†L147-L156】. This suggests that, if our relevance metric is nicely behaved, we might achieve a consolidated set whose size grows sublinearly (or even logarithmically) with $|L|$ for a fixed $\varepsilon$.

**Practical Approach:** We can frame it as an **iterative merge or selection** problem:

1. Start with the full set $L$.
2. While there exist two learnings whose join covers the same decision space nearly as well as them separately, replace them with their join (merge overlapping principles).
3. Also, while a learning $\ell$ exists that is “nearly redundant” (for every query, some other learning is almost as relevant and higher scoring), we can drop $\ell$. This is analogous to removing points that are inside the convex hull of others in an information space.

We need to ensure that after consolidation, **any query that would have triggered a certain advice in $L$ still triggers an equally good (or almost as good) advice in $C(L)$.** Formally, for every original learning $\ell \in L$, either $\ell$ (or its descendant) is still in $C(L)$, or some $\ell' \in C(L)$ has equal or higher $S(\ell')$ and covers the same situations as $\ell$ did. This would guarantee $\varepsilon=0$ loss for those queries. In practice, we allow a tiny loss $\varepsilon>0$ to achieve more compression (for instance, maybe two very similar principles have slight nuances; merging them into one might lose that nuance but if it hardly ever affects decision outcomes, we accept it).

**Efficiency:** We want to compute $C(L)$ in time polynomial in $|L|$. A naive pairwise comparison of all $\binom{|L|}{2}$ learning pairs with LLM entailment checks is too slow (and costly). Instead, we use a combination of **clustering and selection**:

- Cluster learnings by semantic similarity (using embeddings) to limit pairwise checks to only those above a similarity threshold.
- Use fast vector indexing to propose candidate merges.
- Employ deterministic rules for obvious cases (exact duplicates or paraphrases are merged on ingestion already【7†L288-L297】, so consolidation focuses on partial overlaps).
- For the NP-hard core of it, use a greedy cover heuristic: start with the highest-score learning, then add the next learning that covers the largest portion of uncovered query distribution $Q$, and so on (akin to a greedy set cover which has a $H(n)$ approximation bound).

In summary, consolidation compresses $L$ to $C(L)$ such that $C(L)$ is **minimal** (no learnings can be removed without dropping decision value) and **non-contradictory** (closed under the lattice operations, no unresolved direct conflicts), with provable bounds on the worst-case decision-value loss $\varepsilon$ and hopefully a polynomial runtime construction via clustering and greedy selection. This gives us a formal assurance that the agent’s performance using the consolidated knowledge will be almost as good as using the full set, but with far fewer tokens.

## Polarity-Aware Scoring Under Decay

Our system’s dual polarity (guiding vs cautionary) is a strength, but we must ensure consolidation respects it. We introduce a **bipolar scoring function** for pairs of complementary learnings. If $g$ is a guiding principle and $c$ a cautionary principle on the _same underlying concept_, define an augmented score:

$$ S\_{\text{bipolar}}(g,c) = S(g) + S(c) + \alpha \min{S(g), S(c)}, $$

for some $\alpha > 0$ that rewards balanced support. The intuition is that if both a guiding and a cautionary principle exist on the same topic (e.g. "Always do X" and "PITFALL: avoid doing X in bad context"), then having both provides more nuanced, **decision-conditioned** guidance. The $\min{S(g),S(c)}$ term (scaled) boosts the pair’s joint value beyond the sum of their individual scores【7†L273-L282】. This incentivizes the system to keep complementary pairs together. In contrast, if a guiding and cautionary truly contradict each other (one’s advice negates the other universally), then that pair would have $S\_{\text{bipolar}}(g,c) \approx 0$ or even negative (if we assign a penalty for contradiction), prompting consolidation to resolve the conflict (ideally by eliminating or refining one).

**Consolidation with Polarity Constraints:** We treat a well-supported guiding principle and its well-supported cautionary refinement as an **inseparable atomic unit** unless we can merge them into a single refined principle. The consolidation operator should never drop one without the other if doing so would remove important context. In effect, the search for an $\varepsilon$-lossless subset must happen in the space of either single learnings or these _paired_ learnings. For example, if “Always validate tokens” and “PITFALL: Don’t skip token validation” cover the same concept, it might be redundant to keep both separately – instead, consolidation could merge them into **one rule with an embedded caution** (“Always validate tokens (never skip validation even in quick fixes)”). If merging them textually is not feasible automatically, at least we treat them as a linked pair that either survive or phase out together.

**Temporal Decay Interplay:** Under exponential decay of feedback, one polarity might wane faster than the other. To mitigate dangerous imbalances, $S\_{\text{bipolar}}$ could include a coupling term. For instance, if the cautionary $c$’s feedback decays (no recent incidents) but the guiding $g$ remains reinforced, the system, by using $S\_{\text{bipolar}}$, still “remembers” that $g$ has an associated caveat and will be cautious about consolidating away or overgeneralizing $g$. Only when $c$’s knowledge truly becomes obsolete or is subsumed by another cautionary should it be dropped – ideally _after_ $g$ has been updated to include that exception or another mechanism covers it.

In summary, the consolidation operator is **polarity-aware**: it treats guiding–cautionary pairs as first-class. It will:

- Merge a guiding/cautionary pair into a single **conditional principle** if possible (thus eliminating redundancy while preserving the caution).
- Retain both if they are complementary and high-value, and never drop the cautionary while keeping the guiding (to avoid “one-sided advice” that could be harmful).
- If one side becomes unsupported (e.g. cautionary loses all confidence), flag the pair for review rather than blindly dropping it. Perhaps the lack of recent harmful feedback means the pitfall hasn’t occurred – but it might just be rare. The system might reduce its prominence but not forget it entirely (see staleness below).

## Formalizing Staleness and Phase-Out

Time decay of feedback means old evidence gradually vanishes, but currently a learning never gets removed unless it has _negative_ feedback (deprecation path). We need a criterion for when a learning is simply _stale_ – no evidence of being relevant or needed in the current environment – and a policy to handle it.

**Defining Staleness:** We can combine multiple signals into a staleness score. Let:

- $t\_{\text{last}}(\ell)$ = time since last feedback event on learning $\ell$ (helpful or harmful).
- $r\_{\text{recent}}(\ell)$ = number of times $\ell$ was retrieved in context in the last $W$ days (retrieval count in a sliding window).
- $S(\ell)$ = current decayed score (which diminishes over time with no reinforcement).

One simple rule: a learning $\ell$ is _stale_ if

1. **No recent feedback:** $t\_{\text{last}}(\ell) > T\_{\text{max}}$ (e.g. no feedback in the last 6 months),
2. **Little to no usage:** $r\_{\text{recent}}(\ell) < k\_{\min}$ (e.g. retrieved fewer than 2 times in that period),
3. **Negligible confidence:** $S(\ell)$ has decayed below a low threshold $\theta\_{\text{stale}}$ (close to 0).

All three conditions indicate $\ell$ is not _confirmed_ to be relevant anymore【24†L850-L858】. We can set, for example, $T\_{\max} = 180$ days, $k\_{\min}=1$, $\theta\_{\text{stale}}=0.05$. These parameters might be learned or tuned experimentally.

However, a hard threshold approach can be brittle. We can instead model _staleness probability_ in a Bayesian way:

- Assume each learning has a hidden state: “alive (relevant)” vs “obsolete”.
- Feedback events are evidence that it’s alive; lack of feedback is evidence pushing it toward obsolete. We could use a Beta prior and update with feedback frequency. For instance, if a learning hasn’t been tried in a long time, the posterior probability it’s still useful drops.
- We define $P\_{\text{alive}}(\ell)$ and declare $\ell$ stale if $P\_{\text{alive}}(\ell)$ falls below some confidence (like 5%).

This probabilistic view would naturally make _rare but critical_ learnings remain “alive” longer: we might assign a prior for critical topics that decays slower (more on criticality below). It also avoids sudden death – instead a learning “fades out” in probability, which we then act on.

**Phase-Out Strategies:** Once a learning is flagged as stale, what do we do? We have options:

- **Soft Quarantine:** Mark it as _dormant_. It’s removed from active retrieval results but kept in a separate pool. This is reversible: if conditions change or new feedback arrives (e.g. someone explicitly references it or a similar situation recurs), we can reinstate it. Dormant learnings are periodically reviewed by consolidation routines to see if they can be merged into a more general active learning (absorbing any remaining knowledge). This addresses the case where maybe the learning was a special case of a more general rule that’s now present.

- **Hard Removal:** Completely delete it after a quarantine period. This reclaims space fully, but we risk losing information. To mitigate regret, we might keep a tombstone record (metadata only, like “Removed principle about X (stale as of 2026)”) for audit, but not use it in retrieval.

- **Score Freezing:** Another approach is to leave it in the active set but force its score $S(\ell)=0$ once stale. This means it will never be retrieved unless directly queried by exact text. Practically, that’s similar to removal from retrieval, but it keeps the data around. However, having many zero-score entries can bloat the index, so it’s not ideal long-term.

Our preferred approach: **Quarantine + Absorption**. When a learning becomes stale, we:

1. Mark it dormant (excluded from normal context retrieval).
2. During the next consolidation sweep, check if this learning (or its knowledge) is subsumed by some active learning:
   - If yes, we can safely remove it (it taught us something that’s now covered by a broader principle).
   - If not, then why is it unused? Possibly it’s an edge-case rule that hasn’t been needed _yet_ but could be vital later. Here we consider criticality (below).
3. Optionally, present stale learnings in a periodic review to a human or a governance agent: “These 10 learnings haven’t been used in a year. Are they still valid?” This adds a manual safety net to catch anything the automated signals miss.

**Rare-but-Critical Learnings:** The classic example: "Never run DB migrations without a backup." You might rarely do prod migrations, so this principle gets no recent feedback. But dropping it would be disastrous the one time it’s needed. To handle this, we introduce a **criticality factor** $C(\ell) \ge 1$. If $\ell$ is deemed highly critical, it effectively has a stronger prior against staleness. Factors influencing $C(\ell)$:

- **Polarity**: Many cautionary principles (especially ones with words like “Never”, “PITFALL”, “dangerous”) are safety-critical. We can assign a higher $C$ to any cautionary in certain categories (security, prod ops, etc.).
- **Content**: Use a simple classifier or keyword match for terms like “production”, “security”, “backup”, “irreversible”, etc., to identify possibly critical advice.
- **Scope**: If a learning is global or cross-cutting in scope (not tied to a single project or outdated API), it may be more fundamental.
- **Manual tag**: We could allow users to explicitly pin a learning as important.

The staleness condition then becomes:

$
\text{stale}(\ell) \text{ if } P\_{\text{alive}}(\ell) < \frac{\theta\_{\text{stale}}}{C(\ell)},
$

so a higher criticality $C(\ell)$ means we require much stronger evidence of obsolescence to declare it stale. In effect, critical learnings decay slower and need more time or lack of use before quarantine【24†L850-L858】.

**Lifecycle Integration:** We adapt the maturity lifecycle to include staleness:

- A learning with consistently harmful feedback -> **Deprecated** (this is a quality failure; we might convert it to a cautionary principle if appropriate, as cass-memory suggests).
- A learning with no feedback at all for a long time -> **Dormant** (this is staleness; it might just not have been needed recently).
- Deprecated vs Dormant are distinct end states: deprecated means “we have evidence this is wrong/harmful,” whereas dormant means “no evidence it’s relevant anymore.”

In our table of states:

```
Candidate -> Established -> Proven -> ...(time passes)...
    \                      \                   \
     \__(stale?)             \__(stale?)          \__(stale?)
            \                      \                   \
         Dormant (quarantine)   Dormant            Dormant
            |                      |                   |
            |__(absorb or remove)__|__(absorb/remove)__|__(absorb/remove)__
```

Parallel to:

```
Candidate --(high harmful ratio)-> Deprecated (quality failure)
```

Dormant learnings are periodically scanned. Many will be removed after absorption. A few might linger if they remain unique but unused – those might effectively become an _archival memory_ that’s not actively used but can be searched explicitly if needed.

This two-pronged approach ensures the active set doesn’t grow without bound: as new learnings come in, some old ones phase out, keeping the total manageable. It also cleanly separates **fading by time** from **failing by evidence**.

## Feedback Ingestion and Scoring

A consolidation and staleness regime is only as good as the feedback driving it. Currently, our system has a sophisticated scoring formula (positive feedback minus 4× negative, decayed by half-life 90 days【7†L317-L325】) but no actual feedback events feeding in. We need to establish _how_ learnings get feedback, and ensure it's sufficient and reliable.

**Manual Feedback Channels:**

- **Explicit CLI command:** e.g. `mmry feedback <learning-id> helpful/harmful`. This relies on a user noticing a principle in context and deciding if it was useful or misleading. It’s high-quality (direct human judgment) but very low volume.
- **TUI Inline Ratings:** If the text UI listing memories allows quick thumbs-up/down on each retrieved learning, we could gather more signals in interactive use. This is still user-driven, hence reliable but limited to interactive sessions.
- **Periodic Review Mode:** A maintainer could review a list of candidates or low-score learnings and mark them helpful or obsolete. This is essentially manual curation and doesn’t scale well for daily feedback, but could be done monthly to clean house.

Because human feedback is precious, we must _treasure_ it in the scoring. A single explicit “helpful” from a user might outweigh 10 implicit signals. Thus, we assign weights $w(\text{source})$ to feedback events. For example, $w(\text{human}) = 1.0$ (baseline), $w(\text{agent-self}) = 0.5$ (if an agent claims it helped), $w(\text{implicit}) = 0.1$ (if inferred from usage). Then:

$$ S(\ell) = \sum\_{e \in \text{helpful}(\ell)} w(e),0.5^{\frac{\text{age}(e)}{\tau}} ;-; \lambda \sum\_{e \in \text{harmful}(\ell)} w(e),0.5^{\frac{\text{age}(e)}{\tau}}, $$

with $\lambda=4$ as before. This ensures a human marking something harmful really tanks it (as it should), whereas an agent’s self-congratulation counts less.

**Automatic Feedback Channels:**

- **Outcome Attribution:** Whenever an agent completes a task successfully, we attribute a "success" to all learnings that were provided in context (on the assumption they _might_ have helped). If the task failed or got abandoned, maybe attribute a neutral or slight negative to those learnings. Over many sessions, useful principles will correlate with successes. This is a noisy signal but essentially free. For example, if `mmry context` returned 5 principles and the agent succeeded, each gets a +0.2 weight helpful event.
- **Retrieval Usage Analysis:** We can parse the agent’s reasoning or actions to see if it _used_ a retrieved principle. If the agent’s solution clearly followed the advice (e.g., the agent reasoning says “I will be careful to do X as advised”), that’s a strong implicit helpful signal. If the agent ignored the memory (never referenced it, or worse, did the opposite and succeeded anyway), that could be a slight negative (the principle might not apply or was overly cautious). This requires NLP analysis of the agent’s behavior, which could itself be done by an LLM in a log-processing step.
- **Conflict signals:** If a new learning is extracted that directly contradicts an existing one (and both are in candidate stage), mark them mutually harmful until resolved. For instance, if we extract “Always use AES-256 encryption” and later “Avoid over-encryption for performance” and they conflict, we might give each a provisional harmful vote relative to the other until a refined principle is made. This uses the extraction pipeline’s knowledge of contradictions.
- **Cross-agent corroboration:** If multiple independent agents (or users) all have the same learning extracted or rated, that principle’s credibility increases. E.g., if in different projects, three separate sessions all distilled “Write unit tests for bug fixes,” we treat that convergence as evidence of general usefulness – even before explicit feedback.
- **External signals:** Integration with CI: if a learning says “Run lint before committing” and we see via repository events that lint warnings indeed dropped after it was introduced, that’s real-world positive evidence. Conversely, if a learning claims something that actual metrics disprove (like “This optimization always improves memory” but memory usage hasn’t changed), that could be a negative. These signals are domain-specific and require hooking into external data (which is non-trivial but possible in a dev environment).

The challenge is balancing **quality vs quantity**. Human feedback is high quality but sparse; implicit signals are noisy but abundant. By weighting sources as above, we incorporate many signals without letting noise overwhelm truth. For example, a hundred “attributed success” points (each low weight) might equal just a couple of direct human approvals.

**Cold Start & Exploration:** A new learning starts with $S(\ell)=0$ and no maturity. If we rank strictly by score, it may rarely be retrieved, so it never gets a chance to earn feedback – a catch-22. To prevent this, we can:

- Give every new learning a small initial positive boost (e.g. as if one fake helpful event just occurred). EvolveR’s Laplace smoothing is an example: initial score \~0.5【7†L317-L325】 ensures new principles aren’t at bottom rank by default.
- Use an **exploration strategy** in retrieval: Occasionally include one random _Candidate_ learning in the context, even if its score is lower than the top-$k$. This is akin to an $\varepsilon$-greedy bandit strategy where $\varepsilon$% of the time we try something new. This guarantees that over time, every candidate gets tried and can accumulate feedback.
- Utilize extraction confidence: If the LLM that extracted the learning gave a high confidence or it came from a very successful session, treat that as initial evidence. For example, “principle distilled from a flawless task completion” could start with a pseudo-helpful event.
- **Bandit approach:** More formally, we can maintain a Bayesian estimate for each learning’s probability of being helpful. Each time we have a chance to serve a learning (context query), we sample from these probabilities (Thompson sampling). This naturally balances exploration and exploitation – uncertain candidates will sometimes be tried, and if they succeed, their probability increases【19†L147-L156】. This addresses the “rich get richer” problem by not deterministically starving low-score items.

**Feedback Loop Integrity:** We must be cautious that our feedback sources don’t create bias or feedback loops. For example, outcome attribution might reward a learning even if it had no effect on the success. Over many trials, truly irrelevant learnings will get mixed outcomes and their score may oscillate around zero, whereas truly helpful ones will consistently correlate with success and rise. We should monitor if any learning’s score is just tracking how often it’s shown rather than actual utility. If so, our weighting might need adjustment (e.g., down-weight the outcome attribution if it’s giving credit too liberally).

Finally, we implement feedback ingestion tools in the platform (e.g., add a hook so that every time an agent finishes a session or a user runs tests, we call a function to log feedback events for relevant learnings). Once this pipeline flows, the consolidation and staleness detection will have rich data to operate on, enabling the confidence scores to genuinely reflect current usefulness.

## Recursive Language Model (RLM) for Scalable Consolidation

A recent development by Zhang, Kraska, and Khattab (2025) proposes **Recursive Language Models (RLMs)**【1†L50-L58】, which allow an LLM to handle inputs far beyond its normal context by recursively reading and writing from an external “scratchpad” or environment. This is an ideal fit for our consolidation problem: instead of feeding 500 learnings into a single prompt and hoping the model can digest it, we let the LLM **decompose the task**.

**RLM Consolidation Process:** Imagine an LLM agent that can browse the learnings database like an external memory:

1. **Plan the Consolidation:** The model could first read a summary of the learnings (perhaps just their titles, categories, and scores, which is small) and then plan: “I see 50 security-related principles, many might overlap; 30 debugging principles; etc. I will handle each category separately.”
2. **Cluster Subsets:** For example, it might take the 50 security learnings and instruct itself to retrieve the full text of those 50 in chunks (say 5 at a time) and consolidate them. Each chunk easily fits in context. It produces intermediate consolidated principles for each chunk, then recursively consolidates those results together. This is a **divide-and-conquer** approach.
3. **Cross-Cluster Merging:** After doing this for each category, it can look at the consolidated results from each and see if any redundancies exist across categories (e.g., a “validate input” rule might appear in both security and debugging contexts). It can then merge those at a higher level.

Throughout, the RLM is effectively writing a small program that calls itself on subproblems. This matches how RLMs achieved processing 10×-100× longer texts with quality gains【1†L50-L58】 – they structure the reading task instead of brute-forcing it.

**Advantages:**

- We avoid quadratic pairwise comparisons among 500 learnings; instead we maybe do linear number of LLM calls, each on manageable input sizes.
- The LLM can apply **semantic judgment** to merge principles that a deterministic algorithm might struggle to generalize. For instance, an RLM could read five similar rules and abstract “the pattern is: always check preconditions before doing actions on a shared resource” – essentially inducing a higher-level principle. Deterministic methods (like anti-unification) work on formal symbols; the LLM can do it on natural language and real code contexts.
- The RLM can also handle the **guiding–cautionary pairing** intelligently: it might notice “These two principles are the positive/negative framing of the same idea – I should combine them.”

**Hybrid Deterministic + RLM:** We likely don’t want to trust the LLM to do everything, given concerns about determinism and correctness. A pragmatic design is:

- **Deterministic baseline consolidation:** Do the obvious merges and subsumption removals with the rules we’ve described (embedding similarity, entailment, etc.). This shrinks the set and removes blatant duplicates or contradictions without an LLM.
- **RLM deep consolidation:** Then feed the reduced set into an RLM process to identify less obvious redundancies and to **abstract** clusters of specifics into generals. Because the input is smaller after the baseline, the RLM has an easier job.

For example, deterministic logic might reduce 500 learnings to 300 by removing near-duplicates and those strictly subsumed. Then the RLM might reduce 300 to 200 by finding higher-level abstractions and merging complementary pairs.

**RLM for Staleness:** The RLM approach isn’t just for merging content – it can also reason about time and usage. We could instruct the RLM: “Here are 20 learnings that haven’t been used in a year. Compare them to the active learnings and see if their knowledge is already covered or if they refer to outdated technology. Decide which can be pruned or merged.” The model can then, for each stale learning, do a focused retrieval of similar active ones (embedding search) and decide:

- If a stale learning is essentially duplicating an active one (just phrased differently or at a smaller scope), then it should be merged/absorbed.
- If it’s unique but possibly outdated (e.g. it mentions an old version of a library), the model might flag it as obsolete.
- If it’s unique and important but just not recently tested, it might recommend keeping it with a “pinned” status due to criticality.

Because the RLM can use reasoning and even external knowledge (if allowed) about versions and best practices, it might catch things like “This principle was about AngularJS, and we’re now on Angular 12 – it’s likely irrelevant now” or “This rule is about a once-buggy API that has since been deprecated; safe to remove.” Such nuance is hard to encode in simple rules.

**Idempotence and Convergence:** A concern is that an LLM’s output can be nondeterministic. If we run the consolidation twice, we could (in theory) get two different phrased sets of learnings. To mitigate this:

- We can run the RLM in a few-shot or chain-of-thought mode where it explicitly lists out the merges it’s doing and justifications, to make its process more stable and debuggable.
- After consolidation, we compare the new set to the old. Ideally, if we feed the new set back into the pipeline, nothing further should change (fixed point). We enforce a rule: do not introduce new principles that are _more specific_ than what we had (to avoid oscillation). Each RLM pass should only generalize or remove redundancy, never create _new_ specific details. This tends to guarantee monotonic reduction in size.
- We can also use embedding similarity to measure how close the consolidated knowledge is to the original. If an RLM run produced a principle that seems off (e.g., low similarity to the cluster it was supposed to generalize), we might reject that change or try a second attempt.

In practice, we expect a well-designed consolidation prompt to yield consistent results (especially if we use a relatively deterministic model or fix the random seed). RLMs have been shown to approach the quality of a GPT-5 on long inputs【1†L53-L61】, so the semantic merges should be reliable.

**RLM for Synthetic Feedback:** Another use of RLM is post-hoc session analysis. We could have an RLM agent read an entire coding session’s transcript (via recursion) and identify which learnings from context were helpful or not. For example, it might annotate: “Learning #12 was suggested and the agent followed it, leading to success – mark it helpful. Learning #15 was suggested but the agent ignored it and succeeded regardless – perhaps irrelevant.” This would generate feedback events automatically with an LLM’s understanding. While not 100% accurate, it can dramatically increase feedback volume. Essentially, the LLM becomes a reviewer of the agent’s performance w\.r.t. the provided advice, a bit like a tutor grading a student’s use of hints.

Such synthetic judgments could be weighted lower than direct human feedback, but they add yet another channel to keep scores moving.

## Convergence and Minimality of Consolidation

We want to ensure that if we apply the consolidation operator repeatedly (over time as new learnings are added, feedback changes, etc.), the system approaches a _stable state_ rather than thrashing or growing unboundedly.

**Convergence to Fixed Point:** If no new learnings were ever added, does repeatedly consolidating terminate with a fixed knowledge base? In our algebraic framework, the join and meet operations have idempotent and associative properties on a lattice: doing them once should yield a set that cannot be further joined without loss. In other words, once all possible beneficial merges are done, you’re at a fixed point. If our consolidation correctly identifies all overlaps and redundancies, a second run should find nothing new to merge. We should formally have $C(C(L)) = C(L)$ for the operator to be well-defined. Designing $C$ to be idempotent means we prefer merging maximal clusters in one go rather than many small merges that could depend on order.

One approach: always merge the _most overlapping_ pair of learnings first (those with highest similarity or redundancy), and repeat until no pair has overlap above a threshold. This greedy strategy needs care – does it always produce the same result regardless of tie-breaks? If the lattice is modular or if overlaps are clear-cut, likely yes. If not, different merge orders could yield slightly different generalizations. To be safe, we might incorporate an order-independence by merging in clusters (e.g., cluster all learnings above some similarity into one group and merge them collectively). That tends to be confluent (everyone ends up with the same cluster regardless of processing order).

**Lyapunov Function:** We can define a measure like total number of learnings + a penalty for contradictions or redundancies. Consolidation should strictly decrease this measure. For instance, $F(L) = |L| - \sum\_{\ell\neq \ell'} \text{overlap}(\ell,\ell')$ in some sense, where overlap is counted for redundant info. Each merge reduces $|L|$ by 1 and probably lowers overlap counts significantly. So $F(L)$ should decrease until no overlaps above threshold remain. That gives a quasi-monotonic decrease bounded below by 0, ensuring termination.

**Information Loss Bound:** Each consolidation (with $\varepsilon$-lossless guarantee) ensures decision-value doesn’t drop much. Over successive consolidations, these guarantees compose. If we ensure each merge has at most $\varepsilon\_i$ loss and these are on disjoint query regions, overall loss is $\le \sum \varepsilon\_i$. If any merge would cause >$\varepsilon$ loss, our algorithm would skip it. Thus the final knowledge base preserves a provable fraction of the original value (which was presumably high when all raw learnings are there). In practice, as long as $\varepsilon$ is small (say 0.05 or 5% loss) per consolidation cycle and we only consolidate when new redundancies actually arise, the system should maintain nearly maximal performance.

**Steady-State Size:** If the agent keeps extracting new learnings indefinitely, but also old ones phase out, does the active set size reach an equilibrium? We hypothesize yes. There’s likely a **carrying capacity** determined by the diversity of tasks and concepts in the domain. If the team’s work spans, say, 20 categories with 10 key principles each, then beyond \~200 learnings you mostly get variations of existing ones. Our consolidation will merge those, so the count stops growing. Meanwhile, truly new domains or technologies could add new principles, but older ones may become stale as the tech stack changes. So the system continuously refreshes. In effect, it’s performing **concept drift adaptation**: as the environment evolves, some principles are forgotten and new ones take their place, but the total remains within a band. Empirically, we’d measure $|L\_{\text{active}}(t)|$ over time – we expect it to plateau or oscillate within a range, rather than linearly grow.

**Coreset analogy:** In streaming coreset algorithms, new points come in and one keeps a small summary that approximates all seen points. Our system is similar – new learnings come, we keep a compressed set approximating the “experience space.” Known results in data summarization indicate you often can keep a summary whose size depends on complexity of the data distribution, not on time. For example, if the team keeps encountering the _same kinds_ of problems, the memory saturates. Only truly novel problems expand it.

**RLM Non-determinism:** If using RLM consolidation, to be safe, we might run multiple rounds and cross-compare. If two runs yield slightly different phrasing, we merge those differences deterministically (like take the union of both results and run a quick consolidation on that). If they yield different abstraction levels, perhaps take the more general one (since general covers specific in our lattice). The **bounded-divergence** idea is that two reasonable consolidations of the same input should overlap heavily in content – any major difference would indicate an ambiguous choice that needs resolution (possibly by a human or a more constrained prompt).

In summary, we aim for consolidation to be **confluent** (order of merges doesn’t change the final set significantly) and **idempotent** (applying it once fully yields a fixed point). Formal proofs might be complex given the semantic nature, but we can test empirically by running the algorithm to saturation and seeing if it stabilizes.

## Empirical Evaluation Plan

To validate the consolidation operator and associated mechanisms, we will measure several key metrics:

- **Compression Ratio:** The size of the active learning set after consolidation versus before. E.g., if 500 raw learnings become 300 consolidated, that’s a 40% compression. We want high compression without performance loss. We’ll track this over time as well.

- **Retrieval Decision Fidelity:** For a set of benchmark queries or tasks, compare the retrieval results using the full learnings vs the consolidated learnings. We measure if the same or equally useful principles are retrieved. Concretely, we can measure Recall\@k (did the consolidated set return an appropriate learning that was in the top-k of the full set?) and the success rate of agents on tasks when using $L$ vs using $C(L)$. An $\varepsilon$-lossless claim would be validated if task success drops < $\varepsilon$.

- **Context Window Utilization:** Fewer learnings means fewer tokens in prompts. We can measure average tokens used by retrieved learnings pre- and post-consolidation. We expect significant savings. Also measure latency of embedding search and ranking (should improve with smaller set).

- **Contradiction Rate:** After consolidation, are there any cases where the agent is given a guiding principle without the corresponding caution (because maybe it got dropped or not linked)? We will manually flag known guiding-cautionary pairs and ensure they either appear together or as a merged principle. Ideally, this rate is zero for well-designed operator – no one-sided advice.

- **False Pruning Rate:** Instances where the system removed a learning as stale, but that learning turns out to be needed in a future query. We can simulate this by taking some learnings that were pruned for staleness, then see if any upcoming tasks in the next $N$ days touch on that concept and would have benefitted from it. This requires replaying or analyzing future queries with and without that learning. We aim to minimize this, and any occurrence would be studied to adjust thresholds or increase criticality.

- **Critical Learning Survival:** We will tag a set of principles as “safety-critical” or “rare-critical” (using domain knowledge) and check how the system treats them. Over a long period with no feedback, do they remain (appropriately) due to the criticality factor? If we see them being removed or decayed erroneously, that’s a problem. Ideally, a critical principle with no usage for a year should still be present (maybe marked dormant but _promptly revived_ if a matching query appears).

- **Steady-State Behavior:** In a long-running simulation or real deployment, track the total number of active learnings over time. Confirm that it doesn’t just grow without bound. We expect a curve that grows early on (lots of new extractions), then levels off as consolidation and staleness removal catch up, perhaps oscillating around some equilibrium. If it keeps growing linearly, our removal is too conservative; if it shrinks drastically, maybe too aggressive.

- **Dormant Reactivation Rate:** How often does a dormant (stale) learning get resurrected by new feedback? If often, maybe our staleness criteria are too sensitive (we’re shelving things that are still needed occasionally). We can measure: of all learnings that went dormant, what % received a new helpful feedback within say 3 months (meaning we incorrectly offloaded them)? We want this low, but non-zero indicates the quarantine approach is working (we _could_ recover them).

- **RLM Consolidation Cost:** Measure tokens and time taken by the RLM consolidation runs versus baseline. If we schedule RLM-based deep consolidation, say, nightly or weekly, what is the overhead? It should be comparable to a few LLM calls on subsets. If it’s e.g. 1/10th the cost of summarizing the entire memory in one shot, that’s good. We’ll also compare to a hypothetical single-pass consolidation prompt (which might not even fit, but we can try with truncation) to demonstrate the RLM’s superior quality/cost tradeoff【1†L53-L61】.

- **RLM Consistency:** Run the RLM consolidation twice on the same input (with different random seeds or slight prompt variations). Then compute the similarity between the two outputs: e.g., match resulting principles by embedding and see if each principle from run A has a near-equivalent in run B. Ideally, high consistency (most consolidated principles are essentially the same). If we find large discrepancies, we need to refine the prompt or constraints.

- **User/Agent Satisfaction:** Though harder to quantify, we can survey users or analyze agent behavior for signs of improvement: Are agents making fewer conflicting decisions? Is the average number of steps or backtracks reduced because advice is clearer? Do human users report that the memory suggestions feel more on-point and less repetitive? These qualitative measures will complement the quantitative ones.

By evaluating these, we’ll iteratively refine the consolidation operator. For instance, if we find decision fidelity $\approx 100%$ but compression only 10%, we can push more aggressive merges (allow a tiny $\varepsilon$ loss for big size wins). If we find some drops in performance, we dial back merging in that area. The goal is a **compact, consistent, and current** knowledge base that demonstrably maintains the agent’s effectiveness (or even improves it by removing noise).

## Related Work and Theoretical Foundations

Our approach touches on several areas:

- **EvolveR (Wu et al. 2023):** This system introduced the dual extraction (guiding vs cautionary) similar to our setup【7†L273-L282】. They perform deduplication by embedding similarity and an LLM judgement【7†L288-L297】, and maintain a dynamic success score (Laplace-smoothed success rate) to prune low performers【7†L317-L325】. However, they do not attempt semantic _generalization_. Principles remain as distilled; low-score ones are pruned (threshold \~0.3 success rate in their experiments). Our work extends this by merging overlapping principles and handling long-term drift (staleness), which EvolveR left to simple threshold pruning.

- **Cassidy’s Memory (Hypothetical cass-memory):** Mentioned design suggests converting disproven guiding principles into cautionary “anti-pattern” entries when harmful feedback dominates, and a manual `stale` listing. This acknowledges that a principle can flip polarity (what was once good becomes a pitfall). Our framework could accommodate that: a Deprecated learning (too many negatives) might be reinserted as a cautionary principle to warn against what it once advised. But cass-memory didn’t automate staleness removal beyond decaying their confidence. We formalize an automated staleness pipeline.

- **GitHub Copilot Memory (2025):** Instead of heavy consolidation, Copilot’s team used **just-in-time verification**【28†L539-L548】. They store each memory with a link to source code and at retrieval time, check if the code still matches. This implicitly handles staleness (if the code changed, the memory is effectively invalid). It’s a clever solution for code facts, but for procedural knowledge (abstract principles), you don’t have a direct citation to check. Also, Copilot deliberately avoided building an offline consolidation service due to complexity【28†L531-L539】. Our scenario is smaller-scale (organization-specific memories, not all of GitHub) so we can afford more curation. In fact, our approach can be seen as complementary: where possible we could attach concrete references or examples to each principle and do a mini verification at use-time. But for general advice (“validate inputs”), there’s no single code line to check – you rely on accumulated feedback.

- **Reflexion (Shinn et al. 2023):** Reflexion agents store self-reflections in a limited FIFO buffer (sliding window of say 3 past experiences)【22†L609-L617】【24†L850-L858】. This ensures old reflections automatically fall off – a simple form of staleness handling by fixed capacity. But it’s a blunt instrument; it doesn’t consider which reflections are more important. Our approach is more targeted (remove only what’s stale by evidence). Reflexion’s simplicity worked for short-term learning, but for a persistent knowledge base spanning months, we need explicit consolidation and weighting.

- **MemGPT / Letta (2023-2024):** MemGPT introduced an agent memory that could store and retrieve information, later evolving into the Letta platform. They offered a “filesystem-like” memory (nodes, directories of info) and relied on the agent (an LLM) to search and prune it. No automatic staleness except the agent might decide to delete or modify files. This is essentially an LLM-managed memory with no formal guarantees. Our use of RLM moves in that direction (LLM managing memory), but we wrap it in some guarantees and systematic structure (lattice and scoring). We improve on MemGPT by introducing formal loss bounds and separating high-level consolidation logic from the day-to-day agent (ours can be a periodic maintenance job rather than continuous overhead).

- **Mem0 (Chhikara et al. 2025):** They focus on scalable long-term memory with graph-structured representation【26†L51-L59】. They mention _consolidation_ and likely do summarize conversations and resolve conflicts in facts. The emphasis is on conversation coherence and latency improvement. They likely use summarization (compress a long dialogue into key facts) which is inherently lossy without clear bounds. Our consolidation for procedural knowledge is analogous, but we aim for provably minimal loss. Mem0 also doesn’t highlight any automated staleness removal (they likely assume if it’s not retrieved, it doesn’t hurt since it’s just stored facts).

- **TITANS (Google, 2024):** A test-time learning approach that uses a **surprise metric** to update a neural memory. The idea is the model retains information that surprises it (deviates from prior) and decays old info unless reinforced【8†L29-L37】. This is an inspiration for our “surprise/novelty gated retention.” In our terms, a learning that is very similar to others (not novel) and not recently reinforced is a candidate to forget; a learning that is unique (high surprise) should be kept even if rarely used (because it might cover an unusual but important scenario). We implement a symbolic version: if a learning is the only one in its cluster or category, we keep it longer (it’s providing unique coverage). If there are 10 variants of the same theme, we can consolidate them – their information wasn’t unique (low surprise in that cluster).

- **Bipolar Argumentation (Amgoud et al., Cayrol, etc.):** In AI theory, a **bipolar argumentation framework** has arguments with support (positive relations) and attack (negative relations)【2†L21-L24】. Guiding principles can be seen as arguments recommending an action; cautionary principles attack certain uses of that action. Preferred extensions (maximally consistent sets of arguments) in such frameworks correspond to picking a set of principles that is internally coherent (no undefeated attacks among them). We can draw on this: our consolidation is effectively computing something like a _preferred set of principles_ that is conflict-free and covers as many supports as possible. Formal results from argumentation could provide guarantees about the existence and size of such sets. However, classical argumentation deals with binary accept/reject, whereas our scenario weighs principles by confidence. An extension of argumentation called **graded or weighted argumentation** would be closer, ensuring that we keep a principle if its support (feedback) outweighs attacks (contradictions). We’ve implicitly done that by using scores and pairing complementary ones.

- **AGM Belief Revision (Alchourrón, Gärdenfors, Makinson):** This is a set of postulates for how to incorporate new information into a knowledge base and remove contradictions minimally. One concept is **belief contraction** (removing a belief to restore consistency) with minimal change【2†L9-L17】. Our consolidation can be seen as a kind of _knowledge base contraction/compression_: we remove or merge statements while trying to preserve as much inferred knowledge as possible. The difference is AGM assumes logical entailment in a closed form, whereas we have heuristic, semantic knowledge. Still, principles like _minimality of change_ and _preservation of consistency_ are guiding us. We could say our system aspires to satisfy an analog of the AGM postulates: if two principles conflict, remove (or modify) the smallest element to resolve it; if they overlap, merge instead of drop, etc.

- **Formal Concept Analysis (FCA):** This technique would take a binary relation of learnings to attributes (like categories or situations) and derive a concept lattice, whose join-irreducible elements might correspond to general principles that generate all specifics. That’s an intriguing angle: if each learning is an object with attribute set (e.g. “applies to: {debugging, Python, API tokens}”), FCA would find general concepts (like “{API tokens}” might generate “Always validate API tokens”). Consolidation then is finding a base of concepts such that all learnings can be derived. FCA guarantees minimal bases (like Duquenne-Guigues basis) for formal contexts. We might explore using FCA on a more abstract representation of principles to mathematically guarantee minimality. However, it requires well-defined attributes for each principle, which might be approximated by scope or tags.

In conclusion, our consolidation operator synthesizes ideas from these areas but tailors them to the unique needs of an LLM agent memory:

- Like EvolveR, we extract and deduplicate, but we go further by generalizing.
- Like Copilot, we consider staleness and validity, but we address it with offline compression rather than just-in-time checks.
- Like argumentation, we handle conflicting advice via support/attack analysis, but we integrate continuous feedback weights.
- Like coreset theory, we aim for a compressed representation with bounded loss, bringing a new formal lens to what has mostly been handled ad-hoc in prior agent memory systems.

By grounding our design in these principles and evaluating rigorously, we aim to achieve a memory system that **learns continuously**, **forgets gracefully**, and **consolidates optimally**, providing the agent with the _right_ amount of knowledge: no more, no less, at all times. 【7†L273-L282】【28†L539-L548】
