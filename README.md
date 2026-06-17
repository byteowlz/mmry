![mmry-banner](banner.png)

# mmry

A tiny local memory file for humans and AI agents. `mmry` records distilled, durable memories in an append-only JSONL file at `.mmry/mmry.jsonl`, then lets you list and search them deterministically. It is intentionally boring: no daemon, no semantic index, no hidden global store in the default CLI.

The legacy indexed SQLite/service backend still exists during migration behind `--indexed`, but the agent-facing CLI now defaults to the workspace memory file.

## Quick Start

```bash
# Create .mmry/mmry.jsonl and gitignore it by default
mmry init

# Or make workspace memories tracked by git
mmry init --tracked

# Add distilled memories
mmry add "Decision: keep mmry as an append-only workspace memory file"
mmry add "Run just fmt after Rust code edits" --memory-type procedural --tags rust,workflow

# List and search local memories
mmry list
mmry search "rust fmt"

# Script with JSON
mmry list --json
mmry search "workspace memory" --json

# Append a removal/deprecation event; does not rewrite history
mmry rm mem_<id>
```

## What belongs in mmry?

Use `mmry` for concise, reusable takeaways:

- repo-specific decisions
- user/project preferences
- gotchas and known failure modes
- durable workflow rules
- “when X, do Y” memories

Do **not** use it as a transcript store or document index. Full sessions, large documents, semantic search, and cross-workspace retrieval belong in separate search/history tools. `mmry` is the distilled memory substrate.

## File format

Default storage is one workspace-local append-only JSONL file:

```text
.mmry/mmry.jsonl
```

Each line is a memory event, for example:

```json
{"schema_version":1,"type":"memory.add","memory_id":"mem_...","content":"Run just fmt after Rust code edits","tags":["rust"]}
{"schema_version":1,"type":"memory.deprecate","memory_id":"mem_...","target_memory_id":"mem_..."}
```

The active memory list is produced by replaying the file. This keeps the source of truth simple, inspectable, and rebuildable.

## Core commands

```bash
mmry init [--tracked]          # initialize .mmry/mmry.jsonl
mmry add <text|->              # add a memory, read stdin with -
mmry list                      # list active memories; alias: mmry ls
mmry search <query>            # deterministic lexical search
mmry rm <memory-id>            # append memory.deprecate
mmry doctor                    # diagnostics
```

Useful flags:

```bash
mmry add "..." --memory-type episodic|semantic|procedural
mmry add "..." --tags tag1,tag2
mmry list --json
mmry search "..." --limit 20 --json
```

## Legacy indexed backend

The old SQLite/indexed/service backend remains available for transition and integrations that still rely on it:

```bash
mmry init --indexed
mmry add --indexed "legacy indexed memory"
mmry list --indexed
mmry search --indexed "semantic query"
mmry rm --indexed <uuid>
```

Service mode, embeddings, stores, TUI, and MCP are legacy-indexed features for now. New agent integrations should prefer `mmry-core::MemoryFile` and `.mmry/mmry.jsonl`.

## Embedding mmry-core

Rust integrations can use the memory file directly:

```rust
use mmry_core::{MemoryEvent, MemoryFile};
use mmry_core::agent_ctx::AgentCtx;
use mmry_core::memory::MemoryType;

let memory_file = MemoryFile::open_workspace(workspace_path);
memory_file.init(false)?;

let event = MemoryEvent::add(
    "Run just fmt after Rust edits".to_string(),
    MemoryType::Procedural,
    vec!["rust".to_string()],
    &AgentCtx::from_env(),
);
memory_file.append(&event)?;

let memories = memory_file.active_memories()?;
let hits = memory_file.search("rust fmt", 10)?;
```

## Migrating legacy memories

A standalone migration helper converts legacy SQLite rows to JSONL and filters out bulky session/history imports by default:

```bash
scripts/migrate_legacy_mmry_to_jsonl.py ~/.local/share/mmry/stores/mmry.db \
  --store oqto \
  -o /path/to/workspace/.mmry/mmry.jsonl
```

Dry run first:

```bash
scripts/migrate_legacy_mmry_to_jsonl.py ~/.local/share/mmry/stores/mmry.db --dry-run
```

Use `--include-sessions` only if you intentionally want imported hstry/session records.

## Service Mode (Fast Embeddings)

mmry includes an optional background service that holds the warm DB/index and a pooled connection to vqtrs-api, so searches skip per-invocation startup:

