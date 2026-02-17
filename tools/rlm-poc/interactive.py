"""Interactive RLM REPL — for hands-on experimentation with sessions.

Usage:
    uv run interactive.py <session_file_or_->
    hstry export -f markdown -c <id> | uv run interactive.py -

Commands:
    /stats          Show session stats
    /vars           Show REPL variables
    /llm <prompt>   Direct llm_query call
    /run            Run the full RLM loop automatically
    /help           Show this help
    /quit           Exit

Any other input is executed as Python code in the REPL.
"""

import argparse
import json
import os
import readline
import sys

from rlm.session import load_session
from rlm.repl import REPLEnv
from rlm.llm_client import LLMClient
from rlm.rlm_loop import RLMLoop
from rlm.hstry import list_sessions, load_from_hstry


BANNER = """\
RLM Interactive REPL
Session loaded. Type Python code, or /help for commands.
Available: session, session_stats, llm_query(prompt)
"""

HELP = """\
Commands:
  /stats              Show session summary statistics
  /vars               Show current REPL variables
  /messages [N]       Show last N messages (default: 10)
  /llm <prompt>       Call llm_query() directly
  /run [query]        Run full RLM loop with optional query
  /export [file]      Export REPL locals to JSON file
  /sessions [N]       List recent hstry sessions
  /load <id>          Load a different session from hstry by ID
  /help               Show this help
  /quit               Exit

Anything else is executed as Python code in the sandboxed REPL.
Use llm_query("prompt") in code for recursive LLM calls.
"""


def interactive(session_source: str, base_url=None, model=None, sub_model=None):
    session = load_session(session_source)

    api_key = os.getenv("OPENAI_API_KEY", "no-key-needed")
    base_url = base_url or os.getenv("RLM_API_URL")
    model = model or os.getenv("RLM_MODEL", "gpt-4o-mini")
    sub_model = sub_model or os.getenv("RLM_SUB_MODEL", model)

    sub_llm = LLMClient(base_url=base_url, api_key=api_key, model=sub_model)
    repl = REPLEnv(session=session, sub_llm=sub_llm)

    print(BANNER)
    stats = session.summary_stats()
    print(f"  Title:    {session.title}")
    print(f"  Messages: {stats['messages']} ({stats['user_msgs']} user, "
          f"{stats['assistant_msgs']} assistant, {stats['tool_msgs']} tool)")
    print(f"  Chars:    {stats['total_chars']:,}")
    print(f"  Model:    {model} (sub: {sub_model})")
    print()

    # Set up readline history
    histfile = os.path.expanduser("~/.rlm_repl_history")
    try:
        readline.read_history_file(histfile)
    except FileNotFoundError:
        pass
    readline.set_history_length(1000)

    try:
        while True:
            try:
                line = input("rlm> ").strip()
            except EOFError:
                print()
                break

            if not line:
                continue

            if line == "/quit" or line == "/exit":
                break

            if line == "/help":
                print(HELP)
                continue

            if line == "/stats":
                for k, v in stats.items():
                    print(f"  {k}: {v}")
                continue

            if line == "/vars":
                for k, v in repl.locals.items():
                    if k.startswith("_"):
                        continue
                    r = repr(v)
                    if len(r) > 100:
                        r = r[:100] + "..."
                    print(f"  {k}: {r}")
                continue

            if line.startswith("/messages"):
                parts = line.split()
                n = int(parts[1]) if len(parts) > 1 else 10
                msgs = session.messages[-n:]
                for m in msgs:
                    content = m.content[:80].replace("\n", " ")
                    print(f"  [{m.role}] {content}")
                continue

            if line.startswith("/llm "):
                prompt = line[5:].strip()
                if not prompt:
                    print("Usage: /llm <prompt>")
                    continue
                print("Querying LLM...")
                try:
                    response = sub_llm.completion(prompt)
                    print(response)
                except Exception as e:
                    print(f"Error: {e}", file=sys.stderr)
                continue

            if line.startswith("/run"):
                query = line[4:].strip() or (
                    "Extract actionable learnings (guiding and cautionary principles) "
                    "from this agent coding session."
                )
                print(f"Running RLM loop: {query[:60]}...")
                rlm = RLMLoop(
                    session=session,
                    root_model=model,
                    sub_model=sub_model,
                    base_url=base_url,
                    verbose=True,
                )
                result = rlm.run(query)
                print(f"\n--- Result ({result.total_iterations} iterations) ---")
                print(result.answer)
                print(f"\nRoot usage: {result.root_usage}")
                print(f"Sub usage:  {result.sub_usage}")
                continue

            if line.startswith("/sessions"):
                parts = line.split()
                n = int(parts[1]) if len(parts) > 1 else 10
                try:
                    sessions = list_sessions(limit=n)
                    for s in sessions:
                        sid = s.get("id", "?")[:8]
                        title = s.get("title", "?")[:50]
                        print(f"  {sid}  {title}")
                except Exception as e:
                    print(f"Error: {e}", file=sys.stderr)
                continue

            if line.startswith("/load "):
                conv_id = line[6:].strip()
                if not conv_id:
                    print("Usage: /load <conversation-id>")
                    continue
                try:
                    print(f"Loading session {conv_id}...")
                    session = load_from_hstry(conv_id)
                    stats = session.summary_stats()
                    # Rebuild REPL with new session
                    repl.cleanup()
                    repl = REPLEnv(session=session, sub_llm=sub_llm)
                    print(f"  Loaded: {session.title}")
                    print(f"  Messages: {stats['messages']}, Chars: {stats['total_chars']:,}")
                except Exception as e:
                    print(f"Error: {e}", file=sys.stderr)
                continue

            if line.startswith("/export"):
                parts = line.split()
                path = parts[1] if len(parts) > 1 else "/tmp/rlm_export.json"
                exportable = {}
                for k, v in repl.locals.items():
                    if k.startswith("_"):
                        continue
                    try:
                        json.dumps(v)
                        exportable[k] = v
                    except (TypeError, ValueError):
                        exportable[k] = repr(v)
                with open(path, "w") as f:
                    json.dump(exportable, f, indent=2)
                print(f"Exported {len(exportable)} variables to {path}")
                continue

            # Multi-line input: if line ends with ":", keep reading
            code = line
            if line.endswith(":") or line.endswith("\\"):
                buffer = [line.rstrip("\\")]
                while True:
                    try:
                        cont = input("...  ")
                    except EOFError:
                        break
                    if cont.strip() == "":
                        break
                    buffer.append(cont)
                code = "\n".join(buffer)

            # Execute as Python
            result = repl.execute(code)
            if result.stdout:
                print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
            if result.stderr:
                print(f"[error] {result.stderr}", file=sys.stderr)

    finally:
        readline.write_history_file(histfile)
        repl.cleanup()
        print("Goodbye.")


def main():
    parser = argparse.ArgumentParser(
        description="Interactive RLM REPL for session analysis",
    )
    parser.add_argument("session", help="Session file or '-' for stdin")
    parser.add_argument("--base-url", default=None, help="OpenAI-compatible API base URL")
    parser.add_argument("--model", default=None, help="Root model name")
    parser.add_argument("--sub-model", default=None, help="Sub-LLM model name")
    args = parser.parse_args()

    interactive(
        args.session,
        base_url=args.base_url,
        model=args.model,
        sub_model=args.sub_model,
    )


if __name__ == "__main__":
    main()
