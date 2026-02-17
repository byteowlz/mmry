"""The RLM iterative loop — root LLM interacts with REPL to analyze sessions."""

import json
import os
import re
import sys
from dataclasses import dataclass, field
from typing import Optional

from rlm.llm_client import LLMClient
from rlm.repl import REPLEnv
from rlm.session import Session
from rlm.prompts import SYSTEM_PROMPT, next_action_prompt


@dataclass
class RLMStep:
    """One iteration of the RLM loop."""
    iteration: int
    response: str
    code_blocks: list[str]
    execution_results: list[str]
    has_final: bool = False
    final_answer: Optional[str] = None


@dataclass
class RLMResult:
    """Complete result of an RLM run."""
    answer: str
    steps: list[RLMStep] = field(default_factory=list)
    total_iterations: int = 0
    root_usage: dict = field(default_factory=dict)
    sub_usage: dict = field(default_factory=dict)


def find_code_blocks(text: str) -> list[str]:
    """Extract ```repl code blocks from LLM response."""
    pattern = r"```repl\s*\n(.*?)\n```"
    return [m.group(1).strip() for m in re.finditer(pattern, text, re.DOTALL)]


def find_final_answer(text: str) -> Optional[tuple[str, str]]:
    """Look for FINAL(...) or FINAL_VAR(...) in response.

    Returns (type, content) or None.
    """
    # FINAL_VAR first (more specific)
    m = re.search(r"^\s*FINAL_VAR\((.+?)\)", text, re.MULTILINE | re.DOTALL)
    if m:
        return ("FINAL_VAR", m.group(1).strip())

    m = re.search(r"^\s*FINAL\((.+?)\)", text, re.MULTILINE | re.DOTALL)
    if m:
        return ("FINAL", m.group(1).strip())

    return None


def format_repl_output(stdout: str, stderr: str, snapshot: dict[str, str]) -> str:
    """Format REPL execution result for the LLM."""
    parts = []
    if stdout:
        parts.append(stdout)
    if stderr:
        parts.append(f"[stderr] {stderr}")

    # Show variable names (not full values — that would blow up context)
    var_names = [k for k in snapshot if k not in ("session", "session_stats")]
    if var_names:
        parts.append(f"REPL variables: {var_names}")

    return "\n\n".join(parts) if parts else "(no output)"


class RLMLoop:
    """Iterative RLM loop: root LLM generates code, REPL executes, repeat."""

    def __init__(
        self,
        session: Session,
        root_model: Optional[str] = None,
        sub_model: Optional[str] = None,
        base_url: Optional[str] = None,
        api_key: Optional[str] = None,
        max_iterations: int = 15,
        verbose: bool = False,
    ):
        self.session = session
        self.max_iterations = max_iterations
        self.verbose = verbose

        base_url = base_url or os.getenv("RLM_API_URL")
        api_key = api_key or os.getenv("OPENAI_API_KEY")

        self.root_llm = LLMClient(
            base_url=base_url,
            api_key=api_key,
            model=root_model or os.getenv("RLM_MODEL", "gpt-4o-mini"),
        )
        self.sub_llm = LLMClient(
            base_url=base_url,
            api_key=api_key,
            model=sub_model or os.getenv("RLM_SUB_MODEL", "gpt-4o-mini"),
        )

        self.repl = REPLEnv(session=session, sub_llm=self.sub_llm)

    def _log(self, *args):
        if self.verbose:
            print(*args, file=sys.stderr)

    def run(self, query: str) -> RLMResult:
        """Execute the RLM loop until FINAL or max iterations."""
        messages: list[dict[str, str]] = [
            {"role": "system", "content": SYSTEM_PROMPT},
        ]

        steps: list[RLMStep] = []

        for iteration in range(self.max_iterations):
            # Add the per-iteration nudge
            nudge = next_action_prompt(query, iteration)
            request_messages = messages + [nudge]

            self._log(f"\n--- Iteration {iteration} ---")

            # Query root LLM
            response = self.root_llm.completion(request_messages)
            self._log(f"Root LLM response ({len(response)} chars)")

            # Check for code blocks
            code_blocks = find_code_blocks(response)
            execution_results: list[str] = []

            if code_blocks:
                for code in code_blocks:
                    self._log(f"Executing code:\n{code[:200]}...")
                    result = self.repl.execute(code)
                    formatted = format_repl_output(
                        result.stdout, result.stderr, result.locals_snapshot,
                    )
                    execution_results.append(formatted)
                    self._log(f"Result: {formatted[:300]}...")

                    # Add code + result to conversation
                    messages.append({
                        "role": "assistant",
                        "content": response,
                    })
                    messages.append({
                        "role": "user",
                        "content": f"Code executed:\n```python\n{code}\n```\n\nREPL output:\n{formatted}",
                    })
            else:
                messages.append({
                    "role": "assistant",
                    "content": response,
                })

            # Check for final answer
            final = find_final_answer(response)
            step = RLMStep(
                iteration=iteration,
                response=response,
                code_blocks=code_blocks,
                execution_results=execution_results,
            )

            if final is not None:
                answer_type, content = final
                if answer_type == "FINAL_VAR":
                    var_name = content.strip().strip("\"'")
                    if var_name in self.repl.locals:
                        step.final_answer = str(self.repl.locals[var_name])
                    else:
                        self._log(f"FINAL_VAR: variable '{var_name}' not found")
                        step.final_answer = None
                else:
                    step.final_answer = content

                if step.final_answer is not None:
                    step.has_final = True
                    steps.append(step)
                    self._log(f"Final answer found at iteration {iteration}")
                    break

            steps.append(step)

        else:
            # Max iterations reached — force a final answer
            self._log("Max iterations reached, forcing final answer")
            force_nudge = next_action_prompt(query, self.max_iterations, final=True)
            response = self.root_llm.completion(messages + [force_nudge])
            step = RLMStep(
                iteration=self.max_iterations,
                response=response,
                code_blocks=[],
                execution_results=[],
                has_final=True,
                final_answer=response,
            )
            steps.append(step)

        # Build result
        answer = ""
        for s in reversed(steps):
            if s.final_answer is not None:
                answer = s.final_answer
                break

        self.repl.cleanup()

        return RLMResult(
            answer=answer,
            steps=steps,
            total_iterations=len(steps),
            root_usage=self.root_llm.usage_summary(),
            sub_usage=self.sub_llm.usage_summary(),
        )
