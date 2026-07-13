#!/usr/bin/env python3
import importlib.util
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("migration", Path(__file__).with_name("migrate_legacy_mmry_to_jsonl.py"))
MIGRATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MIGRATION)


class MigrationTest(unittest.TestCase):
    def test_exports_active_rows_and_preserves_unmapped_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database = root / "legacy.db"
            connection = sqlite3.connect(database)
            connection.execute("CREATE TABLE memories (id TEXT, content TEXT, type TEXT, tags TEXT, metadata TEXT, created_at TEXT, is_active INTEGER, importance INTEGER)")
            connection.execute("INSERT INTO memories VALUES ('a', 'keep me', 'procedural', '[\"rust\"]', '{\"agent_ctx\":{\"harness\":\"pi\"}}', '2026-01-01T00:00:00Z', 1, 9)")
            connection.execute("INSERT INTO memories VALUES ('b', 'inactive', 'semantic', '[]', '{}', '2026-01-02T00:00:00Z', 0, 1)")
            connection.commit()
            output = root / ".mmry/mmry.jsonl"
            self.assertEqual(MIGRATION.migrate(database, output, dry_run=False, append=False), (1, 1))
            event = json.loads(output.read_text())
            self.assertEqual(event["content"], "keep me")
            self.assertEqual(event["agent_ctx"], {"harness": "pi"})
            self.assertEqual(event["metadata"]["legacy_fields"]["importance"], 9)

    def test_rejects_unsupported_schema(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "legacy.db"
            connection = sqlite3.connect(database)
            connection.execute("CREATE TABLE other (id TEXT)")
            connection.commit()
            with self.assertRaisesRegex(ValueError, "missing memories table"):
                MIGRATION.migrate(database, Path(directory) / "out", dry_run=True, append=False)


if __name__ == "__main__":
    unittest.main()
