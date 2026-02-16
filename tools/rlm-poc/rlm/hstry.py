"""hstry integration — load sessions directly from the hstry database."""

import json
import subprocess
from typing import Optional

from rlm.session import Session, load_markdown, load_json


def list_sessions(
    source: Optional[str] = None,
    limit: int = 20,
) -> list[dict]:
    """List available sessions from hstry."""
    cmd = ["hstry", "list", "--json"]
    if source:
        cmd.extend(["--source", source])

    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"hstry list failed: {result.stderr}")

    data = json.loads(result.stdout)
    convs = data.get("result", [])
    return convs[:limit]


def load_from_hstry(conversation_id: str) -> Session:
    """Export a session from hstry and parse it.

    Uses markdown export format.
    """
    cmd = [
        "hstry", "export",
        "-f", "markdown",
        "-c", conversation_id,
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"hstry export failed: {result.stderr}")

    text = result.stdout.strip()
    if not text or text == "No conversations found":
        raise ValueError(f"No conversation found with ID: {conversation_id}")

    session = load_markdown(text)
    # Override ID with the actual hstry ID
    session.id = conversation_id
    return session


def search_sessions(query: str, limit: int = 10) -> list[dict]:
    """Search hstry for sessions matching a query."""
    cmd = ["hstry", "search", query, "--json"]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"hstry search failed: {result.stderr}")

    data = json.loads(result.stdout)
    results = data.get("result", [])
    return results[:limit]
