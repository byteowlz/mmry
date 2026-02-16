"""RLM Proof of Concept — Entry point.

Usage:
    uv run main.py <session_file_or_->  [--query Q] [--verbose] [--max-iter N]
    uv run main.py --eval <session_file> --annotations <file.json>

    hstry export -f markdown -c <id> | uv run main.py -
"""

import argparse
import json
import sys

from rlm.session import load_session
from rlm.rlm_loop import RLMLoop


def main():
    parser = argparse.ArgumentParser(
        description="RLM Session Analyzer — extract learnings from agent sessions",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Examples:\n"
            "  hstry export -f markdown -c <id> | uv run main.py -\n"
            "  uv run main.py /tmp/session.md --verbose\n"
            "  uv run main.py /tmp/session.md --eval --annotations gold.json\n"
        ),
    )
    parser.add_argument(
        "session",
        help="Session file path, or '-' for stdin",
    )
    parser.add_argument(
        "--query", "-q",
        default="Extract actionable learnings (guiding and cautionary principles) from this agent coding session.",
        help="Query for the RLM to answer",
    )
    parser.add_argument(
        "--max-iter", "-n",
        type=int,
        default=15,
        help="Maximum RLM iterations (default: 15)",
    )
    parser.add_argument(
        "--root-model",
        default=None,
        help="Root LLM model name (default: $RLM_MODEL or gpt-4o-mini)",
    )
    parser.add_argument(
        "--sub-model",
        default=None,
        help="Sub-LLM model name (default: $RLM_SUB_MODEL or gpt-4o-mini)",
    )
    parser.add_argument(
        "--base-url",
        default=None,
        help="OpenAI-compatible API base URL (default: $RLM_API_URL)",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Print debug info to stderr",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="json_output",
        help="Output full result as JSON (including steps and usage)",
    )
    parser.add_argument(
        "--eval",
        action="store_true",
        help="Evaluate against manual annotations",
    )
    parser.add_argument(
        "--annotations",
        default=None,
        help="Path to annotation JSON file (for --eval mode)",
    )

    args = parser.parse_args()

    # Load session
    if args.verbose:
        print("Loading session...", file=sys.stderr)

    session = load_session(args.session)

    if args.verbose:
        stats = session.summary_stats()
        print(f"Session: {stats}", file=sys.stderr)

    # Run RLM
    rlm = RLMLoop(
        session=session,
        root_model=args.root_model,
        sub_model=args.sub_model,
        base_url=args.base_url,
        max_iterations=args.max_iter,
        verbose=args.verbose,
    )

    result = rlm.run(args.query)

    # Output
    if args.json_output:
        output = {
            "answer": result.answer,
            "iterations": result.total_iterations,
            "root_usage": result.root_usage,
            "sub_usage": result.sub_usage,
            "steps": [
                {
                    "iteration": s.iteration,
                    "code_blocks": s.code_blocks,
                    "has_final": s.has_final,
                }
                for s in result.steps
            ],
        }
        print(json.dumps(output, indent=2))
    else:
        print(result.answer)

    # Evaluation mode
    if args.eval:
        if not args.annotations:
            print("Error: --eval requires --annotations <file.json>", file=sys.stderr)
            sys.exit(1)

        from eval import evaluate, load_annotations

        try:
            predicted = json.loads(result.answer)
            if isinstance(predicted, dict) and "learnings" in predicted:
                predicted = predicted["learnings"]
        except json.JSONDecodeError:
            print("Error: RLM answer is not valid JSON, cannot evaluate", file=sys.stderr)
            sys.exit(1)

        expected = load_annotations(args.annotations)
        metrics = evaluate(predicted, expected)

        print("\n--- Evaluation ---", file=sys.stderr)
        print(f"Precision: {metrics.precision}", file=sys.stderr)
        print(f"Recall:    {metrics.recall}", file=sys.stderr)
        print(f"F1:        {metrics.f1}", file=sys.stderr)
        print(f"Matched:   {metrics.matched}/{metrics.total_predicted} predicted, "
              f"{metrics.matched}/{metrics.total_expected} expected", file=sys.stderr)

        if args.json_output:
            eval_output = {
                "precision": metrics.precision,
                "recall": metrics.recall,
                "f1": metrics.f1,
                "matched": metrics.matched,
                "total_predicted": metrics.total_predicted,
                "total_expected": metrics.total_expected,
                "details": metrics.details,
            }
            print(json.dumps(eval_output, indent=2))

    # Print usage summary
    if args.verbose:
        print(f"\nRoot LLM usage: {result.root_usage}", file=sys.stderr)
        print(f"Sub LLM usage:  {result.sub_usage}", file=sys.stderr)


if __name__ == "__main__":
    main()
