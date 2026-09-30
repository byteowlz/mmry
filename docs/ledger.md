# Ledger format, contested memories, preview and cleanup

## Events

Each line is a versioned event (`memory.add`, `memory.supersede`, `memory.deprecate`). Active memories are obtained by replaying events sorted by `(ts, id)`, so line order does not matter. A supersede keeps the memory id and increments its revision; `--expected-revision` rejects stale writers.

Edits record the revision they were made against. When ledgers from two machines are merged and an edit turns out to be based on an older revision (both machines superseded the same version, or one removed a version the other changed), the memory is **contested**: it stays visible, is marked `CONTESTED` / `"contested": true`, is never injected automatically, and is listed by `mmry doctor`. A `supersede` or `rm` on the current revision resolves it. There is no last-writer-wins. Damaged lines (e.g. a truncated write) and one event id with two payloads are reported by `mmry doctor [--all] [--json]` instead of making the ledger unreadable. Ordering uses event timestamps, so large clock skew between machines can misorder edits. Malformed lines are errors and are never silently skipped. Appends use an exclusive file lock and durable flush.

## Preview contract

```bash
mmry preview                      # exact text to inject at session start
mmry preview --json --cwd DIR --max-tokens 1200 --limit 20
```

`preview --json` (schema: `examples/preview.schema.json`) returns the selected `entries`, the exact `rendered` bytes to inject, a `selection_hash`, `omitted`, withheld `contested` memories and `warnings` (e.g. a pending migration). Selection is deterministic: repository memories before general ones, newest first; expired, contested and other-machine memories are left out; the budget is estimated at 4 bytes per token. The rendered text contains dates, not relative ages, so the hash only changes when the ledgers do. `preview` never prompts or migrates. `add`, `supersede`, `rm`, `list` and `search` print entries in one schema (`examples/memory.schema.json`) with `--json`. Harnesses should use this CLI contract, never read ledger files.

## Cleanup

```bash
mmry cleanup propose [--json] [--all|--repo NAME|--general]   # never writes
mmry cleanup apply prop_<id> [...] [--dry-run]
mmry cleanup apply --file proposals.json                         # from a cleanup agent; '-' for stdin
```

Built-in proposals are deterministic and model-free: exact duplicates (e.g. the same fact recorded on two machines), near-duplicates (token similarity >= 0.8; review both texts) and expired entries. Other tools, such as an agent or a model you chose, can write proposals in the same schema (`examples/cleanup.schema.json`; `id` may be empty). mmry sends nothing anywhere. Applying records ordinary supersede/deprecate events with the proposal as reason, and refuses when the memory changed since the proposal.
