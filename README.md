![mmry banner](banner.png)

# mmry

`mmry` is a deterministic workspace memory ledger. Its only source of truth is an append-only `.mmry/mmry.jsonl` file in each repository. It makes no model calls and has no database, daemon, semantic index, ingestion pipeline, or global memory store.

## Use

```bash
mmry init                         # create an untracked workspace ledger
mmry init --tracked               # keep the ledger in git
mmry add "Run just fmt" --memory-type procedural --tags rust,workflow
mmry add -                        # read one memory from stdin
mmry list                         # alias: mmry ls
mmry search "rust fmt"
mmry rm mem_<id>                  # append a deprecation event
mmry doctor
```

List and search use wrapped, repository-attributed output for humans. Add `--plain` for stable tab-separated records or `--json` for structured output.

## Cross-repository reads

Configure bounded roots in the normal XDG config file (`~/.config/mmry/config.toml` on Unix):

```toml
[[roots]]
path = "~/byteowlz"
max_depth = 2

[[roots]]
path = "~/work"
max_depth = 2
```

Then use:

```bash
mmry repos
mmry list --repo trx
mmry search "release" --repo trx
mmry list --all
mmry search "release" --all
```

Discovery never searches the home directory unless explicitly configured. It is bounded, skips dependency/build/cache trees and does not follow symlinks. Reads are parallel, merged only in memory, and every result includes its repository name and canonical path in JSON. No global catalog or index is created.

## File format

Each line is a versioned event. Active memories are obtained by replaying additions and deprecations. Malformed lines are errors and are never silently skipped. Appends use an exclusive file lock and durable flush.

## Legacy SQLite migration

Back up both the database and target ledger first:

```bash
cp legacy.db legacy.db.backup
cp .mmry/mmry.jsonl .mmry/mmry.jsonl.backup 2>/dev/null || true
scripts/migrate_legacy_mmry_to_jsonl.py legacy.db --dry-run
scripts/migrate_legacy_mmry_to_jsonl.py legacy.db -o .mmry/mmry.jsonl
```

The migration reports fields that cannot be represented before writing. It exports every active supported memory by default and refuses unsupported schemas rather than silently dropping records. Restore the two backup files to roll back. The migration helper is transitional and is scheduled for removal in the next major release.

## Development

```bash
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

The 500-repository fixture prints its measured cold runtime during tests; no fixed speed claim is made because results depend on filesystem and machine.
