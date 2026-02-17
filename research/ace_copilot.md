# Agent Context Engineering: Teaching AI Systems to Build Their Own Playbooks

How Agentic Context Engineering is Revolutionizing Self-Improving Language ModelsNot a member? read hereA deep dive into the breakthrough framework that’s enabling AI agents to learn from experience without retrainingThe AI landscape is undergoing a quiet revolution. While much of the industry focuses on building bigger models with more parameters, a different paradigm is emerging — one that treats context as the primary lever for improvement rather than model weights. At the forefront of this shift is ACE (Agentic Context Engineering), a framework that fundamentally reimagines how AI systems learn and adapt.The Context RevolutionTo understand why ACE matters, we need to first appreciate the growing importance of context adaptation in modern AI systems. Traditional machine learning has trained us to think about improvement in terms of weight updates — fine-tuning models, running gradient descent, updating billions of parameters. But this approach has significant limitations:Interpretability: Weight updates are black boxes. You can’t easily understand what changed or why.Speed: Fine-tuning is slow and expensive, requiring substantial computational resources.Flexibility: Once weights are updated, it’s difficult to selectively…
---
# Building an agentic memory system for GitHub Copilot

Our vision is to evolve GitHub Copilot into an ecosystem of agents that collaborate across the entire development lifecycle from coding and code review to security, debugging, deployment, and maintenance. To unlock the full potential of multi-agent workflows, we need to move beyond isolated interactions—that start from scratch each session—and toward a cumulative knowledge base that grows with every use.

Cross-agent memory allows agents to remember and learn from experiences across your development workflow, without relying on explicit user instructions.

Each interaction teaches Copilot more about your codebase and conventions, making it increasingly effective over time. For example, if Copilot coding agent learns how your repository handles database connections as it’s fixing a security vulnerability, Copilot code review can then use that knowledge to spot inconsistent patterns in future pull requests. Or if Copilot code review notices that certain files must stay synchronized, in the future Copilot coding agent will automatically update them together when generating new code.

The challenge: What to remember and when to forget

Our agents continuously improve at extracting the context needed for specific tasks. The core challenge for memory systems isn’t about information retrieval, but ensuring that any stored knowledge remains valid as code evolves across branches and time.

In practice, this means a memory system must handle changes to code, abandoned branches, and conflicting observations—all while ensuring that agents only act on information that’s relevant to the current task and code state. For example, a logging convention observed in one branch may later be modified, superseded, or never merged at all.

One option would be to implement an offline curation service to deduplicate, resolve conflicts, track branch status, and expire stale information. At GitHub’s scale, however, such an approach would introduce significant engineering complexity and LLM costs, while still requiring mechanisms to reconcile changes at read time. We started by exploring a simpler, more efficient approach.

Our solution: just-in-time verification

Information retrieval is an asymmetrical problem: It’s hard to solve, but easy to verify. By using real-time verification, we gain the power of pre-stored memories while avoiding the risk of outdated or misleading information.

Instead of offline memory curation, we store memories with citations: references to specific code locations that support each fact. When an agent encounters a stored memory, it verifies the citations in real-time, validating that the information is accurate and relevant to the current branch before using it. This verification boils down to a small number of simple read operations, adding no significant latency to agent sessions in our testing.

We implemented memory creation as a tool that agents can invoke when they discover something that’s likely to have actionable implications for future tasks.

How Copilot agents store learnings worth remembering as they carry out their tasks.

Consider this example: While reviewing a pull request from an experienced developer, Copilot code review discovers that API version tracking must stay synchronized across different parts of a codebase. It might encounter these three updates in the same pull request:

In src/client/sdk/constants.ts:

export const API_VERSION = "v2.1.4";

In server/routes/api.go:

const APIVersion = "v2.1.4"

In docs/api-reference.md:

Version: v2.1.4

In response, Copilot code review can invoke the memory storage tool to create a memory like this:

