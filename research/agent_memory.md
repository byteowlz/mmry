# Memory for AI Agents: A New Paradigm of Context Engineering

For today’s AI agents, memory is a moat. Every conversation counts, but traditional large language models (LLMs) are stateless — they start each interaction without any context or memory, leaving power untapped and insight lost.
Imagining new paradigms of agent memory has become one of the most urgent frontiers in AI development, allowing for active formation and updating of memories, so agents can use all past interactions in a meaningful way. There are many parallels to human memory, whereby remembering previous exchanges and experiences make a conversation richer and more relevant for all parties involved.
The stakes are enormous. A sales copilot that retains context across conversations could cut research time in half. A customer service agent with durable recall could reduce churn and increase customer satisfaction. However, as companies race to build persistent, context-rich systems, they’re finding that memory requires both technical infrastructure and philosophical clarity.
Why Memory Matters Now
When LLMs first entered the enterprise stack, ballooning token windows seemed to promise that we could simply fill a context window with all the information it might ever need. But the illusion collapsed under real workloads. Performance degraded, retrieval was expensive, costs compounded.
Previous memory approaches fell short due to context pollution. Some researchers called this “context rot,” where simply enlarging context windows resulted in degraded performance. Without context management, or managing what goes into a context window, an AI agent’s responses could be inaccurate or unreliable. For short interactions, this works fine. For workflows that span days or departments, it is crippling, impersonal and ineffective.
Human memory evolved as a layered system precisely because holding everything in working memory is impossible. We compress, abstract and forget to function. Neuroscientists describe at least three interlocking systems: working memory (volatile, like RAM), short-term memory (transient, easily disrupted) and long-term memory (stable, consolidated through repetition and relevance). Similarly, unlocking AI memory requires having the right techniques to compress, store and retrieve memories of users.
From Prompt to Persona
In 2024, developers began to experiment with synthetic long-term memory for agents: external databases that persist context across calls. At first, these systems were crude. Engineers serialized prior messages into text files, re-fed them into prompts and called it memory. But as agents matured, so did the infrastructure.
Today, three design philosophies dominate the landscape:

The vector store approach (memory as retrieval): Systems like Pinecone and Weaviate store past interactions as embeddings in a vector database. When queried, the agent retrieves the most relevant fragments by cosine similarity. It’s fast and simple but prone to surface-level recall.
The summarization approach (memory as compression): Models periodically condense transcripts into rolling summaries.
The graph approach (memory as knowledge) in new AI startups: More ambitious systems, such as Zep, organize memories as nodes and relationships: people, places, events and time. The graph stores “who said what about whom and when.”

Many new startups are tackling this:

Zep‘s Temporal Knowledge Graph outperforms baseline retrieval systems by 18.5% on long-horizon accuracy while cutting latency by nearly 90%.
Mem0 takes a different tack through structured summarization and conflict resolution. It achieves a 26% accuracy gain on standard memory benchmarks and slashes token costs.
Letta recently published results showing that even a simple “filesystem” memory (raw text files indexed by timestamp) surpassed several specialized systems.

Every revolution in computing has hinged on a breakthrough in memory. Magnetic tape, semiconductor memory, cloud storage. Each stage brought new capability and new risk. Now, agent platforms are converging on a key insight: Architecting memory is crucial for performance.
The Architecture of Remembering
Extraction
Agents generate enormous amounts of text, much of it redundant. Good memory requires salience detection: identifying which facts matter. Mem0 uses a “memory candidate selector” to isolate atomic statements; Zep encodes entities and relationships; Letta relies on time-based indexing.
Consolidation
Human recall is recursive, by re-encoding memories each time we retrieve them, strengthening some, discarding others. AI systems can mimic this by summarizing or rewriting old entries when new evidence appears. This prevents what researchers call context drift, where outdated facts persist.
Retrieval
Systems weight relevance by recency and importance. Done right, these layers produce agents that can evolve alongside users. Done poorly, they create brittle systems: ones that hallucinate old facts, repeat mistakes or lose trust altogether.
What Enterprises Gain from Memory
For companies experimenting with AI copilots, the memory problem is immediate and practical.
A call center agent that can recall a customer’s prior issues without re-querying reduces average handle time. In marketing automation, memory-enabled assistants improve lead qualification accuracy thanks to better recall of buyer intent. In aggregate, these efficiencies can compound into millions in savings per year.
Memory lowers cognitive friction for employees. When internal assistants “remember” project history, onboarding new team members becomes smoother. The system becomes an institutional historian, one that captures the tacit knowledge stored inside an organization. Persistent memory changes how humans feel about copilots and AI agents’ usefulness and relevance. When an agent recalls a past conversation, it feels more personal, more collaborative. Emotional continuity builds trust.
To be clear, not everyone agrees that memory deserves the hype. Some engineers argue that context windows will continue to expand, and memory will become a strategic imperative for the model labs to own. Others point to performance complexity: Maintaining persistent state adds infrastructure overhead, latency and risk of misalignment.
The Ethics of Forgetting
Every technology of memory also demands a technology of forgetting.
Enterprises adopting persistent AI memory quickly encounter questions around privacy, anonymization and power:

What should a machine remember about us?
Who controls its recollection?
What happens when forgetting becomes a form of privacy?

Will there be a GDPR for stored memories? In the United States, data-retention policies are murky, especially when AI systems store embeddings rather than explicit text. The boundaries between recall, indexing and personal data remain fuzzy.
For businesses, this is an immediate concern. Memory systems that store customer data risk becoming compliance liabilities if not carefully architected. Encryption, deletion protocols and access controls must be native features, not afterthoughts.
What about bias and privacy? Which memories are reinforced? In humans, selective recall shapes identity. When AI has selective recall, it risks amplifying certain user preferences or suppressing dissenting signals.
The Shape of the Future
Three trajectories seem likely:

