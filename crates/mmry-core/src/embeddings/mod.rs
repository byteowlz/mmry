//! Embedding model catalog (informational) and the remote-backed embedding
//! service wrapper. In-process embedding has been removed; embeddings come from
//! an external OpenAI-compatible service (see `crate::config::RemoteBackendConfig`).

mod wrapper;

pub use wrapper::EmbeddingServiceWrapper;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub code: &'static str,
    pub variant: &'static str,
    pub dimensions: usize,
    pub description: &'static str,
}

pub fn list_models() -> Vec<ModelInfo> {
    vec![
        // --- MiniLM / mpnet ---
        ModelInfo {
            code: "Qdrant/all-MiniLM-L6-v2-onnx",
            variant: "AllMiniLML6V2",
            dimensions: 384,
            description: "Fast, lightweight model (default)",
        },
        ModelInfo {
            code: "Xenova/all-MiniLM-L6-v2",
            variant: "AllMiniLML6V2Q",
            dimensions: 384,
            description: "Quantized MiniLM-L6-v2",
        },
        ModelInfo {
            code: "Xenova/all-MiniLM-L12-v2",
            variant: "AllMiniLML12V2",
            dimensions: 384,
            description: "Larger MiniLM variant",
        },
        ModelInfo {
            code: "Xenova/all-mpnet-base-v2",
            variant: "AllMpnetBaseV2",
            dimensions: 768,
            description: "Sentence Transformer mpnet-base-v2",
        },
        // --- BGE (English) ---
        ModelInfo {
            code: "Xenova/bge-small-en-v1.5",
            variant: "BGESmallENV15",
            dimensions: 384,
            description: "BGE small English v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-base-en-v1.5",
            variant: "BGEBaseENV15",
            dimensions: 768,
            description: "BGE base English v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-large-en-v1.5",
            variant: "BGELargeENV15",
            dimensions: 1024,
            description: "BGE large English v1.5",
        },
        // --- BGE (Chinese) ---
        ModelInfo {
            code: "Xenova/bge-small-zh-v1.5",
            variant: "BGESmallZHV15",
            dimensions: 512,
            description: "BGE small Chinese v1.5",
        },
        ModelInfo {
            code: "Xenova/bge-large-zh-v1.5",
            variant: "BGELargeZHV15",
            dimensions: 1024,
            description: "BGE large Chinese v1.5",
        },
        // --- BGE-M3 (multilingual) ---
        ModelInfo {
            code: "BAAI/bge-m3",
            variant: "BGEM3",
            dimensions: 1024,
            description: "BGE-M3 multilingual (100+ languages)",
        },
        // --- GTE ---
        ModelInfo {
            code: "Alibaba-NLP/gte-base-en-v1.5",
            variant: "GTEBaseENV15",
            dimensions: 768,
            description: "GTE base English v1.5",
        },
        ModelInfo {
            code: "Alibaba-NLP/gte-large-en-v1.5",
            variant: "GTELargeENV15",
            dimensions: 1024,
            description: "GTE large English v1.5",
        },
        // --- Nomic ---
        ModelInfo {
            code: "nomic-ai/nomic-embed-text-v1",
            variant: "NomicEmbedTextV1",
            dimensions: 768,
            description: "Nomic Embed Text v1",
        },
        ModelInfo {
            code: "nomic-ai/nomic-embed-text-v1.5",
            variant: "NomicEmbedTextV15",
            dimensions: 768,
            description: "Nomic Embed Text v1.5 (8192 context)",
        },
        // --- MixedBread ---
        ModelInfo {
            code: "mixedbread-ai/mxbai-embed-large-v1",
            variant: "MxbaiEmbedLargeV1",
            dimensions: 1024,
            description: "MixedBread AI large model",
        },
        // --- ModernBERT ---
        ModelInfo {
            code: "lightonai/modernbert-embed-large",
            variant: "ModernBertEmbedLarge",
            dimensions: 1024,
            description: "ModernBERT embedding model",
        },
        // --- Multilingual E5 ---
        ModelInfo {
            code: "intfloat/multilingual-e5-small",
            variant: "MultilingualE5Small",
            dimensions: 384,
            description: "Multilingual E5 small",
        },
        ModelInfo {
            code: "intfloat/multilingual-e5-base",
            variant: "MultilingualE5Base",
            dimensions: 768,
            description: "Multilingual E5 base",
        },
        ModelInfo {
            code: "Qdrant/multilingual-e5-large-onnx",
            variant: "MultilingualE5Large",
            dimensions: 1024,
            description: "Multilingual E5 large",
        },
        // --- Paraphrase ---
        ModelInfo {
            code: "Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
            variant: "ParaphraseMLMiniLML12V2",
            dimensions: 384,
            description: "Paraphrase multilingual MiniLM-L12 (quantized)",
        },
        ModelInfo {
            code: "Xenova/paraphrase-multilingual-mpnet-base-v2",
            variant: "ParaphraseMLMpnetBaseV2",
            dimensions: 768,
            description: "Paraphrase multilingual mpnet-base-v2",
        },
        // --- Jina ---
        ModelInfo {
            code: "jinaai/jina-embeddings-v2-base-code",
            variant: "JinaEmbeddingsV2BaseCode",
            dimensions: 768,
            description: "Jina v2 code embedding",
        },
        ModelInfo {
            code: "jinaai/jina-embeddings-v2-base-en",
            variant: "JinaEmbeddingsV2BaseEN",
            dimensions: 768,
            description: "Jina v2 base English",
        },
        // --- CLIP ---
        ModelInfo {
            code: "Qdrant/clip-ViT-B-32-text",
            variant: "ClipVitB32",
            dimensions: 512,
            description: "CLIP ViT-B/32 text encoder",
        },
        // --- Gemma ---
        ModelInfo {
            code: "onnx-community/embeddinggemma-300m-ONNX",
            variant: "EmbeddingGemma300M",
            dimensions: 768,
            description: "Gemma 300M embedding model",
        },
        // --- Snowflake Arctic ---
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-xs",
            variant: "SnowflakeArcticEmbedXS",
            dimensions: 384,
            description: "Snowflake Arctic Embed XS",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-s",
            variant: "SnowflakeArcticEmbedS",
            dimensions: 384,
            description: "Snowflake Arctic Embed S",
        },
        ModelInfo {
            code: "Snowflake/snowflake-arctic-embed-m",
            variant: "SnowflakeArcticEmbedM",
            dimensions: 768,
            description: "Snowflake Arctic Embed M",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-m-long",
            variant: "SnowflakeArcticEmbedMLong",
            dimensions: 768,
            description: "Snowflake Arctic Embed M Long (2048 context)",
        },
        ModelInfo {
            code: "snowflake/snowflake-arctic-embed-l",
            variant: "SnowflakeArcticEmbedL",
            dimensions: 1024,
            description: "Snowflake Arctic Embed L",
        },
    ]
}
