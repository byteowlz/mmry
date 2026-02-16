"""Sandboxed Python REPL with session context and llm_query()."""

import io
import json
import os
import sys
import tempfile
import time
import threading
from contextlib import contextmanager
from dataclasses import dataclass
from typing import Optional

from rlm.llm_client import LLMClient
from rlm.session import Session


@dataclass
class REPLResult:
    stdout: str
    stderr: str
    locals_snapshot: dict[str, str]  # name -> type or short repr
    execution_time: float


class REPLEnv:
    """Sandboxed Python REPL that holds session data and provides llm_query()."""

    def __init__(
        self,
        session: Session,
        sub_llm: LLMClient,
        max_output_chars: int = 100_000,
    ):
        self.session = session
        self.sub_llm = sub_llm
        self.max_output_chars = max_output_chars
        self._lock = threading.Lock()

        # Temp directory for scratch files
        self.temp_dir = tempfile.mkdtemp(prefix="rlm_repl_")

        # Build the session dict that lives in the REPL
        session_dict = {
            "title": session.title,
            "workspace": session.workspace or "",
            "created": session.created or "",
            "updated": session.updated or "",
            "messages": [
                {
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp or "",
                }
                for m in session.messages
            ],
        }

        stats = session.summary_stats()

        # Build the execution namespace
        self.globals: dict = {
            "__builtins__": self._safe_builtins(),
        }
        self.locals: dict = {
            "session": session_dict,
            "session_stats": stats,
        }

        # Inject llm_query
        def llm_query(prompt: str) -> str:
            """Query a sub-LLM. Accepts a string prompt, returns the response."""
            return self.sub_llm.completion(prompt)

        self.globals["llm_query"] = llm_query

        # Inject FINAL_VAR
        def final_var(variable_name: str) -> str:
            """Return the value of a REPL variable as the final answer."""
            name = variable_name.strip().strip("\"'")
            if name in self.locals:
                return str(self.locals[name])
            return f"Error: Variable '{name}' not found in REPL"

        self.globals["FINAL_VAR"] = final_var

    def _safe_builtins(self) -> dict:
        """Curated set of safe Python builtins."""
        return {
            # Core types
            "print": print, "len": len, "str": str, "int": int, "float": float,
            "list": list, "dict": dict, "set": set, "tuple": tuple, "bool": bool,
            "type": type, "isinstance": isinstance, "issubclass": issubclass,
            # Iteration
            "enumerate": enumerate, "zip": zip, "map": map, "filter": filter,
            "sorted": sorted, "reversed": reversed, "range": range,
            "iter": iter, "next": next,
            # Math
            "min": min, "max": max, "sum": sum, "abs": abs, "round": round,
            "pow": pow, "divmod": divmod,
            # String
            "chr": chr, "ord": ord, "hex": hex, "bin": bin, "repr": repr,
            "format": format, "ascii": ascii,
            # Collections
            "any": any, "all": all, "hasattr": hasattr, "getattr": getattr,
            "setattr": setattr, "dir": dir, "vars": vars,
            "hash": hash, "id": id, "callable": callable,
            "bytes": bytes, "bytearray": bytearray,
            # IO
            "open": open,
            "__import__": __import__,
            # Exceptions
            "Exception": Exception, "ValueError": ValueError, "TypeError": TypeError,
            "KeyError": KeyError, "IndexError": IndexError, "AttributeError": AttributeError,
            "FileNotFoundError": FileNotFoundError, "RuntimeError": RuntimeError,
            "StopIteration": StopIteration,
            # OOP
            "super": super, "property": property,
            "staticmethod": staticmethod, "classmethod": classmethod, "object": object,
            # Blocked
            "input": None, "eval": None, "exec": None, "compile": None,
            "globals": None, "locals": None,
        }

    @contextmanager
    def _capture_output(self):
        """Thread-safe stdout/stderr capture."""
        with self._lock:
            old_out, old_err = sys.stdout, sys.stderr
            out_buf, err_buf = io.StringIO(), io.StringIO()
            try:
                sys.stdout, sys.stderr = out_buf, err_buf
                yield out_buf, err_buf
            finally:
                sys.stdout, sys.stderr = old_out, old_err

    @contextmanager
    def _temp_cwd(self):
        """Temporarily switch to the temp directory."""
        old = os.getcwd()
        try:
            os.chdir(self.temp_dir)
            yield
        finally:
            os.chdir(old)

    def execute(self, code: str) -> REPLResult:
        """Execute Python code in the sandbox, return captured output."""
        start = time.monotonic()

        with self._capture_output() as (out_buf, err_buf):
            with self._temp_cwd():
                try:
                    # Merge namespaces
                    combined = {**self.globals, **self.locals}

                    # Split imports from code so imports go into globals
                    lines = code.split("\n")
                    import_lines = []
                    code_lines = []
                    for line in lines:
                        stripped = line.lstrip()
                        if stripped.startswith(("import ", "from ")) and not stripped.startswith("#"):
                            import_lines.append(line)
                        else:
                            code_lines.append(line)

                    if import_lines:
                        exec("\n".join(import_lines), self.globals, self.globals)
                        # Refresh combined after imports
                        combined = {**self.globals, **self.locals}

                    if code_lines:
                        remaining = "\n".join(code_lines)
                        exec(remaining, combined, combined)

                    # Update locals with new variables
                    for key, value in combined.items():
                        if key not in self.globals:
                            self.locals[key] = value

                    stdout = out_buf.getvalue()
                    stderr = err_buf.getvalue()

                except Exception as e:
                    stdout = out_buf.getvalue()
                    stderr = err_buf.getvalue() + str(e)

        elapsed = time.monotonic() - start

        # Build a snapshot of locals for logging
        snapshot = {}
        for k, v in self.locals.items():
            if k.startswith("_"):
                continue
            try:
                r = repr(v)
                snapshot[k] = r[:120] + "..." if len(r) > 120 else r
            except Exception:
                snapshot[k] = f"<{type(v).__name__}>"

        return REPLResult(
            stdout=stdout[:self.max_output_chars],
            stderr=stderr[:self.max_output_chars],
            locals_snapshot=snapshot,
            execution_time=elapsed,
        )

    def cleanup(self):
        """Remove temp directory."""
        import shutil
        try:
            shutil.rmtree(self.temp_dir)
        except OSError:
            pass