Memory as infrastructure: Developers will call memory.write() as easily as they now call db.save(). Expect specialized providers of middleware memory to evolve into middleware for every agent platform.
Memory as governance: Enterprises will demand visibility into what agents know and why. Dashboards will show “memory graphs” of learned facts, with controls to edit or erase. Transparency will become table stakes; memories will be written in natural language.
Memory as identity
Over time, agents will develop personal histories: records of collaboration, preferences, even moods. That history will anchor trust but raise new philosophical questions. When a model fine-tuned on your interactions generates insight, whose memory is it?

We suspect the answer will mirror human questions: a blend of ownership, consent and shared context. Memory is a lived relationship, not a dumb database.
Wisdom is the ability to remember well. As we teach machines to remember, we may discover an interesting human parallel: What we recall and forget defines who we are.

YOUTUBE.COM/THENEWSTACK
Tech moves fast, don't miss an episode. Subscribe to our YouTube channel to stream all our podcasts, interviews, demos, and more.

Group
Created with Sketch.
---
# A Coding Guide to Build a Procedural Memory Agent That Learns, Stores, Retrieves, and Reuses Skills as Neural Modules - TECHLING AI

Most AI systems are great at learning patterns, but they’re surprisingly bad at holding onto skills. We train a model, move on, and then end up retraining it later for something it already knew how to do.

Humans don’t do that. When we learn a skill, we keep it, reuse it, and adapt it to new situations. That’s procedural memory at work.

This guide is about building an agent that behaves more like that. You’ll learn how an agent can pick up skills, save them as reusable neural modules, pull out the right skill when a similar problem shows up, and even combine multiple skills to solve new tasks.

Instead of one massive model that keeps growing, the focus is on systems that get better over time by building on what they already know. Everything here is hands-on and code-focused, based on real design choices rather than abstract theory.

Skill Representation, Storage, and Retrieval

Skills in a procedural memory agent are represented as independent neural modules rather than being embedded inside a single, growing model. This design allows learned behaviors to persist, remain reusable, and evolve independently over time.

Each skill is stored together with structured metadata, such as task context, training conditions, performance metrics, and usage history, which helps the agent determine when a skill is relevant.

When facing a new state, the agent generates a state embedding and compares it with stored skill embeddings using cosine similarity to measure relevance.

Similarity-based retrieval allows the agent to select or rank previously learned skills instead of relearning behavior from scratch.

Usage statistics such as frequency, recency, and success rate are updated continuously to reinforce reliable skills and deprioritize ineffective ones.

Environment Design for Procedural Memory

To evaluate procedural memory in a clear and interpretable way,  construct a simple, task-driven environment where the agent learns to pick up a key, open a door, and reach a goal. These tasks are intentionally minimal yet structured, allowing us to observe how learning unfolds across episodes.

Early on, the agent relies on primitive actions such as movement and interaction. Over time, these actions begin to form higher-level behaviors that can be reused in different situations.

This environment acts as a controlled playground for testing procedural memory, making it easy to see how stored skills improve efficiency, consistency, and overall task performance.

As training progresses, these primitives naturally evolve into higher-level skills, such as “navigate to key” or “unlock door,” which can be reused across episodes.

This setup acts as a testing ground for the procedural memory system, making it possible to track when skills are learned, stored, retrieved, and reused.

Because the environment is simple and interpretable, improvements in behavior—such as faster completion or fewer errors—are easy to observe and directly attribute to skill reuse.

Learning Embeddings and Skill Extraction

Here, the goal is to turn the agent’s raw experience into something it can actually reuse. Instead of treating each interaction in isolation, we build embeddings that capture the context of a state–action sequence—what was happening and why a particular action worked.

This makes it possible to compare different skills in a meaningful way. From there, we extract skills from successful trajectories, pulling out patterns that consistently lead to good outcomes.

As the code runs, you can see a clear shift: early exploration looks random, but over time it starts producing structured knowledge. The agent begins to recognize familiar situations, recall what worked before, and apply that knowledge later, rather than starting from scratch each time.

Context-aware embeddings enable skill comparison

Skills are extracted from successful trajectories

Exploration evolves into structured, reusable knowledge

Balancing Skill Reuse and Exploration During Training

In this phase, we define how the agent decides between reusing an existing skill and falling back to primitive actions when faced with uncertainty. Rather than always exploiting what it already knows, the agent maintains a balance between exploration and reuse, allowing it to discover new behaviors while still benefiting from previously learned skills.

Training is carried out over multiple episodes, during which we track how the skill library evolves—recording when new skills are added, how often existing skills are selected, and how successful they are over time.

As training progresses, clear patterns begin to emerge. Skills that consistently perform well are reused more frequently, while ineffective ones are gradually deprioritized. This shift leads to shorter episodes, smoother behavior, and higher overall rewards.

The results highlight how controlled skill reuse not only improves efficiency but also stabilizes learning as the agent gains experience.

Clear rules govern when to reuse skills versus explore with primitives

Skill usage frequency and success rates are tracked across episodes

High-performing skills naturally become preferred over time

Skill reuse reduces episode length and improves reward outcomes

Evaluating Learning and Procedural Memory Growth

In the final stage, we run the full training loop and observe the procedural memory system in action. Learned skills are printed and inspected, allowing us to see how raw behaviors have been transformed into structured, reusable capabilities.

Alongside this, we can plot key behavior statistics to visualize how the agent’s performance changes over time. Reward trends clearly show improvement across episodes, while the growth of the skill library highlights when new skills are discovered and when existing ones are reused more effectively.

These visualizations complete the lifecycle of procedural memory formation. They confirm that, with experience, the agent shifts from trial-and-error behavior to more deliberate and efficient decision-making.

Run the full training loop to observe how the procedural memory system operates end to end.

Print and inspect learned skills to understand how primitive behaviors evolve into reusable modules.

Track and visualize reward trends across episodes to measure learning progress and stability.

Plot the growth of the skill library to see when new skills are created and how reuse increases over time.

Analyze behavior statistics to confirm reductions in episode length and improved decision efficiency.

Bottom Line

