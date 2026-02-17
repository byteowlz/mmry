# RLM Proof of Concept — Session Analysis for mmry Learnings

Standalone proof-of-concept for using Recursive Language Models to analyze
agent coding sessions and extract/validate learnings.

## What This Does

Given a completed agent session (exported from hstry), the RLM:
1. Loads the session transcript as an external REPL variable (not in LLM context)
2. Programmatically examines the session structure (turns, tools, outcomes)
3. Recursively calls a sub-LLM over chunks to extract learnings
4. Validates extracted learnings against the session evidence

## Setup

```bash
uv sync
```

## Configuration

Set the endpoint for your local Qwen model (OpenAI-compatible API):

```bash
export RLM_API_URL="http://localhost:8080/v1"   # llama.cpp / vLLM / ollama
export RLM_MODEL="qwen3-8b"                      # or whatever your model is named
export RLM_SUB_MODEL="qwen3-8b"                  # model for recursive sub-calls
```

Or use a cloud model:

```bash
export OPENAI_API_KEY="sk-..."
export RLM_MODEL="gpt-4o-mini"
export RLM_SUB_MODEL="gpt-4o-mini"
```

## Usage

```bash
# Export a session from hstry and analyze it
hstry export -f markdown -c <conversation-id> > /tmp/session.md
uv run main.py /tmp/session.md

# Or pipe directly
hstry export -f markdown -c <conversation-id> | uv run main.py -

# With a specific query
uv run main.py /tmp/session.md --query "What learnings can be extracted from this session?"

# Evaluate against manual annotations
uv run main.py /tmp/session.md --annotations /tmp/annotations.json --eval
```

## Architecture

```
main.py          — Entry point, CLI
rlm/
  repl.py        — Sandboxed Python REPL with llm_query() and session context
  rlm_loop.py    — The RLM iterative loop (root LLM + REPL interaction)
  llm_client.py  — OpenAI-compatible client (works with local or cloud models)
  prompts.py     — System prompts for learning extraction
  session.py     — Session loader/parser (hstry markdown/json formats)
eval/
  harness.py     — Evaluation against manual annotations
  metrics.py     — Agreement metrics (precision, recall, F1)
```