```bash
# Start the service
mmry service start

# Check status
mmry service status

# Stop the service
mmry service stop

# Restart after config changes
mmry service restart

# Enable auto-start on boot (systemd user unit on Linux, launchd plist on macOS)
mmry service enable

# Disable auto-start and remove service unit
mmry service disable

# Run in foreground (for debugging)
mmry service run
```

**Why use service mode?**

- Cold CLI invocation pays DB open + connection setup each time
- With service: the DB/index stay warm and vqtrs-api keeps models loaded, so searches return in ~10-50 milliseconds
- Works on Windows, macOS, and Linux (uses TCP localhost)

Enable in `~/.config/mmry/config.toml`:

```toml
[service]
enabled = true
auto_start = true  # Automatically start service when needed
idle_timeout_seconds = 300  # Unload models after 5 minutes idle
```

Reranking now only runs by default for semantic or hybrid searches; use `--rerank` to force reranking for other modes if you need it.
When service mode is enabled, `mmry search` now delegates the entire search (DB + embeddings + sparse + rerank) to the daemon for fast, warm runs; the CLI falls back to local search if the daemon is unavailable.

Embeddings, sparse vectors, and reranking are served by a local [vqtrs-api](https://github.com/byteowlz/vqtrs) instance (default `http://127.0.0.1:8430`); mmry-service holds the warm DB/index and delegates model work to it.

### gRPC API (EmbeddingService)

`mmry-service` exposes a local-only gRPC API for embeddings and search.

- Proto: `crates/mmry-service/proto/embeddings.proto` (package `mmry.embeddings`)
- Service: `EmbeddingService` with `Embed`, `EmbedBatch`, `Search`
- `SearchRequest.store` scopes search to a store (empty = default store)
- Port is dynamic; read from `$XDG_STATE_HOME/mmry/service.port` or `~/.local/state/mmry/service.port`

Example with `grpcurl`:

```bash
PORT=$(cat ~/.local/state/mmry/service.port)
grpcurl -plaintext \
  -d '{"query":"rust","limit":5,"mode":"KEYWORD","rerank":false,"store":"govnr"}' \
  localhost:$PORT mmry.embeddings.EmbeddingService/Search
```

## Optional analyzer (LLM-based enrichment)

mmry can call any OpenAI-compatible API for intelligent fact extraction and routing decisions.

1) Run a local LLM server (LM Studio, Ollama, vLLM) or use OpenAI directly.
2) Add to `~/.config/mmry/config.toml`:

```toml
[analyzer]
enabled = true
endpoint = "http://127.0.0.1:1234/v1"  # or "https://api.openai.com/v1"
model = "gpt-4o-mini"  # or your local model name
```

For OpenAI, set the `OPENAI_API_KEY` environment variable. For local servers, no API key is needed.

If the analyzer is disabled or no endpoint is configured, mmry falls back to heuristic-based extraction.

## How It Works

