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

## Harness integration

```bash
mmry preview                      # exact text to inject at session start
mmry preview --json --cwd DIR --max-tokens 1200 --limit 20
```

`preview --json` returns the selected entries, the exact `rendered` text and a `selection_hash` (schema: `examples/preview.schema.json`). It never prompts or migrates; contested, expired and other-machine memories are left out. Mutating commands print one entry schema with `--json` (`examples/memory.schema.json`). Harnesses should use this CLI contract, never the ledger files. [pi-mmry](https://github.com/byteowlz/pi-agent-extensions/tree/main/pi-mmry) is the Pi extension built on it. Details: [docs/ledger.md](docs/ledger.md).

## Storage

Memories live in a per-user central store, by default `~/.local/share/mmry` (`$XDG_DATA_HOME/mmry`): `general/` for memories that apply everywhere and one `repos/<name>--<id>/` ledger per repository, identified by its git root commit so clones and worktrees share it. `mmry init --tracked` instead keeps a repository's ledger in `.mmry/mmry.jsonl` to commit with it.

Existing `.mmry` ledgers are not read until you migrate them:

```bash
mmry setup --dry-run              # find .mmry ledgers under home (+ [[roots]]) and show the plan
mmry setup                        # confirm, migrate them all, set migrate = "auto"
```

Migration merges by event id, verifies the result and keeps the old file as a backup. Layout, repository identity, per-repository migration and the legacy SQLite importer: [docs/storage.md](docs/storage.md).

## Sync and cleanup

`mmry sync init --remote URL` makes the store a git repository you sync with `mmry sync` (opt-in; optional auto pull/commit/push in `[sync]`). Use a private remote: history keeps removed memories. Concurrent edits from two machines are marked contested instead of silently overwritten. See [docs/sync.md](docs/sync.md).

`mmry cleanup propose` lists duplicates, near-duplicates and expired memories without writing; `mmry cleanup apply <id>` applies the ones you choose. No model is called.

## Configuration

Precedence: `--state-root`/`--migrate` flags, then `MMRY_STATE_ROOT`/`MMRY_MIGRATE`, then the config file. There is deliberately no repo-local config. The file is `$XDG_CONFIG_HOME/mmry/config.toml` (default `~/.config/mmry/config.toml`). A commented default is created on first run; `--config PATH` or `MMRY_CONFIG` selects another file, which must exist. See `examples/config.toml`:

```toml
# state_root = "~/.local/share/mmry"
# migrate = "prompt"

[[roots]]            # where `--all`, `--repo` and `migrate --all` look for tracked/legacy ledgers
path = "~/byteowlz"
max_depth = 2
```

`mmry repos` lists every known ledger. Discovery never searches the home directory unless configured, skips dependency/build/cache trees and does not follow symlinks.

## Limitations

- Ordering uses event timestamps; large clock skew between machines can misorder edits.
- Search is plain text matching, not semantic.
- Repositories without git are identified by path, so their memories do not follow them to other machines.

## Development

```bash
just                   # list recipes
just check-all         # fmt-check, strict clippy, ast-grep guardrails, rustdoc, tests (same as CI)
just generate-config   # regenerate examples/config.schema.json from the config model
```

The 500-repository fixture prints its measured cold runtime during tests; no fixed speed claim is made because results depend on filesystem and machine.