{ subject: "API version synchronization", fact: "API version must match between client SDK, server routes, and documentation.", citations: ["src/client/sdk/constants.ts:12", "server/routes/api.go:8", "docs/api-reference.md:37"], reason: "If the API version is not kept properly synchronized, the integration can fail or exhibit subtle bugs. Remembering these locations will help ensure they are kept syncronized in future updates." }

The result: The next time an agent updates the API version in any of these locations, it will see this memory and realize that it must update the other locations too, preventing a versioning mismatch that could break integrations. Similarly, if an inexperienced developer opens a pull request that updates only one of these locations, Copilot code review will flag the omission and suggest the missing updates, automatically transferring knowledge from a more experienced team member to a newer one. 💥

Memory usage

Retrieval

When an agent starts a new session, we retrieve the most recent memories for the target repository and include them in the prompt. Future implementations will enable additional retrieval techniques, such as a search tool and weighted prioritization.

How Copilot enriches agent prompts with memories from previous tasks.

Verification

Before applying any memory, the agent is prompted to verify its accuracy and relevance by checking the cited code locations. If the code contradicts the memory, or if the citations are invalid (e.g. point to nonexistent locations), the agent is encouraged to store a corrected version of the memory reflecting the new evidence. If the citations check out and the memory is deemed useful, the agent is encouraged to store it again in order to refresh its timestamp.

Privacy and security

It’s important to note that memories are tightly scoped. Memories for a given repository can only be created in response to actions taken within that repository by contributors with write permissions, and can only be used in tasks on that same repository initiated by users with read permissions. Much like the source code itself, memories about a repository stay within that repository, ensuring privacy and security.

Cross-agent memory sharing

The full power of our memory system emerges as different Copilot agents learn from one another.

Copilot code review discovers a logging convention while reviewing a pull request: “Log file names should follow pattern ‘app-YYYYMMDD.log’. Use Winston for logging with format: timestamp, error code, user ID.”

Copilot coding agent is later assigned a task to implement a new microservice. It sees and verifies the memory and automatically applies the same logging format.

Copilot CLI helps a developer debug an issue, efficiently retrieving the correct log file and finding the relevant timestamps based on the logging format learned by the code review agent.

Each agent contributes to and benefits from the shared knowledge base, allowing agents to reuse validated repository knowledge across tasks. As additional agents adopt memory—whether for development workflows, debugging, or security analysis—they’ll contribute to and benefit from the same evolving understanding of your codebase.

Evaluation

Stress-testing agent resilience

Our biggest concern was the impact of outdated, incorrect, or even maliciously injected memories. To test the system’s resilience, we deliberately seeded repositories with adversarial memories–facts that contradicted the codebase–with citations pointing to irrelevant or nonexistent code locations.

Across all test cases, agents consistently verified citations, discovered contradictions, and updated incorrect memories. The memory pool self-healed as agents stored corrected versions based on their observations. The citation verification mechanism robustly prevented the risk of misleading memories.

Simulating a realistic memory pool

For each repository in our evaluation set, we ran agents on diverse historical tasks (predating our target evaluation tasks) and let them populate the memory database organically, using the “store_memory” tool we provided. To simulate worst-case conditions, we overrepresented memories from branches that were abandoned or closed without merging, ensuring realistically noisy memories.

When we ran Copilot code review on the pull requests in our evaluation set, memory usage led to 3% increase in precision and 4% increase in recall.

Measuring impact on developers

The ultimate test of our memory system was its impact on real developers in their everyday workflows. We ran A/B tests on the first two Copilot agents to deploy memory, Copilot code review and Copilot coding agent, measuring the impact on key user metrics.

Copilot coding agent: 7% increase in pull request merge rates (90% with memories vs. 83% without). This means developers are saving more time and getting the desired results more often when they assign tasks to Copilot.

Copilot code review: 2% increase in positive feedback on comments (77% with memories vs 75% without). This means automated code review is yielding improved quality assurance.

