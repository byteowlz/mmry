# mmry

A local-first memory system for humans and AI agents. Store any text and find it again with multiple search strategies, pipe it anywhere with JSON. Everything runs locally. No API keys, no cloud services, your data stays on your own machine.

## Quick Start

### CLI

```bash
# Install
cargo install --git https://github.com/byteowlz/mmry mmry-cli

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
# Install
cargo install --git https://github.com/byteowlz/mmry mmry-tui

# Launch the TUI
mmry-tui

# Use vi keybindings to navigate:
# - hjkl or arrow keys to navigate
# - e to edit memory in your $EDITOR
# - d to delete (with confirmation)
# - / to search
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
# From source
cargo install --git https://github.com/tommyfalkowski/mmry mmry-cli

# Or clone and build
git clone https://github.com/tommyfalkowski/mmry
cd mmry
cargo build --release
```

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

MIT or Apache-2.0, your choice.
