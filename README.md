# mmry - A memory management system for humans and AI agents

## Overview

## Tech Stack

mmry is written in Rust using sqlx, clap, tokio and sqlite-vec for vector storage.
Embeddings and LLM-Models are integrating though openai-like interfaces so that users can specify the models of their choice. We need to check if <https://github.com/meilisearch/meilisearch> could be of use to us

## Architecture

mmry is build in a cargo workspace comprising multiple crates:

- mmry-core contains the core functionality including interacting with the sqlite database, semantic search via embeddings
- mmry-cli is an ergonomic cli for directly adding, retrieving and managing memories
- mmry-api is the server component for making mmry's functionality available to multiple clients (the cli serving as one of the many potential clients)
- mmry-mcp is the mcp-server component for integrating mmry functionality into AI Agent application

### Database Migrations

mmry automatically applies schema updates when you run any command. When new features are added that require database schema changes, the system will:

1. Detect if the schema is out of date
2. Apply necessary updates (e.g., adding new columns)
3. Log the changes being made

This happens seamlessly - no manual migration commands needed!

## Conecpt

mmry uses a memory architecture modelled after the human memory but enhanced by leveraging the superior recall mechanisms of computer technology. Memory types include:

Short-term memory

- Working memory

Long-term memories

- Procedural memory (text, stored as markdown lists with optional sections)
- Episodic memory (text, stored in database)
- Semantic memory (embeddings, stored in database)

## Search Modes

mmry supports multiple isolated search modes, each optimized for different use cases:

### Available Modes

- **Hybrid** (default): Combines multiple search strategies for best overall results
  - Weights keyword, fuzzy, semantic, BM25, and sparse embedding scores
  - Configurable weights for each component
  
- **Keyword**: Exact and partial keyword matching
  - Fast and precise for known terms
  - Case-insensitive substring matching
  
- **Fuzzy**: Approximate string matching
  - Tolerant to typos and variations
  - Uses Jaro-Winkler similarity
  
- **Semantic**: Dense vector embeddings
  - Finds conceptually similar content
  - Uses fastembed models for local inference
  
- **BM25**: Statistical relevance (traditional search engines)
  - Term frequency and inverse document frequency
  - Configurable k1 and b parameters
  - No model required, pure statistics
  
- **Sparse Embedding**: SPLADE++ learned sparse embeddings
  - Neural sparse retrieval
  - Learns important terms via deep learning
  - Better than BM25 for semantic understanding
  - Uses fastembed's SPLADE++ model

### Reranking

All search modes support optional reranking to improve result quality:

- Uses cross-encoder models for relevance scoring
- Reranks top-k results (default: 20)
- Can be enabled/disabled per search

### Re-embedding

When you change embedding models in your configuration, use the `reembed` command to regenerate embeddings for existing memories:

**Use cases:**
- Switched to a better embedding model (e.g., from all-MiniLM-L6-v2 to bge-base)
- Enabled sparse embeddings after initially using only dense embeddings
- Changed sparse embedding model
- Database was migrated and embeddings are missing

**Options:**
- `--dense-only` - Only regenerate dense vector embeddings
- `--sparse-only` - Only regenerate sparse embeddings
- `--force` - Regenerate even if embeddings already exist
- `--dry-run` - Preview what would be updated without making changes
- `--namespace` - Only process memories in a specific namespace

The command shows progress every 10 memories and provides a summary of what was updated.

## CLI

### Basic Usage

```bash
mmry --help # Help menu

mmry add "I like pizza" # Quickly add a memory

mmry add "I need to pick up the kids next Tuesday at 2PM" 

mmry search "pizza" # Search with default mode (hybrid)

mmry search "pizza" --mode keyword # Use keyword-only search

mmry search "pizza" --mode semantic # Use semantic search only

mmry search "pizza" --mode fuzzy # Use fuzzy matching

mmry search "pizza" --mode bm25 # Use BM25 sparse search

mmry search "pizza" --mode sparse # Use SPLADE++ neural sparse search

mmry search "pizza" --rerank # Force enable reranking

mmry search "pizza" --no-rerank # Disable reranking

# Regenerate embeddings after changing models
mmry reembed # Regenerate all embeddings

mmry reembed --dense-only # Only regenerate dense embeddings

mmry reembed --sparse-only # Only regenerate sparse embeddings

mmry reembed --force # Force regenerate even if embeddings exist

mmry reembed --dry-run # Preview what would be updated

mmry update "I don' like pizza anymore" # Find the top similar memories and ask the user which one to update
```

### JSON Input/Output

mmry supports JSON for input and output, enabling powerful data pipelines:

```bash
# Output results as JSON
mmry search "pizza" --json | jq '.[].content'

mmry ls --limit 5 --json | jq 'length'

# Add from stdin (plain text)
echo "Memory from stdin" | mmry add -

cat notes.txt | mmry add - --importance 8

# Add from JSON (single or batch)
echo '{"content": "JSON memory"}' | mmry add -

echo '[
  {"content": "First memory"},
  {"content": "Second memory", "type": "semantic"}
]' | mmry add -

# Pipeline: copy memories to different namespace
mmry search "important" --json | \
  jq 'map({content, namespace: "archive"})' | \
  mmry add -
```

**JSON Schema**: See `examples/memory-schema.json` for the complete JSON Schema (Draft 7) specification. Only the `content` field is required - `id` is auto-generated and `type` is auto-classified if not provided.

**Documentation**: See `examples/json-input-examples.md` for comprehensive examples and usage patterns.
