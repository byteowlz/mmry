![mmry banner](banner.png)

# mmry

`mmry` is a deterministic, personal operative-memory ledger: short observations ("in this repo, command X needs flag Y") that stay useful for weeks until corrected or expired. Its only source of truth is append-only JSONL. It makes no model calls and has no database, daemon, or semantic index.

## Install

```bash
brew install byteowlz/tap/mmry   # macOS / Linux
yay -S mmry                      # Arch (AUR)
just install-all                 # from source
```

## Use

```bash
mmry add "Run just fmt before commit" --why "CI rejects unformatted code" --memory-type procedural
mmry add --general "Headless Chrome here needs --no-sandbox" --machine arch-dev-01
mmry add "Staging token rotates weekly" --expires 7d --source "ops#123"
mmry add -                        # read one memory from stdin
mmry list                         # general + current repository (alias: ls)
mmry search "fmt"
mmry supersede mem_<id> "Run just fmt && just lint" --reason "lint added" --expected-revision 1
mmry rm mem_<id> --reason obsolete
mmry doctor                       # state root, repository mode, pending migration
```

`list`/`search` read the current scope (general + current repository); `--general`, `--repo NAME` and `--all` select other scopes, `list --include-expired` shows expired entries. Output is wrapped and attributed for humans; `--plain` gives stable tab-separated records and `--json` structured output with `scope`, `repo`, `repo_path`, `revision` and `memory_id`.

## Storage

By default memories live in a per-user central store (`state_root`, default `$XDG_STATE_HOME/mmry`, i.e. `~/.local/state/mmry`):

```text
general/mmry.jsonl              personal memories that apply everywhere
repos/<name>/mmry.jsonl         one ledger per repository
repos/<name>/repo.json          stable identity (git root commit) + known checkouts
```

Clones of the same repository share one ledger; different repositories with the same directory name get separate `<name>--<id>` directories.

A repository is in exactly one mode:

- **central** (default): `mmry init` registers it; nothing is written into the repository.
- **tracked**: `mmry init --tracked` keeps the ledger in `.mmry/mmry.jsonl` to commit with the repository. A ledger already committed to git is treated as tracked too.

### Migrating repo-local ledgers

An untracked `.mmry/mmry.jsonl` in central mode is migrated on first use (`migrate = "auto"`, the default; `"prompt"` asks on a terminal, `"off"` only warns and ignores it). Manually:

```bash
mmry migrate --dry-run            # current repository; --all for every repo under [[roots]]
mmry migrate --untrack            # also move a git-committed ledger (leaves `git rm --cached` uncommitted)
```

Events are merged by id (re-running is a no-op), the central ledger is verified to contain every active local memory, and only then is the local file renamed to `.mmry/mmry.jsonl.migrated-<timestamp>` (kept as backup) with a `.mmry/MIGRATED` note. An event id present in both ledgers with different content aborts the migration with nothing moved.

## Configuration

`$XDG_CONFIG_HOME/mmry/config.toml` (default `~/.config/mmry/config.toml`). A commented default is created on first run; `--config PATH` or `MMRY_CONFIG` selects another file, which must exist. See `examples/config.toml`:

```toml
# state_root = "~/.local/state/mmry"
# migrate = "auto"

[[roots]]            # where `--all`, `--repo` and `migrate --all` look for tracked/legacy ledgers
path = "~/byteowlz"
max_depth = 2
```

`mmry repos` lists every known ledger. Discovery never searches the home directory unless configured, skips dependency/build/cache trees and does not follow symlinks.

## File format

Each line is a versioned event (`memory.add`, `memory.supersede`, `memory.deprecate`). Active memories are obtained by replaying events sorted by `(ts, id)`, so line order does not matter. A supersede keeps the memory id and increments its revision; `--expected-revision` rejects stale writers. Malformed lines are errors and are never silently skipped. Appends use an exclusive file lock and durable flush.

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
just                   # list recipes
just check-all         # fmt-check, strict clippy, ast-grep guardrails, rustdoc, tests (same as CI)
just generate-config   # regenerate examples/config.schema.json from the config model
```

The 500-repository fixture prints its measured cold runtime during tests; no fixed speed claim is made because results depend on filesystem and machine.
