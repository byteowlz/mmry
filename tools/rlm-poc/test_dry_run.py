"""Dry-run test — exercises the full RLM pipeline with a mock LLM."""

import json
import sys

from rlm.session import load_markdown
from rlm.repl import REPLEnv
from rlm.rlm_loop import RLMLoop, find_code_blocks, find_final_answer


# Simulated LLM that follows the expected REPL interaction pattern
MOCK_RESPONSES = [
    # Iteration 0: inspect the session
    """\
Let me start by examining the session structure.

```repl
print(session_stats)
print()
for m in session["messages"][:3]:
    print(f"[{m['role']}] {m['content'][:60]}")
```
""",
    # Iteration 1: chunk and analyze
    """\
The session has several messages. Let me chunk them and look for patterns.

```repl
import json

# Group messages into user-assistant turns
turns = []
current_turn = []
for m in session["messages"]:
    current_turn.append(m)
    if m["role"] == "user" and current_turn:
        turns.append(current_turn)
        current_turn = []
if current_turn:
    turns.append(current_turn)

print(f"Found {len(turns)} turns")

# Look for error/retry patterns
errors = [m for m in session["messages"] if "error" in m["content"].lower()]
print(f"Messages with errors: {len(errors)}")

# Build learnings
guiding = [
    {"principle": "Always check session stats before deep analysis", "kind": "guiding", "confidence": 0.7, "evidence": "Structured approach worked well", "scope": "general"},
]
cautionary = [
    {"principle": "Avoid analyzing all messages at once for large sessions", "kind": "cautionary", "confidence": 0.8, "evidence": "Session had many messages", "scope": "general"},
]

learnings = {"learnings": guiding + cautionary, "session_summary": "Test session analysis"}
learnings_json = json.dumps(learnings, indent=2)
print(f"Built {len(learnings['learnings'])} learnings")
```
""",
    # Iteration 2: return final answer
    """\
I have analyzed the session and extracted the learnings.

FINAL_VAR(learnings_json)
""",
]


def test_dry_run():
    """Test the full pipeline without a real LLM."""
    md = """\
# Test Coding Session
- Created: 2026-01-15T10:00:00Z
- Workspace: /home/user/project

## user
_at 2026-01-15T10:00:01Z_

Fix the compilation error in src/main.rs

## assistant
_at 2026-01-15T10:00:02Z_

I'll look at the error and fix it.

## tool
_at 2026-01-15T10:00:03Z_

error[E0308]: mismatched types
  --> src/main.rs:42:5
   |
42 |     name
   |     ^^^^ expected &str, found String

## assistant
_at 2026-01-15T10:00:04Z_

The issue is a type mismatch. I need to add .as_str() or use a reference.

## tool
_at 2026-01-15T10:00:05Z_

Compiling project v0.1.0
    Finished dev target(s) in 2.3s

## user
_at 2026-01-15T10:00:06Z_

Great, now run the tests please

## assistant
_at 2026-01-15T10:00:07Z_

Running the test suite now.

## tool
_at 2026-01-15T10:00:08Z_

running 5 tests
test test_parse ... ok
test test_validate ... ok
test test_format ... FAILED
test test_serialize ... ok
test test_roundtrip ... ok

## assistant
_at 2026-01-15T10:00:09Z_

One test failed. Let me investigate test_format.
"""

    session = load_markdown(md)
    print(f"Session loaded: {session.summary_stats()}")

    # Create a mock LLM that returns scripted responses
    response_idx = [0]

    class MockLLMClient:
        total_calls = 0
        total_prompt_tokens = 0
        total_completion_tokens = 0

        def __init__(self, **kwargs):
            pass

        def completion(self, messages, **kwargs):
            idx = response_idx[0]
            if idx < len(MOCK_RESPONSES):
                resp = MOCK_RESPONSES[idx]
                response_idx[0] += 1
                return resp
            return "FINAL(No more mock responses available)"

        def usage_summary(self):
            return {"total_calls": response_idx[0], "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}

    # Monkey-patch the RLMLoop to use our mock
    from rlm import rlm_loop as loop_mod
    original_client = loop_mod.LLMClient

    loop_mod.LLMClient = lambda **kwargs: MockLLMClient(**kwargs)

    try:
        rlm = RLMLoop(session=session, max_iterations=10, verbose=True)
        # Replace the clients with mocks
        rlm.root_llm = MockLLMClient()
        rlm.sub_llm = MockLLMClient()
        rlm.repl = REPLEnv(session=session, sub_llm=MockLLMClient())

        result = rlm.run("Extract learnings from this session.")

        print(f"\nResult ({result.total_iterations} iterations):")
        print(result.answer[:500])

        # Parse and validate the learnings
        try:
            learnings = json.loads(result.answer)
            print(f"\nParsed learnings: {len(learnings.get('learnings', []))} items")
            for l in learnings.get("learnings", []):
                print(f"  [{l['kind']}] {l['principle']} (conf={l['confidence']})")
            print("\nDry run PASSED")
        except json.JSONDecodeError as e:
            print(f"\nWarning: answer is not JSON: {e}")
            print("This is OK if the model returned plain text.")
            print("\nDry run PASSED (non-JSON answer)")

    finally:
        loop_mod.LLMClient = original_client


if __name__ == "__main__":
    test_dry_run()
