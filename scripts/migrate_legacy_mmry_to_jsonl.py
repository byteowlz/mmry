#!/usr/bin/env python3
"""Migrate legacy mmry SQLite memories to lean .mmry/mmry.jsonl.

The script intentionally filters bulky hstry/session-ingest records by default.
It writes append-only `memory.add` events compatible with the lean JSONL format.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SESSION_TAGS = {"hstry-session", "hstry-message", "hstry-message-chunk"}
SESSION_TAG_PREFIXES = ("conv:", "harness:", "source:")
SESSION_METADATA_KEYS = {"conv_id", "external_id", "msg_index", "role", "message_count"}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def parse_json(value: Any, default: Any) -> Any:
    if value is None:
        return default
    if isinstance(value, (dict, list)):
        return value
    try:
        return json.loads(value)
    except Exception:
        return default


def is_session_dump(row: sqlite3.Row, tags: list[str], metadata: dict[str, Any], max_chars: int) -> tuple[bool, str | None]:
    content = row["content"] or ""
    if len(content) > max_chars:
        return True, f"content>{max_chars}"
    if any(tag in SESSION_TAGS for tag in tags):
        return True, "hstry tag"
    if any(tag.startswith(SESSION_TAG_PREFIXES) for tag in tags):
        return True, "conversation/source tag"
    if any(key in metadata for key in SESSION_METADATA_KEYS):
        return True, "conversation metadata"
    if row["parent_id"] is not None or row["chunk_index"] is not None or row["total_chunks"] is not None:
        return True, "chunk/session hierarchy"
    return False, None


def event_from_row(row: sqlite3.Row) -> dict[str, Any]:
    tags = parse_json(row["tags"], [])
    metadata = parse_json(row["metadata"], {})
    agent_ctx = metadata.pop("agent_ctx", {}) if isinstance(metadata, dict) else {}
    ts = row["created_at"] or utc_now()
    return {
        "schema_version": 1,
        "id": f"evt_{uuid.uuid4()}",
        "ts": ts,
        "type": "memory.add",
        "memory_id": f"mem_{row['id']}",
        "content": row["content"],
        "memory_type": row["type"] or "semantic",
        "tags": tags if isinstance(tags, list) else [],
        "metadata": metadata if isinstance(metadata, dict) else {},
        "agent_ctx": agent_ctx if isinstance(agent_ctx, dict) else {},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Migrate legacy mmry SQLite DB to .mmry/mmry.jsonl")
    parser.add_argument("db", type=Path, help="Path to legacy mmry SQLite database")
    parser.add_argument("-o", "--output", type=Path, default=Path(".mmry/mmry.jsonl"), help="Output JSONL path")
    parser.add_argument("--include-sessions", action="store_true", help="Do not filter hstry/session/chunk records")
    parser.add_argument("--max-content-chars", type=int, default=2000, help="Filter records longer than this unless --include-sessions")
    parser.add_argument("--store", help="Only migrate rows from this legacy store")
    parser.add_argument("--dry-run", action="store_true", help="Report counts without writing")
    parser.add_argument("--append", action="store_true", help="Append to output instead of replacing it")
    args = parser.parse_args()

    if not args.db.exists():
        parser.error(f"database not found: {args.db}")

    conn = sqlite3.connect(args.db)
    conn.row_factory = sqlite3.Row
    where = ""
    params: list[Any] = []
    if args.store:
        where = "WHERE store = ?"
        params.append(args.store)
    rows = conn.execute(
        f"""
        SELECT id, type, content, metadata, tags, created_at, parent_id, chunk_index, total_chunks, store
        FROM memories
        {where}
        ORDER BY created_at ASC
        """,
        params,
    ).fetchall()

    events: list[dict[str, Any]] = []
    skipped: dict[str, int] = {}
    for row in rows:
        tags = parse_json(row["tags"], [])
        tags = tags if isinstance(tags, list) else []
        metadata = parse_json(row["metadata"], {})
        metadata = metadata if isinstance(metadata, dict) else {}
        if not args.include_sessions:
            skip, reason = is_session_dump(row, tags, metadata, args.max_content_chars)
            if skip:
                skipped[reason or "filtered"] = skipped.get(reason or "filtered", 0) + 1
                continue
        events.append(event_from_row(row))

    print(f"read: {len(rows)}", file=sys.stderr)
    print(f"write: {len(events)}", file=sys.stderr)
    if skipped:
        print("skipped:", file=sys.stderr)
        for reason, count in sorted(skipped.items()):
            print(f"  {reason}: {count}", file=sys.stderr)

    if args.dry_run:
        return 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    mode = "a" if args.append else "w"
    with args.output.open(mode, encoding="utf-8") as f:
        for event in events:
            f.write(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"wrote {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
