# mmry Benchmark Specification

## Overview

Add a `bench` subcommand to mmry for standardized embeddings and search benchmarking as part of the OwlBench suite.

**Issue**: `owlbench-cgh.3` - Add bench subcommand to mmry (embeddings/search benchmarking)

## Command Interface

```bash
mmry bench [OPTIONS]
mmry bench --output json > results.json
mmry bench --mode all          # benchmark all search modes
mmry bench --service           # benchmark with service daemon
mmry bench --cold              # include cold start measurements
```

## Required Metrics

### Embedding Metrics

| Metric | Description | Unit |
|--------|-------------|------|
| `embed_latency_cold_ms` | First embedding (model load + inference) | ms |
| `embed_latency_warm_ms` | Subsequent embedding (service mode) | ms |
| `embed_throughput` | Documents embedded per second | docs/sec |
| `embed_memory_mb` | Memory used by embedding model | MB |

### Search Metrics

| Metric | Description | Unit |
|--------|-------------|------|
| `search_latency_p50_ms` | 50th percentile search latency | ms |
| `search_latency_p99_ms` | 99th percentile search latency | ms |
| `search_throughput` | Queries per second | qps |

### Per-Mode Search Metrics

Benchmark each search mode separately:
- `hybrid`, `keyword`, `fuzzy`, `semantic`, `bm25`, `sparse`

### Index Metrics

| Metric | Description | Unit |
|--------|-------------|------|
| `index_throughput` | Documents indexed per second | docs/sec |
| `index_size_mb` | Database size per 1000 documents | MB/1k docs |

## Test Dataset

Use a standardized test corpus:
- 1000 short documents (~50 words each)
- 100 medium documents (~200 words each)
- 10 long documents (~1000 words each)
- 50 test queries of varying complexity

## JSON Output Schema

```json
{
  "benchmark": "mmry-rag",
  "version": "0.1.0",
  "timestamp": "2025-12-11T16:00:00Z",
  "hardware": {
    "cpu": "Apple M3 Max",
    "gpu": "Apple M3 Max (CoreML)",
    "memory_gb": 64
  },
  "config": {
    "embedding_model": "Xenova/all-MiniLM-L6-v2",
    "sparse_model": "prithivida/Splade_PP_en_v1",
    "service_mode": true,
    "ort_backend": "coreml",
    "runs": 5
  },
  "results": {
    "embedding": {
      "latency_cold_ms": 2800,
      "latency_warm_ms": 25,
      "throughput_docs_sec": 40,
      "memory_mb": 180
    },
    "search": {
      "hybrid": {
        "latency_p50_ms": 45,
        "latency_p99_ms": 120,
        "throughput_qps": 22
      },
      "semantic": {
        "latency_p50_ms": 35,
        "latency_p99_ms": 95,
        "throughput_qps": 28
      },
      "bm25": {
        "latency_p50_ms": 8,
        "latency_p99_ms": 25,
        "throughput_qps": 120
      }
    },
    "index": {
      "throughput_docs_sec": 35,
      "size_mb_per_1k": 12.5
    }
  }
}
```

## Implementation Notes

1. **Service mode**: Benchmark both cold (CLI) and warm (service daemon) paths
2. **Isolation**: Use temporary database for benchmarks, don't pollute user data
3. **ONNX backends**: Report which ORT backend is active (CoreML, CUDA, CPU, etc.)
4. **Sparse embeddings**: Include SPLADE++ if enabled in config
5. **Reranking**: Optionally benchmark reranking overhead

## Integration with OwlBench

Results will be collected by `owlbench collect` command and aggregated into composite RAG Score.

```bash
# OwlBench will call:
mmry bench --output json > /tmp/mmry_bench.json
```

## Benchmark Database

Create temporary benchmark database to avoid polluting user memories:

```bash
MMRY_DB=/tmp/mmry_bench.db mmry bench
```

Or add `--bench-db` flag that automatically uses a temp location.
