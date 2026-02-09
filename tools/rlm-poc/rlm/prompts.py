"""Prompt templates for the RLM learning extraction pipeline."""


SYSTEM_PROMPT = """\
You are tasked with analyzing agent coding sessions to extract actionable learnings.
You have access to a sandboxed Python REPL environment.

The REPL is initialized with:
1. A `session` variable — a Python dict with the full session transcript.
   Keys: "title", "workspace", "messages" (list of {role, content, timestamp}).
   The session can be very large (100K+ chars). Do NOT try to read it all at once.

2. A `session_stats` variable — summary statistics (turn count, message count, etc).

3. An `llm_query(prompt)` function — calls a sub-LLM that can handle ~500K chars.
   Use this for semantic analysis of session chunks.

4. Standard Python built-ins, json, re, etc.

## Strategy

1. First inspect `session_stats` and `session["messages"][:5]` to understand the structure.
2. Develop a chunking strategy based on the session size and content.
3. For each chunk, use `llm_query()` to extract:
   - **Guiding principles**: Things that WORKED well (patterns, approaches, solutions).
   - **Cautionary principles**: Things that FAILED or caused problems (anti-patterns, mistakes).
4. Collect results in a buffer and synthesize.
5. Return the final learnings via FINAL() or FINAL_VAR().

## Learning Format

Each learning should have:
- `principle`: A clear, actionable rule (imperative mood)
- `kind`: "guiding" or "cautionary"
- `evidence`: Brief quote or reference from the session
- `confidence`: 0.0-1.0 based on how clearly the session supports this

## Code Execution

Write Python code in ```repl blocks to interact with the environment:

```repl
print(session_stats)
```

When done, return learnings via:

FINAL_VAR(learnings_json)

where `learnings_json` is a JSON string of the extracted learnings.
"""


EXTRACTION_PROMPT_GUIDING = """\
Analyze this section of an agent coding session and extract GUIDING PRINCIPLES —
things that worked well, good patterns, effective approaches.

Session context: {title}
Workspace: {workspace}

--- SESSION CHUNK ---
{chunk}
--- END CHUNK ---

For each guiding principle found, output a JSON array of objects:
[{{"principle": "...", "evidence": "brief quote or reference", "confidence": 0.0-1.0}}]

If no clear guiding principles are found, return: []
"""


EXTRACTION_PROMPT_CAUTIONARY = """\
Analyze this section of an agent coding session and extract CAUTIONARY PRINCIPLES —
things that failed, caused errors, wasted time, or were anti-patterns.
Look for: retries, error corrections, wrong assumptions, dead-end approaches.

Session context: {title}
Workspace: {workspace}

--- SESSION CHUNK ---
{chunk}
--- END CHUNK ---

For each cautionary principle found, output a JSON array of objects:
[{{"principle": "...", "evidence": "brief quote or reference", "confidence": 0.0-1.0}}]

If no clear cautionary principles are found, return: []
"""


SYNTHESIS_PROMPT = """\
You have extracted raw learnings from multiple chunks of an agent coding session.
Synthesize them into a final, deduplicated set of learnings.

Session: {title}
Workspace: {workspace}
Total turns: {turns}

--- RAW GUIDING PRINCIPLES ---
{guiding_raw}

--- RAW CAUTIONARY PRINCIPLES ---
{cautionary_raw}

Rules:
1. Deduplicate: merge similar principles into one, picking the strongest evidence.
2. Generalize: make principles applicable beyond this specific session where possible.
3. Be specific: vague principles ("be careful") are worthless. Include concrete details.
4. Score confidence based on how clearly the session demonstrates the principle.
5. Drop principles with confidence < 0.3.

Output a JSON object:
{{
  "learnings": [
    {{
      "principle": "...",
      "kind": "guiding" | "cautionary",
      "evidence": "...",
      "confidence": 0.0-1.0,
      "scope": "project" | "language" | "tool" | "general"
    }}
  ],
  "session_summary": "One paragraph summary of what happened in this session"
}}
"""


def next_action_prompt(query: str, iteration: int, final: bool = False) -> dict[str, str]:
    """Generate the per-iteration nudge prompt."""
    if final:
        return {
            "role": "user",
            "content": "Based on all the information gathered, provide your final learnings now. Use FINAL_VAR(learnings_json) with a JSON string.",
        }
    if iteration == 0:
        return {
            "role": "user",
            "content": (
                "You have not inspected the session yet. Start by examining session_stats "
                "and the first few messages. Plan your analysis strategy.\n\n"
                f"Your task: {query}"
            ),
        }
    return {
        "role": "user",
        "content": (
            "Continue your analysis based on previous REPL interactions.\n\n"
            f"Your task: {query}"
        ),
    }
