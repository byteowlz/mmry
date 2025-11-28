# Adding graph functionality

**Session ID:** ses_534fd818effecUfsXyQoDTBp7l
**Created:** 11/28/2025, 4:08:49 PM
**Updated:** 11/28/2025, 4:23:20 PM

---

## User

Would it make sense to add some graph functionality to this?

---

## Assistant

That's an interesting question. Let me first understand what mmry is and what it currently does before I can give you a thoughtful answer about whether graph functionality would make sense.

```
Tool: read
```

```
Tool: read
```

```
Tool: read
```

---

## Assistant

Now let me look at the database schema to understand the current data model better:

```
Tool: read
```

```
Tool: read
```

---

## Assistant

This is very interesting! I can now give you a well-informed answer.

## Analysis

**You already have graph infrastructure in place!** Looking at your schema, you have:

1. **`entities` table** - Stores named entities with types and metadata
2. **`memory_entities` junction table** - Links memories to entities (many-to-many)
3. **`relationships` table** - Entity-to-entity relationships with `relation_type` and `strength`

However, this infrastructure appears to be **unused**. The `Memory` struct has an `entities: Vec<String>` field in `EpisodicMemory`, but the main `Memory` struct doesn't expose entities, and I don't see any CLI commands or operations that populate these tables.

## Would Graph Functionality Make Sense?

**Yes, absolutely.** Here's why it fits well with mmry's design:

### Strong Arguments For

1. **Natural fit for memory systems** - Human memory is associative. "Sarah" connects to "Friday meeting" connects to "API project". Graph traversal matches how we recall things.

2. **Enhanced search** - You could query: "What do I know about Sarah?" and get not just memories mentioning her, but related entities (her projects, associated people, locations).

3. **Knowledge graphs for AI agents** - The MCP server could expose graph queries, letting agents understand relationships between concepts they've stored.

4. **Temporal chains** - Connect memories chronologically: "What happened after the API meeting?"

5. **You already have the schema** - The tables exist, you'd just need to wire them up.

### Potential Features

- **Auto-extract entities** (NER) when adding memories
- **`mmry link`** - Manually create relationships
- **`mmry graph <entity>`** - Show related entities/memories
- **Graph-augmented search** - Expand search to related entities
- **Visualization in TUI** - Right pane could show entity relationships

### Considerations

- **Complexity vs. value** - How much does this add over semantic search, which already captures conceptual relationships?
- **Entity extraction quality** - Would require NER (could use a local model or simple regex for names/dates)
- **Scope creep** - mmry is clean and focused right now

## Recommendation

Start with **lightweight entity linking** - populate the existing tables with simple extracted entities (proper nouns, dates, etc.) and add a `mmry entities` command to explore them. If that proves useful, expand to full graph traversal and visualization.

Would you like me to explore what implementing this would look like?

---

## User

how would we do NER?

---

## Assistant

Good question. Let me look at what's already in the codebase for NLP/ML and what options would fit mmry's local-first philosophy.

```
Tool: grep
```

```
Tool: grep
```

```
Tool: read
```

---

## Assistant

No existing NER. Let me look at fastembed more closely since you're already using it, and explore what options exist:

```
Tool: read
```

---

## Assistant

Now I have a complete picture. Here are the NER options for mmry, ordered from simplest to most sophisticated:

## NER Options for mmry

### 1. Regex-based extraction (Zero dependencies)

**Pros:** No new deps, fast, predictable, works offline
**Cons:** Limited accuracy, English-centric, misses context

```rust
// Extract common entity patterns
fn extract_entities(text: &str) -> Vec<Entity> {
    let mut entities = vec![];
    
    // Capitalized words (likely proper nouns): "Sarah", "Paris"
    let name_re = Regex::new(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b").unwrap();
    
    // Dates: "Friday", "January 15", "2024-01-15"
    let date_re = Regex::new(r"\b(?:Monday|Tuesday|...)\b|\d{4}-\d{2}-\d{2}").unwrap();
    
    // Emails, URLs, @mentions
    // ...
}
```

**Best for:** Quick wins, bootstrapping the entity tables

---

### 2. rust-bert (Local transformer models)

**Pros:** Real NER models (BERT-based), runs locally via ONNX/libtorch
**Cons:** ~200MB model download, adds libtorch dependency, slower

```toml
[dependencies]
rust-bert = "0.22"
```

```rust
use rust_bert::pipelines::ner::NERModel;

let ner_model = NERModel::new(Default::default())?;
let entities = ner_model.predict(&["Meeting with Sarah about the API"]);
// Returns: [Entity { word: "Sarah", label: "PER", score: 0.98 }]
```

**Models available:** BERT, DistilBERT, RoBERTa for NER