Both increases are highly statistically significant, with p-value <0.00001

These results demonstrate that cross-agent memory delivers measurable value to developers in their daily workflows.

What’s next

We’ve deployed repository-scoped memory storage and usage in Copilot CLI, Copilot coding agent, and Copilot code review on an opt-in basis. We’re listening to user feedback and tracking performance metrics closely as we iterate and prepare for a wider rollout across more Copilot workflows. We’re also exploring a range of approaches to tuning memory generation, curation, prioritization, and usage.

Cross-agent memory reduces the need to re-establish context at the start of each task by allowing validated information to persist across agentic workflows. We’re excited about the possibilities memory will unlock, and we’re just getting started. We look forward to your feedback so we can ensure GitHub Copilot continues to evolve in ways that best support your needs. Happy coding!

Read our Docs to learn how to enable memory in Copilot >

Tags:
agentic memory agentic workflows
GitHub Copilot
GitHub Copilot CLI
GitHub Copilot code review
GitHub Copilot coding agent

Written by
Tiferet Gazit is a principal machine learning engineer at GitHub building AI agents for code security, code quality, and developer productivity. With a background in medical computer vision and deep learning, she’s passionate about developing intelligent products that impact people’s lives.

Explore more from GitHub
Docs
Everything you need to master GitHub, all in one place.
Go to Docs

GitHub
Build what’s next on GitHub, the place for anyone from anywhere to build anything.
Start building

Customer stories
Meet the companies and engineering teams that build with GitHub.
Learn more

The GitHub Podcast
Catch up on the GitHub podcast, a show dedicated to the topics, trends, stories and culture in and around the open source developer community on GitHub.
Listen now
---
# Benchmarking AI Agent Memory: Is a Filesystem All You Need?  | Letta

Summary: Letta agents running on gpt-4o-mini achieve 74.0% accuracy on LoCoMo by simply storing conversation histories in files, rather than using specialized memory or retrieval tools. This suggests that: 1) current memory benchmarks may not be very meaningful, and 2) memory is more about how agents manage context than the exact retrieval mechanism used.Memory for AI AgentsSince the dawn of GPT-4, LLMs have been their limited context length. Without long-term memory, LLMs and agents face significant limitations: they forget information, cannot learn and improve over time, and lose track of their objectives during long-running, complex tasks (a phenomenon often referred to  as “derailment”).MemGPT introduced memory management for agents by creating a memory hierarchy inspired by a traditional operating system (OS). Agents actively manage what remains in their immediate context (core memory) versus what gets stored in external layers (conversational memory, archival memory, and external files) that can be retrieved as needed. This approach allows agents to maintain unlimited memory capacity within fixed context windows. Many agentic systems today, including Letta, implement MemGPT’s design to enable long-term memory in LLM agents.Additionally, various memory-specific tools have emerged to offer "memory" as a pluggable service, providing agents with tools to store and retrieve information, often using specialized knowledge graphs or vector database solutions.Attempts at Benchmarking Memory Tools (e.g., Mem0, LangMem, Zep)Evaluating the effectiveness of these memory tools in isolation is extremely challenging. The quality of an agent's memory often depends more on the underlying agentic system's ability to manage context and call tools than on the memory tools themselves. For example, even if a search tool is theoretically more performant, it won't work well for memory if the agent cannot use it effectively (due to poor prompting or lack of examples in training data).As a result, evaluation of memory tools has primarily focused on retrieval benchmarks like LoCoMo, rather than agentic memory itself. LoCoMo is a question-answering benchmark focusing on retrieval from long conversations. Each sample contains two fictional speakers and a list of AI-generated, timestamped conversations. The task involves answering questions about the speakers or facts presented in their conversations.One memory tool creator, Mem0, published controversial results claiming to have run MemGPT on LoCoMo. The results were puzzling, since our research team (the same team behind MemGPT) was unable to determine a way to backfill LoCoMo data into MemGPT/Letta without significant refactoring of the codebase. Mem0 did not respond to requests for clarification on how the benchmarking numbers were computed, or provide any modified MemGPT implementation that supports meaningful backfill of LoCoMo data. Benchmarking Letta Filesystem with LoCoMoAlthough Letta does not have a native way to ingest conversational histories like those in LoCoMo, we recently added support for connecting files to Letta agents (including MemGPT agents) - called Letta Filesystem. We were curious to see how Letta would perform by simply placing the LoCoMo conversational history into a file, without any specialized memory tools.When files are attached to a Letta agent, the agent gains access to a set of file operation tools:grepsearch_filesopencloseThe conversational data is placed into a file, which is uploaded and attached to the agent. Files in Letta are automatically parsed and embedded to enable semantic (vector) search over their contents. The agent is given tools for semantic search (search_files), text matching (grep), and answering questions (answer_question).We used GPT-4o mini for the agent to match the original experiment that was said to have been run with MemGPT. Since GPT-4o mini is a weaker model, we made the agent only partially autonomous by defining tool rules to limit the agent's tool-calling patterns. The agent must start by calling search_files and continue searching through files until it decides to call answer_question and terminate. What it searches for and how many times it calls tools is up to the agent.This simple agent achieves 74.0% on LoCoMo with GPT-4o mini and minimal prompt tuning, significantly above Mem0's reported 68.5% score for their top-performing graph variant.Why Does a Filesystem Beat Specialized Memory Tools?Agents today are highly effective at using tools, especially those likely to have been in their training data (such as filesystem operations). As a result, specialized memory tools that may have originally been designed for single-hop retrieval are less effective than simply allowing the agent to autonomously search through data with iterative querying.Agents can generate their own queries rather than simply searching the original questions (e.g., transforming "How does Calvin stay motivated when faced with setbacks?" into "Calvin motivation setbacks"), and they can continue searching until the right data is found.Memory for Agents: Agent Capabilities Matter More Than the ToolsWhether an agent "remembers" something depends on whether it successfully retrieves the right information when needed. Therefore, it's much more important to consider whether an agent will be able to effectively use a retrieval tool (knowing when and how to call it) rather than focusing on the exact retrieval mechanisms (e.g. knowledge graphs vs vector databases).Agents today are extremely effective at using filesystem tools, largely due to post-training optimization for agentic coding tasks. In general, simpler tools are more likely to be in the training data of an agent and therefore more likely to be used effectively. While more complex solutions like knowledge graphs may help in specific domains, they may also come at the cost of being more difficult for the LLM (agent) to understand.How to Properly Evaluate Agent MemoryAn agent's memory depends on the agent architecture, its tools, and the underlying model. Comparing agent frameworks and agent memory tools is like comparing apples to oranges, as you can always mix and match frameworks and tools (and, of course, models).The Letta Memory Benchmark (Letta Leaderboard) provides an apples-to-apples comparison evaluating different models' capabilities in terms of memory management, keeping the framework (currently just Letta) and tools constant. The benchmark creates memory interactions on-the-fly to evaluate memory in a dynamic context, rather than just retrieval (as with LoCoMo).Another approach to evaluating memory is to assess the agent's holistic performance on specific tasks that require memory. One example is Terminal-Bench, which evaluates how well agents can solve complex, long-running tasks. Because tasks are long-running and require processing far more state than what fits into context, agents can leverage their memory to keep track of their task state and progress. Letta's OSS terminal-use agent is currently #4 overall (#1 for OSS) on the Terminal-Bench coding benchmark.ConclusionWith a well-designed agent, even simple filesystem tools are sufficient to perform well on retrieval benchmarks such as LoCoMo. More complex memory tools can be plugged into agent frameworks like Letta via MCP or custom tools.For more resources, see:Letta Memory Benchmark for evaluating model capabilities for agentic memoryCode for running the LoCoMo benchmark You can get started with Letta agents on Letta Platform.