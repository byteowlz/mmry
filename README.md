![mmry-banner](banner.png)

# mmry

A local-first memory system for humans and AI agents. Store any text and find it again with multiple search strategies, pipe it anywhere with JSON. Everything runs locally. No API keys, no cloud services, all data stays on your own machine.

## Quick Start

### CLI

```bash
# Install (interactive script installs both CLI + TUI and asks for ORT features)
just install-all

# Add memories
mmry add "I love coding in Rust"
mmry add "Meeting with Sarah about the new API on Friday"

# Search however you want
mmry search "rust programming"
mmry search "sara friday" --mode fuzzy  # typo-tolerant
mmry search "api" --mode semantic       # conceptual similarity

# Pipe things around
echo "Important note" | mmry add -
mmry search "important" --json | jq '.[].content'
```

### TUI

```bash
# Install (already handled by `just install-all`)

# Launch the TUI
mmry-tui

# Use vi keybindings to navigate:
# - hjkl or arrow keys to navigate
# - e to edit memory in your $EDITOR
# - d to delete (with confirmation)
# - / to search
# - Tab to cycle search mode
# - s to sort
# - ? for help
```

## What It Does

**Search modes**: Pick what works for your query

- `hybrid` - Combines everything (default, usually best)
- `keyword` - Exact matching
- `fuzzy` - Typo-tolerant
- `semantic` - Finds similar concepts using embeddings
- `bm25` - Statistical relevance (like a search engine)
- `sparse` - Neural sparse embeddings (SPLADE++)

**Memory types**: Three types (auto-guessed or specify with `--memory-type`)

- Episodic (events and experiences) - default
- Semantic (facts and knowledge) - if it contains "is" or "are"
- Procedural (how-to and instructions) - if it contains "step" or "how to"

The auto-classification is basic keyword matching, so specify the type explicitly for anything important.

**Categories & Tags**: Organize your memories

Each memory belongs to one category (like a folder) and can have multiple tags:

```bash
mmry add "Sprint planning notes" --category work --tags "planning,team"
mmry add "Birthday party ideas" --category personal --tags "family,celebration,todo"
mmry search "notes" --category work           # filter by category
mmry ls --category personal                   # list by category
```

**JSON all the way**: Every command supports `--json` for scripting

```bash
mmry ls --json | jq 'map(select(.importance > 7))'
echo '{"content": "From JSON"}' | mmry add -
```

## Service Mode (Fast Embeddings)

mmry includes an optional background service that keeps the embedding model loaded in memory for near-instant embeddings:

```bash
# Start the service
mmry service start

# Reload after changing config
mmry service reload

# Check status
mmry service status

# Stop the service
mmry service stop

# Run in foreground (for debugging)
mmry service run
```

**Why use service mode?**

- First embedding: ~2-3 seconds (cold start, loading model)
- With service: ~10-50 milliseconds (model stays loaded)
- Automatically unloads after 5 minutes of inactivity to save memory
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

The CLI exits directly once a command finishes to sidestep an upstream fastembed/ONNX shutdown bug; the OS reclaims any leaked service resources.

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

## HMLR: Hierarchical Memory Ledger with Routing

mmry includes optional HMLR features for AI agent workflows. When enabled, memories are enriched with extracted facts, organized into bridge blocks, and tracked with agent attribution.

### Quick Start

Enable HMLR in `~/.config/mmry/config.toml`:

```toml
[hmlr]
enabled = true
extract_facts = true      # Extract key-value facts from content
bridge_routing = true     # Group related memories into bridge blocks
audit_trail = true        # Log agent events for debugging
track_human_agent = true  # Track human as an agent
human_agent_name = "human"
```

### What HMLR Adds

**Facts**: Key-value pairs extracted from memory content

```bash
# Search now includes facts
mmry search "api key" --hmlr --search-facts

# View facts in TUI (press 'b' to switch to Facts view)
```

**Bridge Blocks**: Logical groupings of related memories

- Memories within a conversation or task share the same bridge block
- Blocks track topic, keywords, and status (open/closed)
- Search can filter or group by blocks

**Agent Attribution**: Track who created each memory

- Distinguishes human vs AI agent entries
- Logs agent events for debugging and auditing

### HMLR Search

```bash
# Include HMLR enrichments in search results
mmry search "project" --hmlr

# Also search facts (not just memories)
mmry search "deadline" --hmlr --search-facts

# Group results by bridge blocks
mmry search "api" --hmlr --group-by-blocks

# Exclude closed/inactive blocks
mmry search "task" --hmlr --inactive-blocks exclude
```

### TUI Integration

The TUI shows HMLR enrichments when viewing a memory:

- Creator agent name and type
- Associated bridge block (topic, keywords, status)
- Extracted facts

Press `b` to cycle through views: Memories, Bridge Blocks, Facts, Agent Events.

### Architecture

HMLR runs as a post-ingestion pipeline:

1. **Governor**: Orchestrates enrichment in parallel
2. **FactScrubber**: Extracts key-value facts from content
3. **Scribe**: Async updates to user profile (preferences, constraints)
4. **LatticeCrawler**: Finds candidate bridge blocks for routing
5. **ContextHydrator**: Assembles context from multiple sources

All components are opt-in via config. HMLR adds no overhead when disabled.

## How It Works

mmry stores everything in SQLite with vector extensions for similarity search. It uses [fastembed](https://github.com/Anush008/fastembed-rs) to run embedding models locally via ONNX Runtime - no external APIs needed.

Search combines multiple strategies:

- Text matching (keyword + fuzzy)
- Statistical relevance (BM25)
- Dense embeddings (semantic similarity)
- Sparse embeddings (learned term importance)
- Optional reranking with cross-encoders

You can tweak the weights of each strategy in the config, or just use hybrid mode and let it figure it out.

## Installation

```bash
# Clone the repository
git clone https://github.com/tommyfalkowski/mmry
cd mmry

# Option 1: Using just (recommended if you have it installed)
just install-all

# Option 2: Run the install script directly
# macOS/Linux:
./scripts/install-mmry.sh

# Windows (PowerShell):
powershell -ExecutionPolicy Bypass -File scripts\install-mmry.ps1

# From source (CLI only)
cargo install --git https://github.com/tommyfalkowski/mmry mmry-cli

# Manual build
cargo build --release
```

The install script builds and installs mmry-cli, mmry-tui, and mmry-service with your choice of ONNX Runtime acceleration:

- none (default) - CPU-only, works everywhere
- ort-coreml - Apple Neural Engine acceleration (macOS)
- ort-cuda - NVIDIA GPU acceleration
- ort-directml - DirectML acceleration (Windows)
- ort-openvino - Intel OpenVINO
- ort-tensorrt - NVIDIA TensorRT
- ort-rocm - AMD ROCm
- ort-nnapi - Android Neural Networks API
- ort-xnnpack - XNNPACK acceleration
- ort-load-dynamic - Dynamic loading

The script will prompt you to select an option, then build and install both binaries to `~/.cargo/bin`.

Binary releases coming soon.

## Configuration

Config lives at `~/.config/mmry/config.toml` (creates itself on first run).

```toml
[database]
path = "~/.local/share/mmry/memories.db"  # Paths support ~ and $HOME

[search]
mode = "hybrid"
similarity_threshold = 0.7

[embeddings]
model = "Xenova/all-MiniLM-L6-v2"  # Fast and local
```

See `examples/config.toml` for all options. Path expansion works (`~`, `$HOME`, `$XDG_DATA_HOME`).

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

Built with Rust using sqlx, fastembed, and tokio. Check `AGENTS.md` if you're an AI agent working on this codebase.

## License

MIT License