---

### 3. candle + ONNX NER model

**Pros:** Pure Rust, no Python/libtorch, fits mmry's fastembed pattern
**Cons:** More setup work, need to find/convert ONNX NER model

You're already using ONNX Runtime via fastembed. You could:

1. Download a HuggingFace NER model (e.g., `dslim/bert-base-NER`)
2. Convert to ONNX
3. Run inference similar to your embedding code

---

### 4. Hybrid: GLiNER (Generalist NER)

**Pros:** Zero-shot NER - specify entity types at runtime ("person", "project", "date")
**Cons:** Larger model, newer approach

GLiNER lets you say "extract person, organization, project names" without training. There's a Rust port emerging, or you could call it via ONNX.

---

### 5. LLM-based extraction (via MCP or local model)

**Pros:** Highly accurate, handles context well
**Cons:** Slower, requires LLM availability

```
Extract entities from: "Meeting with Sarah about the API on Friday"
→ {"people": ["Sarah"], "topics": ["API"], "dates": ["Friday"]}
```

Could be optional: `mmry add "text" --extract-entities` calls out to a local LLM.

---

## Recommendation for mmry

**Start with Option 1 (regex)** to populate the existing schema and prove value:

- Capitalized proper nouns → `PERSON` or `UNKNOWN`
- Day/date patterns → `DATE`
- @mentions, emails → `CONTACT`
- URLs → `LINK`

Then **add Option 2 or 3** as an optional feature flag:

```toml
[features]
ner = ["rust-bert"]  # or candle-based
```

This keeps mmry lightweight by default while allowing power users to enable ML-based NER.

Would you like me to prototype the regex-based extractor, or explore the rust-bert/ONNX route?

---

## User

does rustbert support modernbert?

---

## Assistant

Good question - ModernBERT is quite new (December 2024). Let me check.

```
Tool: webfetch
```

```
Tool: webfetch
```

---

## Assistant

Let me check the rust-bert source and issues for ModernBERT support:

```
Tool: webfetch
```

---

## Assistant

No results for ModernBERT. Let me check the source to see what architectures are supported:

```
Tool: webfetch
```

---

## Assistant

The page didn't load the directory listing. Let me check if there are any Rust NER options that support ModernBERT, or check if candle (Hugging Face's Rust ML framework) supports it:

```
Tool: webfetch
```

---

## Assistant

Excellent find! Here's the answer:

## ModernBERT Support

**rust-bert: No** - No ModernBERT support. The supported architectures are BERT, DistilBERT, RoBERTa, DeBERTa, Electra, etc. (the "classic" transformer models).

**candle: Yes!** - Hugging Face's `candle` crate has ModernBERT support:

- PR #2713 (Jan 2025): Added ModernBERT model
- PR #2791 (Mar 2025): Added embedding generation support
- PR #2796 (Mar 2025): Added `ModernBertForSequenceClassification`

## What This Means for mmry

If you want ModernBERT-based NER, you have a few options:

### Option 1: candle + ModernBERT for NER

candle has the base ModernBERT model, but **not a dedicated NER head**. You'd need to:

1. Use `candle-transformers` with ModernBERT
2. Fine-tune or find a ModernBERT model with a token classification head for NER
3. Implement the token classification wrapper

This is more work but gives you native Rust + ModernBERT.

### Option 2: candle + BERT-base NER (simpler)

candle supports standard BERT. You could use an existing BERT-based NER model (like `dslim/bert-base-NER`) via ONNX export, similar to how you're using fastembed.

### Option 3: ONNX + any NER model

Since you're already using ONNX Runtime via fastembed, you could:

1. Export any HuggingFace NER model (including ModernBERT-based ones if they exist) to ONNX
2. Run inference directly with `ort` (the Rust ONNX runtime bindings)

This is probably the **most practical path** - it matches your existing architecture.

### Option 4: Wait for ecosystem maturity

ModernBERT is still new (Dec 2024). There aren't many fine-tuned NER models for it yet. The classic `bert-base-NER` and `bert-large-NER` from dbmdz are battle-tested.

## Recommendation

For mmry, I'd suggest:

1. **Start with regex** for immediate entity extraction (zero deps)
2. **Add ONNX-based BERT NER** as an optional feature (`--features ner`)
3. **Consider ModernBERT later** once fine-tuned NER models are available

Would you like me to explore the ONNX NER route, or start with the regex implementation?

---

## User

lets stay with onnx, whats the fastest/best sota NER model in november 2025?

---

## Assistant

Let me research current state-of-the-art NER models available in ONNX format.

```
Tool: webfetch
```

```
Tool: webfetch
```

---

## Assistant

---

## User

---

## Assistant

---