mmry stores everything in SQLite with vector extensions for similarity search. Embeddings, sparse vectors, and reranking are served out-of-process by [vqtrs-api](https://github.com/byteowlz/vqtrs) over its local HTTP/Unix-socket API (default `http://127.0.0.1:8430`); mmry itself no longer loads any model. When the service isn't reachable, semantic search degrades gracefully to lexical (keyword/fuzzy/BM25) results.

Search combines multiple strategies:

- Text matching (keyword + fuzzy)
- Statistical relevance (BM25)
- Dense embeddings (semantic similarity)
- Sparse embeddings (learned term importance)
- Optional reranking with cross-encoders

You can tweak the weights of each strategy in the config, or just use hybrid mode and let it figure it out.

## Installation

### Package Managers

```bash
# macOS (Homebrew)
brew install byteowlz/tap/mmry

# Arch Linux (AUR)
yay -S mmry          # Pre-built binary
```

### From Source

```bash
# Clone the repository
git clone https://github.com/byteowlz/mmry
cd mmry

# Option 1: Using just (recommended if you have it installed)
just install-all

# Option 2: Run the install script directly
# macOS/Linux:
./scripts/install-mmry.sh

# Windows (PowerShell):
powershell -ExecutionPolicy Bypass -File scripts\install-mmry.ps1

# From source (CLI only)
cargo install --git https://github.com/byteowlz/mmry mmry-cli

# Manual build
cargo build --release
```

The install script builds and installs mmry-cli, mmry-tui, and mmry-service to
`~/.cargo/bin`. It runs non-interactively — there are no acceleration options to
pick, since embeddings and reranking run out-of-process in
[vqtrs-api](https://github.com/byteowlz/vqtrs), which owns the GPU/ONNX backends.
Install and run vqtrs separately for semantic search.

Binary releases coming soon.

## Configuration

Config lives at `~/.config/mmry/config.toml` (creates itself on first run).

```toml
[stores]
directory = "~/.local/share/mmry/stores"
default = "default"

[search]
mode = "hybrid"
similarity_threshold = 0.7

[embeddings]
model = "Xenova/all-MiniLM-L6-v2"  # must match what vqtrs-api serves
backend = "remote"                  # embeddings come from vqtrs-api
[embeddings.remote]
base_url = "http://127.0.0.1:8430"  # local vqtrs-api
required = false                    # false = degrade to lexical if unreachable

[service]
enabled = false         # Background service: warm DB/index + pooled vqtrs connection
auto_start = true

[external_api]
enabled = false         # mmry's own HTTP API (separate from vqtrs)
host = "127.0.0.1"
port = 8081

[analyzer]
enabled = false         # LLM-based fact extraction
# endpoint = "http://127.0.0.1:1234/v1"
# model = "gpt-4o-mini"
```

See `examples/config.toml` for all options. Path expansion works (`~`, `$HOME`, `$XDG_DATA_HOME`).

Precedence: CLI flags > environment variables (`MMRY__SECTION__KEY`) > local `mmry.config.toml` > global config.

## More Examples

```bash
# Basic memory management
mmry add "I love pizza"
mmry ls --limit 10
mmry search "food"

# Specify type, category, tags, and importance
mmry add "Paris is the capital of France" --memory-type semantic --importance 8
mmry add "Team standup meeting notes" --category work --tags "meetings,daily,team"

# Different search modes
mmry search "pizza" --mode keyword
mmry search "piza" --mode fuzzy      # finds "pizza"
mmry search "italian food" --mode semantic

# JSON output (embeddings omitted by default)
mmry add "test" --json           # Clean output without embeddings
mmry add "test" --json --full    # Include full embeddings
mmry search "work" --json        # Search results without embeddings
mmry ls --json --full            # List with full embeddings

# JSON pipelines
mmry search "work" --json | \
  jq 'map({content, category: "archive"})' | \
  mmry add -

# Batch operations
echo '[
  {"content": "First memory"},
  {"content": "Second memory", "type": "semantic"}
]' | mmry add - --json

# After changing embedding models
mmry reembed --force
```

See `examples/json-input-examples.md` for the full JSON schema and more pipeline examples.

## Project Structure

```
crates/
  mmry-core/    # Core library (database, embeddings, search)
  mmry-cli/     # Command-line interface
  mmry-tui/     # Terminal UI (Yazi-inspired, vi keybindings)
  mmry-mcp/     # Model Context Protocol server

examples/       # Config examples and JSON schema
```

## TUI Features

The TUI (`mmry-tui`) provides an interactive interface for managing memories:

**Layout**

- Three-pane Yazi-inspired layout
- Left: Categories, tags, and filters
- Middle: Memory list with previews
- Right: Full memory details and content

**Keybindings** (vi-style)

Navigation:

- `hjkl` or arrow keys - Navigate panes and lists
- `gg` - Jump to top
- `G` - Jump to bottom
- `Ctrl-d/u` - Page down/up

Selection (Yazi-style):

- `Space` - Toggle selection on current memory and move down
- `Ctrl-a` - Select all memories
- `V` - Clear all selections

Memory Operations:

- `e` - Edit memory in external editor ($EDITOR, $VISUAL, or vim/nano)
- `d` - Delete memory or all selected memories (with confirmation)
- `a` - Add new memory
- `r` - Refresh memory list

Other:

- `/` - Search/command palette
- `s` - Sort menu
- `?` - Help overlay
- `q` or `Ctrl-c` - Quit

**Features**

- Multi-select memories (Yazi-style with Space key)
- Bulk delete selected memories
- Memory editing in your preferred editor (respects $EDITOR/$VISUAL)
- Memory content serialized as readable YAML for editing
- Delete confirmation dialogs (shows count for bulk operations)
- Sort by date, importance, category, or type
- Visual selection indicators (◉ marker)
- Selection count in memory list title
- Adapts to terminal color scheme
- Status bar with helpful hints

Built with Rust using sqlx, tokio, and [vqtrs](https://github.com/byteowlz/vqtrs) for embeddings/reranking. Check `AGENTS.md` if you're an AI agent working on this codebase.

## Credits

Inspiration and prior art:

- Cloudflare Discord Agent: https://github.com/cloudflare/awesome-agents/tree/main/agents/discord-agent
- Cass memory system: https://github.com/Dicklesworthstone/cass_memory_system
- Letta: https://github.com/letta-ai/letta-code

## License

MIT License
