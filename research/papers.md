# Reflexion: Language Agents with Verbal Reinforcement Learning

View PDF
Abstract:Large language models (LLMs) have been increasingly used to interact with external environments (e.g., games, compilers, APIs) as goal-driven agents. However, it remains challenging for these language agents to quickly and efficiently learn from trial-and-error as traditional reinforcement learning methods require extensive training samples and expensive model fine-tuning. We propose Reflexion, a novel framework to reinforce language agents not by updating weights, but instead through linguistic feedback. Concretely, Reflexion agents verbally reflect on task feedback signals, then maintain their own reflective text in an episodic memory buffer to induce better decision-making in subsequent trials. Reflexion is flexible enough to incorporate various types (scalar values or free-form language) and sources (external or internally simulated) of feedback signals, and obtains significant improvements over a baseline agent across diverse tasks (sequential decision-making, coding, language reasoning). For example, Reflexion achieves a 91% pass@1 accuracy on the HumanEval coding benchmark, surpassing the previous state-of-the-art GPT-4 that achieves 80%. We also conduct ablation and analysis studies using different feedback signals, feedback incorporation methods, and agent types, and provide insights into how they affect performance.

Submission history From: Noah Shinn [view email] [v1]
Mon, 20 Mar 2023 18:08:50 UTC (506 KB) [v2]
Sun, 21 May 2023 06:20:36 UTC (404 KB) [v3]
Sat, 10 Jun 2023 04:32:30 UTC (396 KB) [v4]
Tue, 10 Oct 2023 05:21:45 UTC (386 KB)
---
# EvolveR: Self-Evolving LLM Agents through an Experience-Driven Lifecycle

Rong Wu1,2, Xiaoman Wang311footnotemark: 1, Jianbiao Mei1,2, Pinlong Cai2, Daocheng Fu2,4,
Cheng Yang2,5,
Licheng Wen2,7,8, Xuemeng Yang2, Yufan Shen2, Yuxin Wang9, Botian Shi2
1 Zhejiang University, 2 Shanghai Artificial Intelligence Laboratory,
3 East China Normal University, 4 Fudan University, 5 Central South University,
7 Shanghai Innovation Institute,
8 Shanghai Jiao Tong University,
9 University of Science and Technology of China

Abstract
Current Large Language Model (LLM) agents show strong performance in tool use, but lack the crucial capability to systematically learn from their own experiences. While existing frameworks mainly focus on mitigating external knowledge gaps, they fail to address a more fundamental limitation: the inability to iteratively refine problem-solving strategies. In this work, we introduce EvolveR, a framework designed to enable agent to self-improve through a complete, closed-loop experience lifecycle. This lifecycle comprises two key stages: (1) Offline Self-Distillation, where the agent’s interaction trajectories are synthesized into a structured repository of abstract, reusable strategic principles; (2) Online Interaction, where the agent interacts with tasks and actively retrieves distilled principles to guide its decision-making, accumulating a diverse set of behavioral trajectories. This loop employs a policy reinforcement mechanism to iteratively update the agent based on its performance. We demonstrate the effectiveness of EvolveR on complex multi-hop question-answering benchmarks, where it achieves superior performance over strong agentic baselines. Our work presents a comprehensive blueprint for agents that learn not only from external data but also from the consequences of their own actions, paving the way for more autonomous and continuously improving systems. Code is available at https://github.com/Edaizi/EvolveR.

1 Introduction

Figure 1: An illustration of four major paradigms for LLM agent learning. (1) Stateless Execution: Standard agents discard experiences after each task; (2) Learning by Raw Trajectories: Agents retrieve raw, un-distilled past trajectories; (3) Learning via External Scribing: Agents rely on an external teacher model to distill insights; (4) EvolveR (Ours): A complete, self-contained lifecycle where the agent autonomously distills its own experiences into principles and evolves its policy.

Large Language Models (LLMs) have driven the development of autonomous agents capable of solving diverse tasks through advanced reasoning and tool use [1, 2, 3]. However, a significant limitation emerges when these agents engage in sequential tasks: each interaction is treated independently. They approach tasks as isolated episodes, suffering from operational amnesia and failing to learn from past successes or avoid prior mistakes[4]. This inability to leverage experience fundamentally hinders their development toward greater autonomy and intelligence.
Humans, by contrast, learn through a continuous lifecycle, leveraging both successes and failures to refine strategies over time [5]. For example, a student solving math problems reflects on recurring errors and successful approaches to extract general problem-solving strategies. This cycle of interaction, reflection, and abstraction is the cornerstone of developing expertise [6]. Endowing LLM agents with a comparable lifecycle is the key to bridging the gap between episodic problem-solving and sustainable self-improvement. While existing frameworks like Retrieval-Augmented Generation (RAG) effectively address knowledge gaps, they fail to solve a more fundamental limitation: the agent’s inability to systematically learn from the consequences of its own interactions [7].
As Figure 1 shows, prior works have attempted to address this limitation, but with critical shortcomings. Researchers store natural language reflections across tasks with a powerful external LLM in an external memory [8, 9]. While resource-efficient, this approach treats such reflections as a transient hint, leaving the agent’s intrinsic policy unchanged. On the other hand, learning by recalling raw cases retrieves entire past trajectories to directly guide decision-making. However, this reliance on raw cases struggles to generalize and, more importantly, fails to abstract. The agent merely mimics past solutions instead of distilling the reusable strategic principles that made them successful [10].
To overcome these challenges, we introduce EvolveR, a framework that enables agents to self-evolve by utilizing their own experiences. EvolveR implements a full experience lifecycle, in which agents collect trajectories through Online Interaction, distill them into a library of abstract strategic principles during Offline Self-Distillation, and subsequently learn to apply these principles to new tasks. Crucially, EvolveR completes the experience lifecycle with a reinforcement learning mechanism that enables the agent to utilize experience. The agent does not merely mimic its past interactions; it evolves based on what it has learned. EvolveR maintains a dynamic experience base where newly distilled principles are semantically deduplicated and continuously evaluated via a metric score that tracks historical effectiveness.
We demonstrate EvolveR’s effectiveness on complex question-answering benchmarks, where it significantly outperforms strong agentic baselines. Our contributions can be summarized as follows:

•
We propose the Experience-Driven Self-Evolution Paradigm, a novel, closed-loop lifecycle for LLM agents. In contrast to agents that forget past interactions, EvolveR systematically integrates a complete cycle of online interaction, offline experiences self-distillation and policy evolution. This process enables the agent to continuously transform raw trajectories into a curated repository of strategic principles, establishing a foundation for adaptive agents.

•
We introduce a complete system for dynamic experiences curation. This system goes far beyond simple experience storage. It features: (1) a self-distillation mechanism, where the agent autonomously distills principles from previous interactions; and (2) a full maintenance pipeline, including semantic deduplication, integration, and quality control guided by a dynamic metric score.

•
We provide extensive empirical validation of the EvolveR paradigm across multiple model scales. Our experiments on a diverse suite of complex QA benchmarks demonstrate the effectiveness of our approach.
Detailed ablation studies confirm that the synergy of our proposed curation and self-distillation mechanisms is critical to the framework’s success, revealing a key insight: while the self-distillation mechanism is less effective on smaller-scale models, it surpasses distillation by a stronger, external teacher model at the 3B scale, validating the importance of cognitive alignment.

2 Related Work

2.1 Continual Learning and Self-Evolving Agents
Continual learning (CL) aims to enable models to learn sequentially while mitigating catastrophic forgetting [11, 12]. While various replay-based and regularization methods have been proposed, most CL paradigms assume predefined task boundaries and focus on knowledge preservation rather than active acquisition in open-ended environments [13, 14, 15, 16]. The pursuit of self-evolving agents moves beyond these limitations by enabling systems to grow autonomously from experience. Frameworks such as Reflexion and Generative Agents explore self-improvement through self-play and reflective reasoning, often storing past trajectories as memory to guide future actions [17, 18, 19, 20, 4]. However, these systems either store raw, unstructured data or rely on memory mechanisms that are not designed for the systematic, long-term distillation and refinement of abstract strategic knowledge.
Instead of relying on external data streams, our agent autonomously generates and refines its own experiences through an iterative cycle of online interaction and offline reflection.

2.2 LLM Agents and Reinforcement Learning
LLM agents have been widely explored through frameworks such as ReAct, which interleaves reasoning and actions, and Reflecion, which improves task performance via self-reflection [4, 17]. While these approaches are primarily prompt-based and stateless, they prevent long-term accumulation of strategic knowledge. External memory frameworks like ExpeL address this limitation by reusing past trajectories, but they do not enable systematic self-improvement across tasks [8]. While effective, these methods often rely on simple prompting and are inherently stateless, limiting their ability to internalize knowledge across tasks. Recent work has increasingly turned to reinforcement learning (RL) to train agents for long-horizon, multi-turn tasks. However, applying RL is challenging due to sparse rewards and the need for stable training signals. Search-R1 [21], O2-Searcher [22], and AutoRefine [23] all use RL to train LLMs to generate and interact with external search tools. While these works successfully optimize the LLM’s interaction with external factual knowledge, they do not address the broader challenge of an agent’s self-improvement through its own internal experience.

3 Method

Figure 2:
Overview of the EvolveR framework’s experience lifecycle.
Left: The main loop alternates between an Online Phase, where the agent interacts with the environment and its policy parameters are updated via RL, and an Offline Phase, where the agent’s parameters are frozen and it performs self-distillation and maintains its Experience Base (ℰ\mathcal{E}).
Top Right: A detailed view of the Search ExpBase action, where the agent retrieves scored principles along with their associated trajectories.
Bottom Right: The Update ExpBase process, which involves summarizing trajectories and applying a suite of curation operations (distill, deduplicate, update, and filter).

In this section, we present EvolveR, a novel framework designed to enable agent self-evolution through a complete, closed-loop experience lifecycle. Inspired by the human cycle of work and reflection, our approach is structured around three core, interconnected components, as depicted in Figure 2. First, in the Offline Experience Self-Distillation phase, the agent’s policy parameters are frozen, and it systematically distills raw trajectories into a curated base of strategic principles. Second, during the Online Interaction phase, the agent applies this distilled wisdom to guide its deliberative reasoning and action, generating new, high-quality interaction data. Finally, the entire cycle is driven by a Policy Evolution mechanism, where the trajectories collected online are used to update the agent’s policy parameters via reinforcement learning, thus closing the loop. This iterative process allows the agent to continuously transform its interactions into evolving expertise.

3.1 Preliminaries: Formalizing Agent Interaction
At each state tt, the agent, situated in an unknown state sts_{t}, selects an action at∈𝒜a_{t}\in\mathcal{A} based on its policy. Our agent’s action space 𝒜\mathcal{A} is designed for complex, knowledge-intensive tasks and comprises three key operations:

• <search_experience>: Agent queries its internal experience base ℰ\mathcal{E} to retrieve relevant principles distilled from past trajectories. Environment returns retrieved principles as an observation.

• <search_knowledge>: Agent queries an external knowledge base (e.g., a search engine) to acquire factual information. Environment returns retrieved information as an observation.

• <answer>: Agent outputs its final answer to the problem and concludes the interaction.

3.2 The EvolveR Lifecycle: From Interactions to Principles

3.2.1 Offline Experience Self-Distillation
The core of EvolveR is a self-perpetuating lifecycle designed to transform raw interaction data into a strategic principle. This process is divided into two distinct, alternating phases: an offline self-distillation phase for distilling the principle, and an online interaction phase for applying the principle and gathering new interaction data.

Principle from Self-Distillation.
The process begins with self-distillation. We leverage the agent’s own policy model πθ\pi_{\theta} to analyze its past interaction trajectories. By adopting the persona of an expert through carefully designed prompts, the model reviews each trajectory and, based on its outcome, distills the core strategic insight into a concise natural language statement. This results in either a guiding principle from a success or a cautionary principle from a failure.
Inspired by structured memory frameworks such as Mem0 [24] and G-Memory [25], each principle consists of two components: a natural language description paired with several structured knowledge triples, as illustrated in Figure 2.
This self-distillation approach enables the agent to autonomously generate reusable knowledge.

Deduplication and Integration.
To maintain a high-quality experience base (ℰ\mathcal{E}), we do not add every distilled principle. Instead, each new principle undergoes a rigorous integration process. First, to handle redundancies arising from similar trajectories (e.g., from GRPO sampling), we perform a deduplication step. We use the agent model πθ\pi_{\theta} to pair-wise check for semantic equivalence among newly generated principles that originate from the same problem, keeping only one representative from each semantically equivalent cluster.
Second, for each unique principle, we apply a two-stage matching procedure: we first retrieve the most similar existing principles from ℰ\mathcal{E} via embedding similarity, then prompt the agent model to provide a binary semantic equivalence judgment.
If a principle is novel, it is added as a new entry in ℰ\mathcal{E}; otherwise, the new trajectory is merged under the existing principle, enriching it without introducing redundancy.
Let pcandp_{\text{cand}} be a new candidate principle distilled from trajectory τsrc\tau_{\text{src}}. We update the experience base ℰ\mathcal{E} as follows:

ℰ←{ℰ∪{pcand}if ​maxp∈ℰ⁡sim​(pcand,p)<θsimMerge​(ℰ,p∗,τsrc)otherwise\mathcal{E}\leftarrow\begin{cases}\mathcal{E}\cup\{p_{\text{cand}}\}&\text{if }\max_{p\in\mathcal{E}}\text{sim}(p_{\text{cand}},p)<\theta_{\text{sim}}\\ \text{Merge}(\mathcal{E},p^{*},\tau_{\text{src}})&\text{otherwise}\end{cases}

(1)

where sim​(⋅,⋅)\text{sim}(\cdot,\cdot) is the cosine similarity between principle, θsim\theta_{\text{sim}} is a similarity threshold, and p∗=argmaxp∈ℰ​sim​(pcand,p)p^{*}=\text{argmax}_{p\in\mathcal{E}}\text{sim}(p_{\text{cand}},p). The Merge operation links τsrc\tau_{\text{src}} to its best match p∗p^{*}.
This two-level check ensures that ℰ\mathcal{E} grows with novel insights while strengthening existing ones with new evidence.

Quality Control via Dynamic Scoring.
As the experience base accumulates principles over time, it becomes essential to evaluate their practical utility and prioritize the most effective strategies. To this end, each principle tracks its usage and success counts, enabling the computation of an empirical score that reflects historical performance.
We quantify the empirical utility of each principle using a metric score, which is updated as:

s​(p)=csucc​(p)+1cuse​(p)+2s(p)=\frac{c_{\text{succ}}(p)+1}{c_{\text{use}}(p)+2}

(2)

where csucc​(p)c_{\text{succ}}(p) and cuse​(p)c_{\text{use}}(p) are the success and usage counts for a given principle pp, s​(p)s(p) is the metric score.
This score provides a reliable measure of a principle’s historical effectiveness. To ensure the long-term health of the experience base, we periodically prune principles whose scores fall below a threshold θprune\theta_{\text{prune}}. This systematic process of distillation, integration, and quality control ensures that the agent’s wisdom remains a compact and high-quality repository of its most effective strategies.

3.2.2 Online Interaction
The online phase serves as the interactive testbed where the agent applies its distilled principles to solve problems. The agent operates within a deliberative reasoning loop (e.g., Think-Act-Observe), which enables it to engage in multi-turn, autonomous tool use. However, the core novelty of EvolveR’s online phase is not the loop itself, but how the principles retrieved from the experience base (ℰ\mathcal{E}) fundamentally alter the agent’s behavior within it.

Experience as a Strategic Principle.
Unlike standard agents that must discover reasoning patterns from scratch through trial and error, an EvolveR agent is guided by a strategic wisdom provided by its own past experiences. At any point in its reasoning loop, the agent can issue a <search_experience> action. The retrieved principles 𝒫k\mathcal{P}_{k} do not merely provide factual information; they offer heuristic guidance that shapes the agent’s subsequent reasoning. For instance, retrieving a principle such as “For comparison questions, gather data on both items before concluding,” can directly influence the agent’s internal monologue (<think>) and steer its subsequent potential <search_knowledge> actions. This makes the agent’s exploration more efficient and less prone to common pitfalls, as it learns to follow the wisdom in its own distilled principles.

Generating High-Quality Trajectories for Future Distillation.
The ultimate purpose of the online phase, within the EvolveR paradigm, extends beyond solving the immediate task. It is responsible for generating high-quality data for the next cycle of offline reflection. Because the agent’s actions are guided by proven principles, the resulting trajectories, τnew\tau_{\text{new}}, are not random walks but are instead rich recordings of structured, experience-guided problem-solving.
These trajectories capture the interplay between distilled principles, internal reasoning, and external tool use (e.g., <search_knowledge>), and serve as valuable input for the offline phase, enabling EvolveR to refine existing principles and discover more effective strategies in a virtuous cycle.

3.3 Policy Evolution: Closing the Loop with Reinforcement Learning
To enable the agent to learn from its actions and evolve its policy πθ\pi_{\theta}, we employ a reinforcement learning framework. The learning process is guided by a composite reward function and a policy optimization algorithm that leverages the trajectories collected during the online phase.

Reward Function.
We design a composite reward function R​(τ)R(\tau) for a given trajectory τ\tau that balances task success with procedural correctness. It is a weighted sum of an outcome reward and a format reward: R​(τ)=wo​Routcome​(τ)+wf​Rformat​(τ)R(\tau)=w_{o}R_{\text{outcome}}(\tau)+w_{f}R_{\text{format}}(\tau).

Figure 3: Policy model update optimization algorithm of EvolveR.

•
Outcome Reward RoutcomeR_{\text{outcome}}, is a sparse, binary reward based on the final answer’s correctness. Following prior work, it is determined by an exact match with the ground truth:

Routcome​(τ)=EM​(apred,agold)R_{\text{outcome}}(\tau)=\text{EM}(a_{\text{pred}},a_{\text{gold}})

(3)

where apreda_{\text{pred}} is the final answer extracted from the trajectory τ\tau and agolda_{\text{gold}} is the ground truth answer.

•
Format Reward RformatR_{\text{format}}, is a dense shaping reward that evaluates the quality of the reasoning process. Let Nthink​(τ)N_{\text{think}}(\tau), Nexp​(τ)N_{\text{exp}}(\tau) and Nknow​(τ)N_{\text{know}}(\tau) denote the counts of valid <think>, <search_experience> and <search_knowledge> actions within τ\tau. RformatR_{\text{format}} is composed of a think score RthinkR_{\text{think}}, rewarding a balanced number of reasoning steps, and a search score RsearchR_{\text{search}} promoting search experience and knowledge. The final format reward is calculated as:

Rformat​(τ)=𝕀​(τcomplete)⋅Rthink​(τ)+Rsearch​(τ)2R_{\text{format}}(\tau)=\mathbb{I}(\tau_{\text{complete}})\cdot\frac{R_{\text{think}}(\tau)+R_{\text{search}}(\tau)}{2}

(4)

where 𝕀​(τcomplete)\mathbb{I}(\tau_{\text{complete}}) is an indicator function that is 11 only if the trajectory contains at least one of each required action type (<think>, any search, and <answer>), and 0 otherwise. This ensures that only structurally complete trajectories receive a format reward.

Policy Optimization.
The policy πθ\pi_{\theta} is updated using the collected trajectories. We utilize Group Relative Policy Optimization (GRPO) [26], which balances the optimization stability and efficiency by using the average reward of multiple sampled trajectories as a baseline, thus avoiding the need for a learned value function. Specifically, for each input, we sample a group of GG trajectories. The policy is then optimized by maximizing the following objective function:

𝒥GRPO(θ)=𝔼τ∈𝒟[∑t=1|τ|min(ρt(θ)A^t,clip(ρt(θ),1−ϵ,1+ϵ)A^t)−βDKL[πθ||πref]]\mathcal{J}_{\text{GRPO}}(\theta)=\mathbb{E}_{\tau\in\mathcal{D}}\left[\sum_{t=1}^{|\tau|}\min\left(\rho_{t}(\theta)\hat{A}_{t},\text{clip}(\rho_{t}(\theta),1-\epsilon,1+\epsilon)\hat{A}_{t}\right)-\beta D_{\text{KL}}[\pi_{\theta}||\pi_{\text{ref}}]\right]

(5)

where ρt​(θ)=πθ​(at|ht)πold​(at|ht)\rho_{t}(\theta)=\frac{\pi_{\theta}(a_{t}|h_{t})}{\pi_{\text{old}}(a_{t}|h_{t})} is the importance sampling ratio, A^t\hat{A}_{t} is the advantage estimate, and the final term is a KL-divergence penalty to constrain policy updates.
Crucially, this optimization process is deeply integrated with our experience lifecycle. As the agent’s actions during the online phase are conditioned on the principles 𝒫k\mathcal{P}_{k} retrieved from its experience base, the trajectories collected in 𝒟\mathcal{D} are inherently experience-guided. Consequently, the GRPO update does not merely learn a generic reasoning policy. Instead, it explicitly learns a policy of how to effectively utilize its own distilled wisdom to generate successful outcomes. The optimization process, therefore, reinforces the valuable connections between retrieving high-quality principles and producing high-reward trajectories, successfully closing the learning loop.

4 Experiments

4.1 Experimental Implementation Details

4.1.1 Tasks and Datasets
To comprehensively evaluate the EvolveR paradigm, we assess its performance on seven question-answering benchmarks, encompassing both in-domain and out-of-domain datasets. Following prior work [21, 22], the in-domain datasets, whose training splits are used to build the experience base, include Natural Questions (NQ) [27] and the multi-hop benchmark HotpotQA [28]. The out-of-domain datasets, used exclusively for evaluating generalization, encompass the general QA benchmarks TriviaQA [29] and PopQA [30], as well as the more complex multi-hop challenges 2WikiMultiHopQA [31], Musique [32], and Bamboogle [33].

4.1.2 Baseline Methods
Following prior works, we compare against a comprehensive suite of baselines built upon the Qwen2.5 foundational models. The baselines represent three primary paradigms. First, prompting-based methods, which require no parameter updates, include Direct Inference, Chain-of-Thought (CoT) [18], Retrieval-Augmented Generation (RAG) [34], and advanced variants like IRCoT [35] and Search-o1 [36]. Second, Supervised Fine-Tuning (SFT) methods represent approaches that learn from static expert data, including standard SFT [37] and Rejection Sampling [38]. Finally, the most direct competitors are RL methods, against which we benchmark extensively. This category is primarily composed of Search-R1 [21], DeepSeek-R1 [39], which are also trained with trajectory-level feedback. Specifically, DeepSeek-R1 performs reasoning and answer steps without a search engine, whereas Search-R1 incorporates an external local or web search engine. Together, these baselines provide a challenging evaluation landscape for our proposed paradigm.

4.1.3 Evaluation Metrics
To ensure a direct and fair comparison with prior work in our main results, our primary evaluation metric is Exact Match (EM), a strict measure that requires the predicted answer to exactly match the ground truth after standard normalization. We also report the F11 Score in the analysis of model scales’ generalizability, which provides a more comprehensive and robust measure of performance, particularly since ground truths may contain multiple valid answers or aliases.

4.1.4 Implementation Details
Our experiments are conducted on the Qwen2.5 model family. Inspired by DeepSeek-R1 [39], we introduce a cold-start stage to stabilize early RL training by first fine-tuning the base model on a small, curated dataset of CoT interaction trajectories. Following the setup of Search-R1, we construct this dataset from approximately 700 samples from the NQ and HotpotQA training sets.
We utilize the LLama_Factory [40] to fine-tune the model with LoRA. For the agent evolution phase, we employ GRPO for optimization. At each RL step, we sample a batch of 128 prompts, generating G=8G=8 trajectories for each. The agent is then updated, again using Adam, but with a reduced learning rate of 1×10−61\times 10^{-6}, a warm-up step of 20 and a mini-batch size of 128. All training is conducted on 8 A100 GPUs, leveraging the Verl framework 111https://github.com/volcengine/verl for efficient implementation. We will show more details in Appendix 4.1.

4.2 Main Results
The main results of our evaluation are presented in Table 1. Our analysis focuses on the comprehensive evaluation conducted on the Qwen2.5-3B model family (we will show more results of different model scales in the 5.1). EvolveR achieves the highest average score (0.382) in the 3B scale, outperforming all baselines, including strong RL agents like Searcher-R1. This robust overall performance is not driven by a narrow specialty, but by consistent, top-tier results across a wide spectrum of tasks; it secures the best scores on diverse benchmarks, including the in-domain NQ, the out-of-domain PopQA, and the adversarial Bamboogle dataset, while remaining highly competitive on all others.
This consistent, high-level performance across diverse benchmarks validates that by systematically distilling, managing, and utilizing, agents can develop more generalizable and powerful problem-solving strategies.

Table 1: Main results on QA benchmarks. The best performance in each column is set in bold. Our proposed model, EvolveR, is highlighted in gray.

5 Further Analysis

5.1 Analysis of Model Scales Generalizability

Figure 4: Performance of EvolveR across various model scales.

To validate that our EvolveR framework is a generalizable paradigm rather than a method tailored to a specific model size, we evaluated its performance across a spectrum of open-source model scales. As presented in Figure 4, we applied EvolveR to Qwen2.5 models of 0.5B, 1.5B, and 3B parameters. The results reveal a clear and consistent positive trend: as the parameter count of the base model increases, the performance of the EvolveR agent improves significantly on every benchmark. The average performance rises monotonically from 0.150 on the 0.5B model to 0.270 on the 1.5B model, and further to 0.382 on the 3B model. This scaling behavior demonstrates that our experience-driven lifecycle effectively harnesses the superior reasoning and instruction-following capabilities inherent in larger foundational models. It confirms that EvolveR acts as a synergistic layer on top of the base model, and suggests that its performance will continue to improve with future advancements in the open-source LLM landscape.

5.2 Ablation Studies: Dissecting the EvolveR Framework

5.2.1 Validating the Self-Distillation Mechanism
A central claim of our work is that an agent can learn effectively through self-distillation. To rigorously investigate this, we compare our standard EvolveR (self-distill) against a strong alternative, EvolveR(teacher-distill), which uses the powerful GPT-4o-mini as an external model for experience distillation.
The results, presented in Table 2, reveal a nuanced, scale-dependent relationship. For smaller models like the 0.5B variant, the stronger external teacher provides a clear benefit, as the base model’s own distillation capabilities are limited. However, as the model scales to 3B, a reversal occurs: our EvolveR (self-distill) (0.382 avg.) outperforms the teacher-guided variant (0.370 avg.). This is a critical finding, suggesting that as an agent’s own reasoning becomes more sophisticated, principles distilled from its own internal policy are ultimately more effective due to better ”cognitive alignment”. This validates self-distillation as a core, scaling strength of the EvolveR paradigm.

Table 2: Validating the self-distillation mechanism. We compare our EvolveR, which uses its own model for distillation, against a variant that uses a larger, external model (GPT-4o-mini).

5.2.2 The Role of Experience Retrieval
To quantify the direct benefit of providing the agent with access to its distilled principles at inference time. To achieve this, we compare our full EvolveR model against an ablated variant, EvolveR w/o exp-retrieve. It is critical to note that both models undergo the identical experience-driven RL training process. The sole difference is that the w/o exp-retrieve variant is denied access to the experience base during evaluation.
The results in Table 3 show a stark performance degradation across all model scales when experience retrieval is disabled. For the 3B model, for instance, the average performance drops significantly from 0.382 to 0.340. This substantial gap underscores a key finding: an agent trained with our EvolveR framework, while powerful on its own, achieves its full potential only when it can access and condition on the relevant principles from its past. This demonstrates that experience retrieval is a critical and indispensable component of the EvolveR paradigm for optimal performance.

Table 3: Investigating the role of experience retrieval at inference time. The w/o exp-retrieve variant uses the same model but is not allowed to access the experience base during evaluation.

6 Conclusion
In this work, we introduced EvolveR, a novel paradigm for self-evolving LLM agents centered on a complete, closed-loop experience lifecycle. Our extensive experiments demonstrate the effectiveness of this approach, showing that EvolveR consistently and significantly outperforms a wide range of strong baseline methods on a comprehensive suite of QA benchmarks. Furthermore, our detailed ablation studies rigorously validate the core tenets of our framework, confirming the significant value of the agent’s self-distilled experiences and demonstrating the high efficacy of the self-distillation mechanism itself. While the quality of distilled principles is inherently tied to the base model’s capabilities, pointing to promising future work, EvolveR provides a concrete blueprint for agents that learn from the consequences of their own experiences, shifting the focus from merely accessing knowledge to actively building and evolving expertise.

References

Shen et al. [2023]

Yongliang Shen, Kaitao Song, Xu Tan, Dongsheng Li, Weiming Lu, and Yueting Zhuang.

Hugginggpt: Solving ai tasks with chatgpt and its friends in hugging face.

Advances in Neural Information Processing Systems, 36:38154–38180, 2023.

Luo et al. [2025]

Junyu Luo, Weizhi Zhang, Ye Yuan, Yusheng Zhao, Junwei Yang, Yiyang Gu, Bohan Wu, Binqi Chen, Ziyue Qiao, Qingqing Long, Rongcheng Tu, Xiao Luo, Wei Ju, Zhiping Xiao, Yifan Wang, Meng Xiao, Chenwu Liu, Jingyang Yuan, Shichang Zhang, Yiqiao Jin, Fan Zhang, Xian Wu, Hanqing Zhao, Dacheng Tao, Philip S. Yu, and Ming Zhang.

Large language model agent: A survey on methodology, applications and challenges, 2025.

URL https://arxiv.org/abs/2503.21460.

ang Gao et al. [2025]

Huan ang Gao, Jiayi Geng, Wenyue Hua, Mengkang Hu, Xinzhe Juan, Hongzhang Liu, Shilong Liu, Jiahao Qiu, Xuan Qi, Yiran Wu, Hongru Wang, Han Xiao, Yuhang Zhou, Shaokun Zhang, Jiayi Zhang, Jinyu Xiang, Yixiong Fang, Qiwen Zhao, Dongrui Liu, Qihan Ren, Cheng Qian, Zhenhailong Wang, Minda Hu, Huazheng Wang, Qingyun Wu, Heng Ji, and Mengdi Wang.

A survey of self-evolving agents: On path to artificial super intelligence, 2025.

URL https://arxiv.org/abs/2507.21046.

Yao et al. [2023a]

Shunyu Yao, Jeffrey Zhao, Dian Yu, Nan Du, Izhak Shafran, Karthik Narasimhan, and Yuan Cao.

React: Synergizing reasoning and acting in language models, 2023a.

URL https://arxiv.org/abs/2210.03629.

Flesch et al. [2018]

Timo Flesch, Jan Balaguer, Ronald Dekker, Hamed Nili, and Christopher Summerfield.

Comparing continual task learning in minds and machines.

Proceedings of the National Academy of Sciences, 115(44):E10313–E10322, 2018.

Anderson [1993]

John R Anderson.

Problem solving and learning.

American psychologist, 48(1):35, 1993.

Yan et al. [2025]

Sikuan Yan, Xiufeng Yang, Zuchao Huang, Ercong Nie, Zifeng Ding, Zonggen Li, Xiaowen Ma, Hinrich Schütze, Volker Tresp, and Yunpu Ma.

Memory-r1: Enhancing large language model agents to manage and utilize memories via reinforcement learning.

arXiv preprint arXiv:2508.19828, 2025.

Zhao et al. [2024]

Andrew Zhao, Daniel Huang, Quentin Xu, Matthieu Lin, Yong-Jin Liu, and Gao Huang.

Expel: Llm agents are experiential learners, 2024.

URL https://arxiv.org/abs/2308.10144.

Zhou et al. [2025]

Huichi Zhou, Yihang Chen, Siyuan Guo, Xue Yan, Kin Hei Lee, Zihan Wang, Ka Yiu Lee, Guchun Zhang, Kun Shao, Linyi Yang, and Jun Wang.

Memento: Fine-tuning llm agents without fine-tuning llms, 2025.

URL https://arxiv.org/abs/2508.16153.

Chen et al. [2023]

Liting Chen, Lu Wang, Hang Dong, Yali Du, Jie Yan, Fangkai Yang, Shuang Li, Pu Zhao, Si Qin, Saravan Rajmohan, et al.

Introspective tips: Large language model for in-context decision making.

arXiv preprint arXiv:2305.11598, 2023.

Parisi et al. [2019]

German I Parisi, Ronald Kemker, Jose L Part, Christopher Kanan, and Stefan Wermter.

Continual lifelong learning with neural networks: A review.

Neural networks, 113:54–71, 2019.

Wang et al. [2024]

Liyuan Wang, Xingxing Zhang, Hang Su, and Jun Zhu.

A comprehensive survey of continual learning: Theory, method and application, 2024.

URL https://arxiv.org/abs/2302.00487.

Kirkpatrick et al. [2017]

James Kirkpatrick, Razvan Pascanu, Neil Rabinowitz, Joel Veness, Guillaume Desjardins, Andrei A. Rusu, Kieran Milan, John Quan, Tiago Ramalho, Agnieszka Grabska-Barwinska, Demis Hassabis, Claudia Clopath, Dharshan Kumaran, and Raia Hadsell.

Overcoming catastrophic forgetting in neural networks.

Proceedings of the National Academy of Sciences, 114(13):3521–3526, March 2017.

ISSN 1091-6490.

doi: 10.1073/pnas.1611835114.

URL http://dx.doi.org/10.1073/pnas.1611835114.

Ding et al. [2024]

Xuanwen Ding, Jie Zhou, Liang Dou, Qin Chen, Yuanbin Wu, Chengcai Chen, and Liang He.

Boosting large language models with continual learning for aspect-based sentiment analysis, 2024.

URL https://arxiv.org/abs/2405.05496.

Huai et al. [2025a]

Tianyu Huai, Jie Zhou, Yuxuan Cai, Qin Chen, Wen Wu, Xingjiao Wu, Xipeng Qiu, and Liang He.

Task-core memory management and consolidation for long-term continual learning, 2025a.

URL https://arxiv.org/abs/2505.09952.

Huai et al. [2025b]

Tianyu Huai, Jie Zhou, Xingjiao Wu, Qin Chen, Qingchun Bai, Ze Zhou, and Liang He.

Cl-moe: Enhancing multimodal large language model with dual momentum mixture-of-experts for continual visual question answering, 2025b.

URL https://arxiv.org/abs/2503.00413.

Shinn et al. [2023]

Noah Shinn, Federico Cassano, Edward Berman, Ashwin Gopinath, Karthik Narasimhan, and Shunyu Yao.

Reflexion: Language agents with verbal reinforcement learning, 2023.

URL https://arxiv.org/abs/2303.11366.

Wei et al. [2022]

Jason Wei, Xuezhi Wang, Dale Schuurmans, Maarten Bosma, Fei Xia, Ed Chi, Quoc V Le, Denny Zhou, et al.

Chain-of-thought prompting elicits reasoning in large language models.

Advances in neural information processing systems, 35:24824–24837, 2022.

Yao et al. [2023b]

Shunyu Yao, Dian Yu, Jeffrey Zhao, Izhak Shafran, Tom Griffiths, Yuan Cao, and Karthik Narasimhan.

Tree of thoughts: Deliberate problem solving with large language models.

Advances in neural information processing systems, 36:11809–11822, 2023b.

Besta et al. [2024]

Maciej Besta, Nils Blach, Ales Kubicek, Robert Gerstenberger, Michal Podstawski, Lukas Gianinazzi, Joanna Gajda, Tomasz Lehmann, Hubert Niewiadomski, Piotr Nyczyk, et al.

Graph of thoughts: Solving elaborate problems with large language models.

In Proceedings of the AAAI conference on artificial intelligence, volume 38, pages 17682–17690, 2024.

Jin et al. [2025]

Bowen Jin, Hansi Zeng, Zhenrui Yue, Jinsung Yoon, Sercan Arik, Dong Wang, Hamed Zamani, and Jiawei Han.

Search-r1: Training llms to reason and leverage search engines with reinforcement learning, 2025.

URL https://arxiv.org/abs/2503.09516.

Mei et al. [2025]

Jianbiao Mei, Tao Hu, Daocheng Fu, Licheng Wen, Xuemeng Yang, Rong Wu, Pinlong Cai, Xinyu Cai, Xing Gao, Yu Yang, Chengjun Xie, Botian Shi, Yong Liu, and Yu Qiao.

O2-searcher: A searching-based agent model for open-domain open-ended question answering, 2025.

URL https://arxiv.org/abs/2505.16582.

Shi et al. [2025]

Yaorui Shi, Sihang Li, Chang Wu, Zhiyuan Liu, Junfeng Fang, Hengxing Cai, An Zhang, and Xiang Wang.

Search and refine during think: Autonomous retrieval-augmented reasoning of llms, 2025.

URL https://arxiv.org/abs/2505.11277.

Chhikara et al. [2025]

Prateek Chhikara, Dev Khant, Saket Aryan, Taranjeet Singh, and Deshraj Yadav.

Mem0: Building production-ready ai agents with scalable long-term memory, 2025.

URL https://arxiv.org/abs/2504.19413.

Zhang et al. [2025]

Guibin Zhang, Muxin Fu, Guancheng Wan, Miao Yu, Kun Wang, and Shuicheng Yan.

G-memory: Tracing hierarchical memory for multi-agent systems, 2025.

URL https://arxiv.org/abs/2506.07398.

Shao et al. [2024]

Zhihong Shao, Peiyi Wang, Qihao Zhu, Runxin Xu, Junxiao Song, Xiao Bi, Haowei Zhang, Mingchuan Zhang, Y. K. Li, Y. Wu, and Daya Guo.

Deepseekmath: Pushing the limits of mathematical reasoning in open language models, 2024.

URL https://arxiv.org/abs/2402.03300.

Kwiatkowski et al. [2019]

Tom Kwiatkowski, Jennimaria Palomaki, Olivia Redfield, Michael Collins, Ankur Parikh, Chris Alberti, Danielle Epstein, Illia Polosukhin, Jacob Devlin, Kenton Lee, et al.

Natural questions: a benchmark for question answering research.

Transactions of the Association for Computational Linguistics, 7:453–466, 2019.

Yang et al. [2018]

Zhilin Yang, Peng Qi, Saizheng Zhang, Yoshua Bengio, William W Cohen, Ruslan Salakhutdinov, and Christopher D Manning.

Hotpotqa: A dataset for diverse, explainable multi-hop question answering.

arXiv preprint arXiv:1809.09600, 2018.

Joshi et al. [2017]

Mandar Joshi, Eunsol Choi, Daniel S Weld, and Luke Zettlemoyer.

Triviaqa: A large scale distantly supervised challenge dataset for reading comprehension.

arXiv preprint arXiv:1705.03551, 2017.

Mallen et al. [2022]

Alex Mallen, Akari Asai, Victor Zhong, Rajarshi Das, Daniel Khashabi, and Hannaneh Hajishirzi.

When not to trust language models: Investigating effectiveness of parametric and non-parametric memories.

arXiv preprint arXiv:2212.10511, 2022.

Ho et al. [2020]

Xanh Ho, Anh-Khoa Duong Nguyen, Saku Sugawara, and Akiko Aizawa.

Constructing a multi-hop qa dataset for comprehensive evaluation of reasoning steps.

arXiv preprint arXiv:2011.01060, 2020.

Trivedi et al. [2022a]

Harsh Trivedi, Niranjan Balasubramanian, Tushar Khot, and Ashish Sabharwal.

Musique: Multihop questions via single-hop question composition.

Transactions of the Association for Computational Linguistics, 10:539–554, 2022a.

Press et al. [2022]

Ofir Press, Muru Zhang, Sewon Min, Ludwig Schmidt, Noah A Smith, and Mike Lewis.

Measuring and narrowing the compositionality gap in language models.

arXiv preprint arXiv:2210.03350, 2022.

Lewis et al. [2020]

Patrick Lewis, Ethan Perez, Aleksandra Piktus, Fabio Petroni, Vladimir Karpukhin, Naman Goyal, Heinrich Küttler, Mike Lewis, Wen-tau Yih, Tim Rocktäschel, et al.

Retrieval-augmented generation for knowledge-intensive nlp tasks.

Advances in neural information processing systems, 33:9459–9474, 2020.

Trivedi et al. [2022b]

Harsh Trivedi, Niranjan Balasubramanian, Tushar Khot, and Ashish Sabharwal.

Interleaving retrieval with chain-of-thought reasoning for knowledge-intensive multi-step questions.

arXiv preprint arXiv:2212.10509, 2022b.

Li et al. [2025]

Xiaoxi Li, Guanting Dong, Jiajie Jin, Yuyao Zhang, Yujia Zhou, Yutao Zhu, Peitian Zhang, and Zhicheng Dou.

Search-o1: Agentic search-enhanced large reasoning models.

arXiv preprint arXiv:2501.05366, 2025.

Chung et al. [2024]

Hyung Won Chung, Le Hou, Shayne Longpre, Barret Zoph, Yi Tay, William Fedus, Yunxuan Li, Xuezhi Wang, Mostafa Dehghani, Siddhartha Brahma, et al.

Scaling instruction-finetuned language models.

Journal of Machine Learning Research, 25(70):1–53, 2024.

Ahn et al. [2024]

Janice Ahn, Rishu Verma, Renze Lou, Di Liu, Rui Zhang, and Wenpeng Yin.

Large language models for mathematical reasoning: Progresses and challenges.

arXiv preprint arXiv:2402.00157, 2024.

Guo et al. [2025]

Daya Guo, Dejian Yang, Haowei Zhang, Junxiao Song, Ruoyu Zhang, Runxin Xu, Qihao Zhu, Shirong Ma, Peiyi Wang, Xiao Bi, et al.

Deepseek-r1: Incentivizing reasoning capability in llms via reinforcement learning.

arXiv preprint arXiv:2501.12948, 2025.

Zheng et al. [2024]

Yaowei Zheng, Richong Zhang, Junhao Zhang, YeYanhan YeYanhan, and Zheyan Luo.

LlamaFactory: Unified efficient fine-tuning of 100+ language models.

In Yixin Cao, Yang Feng, and Deyi Xiong, editors, Proceedings of the 62nd Annual Meeting of the Association for Computational Linguistics (Volume 3: System Demonstrations), pages 400–410, Bangkok, Thailand, August 2024. Association for Computational Linguistics.

doi: 10.18653/v1/2024.acl-demos.38.

URL https://aclanthology.org/2024.acl-demos.38.

Qwen et al. [2025]

Qwen, :, An Yang, Baosong Yang, Beichen Zhang, Binyuan Hui, Bo Zheng, Bowen Yu, Chengyuan Li, Dayiheng Liu, Fei Huang, Haoran Wei, Huan Lin, Jian Yang, Jianhong Tu, Jianwei Zhang, Jianxin Yang, Jiaxi Yang, Jingren Zhou, Junyang Lin, Kai Dang, Keming Lu, Keqin Bao, Kexin Yang, Le Yu, Mei Li, Mingfeng Xue, Pei Zhang, Qin Zhu, Rui Men, Runji Lin, Tianhao Li, Tianyi Tang, Tingyu Xia, Xingzhang Ren, Xuancheng Ren, Yang Fan, Yang Su, Yichang Zhang, Yu Wan, Yuqiong Liu, Zeyu Cui, Zhenru Zhang, and Zihan Qiu.

Qwen2.5 technical report, 2025.

URL https://arxiv.org/abs/2412.15115.

Chen et al. [2024]

Jianlv Chen, Shitao Xiao, Peitian Zhang, Kun Luo, Defu Lian, and Zheng Liu.

Bge m3-embedding: Multi-lingual, multi-functionality, multi-granularity text embeddings through self-knowledge distillation.

arXiv preprint arXiv:2402.03216, 2024.

Appendix A Appendix

A.1 Experimental Implementation Details
We provide a comprehensive list of hyperparameters and implementation details used in our experiments to ensure full reproducibility.

General Setup.
Across all experiments, we use models from the Qwen2.5 family [41] with their corresponding tokenizers. The maximum sequence length is set to 8192 tokens for all inputs, and the maximum response sequence length is set to 1024 tokens. The GPT-4o-mini model is used as the teacher model in the corresponding ablation study. We use BGE-M3 [42] as our embedding model.

Cold-start Stage.
This SFT stage is conducted for 3 epochs using the Adam optimizer, with an initial learning rate of 1×10−41\times 10^{-4}, a warm-up ratio of 0.1, and a batch size of 16.

Online Interaction Phase.
For each <search_knowledge> action, we retrieve the top-kd=3k_{d}=3 documents from the external knowledge base, following the prior work [21]. Similarly, for each <search_experience> action, we retrieve the top-ke=3k_{e}=3 principles from the experience base ℰ\mathcal{E}.

Offline Distill Phase.
The self-distill mechanism utilizes the agent’s own policy model πθ\pi_{\theta} to distill principles. The temperature is set to 1 during this phase. For the deduplication and integration process, we first use a semantic similarity pre-filter with a threshold of θsim=0.85\theta_{\text{sim}}=0.85 before passing candidates to the LLM-based equivalence check. The periodic principle sweep removes any principle from ℰ\mathcal{E} whose metric_score falls below the pruning threshold of θprune=0.3\theta_{\text{prune}}=0.3.

Reward Function Details.
As described in the Section 3.3, the Format Reward is an average of a think score and a search score. We detail their specific calculation here. The think score RthinkR_{\text{think}} is determined by a discrete mapping based on the number of <think> actions, NthinkN_{\text{think}}: it scales from 0.2 (for Nthink=1N_{\text{think}}=1) to a maximum of 1.0 (for Nthink=6N_{\text{think}}=6), and is capped at 0.5 for excessive reasoning (Nthink>8N_{\text{think}}>8) to encourage conciseness. The search score RsearchR_{\text{search}} is the sum of a diversity score and a quantity bonus. The diversity score is 0.5 if both <search_experience> and <search_knowledge> are used, 0.2 if only one type is used, and 0 otherwise. A quantity bonus of 0.1 is added for each additional search action beyond the first, up to a maximum bonus of 0.5 (for a total of 6 searches).

Policy Optimization.
The composite reward function is weighted with wo=1.0w_{o}=1.0 for the outcome reward and wf=0.1w_{f}=0.1 for the format reward. For the GRPO objective function (Equation 5), the clipping parameter is set to ϵ=0.2\epsilon=0.2 and the KL-divergence coefficient is β=0.001\beta=0.001. During the training procedure, we adopt vLLM to accelerate LLM rollouts. The tensor parallel size is set to 1, and the GPU memory utilization ratio is set at 0.6. For rollout sampling, we use a temperature of 1.0 and a top-p value of 0.95.

A.2 Prompt Details

A.2.1 Cold Start Prompt
The Prompt in Table 5 is used during the cold-start stage to generate the initial trajectories for SFT. This prompt guides a powerful LLM (we used GPT-4o) to act as an expert problem-solver, producing a small dataset with right format trajectories to cold start.

A.2.2 System Prompt
The Prompt in Table 4 is the system prompt used by the EvolveR agent during the online interaction phase. It defines the agent’s core identity, its available actions (<think>, <search_knowledge>, <search_experience>, <answer>), and the overall format for its reasoning process.

A.2.3 Distill Principle Prompt
The Prompt in Table 6 and Table 7 are used during the offline experience self-distillation phase to enable the agent’s self-distillation mechanism. Based on the outcome of a trajectory, one of two distinct prompts is used to guide the agent’s own model (πθ\pi_{\theta}) to distill a principle. The first Prompt is for successful trajectories, focusing on extracting a guiding principle. The second is for failed trajectories, aimed at formulating a cautionary principle.

A.2.4 Judge Same Principle Prompt
The Prompt in Table 8 is a crucial component of the deduplication and integration process within the offline experience self-distillation. It tasks the agent’s own model (πθ\pi_{\theta}) with acting as a semantic judge. Given two principles (a newly distilled candidate and a retrieved existing similar one), the model is asked to determine if they are semantically equivalent. The binary ”yes/no” output of this Prompt is used to decide whether to merge a new experience or create a new principle.

Table 4: System prompt for LLM agents

Answer the given question.

You must conduct reasoning inside <think> and </think> first every time you get new information or get new experience principles.

After reasoning, you can search for past experiences by <search_experience> query </search_experience> to get relevant past experience principles (may be guilding or warning principles) and it will return the top searched results between <experience> and </experience>.

You can use these principles which you think is helpful to help you answer the question.

If you find you lack some knowledge, you can call a search engine by <search_knowledge> query </search_knowledge> and it will return the top searched results between <information> and </information>.

You can search knowledge and experience as many times as your want.

If you find no further external knowledge needed, you can directly provide the answer inside <answer> and </answer>, without detailed illustrations.

For example, <answer> Beijing </answer>

User: {question}

Table 5: Prompt for cold start.

You are a top-notch intelligent reasoning expert, adept at restoring solution paths from given answers and documents in reverse. Your task is to simulate a full reasoning trajectory for answering the question below, based on the provided documents and answer. You must reason step-by-step as if you do not yet know the final answer, even though it is given for supervision.

In <think> blocks, do not reference or confirm the final answer directly. Instead, reason like a human—understand the task, recall prior knowledge, evaluate the need for experience or external information, and gradually infer the answer.

The reasoning trajectory must follow the **exact format below**. If the retrieved **experience alone is sufficient to answer the question**, you may skip the <search_knowledge> and <information> steps.

Output Format:

<think> … </think>

<search_experience>

- Retrieve 2–3 relevant abstract experience principles, using structured triple format.

- For each principle, add a short description of its purpose.

</search_experience>

<think> Explain what you plan to do after retrieving experience. Decide whether you still need to retrieve knowledge. </think>

[IF experience is enough:]

<think>

- List the principles you are applying, include their triple form and description.

- Explain briefly how each principle contributes to your reasoning.

- Continue with reasoning based on these principles and conclude with your final judgment.

</think>

<answer>…</answer>

[ELSE:]

<search_knowledge>

- Generate one or more natural language search queries that would help retrieve the provided documents.

</search_knowledge>

<information>

{relevant_document}

</information>

<think> Reflect on retrieved information. </think>

<think>

- List the principles you are applying, include their triple form and description.

- Explain how each principle guides the reasoning process using the retrieved information.

- Summarize your reasoning path and justify the answer.

</think>

<Answer>…</Answer>

Inputs:

Query: {query}

Relevant Documents: {relevant_document}

Answer: {answer}

Please begin generating the reasoning trajectory.

Table 6: Prompt for summarizing a successful interaction trajectory.

You are an expert in analyzing interaction logs to distill generalizable wisdom.

Analyze the following successful interaction trajectory. Your goal is to extract a ”Guiding Principle” from it.

A ”Guiding Principle” has two parts:

1. A concise, one-sentence natural language description. This is the core advice.

2. A structured representation of the key steps or logic, as a list of simple (subject, predicate, object) triplets.

[Trajectory Log]:
{{trajectory_log}}

Final Outcome: SUCCESS

Your Task:

Based on the trajectory, generate the Guiding Principle.

First, on a new line, write {DESCRIPTION_PART_SEPARATOR}.

Then, write the one-sentence description of the pitfall.

Then, on a new line, write {STRUCTURED_PART_SEPARATOR}.

Finally, provide the structured triplets describing the failure pattern in a valid JSON list format.

[Example]:

{DESCRIPTION_PART_SEPARATOR}

When a file download fails with a 404 error, do not immediately retry the download; instead, verify the source URL’s validity first.

{STRUCTURED_PART_SEPARATOR}[ (file download, results_in, 404 error), (immediate_retry, is, ineffective), (correct_action, is, verify URL) ]

[Output]:

Table 7: Prompt for summarizing a failed interaction trajectory.

You are an expert in analyzing interaction logs to find the root cause of failures.

Analyze the following failed interaction trajectory. Your goal is to extract a ”Cautionary Principle” from it.

A ”Cautionary Principle” has two parts:

1. A concise, one-sentence description of the key mistake to avoid and under what circumstances.

2. A structured representation of the failure pattern, as a list of simple (subject, predicate, object) triplets.

[Trajectory Log]:
{{trajectory_log}}

Final Outcome: FAILURE

Your Task:

Based on the trajectory, generate the Cautionary Principle.

First, on a new line, write {DESCRIPTION_PART_SEPARATOR}.

Then, write the one-sentence description of the pitfall.

Then, on a new line, write {STRUCTURED_PART_SEPARATOR}.

Finally, provide the structured triplets describing the failure pattern in a valid JSON list format.

[Example]:

{DESCRIPTION_PART_SEPARATOR}

When a file download fails with a 404 error, do not immediately retry the download; instead, verify the source URL’s validity first.

{STRUCTURED_PART_SEPARATOR}[ (file download, results_in, 404 error), (immediate_retry, is, ineffective), (correct_action, is, verify URL) ]

[Output]:

Table 8: Prompt for Principle Similarity Analysis.

You are a semantic analysis expert. Determine if two principles describe the same core idea, even if they use different words.

Principle A: “{summary}”

Principle B: “{existing_principle_description}”

Do Principle A and Principle B describe the same essential advice or warning?

Please answer with only “Yes” or “No”.

A.3 Exploring the Influence of Experience Internalization
In our proposed framework, all retrieved information (both from the external knowledge base (<information>) and our internal experience base (<experience>)) is treated as context, with loss masked during the model update phase. A natural question arises from this design: while it is sensible to avoid learning the content of transient external documents, could the agent benefit from directly ‘absorbing‘ its own distilled wisdom into its parameters?
To explore this, we conducted a supplementary experiment on the Qwen2.5-3B model. We created a variant, EvolveR w/ exp-absorb, where we selectively unmasked the loss for the retrieved <experience> tokens, allowing the learning signal to flow through them. Our hypothesis was that this might enable the agent to internalize the strategic logic of its principles. The results, presented in Table 9, were insightful. The EvolveR w/ exp-absorb variant exhibited a slight performance degradation compared to our standard approach. We posit that this is due to the challenge of noise in the training signal. In our current implementation, the agent retrieves a set of top-kk principles at each step. Not all of these principles may be perfectly relevant to the immediate context. By directly internalizing all retrieved principles without a dynamic quality filter, the agent risks being updated with noisy or even counter-productive signals. This finding suggests that for direct internalization to be effective, it may require more sophisticated mechanisms, such as an auxiliary model to weigh the relevance of each principle before absorption. We believe that developing such mechanisms is a promising future direction towards achieving a fully autonomous cycle of self-exploration, self-distillation, and self-absorption.

Table 9: Ablation study on the experience internalization mechanism. EvolveR w/o exp-absorb treats principles as external context by masking gradients during backpropagation.

A.4 Case Study of EvolveR
We provide a detail rollout case of EvolveR in Table 10

Table 10: A rollout case of EvolveR

System: System_Prompt

Question: who was the pinkerton detective agency’s first female detective?

Ground Truth: Kate Warne

EvolveR: <think>I need to determine who was the first female detective employed by the Pinkerton Detective Agency. This requires identifying the historical context and any notable female figures associated with this agency. Based on prior reasoning experience, I anticipate some potentially useful principles:- Principle 1: (Historical Agency, First Female Employee, Female Detective) - This principle helps identify the first female to hold a specific role within a historical organization.
- Principle 2: (Organization, Historical Role, Notable Individual) - This principle assists in linking an organization to a notable individual who played a significant role in its history.</think> <search_experience>- Principle 1: (Historical Agency, First Female Employee, Female Detective) - This principle helps identify the first female to hold a specific role within a historical organization.
- Principle 2: (Organization, Historical Role, Notable Individual) - This principle assists in linking an organization to a notable individual who played a significant role in its history.</search_experience>

Env: <experience>[Principle 0], type: guiding, metric score: 0.67, description: To identify the first woman to hold a specific significant position within an organization or entity, connect the historical narrative context and mention which event or timeline may signify the position in question…<experience>

EvolveR: <think>Based on the experience principles, I plan to apply the principles related to identifying the first female employee in a historical organization and linking this to notable figures. I will also consider using historical documents or specific timelines to confirm the answer.</think> <search_knowledge>- Query: who was the first female detective employed by the Pinkerton Detective Agency?</search_knowledge>

Env: <information>Doc 1(Title: ”Kate Warne”)… Doc 2(Title: ”Private investigator”) … Doc 3(Title: ”Kate Warne”)…</information>

EvolveR: <think>Based on the retrieved documents, I need to determine who was the first female detective employed by the Pinkerton Detective Agency. I will apply the principles to connect the historical context and notable figures mentioned.
- Principle 1: (Historical Agency, First Female Employee, Female Detective) - This principle helps identify the first female detective within the Pinkerton Detective Agency.
- Principle 2: (Organization, Historical Role, Notable Individual) - This principle links the Pinkerton Detective Agency to Kate Warne as the first female detective.</think> <search_knowledge>- Query: who was the first female detective employed by the Pinkerton Detective Agency?</search_knowledge>

Env: <information>Doc 1(Title: ”Kate Warne”) … Doc 2(Title: ”Private investigator”) … Doc 3(Title: ”Kate Warne”) …</information>

EvolveR: <think>Based on the retrieved documents, Kate Warne is identified as the first female detective employed by the Pinkerton Detective Agency. This aligns with the historical context and the principles applied.
- Principle 1: (Historical Agency, First Female Employee, Female Detective) - This principle confirms Kate Warne as the first female detective within the Pinkerton Detective Agency.
- Principle 2: (Organization, Historical Role, Notable Individual) - This principle links Kate Warne to her role as the first female detective in the agency’s history.</think> <search_knowledge>- Query: who was the first female detective employed by the Pinkerton Detective Agency?</search_knowledge>

Env: <information>Doc 1(Title: ”Kate Warne”) … Doc 2(Title: ”Private investigator”) … Doc 3(Title: ”Kate Warne”)…</information>

EvolveR: <think>Based on the retrieved documents, Kate Warne is confirmed as the first female detective employed by the Pinkerton Detective Agency. The information aligns with both historical documentation and the application of principles regarding notable figures in organizational history.
- Principle 1: (Historical Agency, First Female Employee, Female Detective) - This principle confirms Kate Warne as the first female detective within the Pinkerton Detective Agency.
- Principle 2: (Organization, Historical Role, Notable Individual) - This principle links Kate Warne to her role as the first female detective in the agency’s history.</think>

<answer>Kate Warne</answer>

A.5 Limitation and Broader Impact
We acknowledge several limitations and broader implications of our work. The efficacy of our self-distillation mechanism is inherently bounded by the capabilities of the agent’s own model; a less capable model may struggle to distill high-quality principles, thus limiting its evolutionary ceiling. Further research across a broader range of tasks, such as embodied interaction or creative generation, is necessary to fully delineate the boundaries and applicability of the EvolveR paradigm. While our curation mechanisms mitigate experience base growth, ensuring computational efficiency for truly lifelong learning agents also remains an open challenge. Looking forward, the broader impact of this paradigm is significant. On the one hand, EvolveR represents a crucial step towards more autonomous and personalized agents. The explicit nature of its distilled principles also offers a promising avenue for improving interpretability and steerability. On the other hand, this autonomy raises critical safety considerations. An agent that evolves its own principles could develop undesirable strategies if not guided by a robust, value-aligned reward function, necessitating further research into alignment techniques for such self-evolving systems.
---
# Get Experience from Practice: LLM Agents with Record & Replay

Authors:Erhu Feng, Wenbo Zhou, Zibin Liu, Le Chen, Yunpeng Dong, Cheng Zhang, Yisheng Zhao, Dong Du, Zhichao Hua, Yubin Xia, Haibo Chen
View PDF
HTML (experimental)
Abstract:AI agents, empowered by Large Language Models (LLMs) and communication protocols such as MCP and A2A, have rapidly evolved from simple chatbots to autonomous entities capable of executing complex, multi-step tasks, demonstrating great potential. However, the LLMs' inherent uncertainty and heavy computational resource requirements pose four significant challenges to the development of safe and efficient agents: reliability, privacy, cost and performance. Existing approaches, like model alignment, workflow constraints and on-device model deployment, can partially alleviate some issues but often with limitations, failing to fundamentally resolve these challenges.
This paper proposes a new paradigm called AgentRR (Agent Record & Replay), which introduces the classical record-and-replay mechanism into AI agent frameworks. The core idea is to: 1. Record an agent's interaction trace with its environment and internal decision process during task execution, 2. Summarize this trace into a structured "experience" encapsulating the workflow and constraints, and 3. Replay these experiences in subsequent similar tasks to guide the agent's behavior. We detail a multi-level experience abstraction method and a check function mechanism in AgentRR: the former balances experience specificity and generality, while the latter serves as a trust anchor to ensure completeness and safety during replay. In addition, we explore multiple application modes of AgentRR, including user-recorded task demonstration, large-small model collaboration and privacy-aware agent execution, and envision an experience repository for sharing and reusing knowledge to further reduce deployment cost.

Submission history From: Erhu Feng [view email] [v1]
Fri, 23 May 2025 10:33:14 UTC (6,904 KB)
---
# LEGOMem: Modular Procedural Memory for Multi-agent LLM Systems for Workflow Automation

\setcopyrightifaamas \acmConference[AAMAS ’26]Proc. of the 25th International Conference on Autonomous Agents and Multiagent Systems (AAMAS 2026)May 25 – 29, 2026
Paphos, CyprusC. Amato, L. Dennis, V. Mascardi, J. Thangarajah (eds.) \copyrightyear2026 \acmYear2026 \acmDOI \acmPrice \acmISBN \acmSubmissionID14\affiliation\institutionMicrosoft \country

Abstract.
We introduce LEGOMem, a modular procedural memory framework for multi-agent large language model (LLM) systems in workflow automation. LEGOMem decomposes past task trajectories into reusable memory units and flexibly allocates them across orchestrators and task agents to support planning and execution. To explore the design space of memory in multi-agent systems, we use LEGOMem as a lens and conduct a systematic study of procedural memory in multi-agent systems, examining where memory should be placed, how it should be retrieved, and which agents benefit most. Experiments on the OfficeBench benchmark show that orchestrator memory is critical for effective task decomposition and delegation, while fine-grained agent memory improves execution accuracy. We find that even teams composed of smaller language models can benefit substantially from procedural memory, narrowing the performance gap with stronger agents by leveraging prior execution traces for more accurate planning and tool use.
These results position LEGOMem as both a practical framework for memory-augmented agent systems and a research tool for studying memory design in multi-agent workflow automation.

Key words and phrases: Multi-agent systems, Procedural memory, LLM Agents, Workflow

correspond to Dongge Han at donggehan@microsoft.com

{CCSXML}¡ccs2012¿ ¡concept¿ ¡concept_id¿10010147.10010178.10010179¡/concept_id¿ ¡concept_desc¿Computing methodologies Multi-agent systems¡/concept_desc¿ ¡concept_significance¿500¡/concept_significance¿ ¡/concept¿ ¡concept¿ ¡concept_id¿10010147.10010257.10010258.10010259¡/concept_id¿ ¡concept_desc¿Computing methodologies Reasoning about beliefs and knowledge¡/concept_desc¿ ¡concept_significance¿300¡/concept_significance¿ ¡/concept¿ ¡/ccs2012¿

\ccsdesc[500]Computing methodologies Multi-agent systems \ccsdesc[300]Computing methodologies Reasoning about beliefs and knowledge

1. Introduction

(a) Overview of the LEGOMem framework

(b) Example LEGOMem memory structure

Figure 1. LEGOMem framework overview and example memory. The multi-agent system consists of an orchestrator and task agents. The orchestrator performs planning, next agent selection, and subtask allocation, while task agents execute subtasks by interacting with the environment via API tool calls. (Note: For clarity, additional task agents such as Word agent are omitted.)

Large Language Models (LLMs) are increasingly deployed as agents to automate complex multi-step workflows Yao et al. (2023); Song et al. (2023); Wang et al. (2023); Kim et al. (2024); Wang et al. (2024a); Xie et al. (2024); Wang et al. (2025); Zhou et al. (2023); Mialon et al. (2023); Cao et al. (2024); Rana et al. (2023); Ahn et al. (2022); Wang et al. (2024b); Cheng et al. (2024). These agents are especially valuable in productivity environments such as document editing, email handling, and calendar scheduling. To manage the diversity and compositionality of such tasks, recent systems often adopt multi-agent Stone and Veloso (2000); Wooldridge (2009) designs, where multiple LLM-based agents collaborate, specialize, or delegate responsibilities across roles and tools Fourney et al. (2024); Wu et al. (2024a); Dang et al. (2025); Chen et al. (2023); Zhang et al. (2024). This trend reflects a broader shift in AI system design: the real world is inherently multi-agent, involving heterogeneous roles and coordinated decision-making. Multi-agent LLM systems offer a scalable and modular approach to reasoning, tool-use, and workflow execution, positioning them as a natural fit for these increasingly complex productivity environments.
Despite these advances, current multi-agent systems remain largely stateless and transactional: each task is solved from scratch, without reusing prior experience. This lack of memory—particularly procedural memory—limits their ability to learn from past experiences and build up execution skills over time for complex workflows. While recent works have proposed memory modules for single-agent LLMs, such as Synapse Zheng et al. (2023), Agent Workflow Memory (AWM) Wang et al. (2024c), these approaches do not address the unique coordination and specialization challenges of multi-agent systems.
To address this gap, we introduce LEGOMem, a modular procedural memory framework designed for multi-agent LLM systems. In this work, we focus on a common and practical subclass of multi-agent architectures, where a central orchestrator performs planning and delegates subtasks to specialized tool-using task agents, as exemplified by the Magentic-One framework Fourney et al. (2024); Wu et al. (2024a). Our goal is to equip both orchestrators and task agents with memory grounded in prior task trajectories, enabling them to perform better planning, coordination, and task executions.
To this end, we design LEGOMem to distill successful executions into structured memory units: full-task memories (task-level plans and reasoning traces) and subtask memories (agent behavior and tool interactions). These modular memories are stored in a memory bank, indexed by semantic embeddings, and reused at inference time to augment planning and execution.
LEGOMem is instantiated as a retrieval augmentation (RAG) Lewis et al. (2020); Gao et al. (2023); Douze et al. (2024) layer over existing multi-agent systems. During a new task, the orchestrator receives relevant full-task memories to support task decomposition and agent selection, while each task agent is assigned subtask memories aligned with its delegated subtasks. We explore three memory retrieval strategies—vanilla, dynamic retrieval, and query rewriting—to study how retrieval and memory specificity affect multi-agent performance.
This framework allows us to systematically investigate key questions in multi-agent memory design, including where memory should be placed, how it should be retrieved, and which agents benefit most from it.
We evaluate LEGOMem in the context of productivity workflow automation using the OfficeBench Wang et al. (2024a) benchmark, with agent teams composed of LLM-only, hybrid, and small language model configurations. Across these settings, all LEGOMem variants significantly improve task success rates over memory-less and baseline methods. Our ablation studies reveal that orchestrator memory is critical for high-level planning and delegation, while fine-grained subtask retrieval provides meaningful gains for smaller agents that rely more on localized execution support. These findings highlight how memory placement and retrieval strategy shape the effectiveness of multi-agent collaboration in workflow settings.
Overall, LEGOMem provides a practical and extensible framework for memory-augmented multi-agent workflow automation, enabling agents to plan, coordinate, and execute more effectively by reusing structured procedural knowledge. We hope this work facilitates further research on memory design, continual learning, and efficient agent collaboration in complex productivity settings.

2. Related work

Multi-agent LLM systems for workflow automation
The recent advent of LLMs has enabled the development of multi-agent systems able to plan, decompose and solve complex workflows.
Generalist multi-agent frameworks Fourney et al. (2024); Wu et al. (2024a); Dang et al. (2025); Chen et al. (2023); Zhang et al. (2024) such as Magentic-One Fourney et al. (2024) use a common design pattern where a lead orchestrator agent decomposes high-level goals into a step-by-step plan and directs a team of specialized agents to execute specific subtasks. This modular, multi-agent architecture simplifies development and facilitates the reuse of encapsulated skills, a significant advantage over monolithic, single-agent approaches.
However, a key limitation remains that they are often stateless, solving each task from scratch and discarding valuable insights gained during execution. Without memory, agents may repeatedly make the same errors and cannot improve over time.

Memory for LLM agents
Memory offers a natural solution to the limitations of stateless agents. However, a primary challenge is that memory in LLM agents is often designed for single-agent systems and are often episodic/semantic, replaying information from dialogue histories Zhong et al. (2024); Sun and Zeng (2025); Rasmussen et al. (2025); Wu et al. (2024b); Maharana et al. (2024), such as A-MEM Xu et al. (2025) which captures interactions as a network of interconnected notes that form an evolving memory structure, and Mem0 Chhikara et al. (2025), which focus on managing memory from ongoing conversations.
While these systems advance memory capabilities, they are not designed for agentic learning and workflow automation.
Another line of works target memory optimization for agentic workflows including Lee et al. (2025); Kang et al. (2025); Zhou et al. (2025), which focus on short-term context optimization for workflow agents.
Most closely related works on long-term, procedural memory for agents include Synapse Zheng et al. (2023), which uses successful past full trajectories as exemplars, and Agent Workflow Memory (AWM) Wang et al. (2024c) which induces frequently used subtask sequences as reusable skills. However, both works target procedural memory for single-agent scenarios. In contrast, LEGOMem introduces modular, role-aware procedural memory for multi-agent systems. By flexibly allocating memory across orchestrators and task agents, it addresses unique challenges in memory placement and allocation, improving workflow automation through better planning, execution, and coordination.

3. LEGOMem: Modular Procedural Memory for Multi-agent LLM Systems
In this section, we introduce LEGOMem, a modular procedural memory framework designed for multi-agent LLM systems. We begin by formalizing the problem setting of multi-agent workflow execution with procedural memory, then present the detailed LEGOMem framework, its variants and the design choices studied in our experiments.

3.1. Problem formulation

3.1.1. Multi-agent system for workflow automation.
We consider a common multi-agent workflow automation framework (based on Magentic-One system Fourney et al. (2024)) with an orchestrator AorchA_{\text{orch}}, a set of task agents A={A1,…,Ak}A=\{A_{1},\ldots,A_{k}\}, and an external environment ℰ\mathcal{E}. A task TT is specified by a natural language description dd and must be executed within ℰ\mathcal{E}. Specifically, we implemented task agents for Word, Excel, Calendar, Email, System, and OCR-PDF apps. These task agents interact with the simulated apps in a Docker environment via tool APIs, ensuring isolated and reproducible execution.
The orchestrator first generates an initial high-level plan π0={s1,…,sm}\pi_{0}=\{s_{1},\ldots,s_{m}\}, outlining a possible sequence of subtasks.
However, orchestration is not a static plan-following process: after each orchestration step, the orchestrator dynamically generates the next subtask based on the current state σt\sigma_{t} and observations returned from the agents, rather than simply selecting from the initial plan.

Formally, at each orchestration step tt:

(1) the orchestrator proposes the next subtask st=πorch​(σt)s_{t}=\pi_{\text{orch}}(\sigma_{t});

(2) the subtask is assigned to an appropriate task agent AjA_{j};

(3) the task agent executes sts_{t} by issuing tool-use commands to the environment ℰ\mathcal{E}, returning an observation oto_{t} and an execution summary rtr_{t};

(4) the orchestrator updates its state σt+1=f​(σt,rt)\sigma_{t+1}=f(\sigma_{t},r_{t}) and continues orchestration.

If progress stalls (e.g., repeated states or looping behavior), the orchestrator may perform re-planning, generating a revised high-level plan π′\pi^{\prime}, and resuming orchestration from the updated state.
The system is considered successful if the final environment state σfinal∈ℰ\sigma_{\text{final}}\in\mathcal{E} satisfies the task goal.

3.1.2. Multi-agent procedural memory.
While the orchestration loop above defines how agents interact with the environment, it remains stateless: each new task TT is solved from scratch, discarding knowledge from past executions.
To address this limitation and enable agents to improve through experience, we introduce multi-agent procedural memory: modular, role-aware memories distilled from successful trajectories and reused across tasks in a multi-agent system.
In contrast to episodic or semantic memory, which primarily capture events or textual information, multi-agent procedural memory abstracts workflows into reusable subroutines tailored to both orchestrators and task agents.
These memories allow orchestrators to plan more effectively and select agents with greater context, while equipping task agents with execution-level guidance for more accurate and efficient tool use.
Formally, we define a memory store MM as a collection of modular memory units derived from past executions.
These include full memories that capture orchestration plans and summarized execution traces, as well as subtask memories that capture agent-specific subtask executions.
Together, they form a role-aware memory library that can be retrieved and allocated during inference to augment both planning and execution.
In the following section, we present LEGOMem, a concrete framework that implements this formulation through structured memory construction, inference-time allocation, and variant strategies for retrieval and memory reuse for more robust workflow automation.

3.2. The LEGOMem framework
The LEGOMem framework instantiates the problem formulation by equipping multi-agent systems with modular procedural memory.
It operates in two phases: (i) an offline memory construction phase, where successful task trajectories are distilled into reusable memory units; and (ii) an online memory-augmented inference phase, where retrieved memories are allocated to the orchestrator and task agents to guide planning and execution.
As illustrated in 1(a), past task trajectories are curated into a procedural memory bank, which is then queried at inference time to provide high-level orchestration guidance and agent-specific execution traces.
1(b) further shows the structure of these memory units, consisting of a high-level plan, localized agent subtask traces, the final answer, and a brief reflection.
This modular design enables LEGO-like recombination of past experiences to support efficient and reliable task completion across diverse multi-agent environments.

3.2.1. Memory construction.
The first phase of LEGOMem is offline memory construction, where successful task trajectories are distilled into structured and reusable memory units.
From each trajectory, we extract two complementary types of memory:
(i) full-task memories that capture the task description, the high-level plan executed, and (ii) subtask memories, that encapsulate the subtask description, the localized agent behavior and tool-use, and observations.
These modular units are stored in a procedural memory bank ℳ\mathcal{M} for future reuse.
At inference time, the orchestrator receives the full-task memory in its entirety, while task agents are provided with the relevant subtask memories.
Concretely, the construction process operates on execution logs of successfully completed tasks. Each log records the task description, the orchestrator’s planning and orchestration steps, the subtasks delegated to agents, and the corresponding agent executions (tool-use commands, observations, and outcomes).
We use an LLM to transform these logs into structured LEGOMem units, as shown in 1(b).
The resulting memory bank ℳ\mathcal{M} is implemented as a vector database, indexed using dense embeddings.
Let ϕ​(⋅)\phi(\cdot) denote the embedding model used for indexing; for full-task memories, we compute ϕ​(d)\phi(d) based on the task description dd to enable semantic similarity retrieval.
We implement and compare three retrieval and allocation strategies: vanilla LEGOMem, LEGOMem-Dynamic, and LEGOMem-QueryRewrite.
For vanilla LEGOMem, the entire memory bank ℳ\mathcal{M} is indexed using ϕ​(d)\phi(d), enabling direct retrieval of relevant full-task memories at inference time and subtask allocation to task agents. For the other two variants, we separate the global memory bank ℳ\mathcal{M} of full task memories, and subtask memory banks {ℳAj|Aj∈A}\{\mathcal{M}_{A_{j}}|A_{j}\in A\} per task agent, which contain the subtask memories that are easily extracted from the global memories, and subtask memory banks are indexed by the embeddings of the subtask descriptions ϕ​(dsubtask)\phi(d_{\textnormal{subtask}}). More details regarding the LEGOMem variants will be discussed in Section 3.3.

Algorithm 1 Multi-agent Execution with Vanilla LEGOMem

1:Input: task description dnewd_{\text{new}}, memory bank ℳ\mathcal{M}, orchestrator AorchA_{\textnormal{orch}}, task agents A={A1,…​Ak}A=\{A_{1},...A_{k}\}

2:Compute embedding of ϕ​(dnew)\phi({d_{\text{new}}}) and retrieve top-KK semantically similar full-task memories m={m1,…,mK}m=\{m_{1},...,m_{K}\} from ℳ\mathcal{M}.

3:Extract subtask memories {m11,…,mnK}\{m^{\textnormal{1}}_{1},...,m^{\textnormal{K}}_{n}\} from the full-task memories and assign subtask memories corresponding to each agent.

4:Initialize environment ℰ\mathcal{E} and start task dnewd_{\text{new}}.

5:Augment retrieved full-task memories mm to the orchestrator, which then generates initial plan πo\pi_{o}.

6:while task not completed do

7:  Orchestrator AorchA_{\textnormal{orch}} selects next agent At∈AA_{t}\in A, generates the next subtask sts_{t} and assign to AtA_{t},

8:  Augment subtask memories to the task agent AtA_{t}

9:  Task agent AtA_{t} generates a list of tool-use actions, which are executed in the environment.

10:  Agent receives observation oto_{t}, summarize subtask execution and sends summary message rtr_{t} to orchestrator AorchA_{\textnormal{orch}}

11:  if progress stalls then

12:   Orchestrator performs re-planning and update plan π′\pi^{\prime}

13:  end if

14:end while

15:return orchestrator final response.

(a) LEGOMem-Dynamic variant

(b) LEGOMem-QueryRewrite variant

Figure 2. Comparison of LEGOMem variants: (a) LEGOMem-Dynamic dynamically retrieves subtask memories during execution, and (b) LEGOMem-QueryRewrite employs query rewriting to retrieve multiple candidate memories for each subtask.

3.2.2. Memory-augmented inference.
In the second phase, LEGOMem augments the task execution loop by supplying the orchestrator with full-task memories (end-to-end for planning and detailed orchestration) and augment task agents with subtask memories (localized execution guidance). Given a new task dnewd_{\text{new}}, the system retrieves relevant memories from the memory banks and allocates them accordingly. We designed and tested three different LEGOMem variants which exhibit different memory retrieval strategies, which will be detailed in Section 3.3. Here we will describe the vanilla LEGOMem inference approach, as shown in Algorithm 1.
Given a new task with description dnewd_{\text{new}}, we obtain the embedding ϕ​(dnew)\phi(d_{\text{new}}) and the system first retrieves top-K relevant memories from the global memory bank ℳ\mathcal{M} using semantic similarity. Then we allocate the full-task memory to the orchestrator, and extract the subtask memories from the retrieved full-task memories and allocate the subtask memories to the corresponding task agents.
As shown in 1(a), for the vanilla LEGOMem variant, the orchestrator receives full-task memories that provide end-to-end workflows, while task agents are supplied with subtask memories that offer localized execution guidance.
This design enables orchestrators to leverage prior trajectories for informed planning, agent capability grounding, and error recovery, while task agents improve their accuracy and efficiency in tool-use.
As the task starts, the orchestrator receives the full memories and perform initial planning. Then, at each orchestration step, the orchestrator dynamically generates the next subtask using both the current state and retrieved full-task memories.
The selected agent then executes the subtask with its allocated subtask memories, returning observations and summaries to update the orchestrator state.
If progress stalls, the orchestrator can re-plan using memory as additional guidance.
Through this loop, LEGOMem integrates past experiences to make more informed decision during planning and coordination, improving both reliability and efficiency of the multi-agent workflows.

3.3. LEGOMem variants
To explore the impact of subtask retrieval granularity in multi-agent systems, we compare three variants of LEGOMem: (vanilla) LEGOMem, LEGOMem-Dynamic, and LEGOMem-QueryRewrite.
These variants differ in how they store and retrieve subtask memories and allocate them to the task agents.
As discussed in 3.2, vanilla LEGOMem keeps a global procedural memory bank ℳ\mathcal{M}, and during inference, retrieves full-task memories using the task description and augment them to the orchestrator. Subtask memories are then extracted from these retrieved memories straightforwardly and are statically assigned to the relevant task agents. This approach is simple and efficient, and provides strong performance across teams. However, it may occasionally fail to surface relevant subtask memories for certain agents if the retrieved full-task memories differ in subtask structures from the current task. In such cases, even if the overall task appears similar, the subtask components may diverge. To address this, we implement two variants that enable finer-grained subtask-level retrieval, improving task agent-level memory relevance:

LEGOMem-Dynamic:
As illustrated in 2(a), LEGOMem-Dynamic performs subtask-level retrieval during execution. The orchestrator memory storage and retrieval remain the same as the vanilla version, while the system maintains per-agent subtask memory banks segmented from the global memory bank. When the orchestrator generates a subtask sts_{t} for an agent AtA_{t}, we compute its embedding ϕ​(st)\phi(s_{t}) and query the agent’s memory bank ℳAt\mathcal{M}_{A_{t}} to retrieve only the most relevant past subtask traces. This just-in-time retrieval provides more precise execution guidance for task agents and reduces noise from irrelevant memories.

LEGOMem-QueryRewrite:
While LEGOMem-Dynamic performs just-in-time retrieval at each orchestration step, it incurs repeated subtask embedding and retrieval during execution. LEGOMem-QueryRewrite shifts this to the planning stage using query rewriting Ma et al. (2023); Li et al. ([n.d.]). As shown in Figure 2(b), after retrieving full-task memories, a query rewriter LLM ψ\psi uses the memories to generate a draft plan for the new task πdraft′={s1′,s2′,…,sn′}\pi^{\prime}_{\text{draft}}=\{s^{\prime}_{1},s^{\prime}_{2},\dots,s^{\prime}_{n}\} consisting of rewritten subtasks. Each sj′s^{\prime}_{j} is then embedded via ϕ​(sj′)\phi(s^{\prime}_{j}) and used to retrieve relevant subtask memories from the corresponding agent’s memory bank ℳAj\mathcal{M}_{A_{j}} before task execution starts. This approach preserves the fine-grained retrieval benefits of LEGOMem-Dynamic while avoiding repeated queries at runtime, enabling more efficient execution and smoother orchestration.
Interestingly, our experiments show that all three variants achieve similar overall performance in full memory settings, demonstrating the robustness across variants. Furthermore, our ablation study shows that LEGOMem-Dynamic and LEGOMem-QueryRewrite outperform vanilla LEGOMem when only task agent-level memory is used and with small language model task agents. This indicates that fine-grained subtask retrieval may offer more relevant guidance to task agents and may be particularly beneficial in settings with weaker orchestrator support.
Together, the LEGOMem framework and its variants provide a general and modular approach to procedural memory for multi-agent LLM systems, enabling both orchestrators and task agents to learn from and reuse prior task executions. In the following section, we empirically evaluate these variants across different agent team configurations and memory settings.

Table 1. Performance comparison across memory variants, task levels, and multi-agent teams. Results show mean success rates across different LEGOMem variants compared with baseline methods, each data-point is averaged over three random seeds.

Figure 3. Qualitative example of agent execution with and without memory. The memory-less team fails to identify the earliest email due to incomplete planning, stopping after reading the first email, while the team with LEGOMem systematically reads to obtain and compare all email timestamps, producing the correct answer.

4. Experiments
We evaluate LEGOMem on the OfficeBench benchmark, comparing its variants with strong baselines across LLM-only, hybrid, and SLM-only multi-agent teams. Beyond overall performance, we conduct ablations on memory placement, retrieval strategies, and representation formats to analyze the contributions of different design choices. Our results show that LEGOMem consistently improves task success rates across team configurations, and that memory design, particularly the placement of orchestrator memory, plays a central role in enabling effective multi-agent coordination.

4.1. Experimental setup

4.1.1. Dataset and metrics
We evaluate the agents on the OfficeBench, which consists of multi-step office automation tasks with varying levels of complexity. We split the 300 tasks into training (148 instances, for memory curation) and test (152 instances, for evaluation) sets. Tasks span three difficulty levels: Level 1 (single application), Level 2 (two-application), and Level 3 (multi-application workflows).
The evaluation metric is the success rate, i.e. the percentage of tasks solved correctly. The success of a task is evaluated programmatically according to the final state of the environment, including exact match or fuzzy keyword match of the final outputs and expected outputs (e.g., correctly updated spreadsheet entries, calendar events, emails sent and received, and question answering).

4.1.2. Implementation details

We experiment with three team configurations with agents of different sizes and capabilities:

•
LLM team: for the full LLM team, we use GPT-4o Hurst et al. (2024) for both the orchestrator and task agents

•
Hybrid (LLM + SLM) team: GPT-4o for the orchestrator, and GPT-4o-mini for the task agents

•
SLM team: GPT-4o-mini Hurst et al. (2024) for all components

Additionally, for memory storage and retrieval, we use the OpenAI text-embedding-3-large model for embedding the task descriptions, and the FAISS library Douze et al. (2024) for the vector database. For the OCR app, we use the Phi-3.5-mini model Abdin et al. (2024) as the vision language model for image parsing.

We compare the LEGOMem variants with three baselines: (i) No memory, and two state-of-the-art methods on procedural memory for workflow automation (ii) Synapse, which augments agents with semantically similar memories using raw action sequences and full trajectories, and (iii) AWM, which augments agents with summarized subtask memories extracted from full trajectories.

4.1.3. Memory curation and agent inference details
Memory construction uses the 148 training tasks, where we first run the full LLM agent team without memory, and filter for successful trajectories and extracted 93 full task memories from the successful trajectories. For the LEGOMem variants, we further extracted 250 subtask memories for the task agents from the 93 full task memories.
Both Synapse and AWM focus on single-agent systems; for a fair comparison we use the same 93 successful trajectories, and adapt both baselines to the multi-agent team, augmenting the memories to both orchestrators and task agents. For Synapse, we augment both orchestrators and task agents with the full trajectories. For AWM, we cluster the successful trajectories, to extract and consolidate subtask memories from each cluster, and during inference, we augment the task agents with their corresponding extracted subtask memories and augment the orchestrator with a list of extracted subtask memories.
For all variants, we use 5 memories for orchestrator and 3 memories for each task agent from the successful trajectories.

Table 2. Comparing performance with various memory placement mechanism across LEGOMem variants.

Table 3. Comparing memory with and without reasoning across different LEGOMem variants.

4.2. Main results
Table 1 presents the main experiment results, comparing the performance of LEGOMem with baseline methods across different task levels and agentic team configurations.
Across all scenarios and agent team configurations, LEGOMem variants consistently outperform baseline methods in terms of overall success rate. All three LEGOMem variants show similar, consistent performance, with the vanilla LEGOMem variant being lightweight while achieving the best overall performance. The performance improvement shows the effectiveness of modularized memory representations and allocation for multi-agent systems.
Compared with memory-less teams, LEGOMem improves overall task success rate by +12.61%+12.61\%, +12.72%+12.72\%, and +13.38%+13.38\% absolute points on LLM, Hybrid and SLM teams, respectively.
Importantly, LEGOMem enables smaller models to close the gap with, and sometimes outperform, larger ones. For example, the Hybrid team with LEGOMem-QueryRewrite achieves 50.22%50.22\%, surpassing the memory-less LLM team (45.83%45.83\%). Likewise, a full SLM team with vanilla LEGOMem (38.16%38.16\%) outperforms the Hybrid team without memory (35.31%35.31\%). While Synapse remains competitive in LLM teams, reflecting the ability of LLMs to interpret raw procedural traces, its effectiveness is less consistent for Hybrid and SLM teams. In contrast, LEGOMem maintains strong performance across all team settings, highlighting the importance of modularized procedural memory for enabling efficient, smaller-model teams.
To better illustrate the effect of memory on agent behavior, Figure 3 presents a qualitative case study. Without memory, the agent fails to identify the earliest email due to incomplete planning, stopping after reading only the first entry. With LEGOMem, the agent systematically reads and compares all emails, correctly identifying the earliest one. This example highlights how LEGOMem improves reasoning consistency and task completeness beyond what is reflected in aggregate success rates.

(a) Average execution steps across task levels

(b) Average failed steps rate across task levels

Figure 4. Ablations study: execution steps comparison for different LEGOMem memory placement for LLM teams. (a) shows that LEGOMem variants reduce the number of execution steps required, with up to 16.2% reduction for Level 3 tasks. (b) shows lower failure rates of steps, indicating more reliable task execution with procedural memory.

4.3. Ablations experiments
This section investigates how different memory retrieval, allocation, and placement strategies affect the performance of LEGOMem.

4.3.1. Memory retrieval, allocation, and placement
Table 2 summarizes our ablation results across different memory retrieval variants, memory allocation strategies, and memory placement settings.

Memory retrieval
The three subtask memory retrieval strategies—vanilla LEGOMem, LEGOMem-Dynamic, and LEGOMem-QueryRewrite—all perform robustly and achieve similar overall success rates. While dynamic retrieval enables more targeted allocation and query rewriting improves robustness to subtask phrasing variations, these differences are modest compared to the impact of memory placement and allocation strategy.
In the task-agent-only memory setting, both LEGOMem-Dynamic and LEGOMem-QueryRewrite outperform vanilla LEGOMem by 4–5% on average in the Hybrid team where task agents are smaller models and agent-level memory plays a more critical role. These results highlight the advantage of fine-grained subtask retrieval in providing more relevant and contextual guidance to task agents, especially when global planning signals are weaker. We hypothesize that the similar overall performance of all three variants in full-memory settings may be due to the strength of the orchestrator memory, where the orchestrator receives the complete trajectory of prior solutions, compensating for weaker task agent execution by enabling better task decomposition and delegation.
Overall, these findings demonstrate the flexibility of the LEGOMem framework: even the lightweight vanilla variant performs competitively, while more advanced variants offer additional benefits in settings that demand finer-grained memory retrieval.

Memory allocation
Regarding memory allocation, we find that joint allocation of orchestrator and task agent memory (Orchestrator + Agent memory variant) yields the strongest overall results, with orchestrator memory supporting effective planning, task decomposition and subtask orchestration, and task agent memory enabling execution-level precision. Orchestrator memory emerges as essential: when memory is removed from the orchestrator and provided only to task agents (Task Agent memory variant), performance drops noticeably.

Memory placement
Looking at memory placement, even when restricted to the planning and replanning stages, orchestrator memory still improves over task-agent-only variants, confirming its central role in guiding high-level planning and task decomposition. Finally, Task-Agent-only memory while facilitating more accurate tool use and outperforming the no-memory baseline, remains less effective than orchestrator-level memory – indicating that local memory without global coordination is insufficient.

4.3.2. Effectiveness of adding reasoning in memory
We also examine whether augmenting procedural memories with lightweight reasoning improves performance. As shown in Table 3, the differences are minor: overall scores change by less than two points across variants and team types. For example, vanilla LEGOMem improves slightly on LLM teams (56.36%→58.44%56.36\%\rightarrow 58.44\%) but decreases on Hybrid teams (49.78%→48.03%49.78\%\rightarrow 48.03\%). These results suggest that LEGOMem is robust, with its modularized structure already providing sufficient procedural guidance without additional reasoning steps.

4.3.3. Effectiveness of memory on execution steps and failure rates
As an additional ablations study, Figure 4 compares the average number of execution steps taken by the agent with different memory placement variants and the step failure rate (due to wrong tool-use actions issued) per task for the LLM team. As shown in 4(a) Compared to the no memory variant, the agents equipped with LEGOMem can reduce the number of execution steps required to complete the tasks, for example, a -16.2%\% drop from an average of 26.5 to 22.2 steps for Level 3 tasks. The task memory only variant where we remove the orchestrator memory required more steps to complete a task compared with the variant with orchestrator memory, due to the effectiveness of the orchestrator memory for improved planning.
Similarly, 4(b) shows that LEGOMem reduces the average failure rate of agent steps. At Level 3, the failure rate decreases from 0.275 in the no-memory setting to 0.225 with LEGOMem. These results indicate that LEGOMem not only improves task success rates but also enables more efficient and reliable task execution.
In summary, our experiments show that LEGOMem consistently outperforms baselines methods, improving task success by over 12 absolute percentage points compared with memory-less teams. LEGOMem can enable smaller and hybrid teams to match or even surpass LLM-only teams, highlighting its value for efficient multi-agent configurations. Ablations reveal that, as one may expect, the memory placement strategy is critical: orchestrator memory is essential for effective planning, while subtask memory complements execution. Additional analysis also show reductions in execution steps required and per-step failure rates with LEGOMem.

5. Conclusion
We introduced LEGOMem, a modular procedural memory framework for multi-agent systems that enables orchestrators and task agents to learn from prior task executions. By representing workflows as reusable memory units—split into full-task and subtask components—LEGOMem supports efficient task planning and execution through memory retrieval and allocation.
We implemented and evaluated three LEGOMem variants to explore the design space of memory retrieval and placement strategies. Across extensive experiments on workflow automation tasks, we show that LEGOMem significantly improves task success rates over memory-less and baseline methods, with orchestrator memory playing a critical role in planning and coordination, and memory can also benefit smaller agents, highlighting the flexibility and effectiveness of the framework.
Our work shows that integrating procedural memory into multi-agent systems enables more reliable and reusable solutions. Future work may explore continual learning also from failed past trajectories, and scaling LEGOMem to open-ended environments and tool ecosystems.

References

(1)

Abdin et al. (2024)

Marah Abdin, Jyoti Aneja, Harkirat Behl, Sébastien Bubeck, Ronen Eldan, Suriya Gunasekar, Michael Harrison, Russell J Hewett, Mojan Javaheripi, Piero Kauffmann, et al. 2024.

Phi-4 technical report.

arXiv preprint arXiv:2412.08905 (2024).

Ahn et al. (2022)

Michael Ahn, Anthony Brohan, Noah Brown, Yevgen Chebotar, Omar Cortes, Byron David, Chelsea Finn, Chuyuan Fu, Keerthana Gopalakrishnan, Karol Hausman, et al. 2022.

Do as i can, not as i say: Grounding language in robotic affordances.

arXiv preprint arXiv:2204.01691 (2022).

Cao et al. (2024)

Ruisheng Cao, Fangyu Lei, Haoyuan Wu, Jixuan Chen, Yeqiao Fu, Hongcheng Gao, Xinzhuang Xiong, Hanchong Zhang, Wenjing Hu, Yuchen Mao, et al. 2024.

Spider2-v: How far are multimodal agents from automating data science and engineering workflows?

Advances in Neural Information Processing Systems 37 (2024), 107703–107744.

Chen et al. (2023)

Weize Chen, Yusheng Su, Jingwei Zuo, Cheng Yang, Chenfei Yuan, Chen Qian, Chi-Min Chan, Yujia Qin, Yaxi Lu, Ruobing Xie, et al. 2023.

Agentverse: Facilitating multi-agent collaboration and exploring emergent behaviors in agents.

arXiv preprint arXiv:2308.10848 2, 4 (2023), 6.

Cheng et al. (2024)

Yuheng Cheng, Ceyao Zhang, Zhengwen Zhang, Xiangrui Meng, Sirui Hong, Wenhao Li, Zihao Wang, Zekai Wang, Feng Yin, Junhua Zhao, et al. 2024.

Exploring large language model based intelligent agents: Definitions, methods, and prospects.

arXiv preprint arXiv:2401.03428 (2024).

Chhikara et al. (2025)

Prateek Chhikara, Dev Khant, Saket Aryan, Taranjeet Singh, and Deshraj Yadav. 2025.

Mem0: Building Production-Ready AI Agents with Scalable Long-Term Memory.

https://doi.org/10.48550/arXiv.2504.19413

arXiv:2504.19413 [cs].

Dang et al. (2025)

Yufan Dang, Chen Qian, Xueheng Luo, Jingru Fan, Zihao Xie, Ruijie Shi, Weize Chen, Cheng Yang, Xiaoyin Che, Ye Tian, et al. 2025.

Multi-Agent Collaboration via Evolving Orchestration.

arXiv preprint arXiv:2505.19591 (2025).

Douze et al. (2024)

Matthijs Douze, Alexandr Guzhva, Chengqi Deng, Jeff Johnson, Gergely Szilvasy, Pierre-Emmanuel Mazaré, Maria Lomeli, Lucas Hosseini, and Hervé Jégou. 2024.

The faiss library.

arXiv preprint arXiv:2401.08281 (2024).

Fourney et al. (2024)

Adam Fourney, Gagan Bansal, Hussein Mozannar, Cheng Tan, Eduardo Salinas, Friederike Niedtner, Grace Proebsting, Griffin Bassman, Jack Gerrits, Jacob Alber, et al. 2024.

Magentic-one: A generalist multi-agent system for solving complex tasks.

arXiv preprint arXiv:2411.04468 (2024).

Gao et al. (2023)

Yunfan Gao, Yun Xiong, Xinyu Gao, Kangxiang Jia, Jinliu Pan, Yuxi Bi, Yixin Dai, Jiawei Sun, Haofen Wang, and Haofen Wang. 2023.

Retrieval-augmented generation for large language models: A survey.

arXiv preprint arXiv:2312.10997 2, 1 (2023).

Hurst et al. (2024)

Aaron Hurst, Adam Lerer, Adam P Goucher, Adam Perelman, Aditya Ramesh, Aidan Clark, AJ Ostrow, Akila Welihinda, Alan Hayes, Alec Radford, et al. 2024.

Gpt-4o system card.

arXiv preprint arXiv:2410.21276 (2024).

Kang et al. (2025)

Minki Kang, Wei-Ning Chen, Dongge Han, Huseyin A Inan, Lukas Wutschitz, Yanzhi Chen, Robert Sim, and Saravan Rajmohan. 2025.

ACON: Optimizing Context Compression for Long-horizon LLM Agents.

arXiv preprint arXiv:2510.00615 (2025).

Kim et al. (2024)

Sehoon Kim, Suhong Moon, Ryan Tabrizi, Nicholas Lee, Michael W Mahoney, Kurt Keutzer, and Amir Gholami. 2024.

An llm compiler for parallel function calling. In Forty-first International Conference on Machine Learning.

Lee et al. (2025)

Dongjun Lee, Juyong Lee, Kyuyoung Kim, Jihoon Tack, Jinwoo Shin, Yee Whye Teh, and Kimin Lee. 2025.

Learning to contextualize web pages for enhanced decision making by LLM agents.

arXiv preprint arXiv:2503.10689 (2025).

Lewis et al. (2020)

Patrick Lewis, Ethan Perez, Aleksandra Piktus, Fabio Petroni, Vladimir Karpukhin, Naman Goyal, Heinrich Küttler, Mike Lewis, Wen-tau Yih, Tim Rocktäschel, et al. 2020.

Retrieval-augmented generation for knowledge-intensive nlp tasks.

Advances in neural information processing systems 33 (2020), 9459–9474.

Li et al. ([n.d.])

Zhicong Li, Jiahao Wang, Hangyu Mao, ZhiShu Jiang, Zhongxia Chen, Du Jiazhen, Fuzheng Zhang, Di ZHANG, and Yong Liu. [n.d.].

DMQR-RAG: Diverse Multi-Query Rewriting in Retrieval-Augmented Generation.

([n. d.]).

Ma et al. (2023)

Xinbei Ma, Yeyun Gong, Pengcheng He, Hai Zhao, and Nan Duan. 2023.

Query rewriting in retrieval-augmented large language models. In Proceedings of the 2023 Conference on Empirical Methods in Natural Language Processing. 5303–5315.

Maharana et al. (2024)

Adyasha Maharana, Dong-Ho Lee, Sergey Tulyakov, Mohit Bansal, Francesco Barbieri, and Yuwei Fang. 2024.

Evaluating very long-term conversational memory of llm agents.

arXiv preprint arXiv:2402.17753 (2024).

Mialon et al. (2023)

Grégoire Mialon, Clémentine Fourrier, Thomas Wolf, Yann LeCun, and Thomas Scialom. 2023.

Gaia: a benchmark for general ai assistants. In The Twelfth International Conference on Learning Representations.

Rana et al. (2023)

Krishan Rana, Jesse Haviland, Sourav Garg, Jad Abou-Chakra, Ian Reid, and Niko Suenderhauf. 2023.

Sayplan: Grounding large language models using 3d scene graphs for scalable robot task planning.

arXiv preprint arXiv:2307.06135 (2023).

Rasmussen et al. (2025)

Preston Rasmussen, Pavlo Paliychuk, Travis Beauvais, Jack Ryan, and Daniel Chalef. 2025.

Zep: a temporal knowledge graph architecture for agent memory.

arXiv preprint arXiv:2501.13956 (2025).

Song et al. (2023)

Chan Hee Song, Jiaman Wu, Clayton Washington, Brian M Sadler, Wei-Lun Chao, and Yu Su. 2023.

Llm-planner: Few-shot grounded planning for embodied agents with large language models. In Proceedings of the IEEE/CVF international conference on computer vision. 2998–3009.

Stone and Veloso (2000)

Peter Stone and Manuela Veloso. 2000.

Multiagent systems: A survey from a machine learning perspective.

Autonomous Robots 8, 3 (2000), 345–383.

Sun and Zeng (2025)

Haoran Sun and Shaoning Zeng. 2025.

Hierarchical Memory for High-Efficiency Long-Term Reasoning in LLM Agents.

arXiv preprint arXiv:2507.22925 (2025).

Wang et al. (2024b)

Lei Wang, Chen Ma, Xueyang Feng, Zeyu Zhang, Hao Yang, Jingsen Zhang, Zhiyuan Chen, Jiakai Tang, Xu Chen, Yankai Lin, et al. 2024b.

A survey on large language model based autonomous agents.

Frontiers of Computer Science 18, 6 (2024), 186345.

Wang et al. (2023)

Lei Wang, Wanyu Xu, Yihuai Lan, Zhiqiang Hu, Yunshi Lan, Roy Ka-Wei Lee, and Ee-Peng Lim. 2023.

Plan-and-solve prompting: Improving zero-shot chain-of-thought reasoning by large language models.

arXiv preprint arXiv:2305.04091 (2023).

Wang et al. (2025)

Weixuan Wang, Dongge Han, Daniel Madrigal Diaz, Jin Xu, Victor Rühle, and Saravan Rajmohan. 2025.

OdysseyBench: Evaluating LLM Agents on Long-Horizon Complex Office Application Workflows.

arXiv preprint arXiv:2508.09124 (2025).

Wang et al. (2024a)

Zilong Wang, Yuedong Cui, Li Zhong, Zimin Zhang, Da Yin, Bill Yuchen Lin, and Jingbo Shang. 2024a.

Officebench: Benchmarking language agents across multiple applications for office automation.

arXiv preprint arXiv:2407.19056 (2024).

Wang et al. (2024c)

Zora Zhiruo Wang, Jiayuan Mao, Daniel Fried, and Graham Neubig. 2024c.

Agent Workflow Memory.

https://doi.org/10.48550/arXiv.2409.07429

arXiv:2409.07429 [cs].

Wooldridge (2009)

Michael Wooldridge. 2009.

An Introduction to MultiAgent Systems (2nd ed.).

Wiley Publishing.

Wu et al. (2024b)

Di Wu, Hongwei Wang, Wenhao Yu, Yuwei Zhang, Kai-Wei Chang, and Dong Yu. 2024b.

Longmemeval: Benchmarking chat assistants on long-term interactive memory.

arXiv preprint arXiv:2410.10813 (2024).

Wu et al. (2024a)

Qingyun Wu, Gagan Bansal, Jieyu Zhang, Yiran Wu, Beibin Li, Erkang Zhu, Li Jiang, Xiaoyun Zhang, Shaokun Zhang, Jiale Liu, et al. 2024a.

Autogen: Enabling next-gen LLM applications via multi-agent conversations. In First Conference on Language Modeling.

Xie et al. (2024)

Tianbao Xie, Danyang Zhang, Jixuan Chen, Xiaochuan Li, Siheng Zhao, Ruisheng Cao, Toh J Hua, Zhoujun Cheng, Dongchan Shin, Fangyu Lei, et al. 2024.

Osworld: Benchmarking multimodal agents for open-ended tasks in real computer environments.

Advances in Neural Information Processing Systems 37 (2024), 52040–52094.

Xu et al. (2025)

Wujiang Xu, Kai Mei, Hang Gao, Juntao Tan, Zujie Liang, and Yongfeng Zhang. 2025.

A-MEM: Agentic Memory for LLM Agents.

https://doi.org/10.48550/arXiv.2502.12110

arXiv:2502.12110 [cs].

Yao et al. (2023)

Shunyu Yao, Jeffrey Zhao, Dian Yu, Nan Du, Izhak Shafran, Karthik Narasimhan, and Yuan Cao. 2023.

React: Synergizing reasoning and acting in language models. In International Conference on Learning Representations (ICLR).

Zhang et al. (2024)

Chaoyun Zhang, Liqun Li, Shilin He, Xu Zhang, Bo Qiao, Si Qin, Minghua Ma, Yu Kang, Qingwei Lin, Saravan Rajmohan, et al. 2024.

Ufo: A ui-focused agent for windows os interaction.

arXiv preprint arXiv:2402.07939 (2024).

Zheng et al. (2023)

Longtao Zheng, Rundong Wang, Xinrun Wang, and Bo An. 2023.

Synapse: Trajectory-as-exemplar prompting with memory for computer control.

arXiv preprint arXiv:2306.07863 (2023).

Zhong et al. (2024)

Wanjun Zhong, Lianghong Guo, Qiqi Gao, He Ye, and Yanlin Wang. 2024.

Memorybank: Enhancing large language models with long-term memory. In Proceedings of the AAAI Conference on Artificial Intelligence, Vol. 38. 19724–19731.

Zhou et al. (2023)

Shuyan Zhou, Frank F Xu, Hao Zhu, Xuhui Zhou, Robert Lo, Abishek Sridhar, Xianyi Cheng, Tianyue Ou, Yonatan Bisk, Daniel Fried, et al. 2023.

Webarena: A realistic web environment for building autonomous agents.

arXiv preprint arXiv:2307.13854 (2023).

Zhou et al. (2025)

Zijian Zhou, Ao Qu, Zhaoxuan Wu, Sunghwan Kim, Alok Prakash, Daniela Rus, Jinhua Zhao, Bryan Kian Hsiang Low, and Paul Pu Liang. 2025.

MEM1: Learning to Synergize Memory and Reasoning for Efficient Long-Horizon Agents.

arXiv preprint arXiv:2506.15841 (2025).

Appendix A Prompts for Memory Curation
In this section, we provide the detailed prompts for memory curation and the prompt for the query rewriting LLM.

Prompt 1: Memory Curation Prompt

From the following agent trajectory, generate memory that can be useful for future LLM agents’ reference.
# Trajectory:
{\{full_trajectory}\} # Example:
{\{start_tag}\} {{\{\{ ”high_level_plan”: ”1. Check Bob’s calendar availability for the specified time slot. 2. Add the meeting to Bob’s calendar for 5172024 from 10:30 a.m. to 11:00 a.m.”, ”subtasks”: [ {{\{\{ ”agent”: ”calendar_agent”, ”description”: ”Check Bob’s schedule on 5/17/2024 from 10:30 a.m. to 11:00 a.m to ensure there are no conflicts”, ”steps”: ”¡think¿I need to check Bob’s existing calendar events to ensure no scheduling conflicts¡/think¿¡action¿{{”app”:”calendar”,”action”:”list_events”,”username”:”Bob”}\{\{"app":"calendar","action":"list\_events","username":"Bob"\}¡/action¿”, ”observations”: ”No events found for Bob - calendar is available for the requested time slot” }}, {{ ”agent”: ”calendar_agent”, ”description”: ”Add a meeting to Bob’s calendar on 5/17/2024 from 10:30 a.m. to 11:00 a.m”, ”steps”: ”¡think¿Since no conflicts were found, I can now create the new calendar event for Bob¡/think¿¡action¿{{”​a​p​p​”:”​c​a​l​e​n​d​a​r​”,”​a​c​t​i​o​n​”:”​c​r​e​a​t​e​_​e​v​e​n​t​”,”​u​s​e​r​”:”​B​o​b​”,”​s​u​m​m​a​r​y​”:”​M​e​e​t​i​n​g​”,”​t​i​m​e​_​s​t​a​r​t​”:”​2024−05−1710:30:00​”,”​t​i​m​e​_​e​n​d​”:”​2024−05−1711:00:00​”}}\{\{"app":"calendar","action":"create\_event","user":"Bob","summary":"Meeting","time\_start":"2024-05-1710:30:00","time\_end":"2024-05-1711:00:00"\}\}¡/action¿”, ”observations”: ”Successfully created a new event in Bob’s calendar for the specified date and time” }}\}\} ], ”final_answer”: ”The meeting has been successfully added to Bob’s calendar on 5172024 from 10:30 a.m. to 11:00 a.m.”, ”reflections”: ”Task completed successfully without any conflicts or errors. The calendar check confirmed availability, and the meeting was created with proper date/time formatting.” }}\}\} {\{end_tag}\} # Instructions:
Please analyze the trajectory and extract structured memory with clear thinking and well-formed actions. Use the following format for each subtask step:
¡think¿reasoning about what needs to be done and why this action is appropriate¡/think¿ ¡action¿precise tool call command in structured format¡/action¿
The memory object should be formatted as follows:
{{ ”high_level_plan”: ”¡a string that lists the high-level steps taken and which agent performs each subtask¿”, ”subtasks”: [ {{ ”agent”: ”¡copy the exact name of agent that performed the subtask¿”, ”description”: ”¡description of the subtask given by the orchestrator¿”, ”steps”: ”¡Copy the precise actions taken with think-action structure: ¡think¿reasoning¡/think¿¡action¿t​o​o​l​_​c​a​l​l{{tool\_call}}¡/action¿, repeat for each action. Omit some actions if there are too many similar commands (¿10). Remove actions that yielded errors or were malformed.¿”, ”observations”: ”¡a very brief summary of the key observations from the function execution results¿”, }}, … ], ”final_answer”: ”¡The final answer given by the orchestrator or answer agent¿”, ”reflections”: ”¡a concise summary that lists what was successful, what were specific failures, root cause of which action and how to avoid, if any¿”, }} # Rules to follow:
1. Group together actions into subtasks if they are related and can be done together.
2. For each action in the steps field, use the think-action format with clear reasoning followed by structured tool calls.
3. When copying actions, remove function call IDs but keep the essential tool call structure.
4. Only include successful actions; omit actions that resulted in errors. If there are too many repeated similar actions, truncate and omit some, and if the action parameters (such as contents to write to a word document) are too long, you can summarize it.
5. Keep observations very concise but informative.
6. Do not include orchestrator coordination steps in the subtasks.
7. For the subtask steps field, use a string format with think-action pairs, not a list.
Follow the JSON format exactly to ensure it can be parsed automatically, and put the json object between the tags {\{start_tag}\} # your json here {\{end_tag}\} and do not use markdown.

Prompt 2: Query Rewriting Prompt

Based on the following similar task examples, break down the new task into a step-by-step plan.
## Similar Task Examples:
{\{memory_context}\} ## New Task:
{\{task_description}\}
Please provide a numbered list of 3-5 high-level steps that would be needed to complete this task.
Focus on the main phases/subtasks, not detailed actions.
Format your response as a simple numbered list enclosed within ¡start¿ and ¡end¿ tags:
¡start¿
1. [First step]
2. [Second step]
3. [Third step] … ¡end¿
Steps: