# Storage and migration


By default memories live in a per-user central store (`store_root`, default `$XDG_DATA_HOME/mmry`, i.e. `~/.local/share/mmry`):

```text
general/mmry.jsonl              personal memories that apply everywhere
repos/<name>--<id>/mmry.jsonl   one ledger per repository
repos/<name>--<id>/repo.json    identity (git root commit, else path) + name; never rewritten
local/checkouts.json            this machine's checkout paths (not synced)
```

Repositories are identified by their git root commit, not their directory: clones and worktrees share a ledger, while `~/work/app` and `~/byteowlz/app` from unrelated histories get different `app--<id>` directories. The `<id>` comes from the identity, so every machine picks the same directory name. If two machines checked out one repository under different names, the directories are merged by event id on next use. Repositories without git fall back to their path, which is machine-specific.

A repository is in exactly one mode:

- **central** (default): `mmry init` registers it; nothing is written into the repository.
- **tracked**: `mmry init --tracked` keeps the ledger in `.mmry/mmry.jsonl` to commit with the repository. A ledger already committed to git is treated as tracked too.

## Switching to the central store

```bash
mmry setup --dry-run              # scan home (+ [[roots]]) for .mmry ledgers and show the plan
mmry setup                        # confirm, migrate them all, set migrate = "auto" in the config
mmry setup --scan ~/work --scan /data --depth 8
```

Until you decide, an untracked `.mmry/mmry.jsonl` is not read (`migrate = "prompt"`, the default: ask once on a terminal, otherwise warn and continue). `"auto"` migrates on first use, `"off"` only warns. The config file is only edited by `mmry setup`, keeping its comments. Per repository:

```bash
mmry migrate --dry-run            # current repository; --all for every repo under [[roots]]
mmry migrate --untrack            # also move a git-committed ledger (leaves `git rm --cached` uncommitted)
```

Events are merged by id (re-running is a no-op), the central ledger is verified to contain every active local memory, and only then is the local file renamed to `.mmry/mmry.jsonl.migrated-<timestamp>` (kept as backup) with a `.mmry/MIGRATED` note. An event id present in both ledgers with different content aborts the migration with nothing moved.

## Legacy SQLite migration

Back up both the database and target ledger first:

```bash
cp legacy.db legacy.db.backup
cp .mmry/mmry.jsonl .mmry/mmry.jsonl.backup 2>/dev/null || true
scripts/migrate_legacy_mmry_to_jsonl.py legacy.db --dry-run
scripts/migrate_legacy_mmry_to_jsonl.py legacy.db -o .mmry/mmry.jsonl
```

The migration reports fields that cannot be represented before writing. It exports every active supported memory by default and refuses unsupported schemas rather than silently dropping records. Restore the two backup files to roll back. The migration helper is transitional and is scheduled for removal in the next major release.
