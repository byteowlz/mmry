# JSON Input Examples for mmry

The `mmry add` command accepts JSON input via stdin, making it easy to pipe data between commands or import memories from external sources.

## JSON Schema

The JSON schema is available at `examples/memory-schema.json` and follows JSON Schema Draft 7.

## Required Fields

- `content` (string): The content of the memory

## Optional Fields

All other fields are optional and will be auto-generated or classified if not provided:

- `type` or `memory_type` (string): One of "episodic", "semantic", or "procedural"
- `namespace` (string): Namespace for organizing memories (defaults to "default")
- `importance` (integer): Importance level from 1-10 (defaults to 5)
- `metadata` (object): Additional metadata as key-value pairs

**Note**: The `id` field is always auto-generated and cannot be specified.

## Single Memory Examples

### Minimal (only required fields)
```bash
echo '{"content": "Simple memory with auto-generated fields"}' | mmry add -
```

### With type and importance
```bash
echo '{
  "content": "Important fact about the project",
  "type": "semantic",
  "importance": 9
}' | mmry add -
```

### With namespace
```bash
echo '{
  "content": "Meeting notes from today",
  "type": "episodic",
  "namespace": "meetings",
  "importance": 7
}' | mmry add -
```

### Alternative field names
```bash
# You can use either "type" or "memory_type"
echo '{
  "content": "Test memory",
  "memory_type": "procedural"
}' | mmry add -
```

## Batch Input (Array)

Add multiple memories at once:

```bash
echo '[
  {"content": "First memory in batch"},
  {"content": "Second memory in batch", "type": "semantic"},
  {"content": "Third memory", "importance": 8}
]' | mmry add -
```

## Pipeline Examples

### Copy memories to a different namespace
```bash
mmry search "important" --json | \
  jq 'map({content, importance, namespace: "archive"})' | \
  mmry add -
```

### Extract and add memories from external JSON
```bash
cat data.json | jq '.items[] | {content: .text, type: "semantic"}' | \
  mmry add -
```

### Duplicate memories with modifications
```bash
mmry ls --namespace default --json | \
  jq 'map({content: ("Copy: " + .content), namespace: "backup"})' | \
  mmry add -
```

### Filter and re-import with higher importance
```bash
mmry search "project" --json | \
  jq 'map(select(.importance < 7) | {content, importance: 8})' | \
  mmry add -
```

## Command-Line Overrides

Command-line flags always override JSON fields:

```bash
# JSON specifies type=semantic, but CLI overrides to procedural
echo '{"content": "Test", "type": "semantic"}' | \
  mmry add - --memory-type procedural --importance 9
```

## Output as JSON

Use `--json` to get the added memory(ies) as JSON output:

```bash
echo '{"content": "Test"}' | mmry add - --json
# Output: {"id": "...", "memory_type": "episodic", "content": "Test", ...}
```

## Validation

Invalid JSON or missing required fields will result in an error:

```bash
# Missing required 'content' field
echo '{"importance": 5}' | mmry add -
# Error: JSON object must have 'content' field

# Empty content
echo '{"content": ""}' | mmry add -
# Error: Content cannot be empty
```

## Schema Validation

You can validate your JSON against the schema using tools like `ajv-cli`:

```bash
npm install -g ajv-cli
ajv validate -s examples/memory-schema.json -d your-data.json
```