Procedural memory emerges naturally when an agent learns to recognize and extract skills from its own successful experiences. Over time, these skills take on structure, gaining metadata, embeddings, and usage patterns that make them easier to retrieve and reuse in new situations.

What stands out is how quickly this process becomes effective: even within a small environment and using simple heuristics, the agent begins to show meaningful learning dynamics. Skills are no longer isolated behaviors but internal competencies that improve with experience.

This gives us a concrete and intuitive understanding of how agents can move beyond repeated trial and error, gradually developing reusable knowledge that supports smarter, more efficient behavior over time.
---
# LLM-Powered Knowledge Extraction and Concept Modeling: Research Report (2024-2025)

Research Date: October 5, 2025
Compiled by: Claude Code Agent
Focus Areas: Knowledge Graph Construction, Concept Extraction, Relationship Extraction, Entity Linking

Executive Summary
Recent research (2024-2025) demonstrates significant advances in using Large Language Models (LLMs) for automated knowledge extraction and graph construction. Key findings include:

LLMs excel as inference assistants rather than few-shot information extractors
Hybrid approaches combining LLMs with specialized models outperform pure LLM or traditional methods
Fine-tuning shows promise but dataset size and prompt format significantly impact performance
Accuracy challenges persist including hallucinations, schema adherence, and domain-specific gaps
New frameworks like EDC and LLMAEL set state-of-the-art benchmarks
Practical tools from Neo4j, LangChain, and LlamaIndex make the technology accessible

1. Knowledge Graph Construction with LLMs
1.1 Key Research Papers
LLMs for Knowledge Graph Construction and Reasoning (2024)

Paper: "LLMs for Knowledge Graph Construction and Reasoning: Recent Capabilities and Future Opportunities"
Link: https://arxiv.org/abs/2305.13168
GitHub: https://github.com/zjunlp/AutoKG
Key Findings:
Evaluated LLMs across 8 diverse datasets
Tested 4 core tasks: entity extraction, relation extraction, event extraction, link prediction/QA
Finding: "LLMs, represented by GPT-4, are more suited as inference assistants rather than few-shot information extractors"
GPT-4 performs well in KG construction and excels further in reasoning tasks
Introduced AutoKG: multi-agent approach using LLMs and external sources
Proposed Virtual Knowledge Extraction task and VINE dataset

Paper: "Extract, Define, Canonicalize: An LLM-based Framework for Knowledge Graph Construction"
Link: https://aclanthology.org/2024.emnlp-main.548/
GitHub: https://github.com/clear-nus/edc
Methodology:
Extract: Open information extraction from text
Define: Schema definition (or self-generation if unavailable)
Canonicalize: Post-hoc canonicalization for consistency
Key Achievements:
Extracts high-quality triplets without parameter tuning
Handles significantly larger schemas than prior works
Works with or without pre-defined schemas
Includes trained component for schema element retrieval
Performance: Demonstrated on 3 KGC benchmarks with state-of-the-art results

Fine-tuning vs Prompting for KG Construction (2025)

Paper: "Fine-tuning or prompting on LLMs: evaluating knowledge graph construction task"
Link: https://www.frontiersin.org/journals/big-data/articles/10.3389/fdata.2025.1505877/full
Approaches Compared:
Zero-Shot Prompting (ZSP)
Few-Shot Prompting (FSP)
Fine-Tuning (FT)
Models Tested: Llama2, Mistral, Starling
Evaluation Metrics:
Triple Match F1 (T-F1)
Graph Match F1 (G-F1)
Graph Edit Distance (GED)
Novel GM-GBS metric for semantic alignment
Key Findings:
Fine-tuning showed most promising results
Dataset size crucial for model performance
Prompt format more important than base model choice
Smaller models can outperform LLMs after same training
No universal "best" strategy—depends on task constraints

1.2 Industry Tools and Platforms
Neo4j LLM Knowledge Graph Builder (2025)

Link: https://medium.com/neo4j/llm-knowledge-graph-builder-first-release-of-2025-532828c4ba76
Release Date: January 2025
New Features:
Community Summaries generation
Local and global retrievers
Parallel retriever execution
Experimental: Automatic graph consolidation without schema specification
Key Capability: Quick extraction without upfront schema design

LangChain & LlamaIndex Integration (2024)

LangChain Capabilities:
Modular, composable LLM applications
External tool/API/database interfaces
LangGraph for agent deployment (Jan 2024)
Pipeline creation with structured knowledge
LlamaIndex Capabilities:
KnowledgeGraphIndex for automated construction
Entity-based querying
Strong document processing
Agentic Document Workflows (ADW) in 2025
Effective triplet extraction and organization
Integration: Memgraph integration enables GraphRAG solutions
When to Use:
LangChain: End-to-end flexibility, agents, production via LangGraph
LlamaIndex: High-performance indexing, advanced parsing, large datasets

1.3 Accuracy and Limitations
Major Challenges (2024)

Source: NVIDIA Technical Blog, multiple research papers
Link: https://developer.nvidia.com/blog/insights-techniques-and-evaluation-for-llm-driven-knowledge-graphs/

Accuracy Issues:
- Hallucination and inaccurate information generation
- GPT-4 accuracy varies significantly over time (Stanford/Berkeley study)
- Mathematical and code generation tasks show dramatic accuracy drops
Schema Adherence:
- LLMs struggle to follow instructions with complete accuracy
- Improperly formatted triplets (missing punctuation, brackets)
- Less performant models require enhanced parsing and fine-tuning
Complex Reasoning:
- Fails on multi-step reasoning queries
- Requires significant background knowledge
- Context appreciation at fine-grained levels problematic
Scalability:
- Real-time data incorporation challenging
- Managing billions of nodes/edges while maintaining efficiency
- Growth management without performance degradation
Domain Knowledge Gaps:
- Specialized domain knowledge needs persist post-training
- Critical in medical/scientific fields requiring precision
- Diverse training doesn't eliminate domain-specific gaps
Management & Verification:
- Repeatability challenges with closed-access LLMs
- Limited verification capabilities via web APIs
- Experiment management difficulties
Mitigation Strategies

Knowledge Graphs as structured, interpretable data sources
Improved transparency and factual consistency
Reduced hallucinations through KG grounding
Enhanced explainability in LLM-based applications

2.1 OpenAI's Sparse Autoencoder Approach (2024)

Paper: "Extracting Concepts from GPT-4"
Link: https://openai.com/index/extracting-concepts-from-gpt-4/
Date: June 2024

Methodology:
- State-of-the-art sparse autoencoders for finding "features" (interpretable patterns)
- Extracted 16 million features from GPT-4
- Features are human-interpretable activity patterns
Technical Details:
- Passing GPT-4 activations through sparse autoencoder
- Current performance: equivalent to model with 10x less compute
- Scaling challenge: Need billions/trillions of features for complete mapping
Limitations:
- Scaling to billions/trillions of features remains challenging
- Performance trade-off with feature extraction
- Incomplete concept mapping at current scale
2.2 Concept Typicality Using GPT-4 (2023-2024)

Paper: "Uncovering the semantics of concepts using GPT-4"
Published: PNAS, November 2023
Link: https://www.pnas.org/doi/10.1073/pnas.2309350120

Approach:
- Constructed typicality measure: similarity of text to concept
- Zero-shot learning implementation
- Compared against other model-based typicality measures
Performance:
- Improved state-of-the-art correlation with human typicality ratings
- Achieved with zero-shot learning (no training)
- Novel measure of semantic similarity
2.3 Knowledge Graph Construction at Scale (2025)

Paper: "Construction of a knowledge graph for framework material enabled by large language models"
Published: npj Computational Materials, January 2025
Link: https://www.nature.com/articles/s41524-025-01540-6

Scale Achievements:
- 100,000+ academic papers processed
- 2.53 million entities extracted
- 4.01 million relationships identified
- Demonstrates LLM capabilities for complex automation
Applications:
- Ontology mapping
- Semantic enrichment
- Knowledge graph construction
- Scientific literature processing

3.1 Recent Survey and State-of-the-Art (2024-2025)
Comprehensive Survey (2024)

Paper: "A survey on cutting-edge relation extraction techniques based on language models"
Link: https://arxiv.org/html/2411.18157v1
Published: Artificial Intelligence Review, 2025

Key Findings:
- Analyzed 137 papers from ACL conferences (2020-2023)
- BERT-based methods dominate state-of-the-art RE results
- LLMs like T5 show promise in few-shot scenarios
- Language models enable accurate relationship identification
- Captures complex, context-dependent relationships beyond surface associations
Revisiting RE in LLM Era (2023)

Paper: "Revisiting Relation Extraction in the era of Large Language Models"
Link: https://arxiv.org/abs/2305.05003
PMC Link: https://pmc.ncbi.nlm.nih.gov/articles/PMC10482322/

Core Insights:
- LLMs with natural language understanding support KG automation
- Enable entity recognition, relation extraction, schema generation
- Provide generative capabilities for automated construction
3.2 Novel Methods (2025)

Paper: "Large Language Model-Based Event Relation Extraction with Rationales"
Link: https://aclanthology.org/2025.coling-main.500/

LLMERE Method:
- Reduces time complexity: O(n²) → O(n)
- Extracts all events related to specified event at once
- Generates rationales behind extraction results
- Significant efficiency improvement over pairwise methods

Paper: "Post-Training Language Models for Continual Relation Extraction"
Link: https://ui.adsabs.harvard.edu/abs/2025arXiv250405214E/abstract

Models Evaluated:
- Mistral-7B
- Llama2-7B
- Flan-T5 Base
Findings:
- Task-incremental fine-tuning superior to BERT-based approaches
- Tested on TACRED dataset
- Demonstrates LLM advantages in continual learning scenarios
3.3 Generalization Challenges (May 2025)

Paper: "Relation Extraction or Pattern Matching? Unravelling the Generalisation Limits"
Link: https://arxiv.org/abs/2505.12533

Critical Findings:
- RE models struggle with unseen data even in similar domains
- Higher intra-dataset performance ≠ better transferability
- Often signals overfitting to dataset-specific artifacts
- Cross-dataset generalization remains challenging
Implications:
- Need for diverse training datasets
- Importance of domain adaptation techniques
- Recognition of transfer learning limitations

4. Entity Linking and Concept Deduplication
4.1 LLM-Augmented Entity Linking (2024)
LLMAEL Framework (July 2024)

Paper: "LLMAEL: Large Language Models are Good Context Augmenters for Entity Linking"
Link: https://arxiv.org/abs/2407.04020
ACL Anthology: https://aclanthology.org/2025.coling-main.570.pdf

Key Innovation:
- First framework to enhance specialized EL models with LLM augmentation
- LLMs as "context augmenters" generating entity descriptions
- No LLM tuning required
Performance:
- Absolute 8.9% accuracy gain across 6 EL benchmarks
- New state-of-the-art results
- Helps disambiguate long-tail entities with limited training data
Core Insight:
- LLMs struggle with direct entity linking (lack specialized training)
- LLMs excel at context generation
- Hybrid approach leverages both strengths
4.2 Synthetic Context for Scientific Tables (August 2024)

Paper: "Synthetic Context with LLM for Entity Linking from Scientific Tables"
Link: https://aclanthology.org/2024.sdp-1.19/

Methodology:
- LLM-generated synthetic context for table entity linking
- More refined context than raw table data
Performance:
- 10+ point accuracy improvement on S2abEL dataset
- Demonstrates value of context refinement
- Effective for structured data sources
4.3 Biomedical Entity Linking (2024)

Paper: "Improving biomedical entity linking for complex entity mentions with LLM-based text simplification"
Links:
PMC: https://pmc.ncbi.nlm.nih.gov/articles/PMC11281847/
Oxford Academic: https://academic.oup.com/database/article/doi/10.1093/database/baae067/7721591
Published: Database (Oxford Academic), 2024

Approach:
- Simplify complex mentions using GPT-4 (gpt-4-0125-preview)
- Target mentions with little lexical overlap with aliases
- Increase recall for complex entity mentions
Domain Application:
- Biomedical terminology linking
- Complex scientific concept resolution
- Medical knowledge base alignment
4.4 Company Entity Deduplication (October 2024)

Source: TextRazor Blog - "Entity Linking in the LLM Era"
Link: https://www.textrazor.com/blog/2024/10/entity-linking-in-the-llm-era.html

Methodology:
- LLM-based mapping system for company entity deduplication
- Features used: name, industry, description, web presence
- Merges and disambiguates records from multiple sources
Key Insight:
- Specialized EL models excel at KB entity mapping
- Struggle with long-tail entities (limited training data)
- LLMs do reasonable zero-shot identification
- Frontier LLMs lag specialized models in accuracy/speed/consistency
- Trend: Hybrid approaches combining both

5. Vector Embeddings and Concept Matching
5.1 Knowledge Graph Embeddings Evolution (2024)
Knowledge Base Embeddings (2024)

Paper: "Knowledge base embeddings"
Link: https://dl.acm.org/doi/abs/10.24963/kr.2024/77
Conference: 21st International Conference on Principles of KR

Evolution:
- From knowledge graph embeddings to knowledge base embeddings
- Goal: Map facts into vector spaces with conceptual knowledge constraints
- Encodes entities and relations into continuous low-dimensional space
- Crucial for knowledge-driven applications
Hierarchical Concept Embedding (2024)

Paper: "Embedding Hierarchical Tree Structure of Concepts in Knowledge Graph Embedding"
Link: https://www.mdpi.com/2079-9292/13/22/4486
Date: November 2024

HCCE Method:
- Hyper Spherical Cone Concept Embedding
- Explicitly models hierarchical tree structure
- Represents concepts as hyperspherical cones
- Represents instances as vectors
- Maintains anisotropy of concept embeddings
Innovation:
- Captures unique hierarchical structures
- Encompasses rich semantic information
- Concept-level representation advancement
5.2 Core Embedding Concepts (2024)
Fundamentals:
- Vector representations of entities and relationships
- Used for missing link prediction
- Facilitates machine learning tasks
- Similar entities positioned closer in vector space
Applications:
- Clustering
- Classification
- Link prediction
- Similarity computation
RDF2vec Family (2024):
- Paper: "The RDF2vec family of knowledge graph embedding methods"
- Link: https://journals.sagepub.com/doi/full/10.3233/SW-233514
- Authors: Jan Portisch, Heiko Paulheim
5.3 Hybrid Approaches: Vector + Graph (2024)
HybridRAG Concept

Source: Memgraph Blog - "Why Combine Vector Embeddings with Knowledge Graphs for RAG?"
Link: https://memgraph.com/blog/why-hybridrag

Complementary Strengths:
- Vector Databases: Effective at similarity determination
- Knowledge Graphs: Excel at complex dependencies and logic operations
- Combined System: Leverages both strengths
Use Cases:
- Retrieval-Augmented Generation (RAG)
- Semantic search with reasoning
- Context-aware information retrieval
Vector vs Knowledge Graph Decision

Source: FalkorDB Blog
Link: https://www.falkordb.com/blog/knowledge-graph-vs-vector-database/

When to Choose:
- Vector DB: Similarity-based retrieval, embeddings, semantic search
- Knowledge Graph: Relationship reasoning, complex queries, structured knowledge
- Both: Maximum capability for modern AI applications

6.1 Research Workshops and Community
LLM-TEXT2KG 2025 Workshop

Full Name: 4th International Workshop on LLM-Integrated Knowledge Graph Generation from Text
Link: https://aiisc.ai/text2kg2025/
Focus Areas:
LLM-enhanced knowledge extraction
Context-aware entity disambiguation
Named entity recognition
Relation extraction
Ontology alignment

6.2 Open Source Tools and Libraries
AutoKG Repositories

zjunlp/AutoKG: LLMs for KG Construction and Reasoning
Link: https://github.com/zjunlp/AutoKG

Paper: WWWJ 2024

wispcarey/AutoKG: Efficient Automated KG Generation

Link: https://github.com/wispcarey/AutoKG

Paper Collections

zjukg/KG-LLM-Papers: Papers integrating KGs and LLMs
Link: https://github.com/zjukg/KG-LLM-Papers
Comprehensive resource list
Updated with latest research

6.3 Industry Applications (2024-2025)
Scientific Research Applications

Large-scale literature processing (100K+ papers)
Multi-million entity/relationship extraction
Automated ontology mapping
Semantic enrichment pipelines

Healthcare Applications

Biomedical entity linking
Medical knowledge graph construction
Clinical terminology mapping
Drug-disease relationship extraction

Enterprise Applications

Company entity deduplication
Business knowledge graphs
Automated schema generation
Real-time knowledge updates

7. Key Methodologies Summary

Approach
Strengths
Limitations
Use Cases

Zero-Shot Prompting
No training needed, quick deployment
Lower accuracy, inconsistent outputs
Exploratory analysis, prototyping

Few-Shot Prompting
Better than zero-shot, minimal examples
Still limited accuracy, prompt-sensitive
Limited data scenarios

Fine-Tuning
Highest accuracy, task-specific optimization
Requires training data, computational cost
Production systems, specialized domains

Hybrid (LLM + Specialized)
Combines strengths, state-of-the-art
More complex architecture
Enterprise applications, high accuracy needs

7.2 Performance Optimization Strategies
Prompt Engineering:
- Format more important than model choice
- Structured output specifications critical
- Enhanced parsing for error handling
- Schema adherence through instruction design
Model Selection:
- GPT-4: Reasoning and inference tasks
- Claude: Context understanding, long documents
- BERT-based: Relation extraction (current SOTA)
- T5: Few-shot scenarios
- Smaller models + training can outperform large LLMs
Architectural Patterns:
- Multi-agent systems (AutoKG)
- Three-phase frameworks (EDC)
- Context augmentation (LLMAEL)
- Hybrid vector+graph systems

8. Evaluation Metrics and Benchmarks
8.1 Standard Metrics
Extraction Quality:
- Triple Match F1 (T-F1)
- Graph Match F1 (G-F1)
- Graph Edit Distance (GED)
- GM-GBS (semantic alignment)
Entity Linking:
- Accuracy improvements (absolute %)
- Recall for complex mentions
- Precision on long-tail entities
Embedding Quality:
- Correlation with human ratings
- Similarity accuracy
- Hierarchical structure preservation
8.2 Common Benchmarks

TACRED: Relation extraction
S2abEL: Scientific table entity linking
VINE: Virtual knowledge extraction
Multiple EL benchmarks: Entity linking (6 commonly used)
3 KGC benchmarks: Knowledge graph construction

9. Future Directions and Opportunities
9.1 Research Gaps
Identified in Literature:
- Scaling to billions/trillions of features
- Cross-domain generalization
- Real-time knowledge graph updates
- Handling contradictory information
- Multilingual knowledge extraction
- Temporal relationship modeling
9.2 Emerging Trends
2025 Developments:
- Agentic workflows (LlamaIndex ADW)
- Community detection in graphs
- Automatic graph consolidation
- Parallel retrieval systems
- Local and global graph reasoning
Promising Directions:
- Graph neural networks + LLMs
- Neuro-symbolic approaches
- Continuous learning systems
- Explainable knowledge extraction
- Privacy-preserving graph construction

10. Practical Recommendations
10.1 For Researchers
High-Priority Areas:
1. Cross-dataset generalization methods
2. Efficient scaling to larger feature spaces
3. Hybrid architecture optimization
4. Domain adaptation techniques
5. Evaluation metric standardization
10.2 For Practitioners
Implementation Guidance:
1. Start Simple: Zero-shot prompting for prototyping
2. Choose Tools: Neo4j/LangChain/LlamaIndex based on needs
3. Hybrid Approach: Combine vector + graph for RAG
4. Quality Over Speed: Fine-tune for production
5. Monitor Performance: Track accuracy degradation over time
Tool Selection Matrix:
- Neo4j LLM Builder: Quick start, no schema required
- LangChain: Production pipelines, agent systems
- LlamaIndex: Document-heavy, enterprise scale
- Custom Fine-tuned: Domain-specific, high accuracy needs
10.3 For System Designers
Architecture Decisions:
1. Embedding strategy (sparse autoencoders vs. standard)
2. Graph database choice (Neo4j, Memgraph, FalkorDB)
3. LLM provider (OpenAI, Anthropic, open-source)
4. Scaling strategy (batch processing, streaming)
5. Quality assurance (human-in-loop, automated validation)

11. Conclusion
The 2024-2025 research landscape shows LLM-powered knowledge extraction has matured significantly:
Key Takeaways:
1. Hybrid approaches win: Combining LLMs with specialized models achieves state-of-the-art
2. Context matters: LLMs excel at augmentation rather than direct extraction
3. Fine-tuning works: With sufficient data, smaller models can outperform large LLMs
4. Challenges persist: Hallucinations, generalization, and scaling remain active research areas
5. Tools mature: Production-ready frameworks now available (Neo4j, LangChain, LlamaIndex)
Practical Impact:
- Knowledge graph construction is now accessible to non-experts
- Automated pipelines process millions of relationships
- Real-world applications span healthcare, science, and enterprise
- Cost-effective solutions emerging through open-source tools
Future Outlook:
The field is moving toward:
- Agentic, self-improving knowledge systems
- Real-time, continually learning graphs
- Explainable, verifiable extraction
- Trillion-parameter concept spaces
- Seamless human-AI collaboration in knowledge work

12. References and Resources
Key Papers (2024-2025)

AutoKG: https://arxiv.org/abs/2305.13168
EDC Framework: https://aclanthology.org/2024.emnlp-main.548/
LLMAEL: https://arxiv.org/abs/2407.04020
Fine-tuning vs Prompting: https://www.frontiersin.org/journals/big-data/articles/10.3389/fdata.2025.1505877/full
Relation Extraction Survey: https://arxiv.org/html/2411.18157v1
HCCE Embeddings: https://www.mdpi.com/2079-9292/13/22/4486
Knowledge Base Embeddings: https://dl.acm.org/doi/abs/10.24963/kr.2024/77

Industry Resources

Neo4j LLM Builder: https://neo4j.com/blog/developer/llm-knowledge-graph-builder-release/
NVIDIA Technical Blog: https://developer.nvidia.com/blog/insights-techniques-and-evaluation-for-llm-driven-knowledge-graphs/
Memgraph HybridRAG: https://memgraph.com/blog/why-hybridrag
TextRazor Entity Linking: https://www.textrazor.com/blog/2024/10/entity-linking-in-the-llm-era.html

Tool Documentation

LangChain: https://python.langchain.com/docs/
LlamaIndex: https://docs.llamaindex.ai/
Neo4j: https://neo4j.com/docs/
OpenAI: https://platform.openai.com/docs
Anthropic: https://docs.anthropic.com/

GitHub Repositories

zjunlp/AutoKG: https://github.com/zjunlp/AutoKG clear-nus/edc: https://github.com/clear-nus/edc zjukg/KG-LLM-Papers: https://github.com/zjukg/KG-LLM-Papers wispcarey/AutoKG: https://github.com/wispcarey/AutoKG

Community and Workshops

LLM-TEXT2KG 2025: https://aiisc.ai/text2kg2025/
NODES 2024 (Neo4j): https://neo4j.com/videos/nodes-2024-building-knowledge-graphs-with-llms/

Report Compiled: October 5, 2025
Total Sources: 50+ papers, articles, and resources
Coverage Period: January 2024 - October 2025
Focus: Production-ready research and practical implementations
---
# Your Agents Just Got a Memory Upgrade: ACE Open-Sourced on GitHub

Last month, the SambaNova team, in partnership with Stanford and UC Berkeley, introduced the viral paper Agentic Context Engineering (ACE), a framework for building evolving contexts that enable self-improving language models and agents. Today, the team has released the full ACE implementation, available on GitHub, including the complete system architecture, modular components (Generator, Reflector, Curator), and ready-to-run scripts for both Finance and AppWorld benchmarks. The repository provides everything needed to reproduce results, extend to new domains, and experiment with evolving playbooks in your own applications. Feel free to try it out... we’d love your feedback and contributions!

ACE changes how AI learns — instead of updating weights, it grows its memory, storing lessons from every win and mistake. Like an AI that journals after each task, it reflects, notes, and reasons better next time, turning static models into experience-driven, self-improving systems. ACE consistently outperforms strong baselines including +10.6% on agentic benchmarks and +8.6% on domain specific benchmarks, while significantly reducing latency and rollout cost.

1. Why Context Engineering
Context engineering has rapidly become a central theme in building capable, reliable, and self-improving AI systems. It has gained attention across both research and industry, from Anthropic’s guidebook on context engineering [1], to mem0’s and Letta’s development of persistent memory layers for AI agents [2,3], to Databricks’ enterprise agent that integrates prompt optimization [4].
At its core, context engineering arises from the need to dynamically adapt and customize AI systems over the long term, beyond what fine-tuning alone can achieve. This includes remembering a user’s personal preferences, maintaining enterprise records, recalling prior strategies or lessons learned from interaction with environments, etc. The term context can take many forms — prompts, memory states, or input structures — but the fundamental challenge remains the same: How to systematically engineer inputs that enable AI systems to remain capable, consistent, and reliable over time.
Two recent trends suggest that context engineering is increasingly well-supported over time, making it a “friend with time” as both models and systems evolve.

Advances in long-context modeling Large language models (LLMs) are becoming increasingly capable of handling long and complex contexts, both in terms of extended context window sizes and improved recall accuracy — demonstrated, for example, by benchmarks such as needle-in-a-haystack.
Progress in long-context serving infrastructureInference systems are now better equipped to serve long-context workloads efficiently. Recent advances include optimized KV cache transfer and storage mechanisms (e.g., LMCache [5], Mooncake [6]) and high-performance KV cache compression libraries such as NVIDIA’s KVPress [7].

2. ACE: Agentic Context Engineering
2.1 Two Limitations in Existing Methods
Despite the early promise delivered by context adaptation, we observe two major limitations in existing methods.

The Brevity BiasWe observe that many context optimization approaches demonstrate the tendency to collapse toward short, generic prompts. Research papers like The Prompt Alchemist [8] document this effect in prompt optimization for software test generation, where iterative methods repeatedly produce near-identical instructions (e.g., "Create unit tests to ensure methods behave as expected"), sacrificing diversity and omitting domain-specific details. Humans and LLMs have different advantages when handling contexts. Humans benefit from concise, higher-level summarization, while language models increasingly benefit from detailed, dense context (as demonstrated by recent trends in research [9, 10, 11]).
Context CollapseWe observe the phenomenon of “context collapse” — iterative rewriting of context could lead to sudden shrinkage of the context into shorter, less informative summaries, resulting in severe performance degradation. As demonstrated by the figure below, the context at step 60 contained 18,282 tokens and achieved an accuracy of 66.7, but at the very next step it collapsed to just 122 tokens, with accuracy dropping to 57.1 — worse than the baseline accuracy of 63.7 without adaptation.

2.2 Core Principles of ACE

We argue that contexts should function not as concise summaries, but as comprehensive, evolving playbooks — detailed, inclusive, and rich with domain insights.
ACE is designed to achieve this goal of comprehensive and evolvable context. The ACE framework features an agentic architecture that separates the responsibility across three components:

Generator — Produces reasoning trajectories and identifies useful context items.
Reflector — Analyzes successes and failures, and extracts concrete insights.
Curator — Organizes insights into structured, incremental context updates.

Note that ACE is not the first work to adopt this type of architecture. Prior work, like Dynamic Cheatsheet [12], features a Generator-Curator framework for managing adaptive memory as context. The key innovation of ACE as compared to prior work is to enable the scalable growth of contexts in an efficient way, avoiding issues like the brevity bias and context collapse, as well as the high overhead of context update.
ACE adopts the following key recipes for efficient and scalable growth of context:

Incremental, structured updateACE features incremental updates of context in the format of small “delta” updates. Instead of rewriting the entire context, the Curator produces a small piece of context that gets merged into the existing context. This merging operation is achieved via non-LLM components for stability. Further efficiency and capability can be unlocked through (1) parallel learning of multiple “delta” contexts from different input samples, and (2) multi-epoch adaptation, in which the same input samples are revisited to progressively extract more insights.
Grow-and-refineBesides facilitating the growth of context as a first-class principle, ACE periodically refines the context to make it compact and knowledge-dense. A de-duplication step merges context bullets with high semantic similarity scores to reduce redundancy. Based on the need of application, this step can run either lazily (only when necessary) or proactively.

Figure: Example ACE-Generated Context on the AppWorld Benchmark (partially shown).
2.3 Results and Findings

Enabling High-Performance, Self-Improving AgentsACE enables agents to self-improve by dynamically refining the input context. It boosts accuracy on the AppWorld benchmark by up to 17.1% by learning to engineer better contexts from execution feedback, without needing ground-truth labels. This allows a smaller, open-source model (DeepSeek-V3.1) to match the performance of the top-ranked proprietary agent (IBM CUGA with GPT-4.1) on the leaderboard.
Large Gains on Domain-Specific BenchmarksOn complex financial reasoning benchmarks, ACE delivers an average performance gain of 8.6% over strong baselines by constructing comprehensive playbooks with domain-specific concepts and insights.
Lower Cost and Adaptation LatencyACE achieves these gains efficiently, reducing adaptation latency by 86.9% on average, while requiring fewer rollouts and lower token dollar costs.

3. Q & A
Q: Does ACE kill fine-tuning?
A: Short answer: No. We believe that fine-tuning is still crucial in aligning AI systems, reducing inference-time resource usage, etc. ACE offers a new perspective (orthogonal to fine-tuning) that adapts AI systems without changing weights, and could be particularly useful in these scenarios: (1) when the cost of fine-tuning is high, especially when updates are small and frequent, and (2) when model weights are not available (e.g. commercial LLMs), (3) when training data and ground-truth reward are not available, and (4) when concerns like selective unlearning (e.g. due to privacy laws), interpretability, etc. are important.
Q: How does ACE differ from prompt optimization methods like GEPA?
A: ACE does not conflict with methods like GEPA, and in fact, can be used jointly with existing prompt optimization methods. One example is that a solid system prompt can be learned with GEPA based on training data in the offline stage, assuming computation resource and adaptation latency are not concerns; ACE can be used during the online stage to further grow and refine the context, when ground-truth rewards might not be available and adaptation latency is a concern. Overall, the focus of ACE is to grow contexts in a scalable and efficient way, avoiding issues like the brevity bias and context collapse.
Q: What if the context window is exceeded?
A: Though not explicitly addressed in this work, ACE can be used in complementary to many other context management approaches. For example, when the context window limit is reached and something has to be dropped, ACE could benefit from existing methods in context compression, token dropping, etc. We take this as an important future direction, and we are working to evaluate how these approaches might affect ACE’s effectiveness.
Q: What are the limitations of ACE?
A: A potential limitation of ACE is its reliance on a reasonably strong Reflector: if the Reflector fails to extract meaningful insights from generated traces or outcomes, the constructed context may become noisy or even harmful. In domain-specific tasks where no model can extract useful insights, the resulting context will naturally lack them. This dependency is similar to Dynamic Cheatsheet [12], where the quality of adaptation hinges on the underlying model’s ability to curate memory. We also note that not all applications require rich or detailed contexts. Tasks like HotPotQA often benefit more from concise, high-level instructions (e.g., how to retrieve and synthesize evidence) than from long contexts. Similarly, games with fixed strategies such as Game of 24 may only need a single reusable rule, rendering additional context redundant. Overall, ACE is most beneficial in settings that demand detailed domain knowledge, complex tool use, or environment-specific strategies that go beyond what is already embedded in model weights or simple system instructions.
Building Your Own Applications with ACE
Developing with ACE is now extremely easy. Below is a quick start guide to help developers who clone our repo start building with ACE. The code contains a generator, reflector, and curator that continuously improve the playbook as mentioned above. You can read more about how to use ACE in the README included in the repo.

ACE is more than just a framework, but also a new paradigm: We believe that AI systems can be made smarter and better without changing its brain, but with smarter contexts. We would love to engage with the community to further explore this research direction, and make it more useful in practice.
Future Roadmap
We are currently working on the following aspects to make ACE more practical and usable.

Support for more applications. ACE could benefit a diverse set of applications, ranging from agentic AI to domain-specific problem solving. We are actively working to evaluate ACE on different types of applications, and we’d love to engage with the community to see what everyone wants to build with ACE.
Agent framework integration. To make ACE easy to use, we plan to integrate ACE into mainstream agent frameworks as plug-in modules. For now, we are working on ACE integration into DSPy, and we will update the community on that in a few weeks.
Powering AI training with ACE. We are actively exploring how ACE-generated contexts can be used in turn to train more powerful AI models via techniques like RLVR. We believe this could form a virtuous cycle: ACE produces contexts and rewards for training AI systems, while AI systems empower ACE to be more capable.

Citation
If you find our work helpful, please use the following citation.
@article{zhang2025agentic, title={Agentic Context Engineering: Evolving Contexts for Self-Improving Language Models}, author={Zhang, Qizheng and Hu, Changran and Upasani, Shubhangi and Ma, Boyuan and Hong, Fenglu and Kamanuru, Vamsidhar and Rainton, Jay and Wu, Chen and Ji, Mengmeng and Li, Hanchen and others}, journal={arXiv preprint arXiv:2510.04618}, year={2025} }
References [1] Anthropic, Effective context engineering for AI agents, 2025 [2] mem0, Universal memory layer for AI Agents, 2025 [3] Letta, Agent Memory: How to Build Agents that Learn and Remember, 2025 [4] The Mosaic Research Team, Building State-of-the-Art Enterprise Agents 90x Cheaper with Automated Prompt Optimization, 2025 [5] LMCache, Accelerating the Future of AI, One Cache at a Time [6] Mooncake: A KVCache-centric Disaggregated Architecture for LLM Serving [7] NVIDIA/KVPress: LLM KV cache compression made easy [8] Shuzheng Gao, Chaozheng Wang, Cuiyun Gao, Xiaoqian Jiao, Chun Yong Chong, Shan Gao, and Michael Lyu. The prompt alchemist: Automated llm-tailored prompt optimization for test case generation. arXiv preprint arXiv:2501.01329, 2025.
[9] Tianxiang Chen, Zhentao Tan, Xiaofan Bo, Yue Wu, Tao Gong, Qi Chu, Jieping Ye, and Nenghai Yu. Flora: Effortless context construction to arbitrary length and scale. arXiv preprint arXiv:2507.19786, 2025.
[10] Yeounoh Chung, Gaurav T Kakkar, Yu Gan, Brenton Milne, and Fatma Ozcan. Is long context all you need? leveraging llm’s extended context for nl2sql. arXiv preprint arXiv:2501.12372, 2025.
[11] Mingjian Jiang, Yangjun Ruan, Luis Lastras, Pavan Kapanipathi, and Tatsunori Hashimoto. Putting it all into context: Simplifying agents with lclms. arXiv preprint arXiv:2505.08120, 2025.
[12] Mirac Suzgun, Mert Yuksekgonul, Federico Bianchi, Dan Jurafsky, and James Zou. Dynamic cheatsheet: Test-time learning with adaptive memory. arXiv preprint arXiv:2504.07952, 2025.