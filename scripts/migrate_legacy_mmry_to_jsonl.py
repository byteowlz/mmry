#!/usr/bin/env python3
"""Export legacy SQLite memories to an append-only workspace JSONL ledger."""

from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

MAPPED = {"id", "type", "content", "metadata", "tags", "created_at", "updated_at", "is_active", "deprecated_at"}


def parse_json(value: Any, default: Any) -> Any:
    if value is None:
        return default
    if isinstance(value, (dict, list)):
        return value
    try:
        return json.loads(value)
    except (TypeError, json.JSONDecodeError):
        return default


def active(row: sqlite3.Row) -> bool:
    keys = row.keys()
    return not (("is_active" in keys and not row["is_active"]) or ("deprecated_at" in keys and row["deprecated_at"] is not None))


def event_from_row(row: sqlite3.Row) -> dict[str, Any]:
    metadata = parse_json(row["metadata"] if "metadata" in row.keys() else None, {})
    if not isinstance(metadata, dict):
        metadata = {"legacy_metadata": row["metadata"]}
    extras = {key: row[key] for key in row.keys() if key not in MAPPED and row[key] is not None}
    if extras:
        metadata["legacy_fields"] = extras
    tags = parse_json(row["tags"] if "tags" in row.keys() else None, [])
    if not isinstance(tags, list):
        tags = [str(tags)]
    memory_id = str(row["id"])
    return {
        "schema_version": 1,
        "id": f"evt_{uuid.uuid4()}",
        "ts": row["created_at"] or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "type": "memory.add",
        "memory_id": memory_id if memory_id.startswith("mem_") else f"mem_{memory_id}",
        "content": row["content"],
        "memory_type": row["type"] or "semantic",
        "tags": tags,
        "metadata": metadata,
        "agent_ctx": metadata.pop("agent_ctx", {}),
    }


def migrate(db: Path, output: Path, *, dry_run: bool, append: bool) -> tuple[int, int]:
    connection = sqlite3.connect(db)
    connection.row_factory = sqlite3.Row
    tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "memories" not in tables:
        raise ValueError("unsupported legacy schema: missing memories table")
    columns = {row[1] for row in connection.execute("PRAGMA table_info(memories)")}
    missing = {"id", "content"} - columns
    if missing:
        raise ValueError(f"unsupported legacy schema: missing columns: {', '.join(sorted(missing))}")
    rows = connection.execute("SELECT * FROM memories ORDER BY created_at ASC" if "created_at" in columns else "SELECT * FROM memories ORDER BY id ASC").fetchall()
    events = [event_from_row(row) for row in rows if active(row)]
    inactive = len(rows) - len(events)
    extras = sorted(columns - MAPPED)
    print(f"active records: {len(events)}; inactive records: {inactive}", file=sys.stderr)
    if extras:
        print(f"preserved unmapped fields under metadata.legacy_fields: {', '.join(extras)}", file=sys.stderr)
    if dry_run:
        return len(events), inactive
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists() and not append:
        backup = output.with_suffix(output.suffix + ".backup")
        shutil.copy2(output, backup)
        print(f"backup: {backup}", file=sys.stderr)
    with output.open("a" if append else "w", encoding="utf-8") as handle:
        for event in events:
            handle.write(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"wrote: {output}", file=sys.stderr)
    return len(events), inactive


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("db", type=Path)
    parser.add_argument("-o", "--output", type=Path, default=Path(".mmry/mmry.jsonl"))
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--append", action="store_true")
    args = parser.parse_args()
    if not args.db.is_file():
        parser.error(f"database not found: {args.db}")
    try:
        migrate(args.db, args.output, dry_run=args.dry_run, append=args.append)
    except (sqlite3.Error, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
