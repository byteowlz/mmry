use crate::memory::ChunkMethod;
use crate::memory::Memory;
use crate::memory::MemoryType;
use crate::service::manager::ServiceManager;
use crate::Result;

// Include generated protobuf code
mod proto {
    tonic::include_proto!("mmry.embeddings");
}

use proto::embedding_service_client::EmbeddingServiceClient;
use proto::EmbedRequest;
use proto::PingRequest;
use proto::SearchMode;
use proto::SearchRequest;
use tonic::transport::Channel;
use uuid::Uuid;

pub struct DaemonClient {
    client: Option<EmbeddingServiceClient<Channel>>,
    manager: ServiceManager,
}

impl DaemonClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: None,
            manager: ServiceManager::new()?,
        })
    }

    async fn connect(&mut self) -> Result<()> {
        if self.client.is_some() {
            return Ok(());
        }

        // Check if service is running
        if !self.manager.is_running() {
            return Err(crate::Error::Service("Service not running".into()));
        }

        // Read port
        let port = self.manager.read_port()?;
        let addr = format!("http://127.0.0.1:{port}");

        // Connect to service
        let channel = Channel::from_shared(addr)
            .map_err(|e| crate::Error::Service(format!("Invalid service address: {e}")))?
            .connect()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to connect: {e}")))?;

        self.client = Some(EmbeddingServiceClient::new(channel));
        Ok(())
    }

    pub async fn embed(&mut self, text: &str) -> Result<Option<Vec<f32>>> {
        self.connect().await?;

        let mut client = self
            .client
            .as_mut()
            .ok_or_else(|| crate::Error::Service("Not connected".into()))?
            .clone();

        let request = tonic::Request::new(EmbedRequest {
            text: text.to_string(),
        });

        let response = client
            .embed(request)
            .await
            .map_err(|e| crate::Error::Service(format!("Embed request failed: {e}")))?;

        let embedding = response.into_inner().embedding;

        if embedding.is_empty() {
            Ok(None)
        } else {
            Ok(Some(embedding))
        }
    }

    pub async fn ping(&mut self) -> Result<bool> {
        self.connect().await?;

        let mut client = self
            .client
            .as_mut()
            .ok_or_else(|| crate::Error::Service("Not connected".into()))?
            .clone();

        let request = tonic::Request::new(PingRequest {});

        match client.ping(request).await {
            Ok(_) => Ok(true),
            Err(_) => {
                // Connection lost, clear client
                self.client = None;
                Ok(false)
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    pub async fn search(
        &mut self,
        query: &str,
        category: Option<&str>,
        limit: i64,
        mode: crate::config::SearchMode,
        rerank: bool,
    ) -> Result<Vec<Memory>> {
        self.connect().await?;

        let mut client = self
            .client
            .as_mut()
            .ok_or_else(|| crate::Error::Service("Not connected".into()))?
            .clone();

        let request = SearchRequest {
            query: query.to_string(),
            limit,
            category: category.unwrap_or_default().to_string(),
            mode: search_mode_to_proto(mode) as i32,
            rerank,
        };

        let response = client
            .search(tonic::Request::new(request))
            .await
            .map_err(|e| crate::Error::Service(format!("Search request failed: {e}")))?;

        let proto = response.into_inner();
        proto.memories.into_iter().map(memory_from_proto).collect()
    }
}

fn search_mode_to_proto(mode: crate::config::SearchMode) -> SearchMode {
    match mode {
        crate::config::SearchMode::Hybrid => SearchMode::Hybrid,
        crate::config::SearchMode::Keyword => SearchMode::Keyword,
        crate::config::SearchMode::Fuzzy => SearchMode::Fuzzy,
        crate::config::SearchMode::Semantic => SearchMode::Semantic,
        crate::config::SearchMode::Bm25 => SearchMode::Bm25,
        crate::config::SearchMode::SparseEmbedding => SearchMode::Sparse,
    }
}

fn memory_from_proto(mem: proto::MemoryResult) -> Result<Memory> {
    let memory_type = match mem.memory_type.as_str() {
        "episodic" => MemoryType::Episodic,
        "semantic" => MemoryType::Semantic,
        "procedural" => MemoryType::Procedural,
        other => {
            return Err(crate::Error::Service(format!(
                "Unknown memory type in response: {other}"
            )))
        }
    };

    let parent_id =
        if mem.parent_id.is_empty() {
            None
        } else {
            Some(Uuid::parse_str(&mem.parent_id).map_err(|e| {
                crate::Error::Service(format!("Invalid parent_id in response: {e}"))
            })?)
        };

    let chunk_method = if mem.chunk_method.is_empty() {
        None
    } else {
        match mem.chunk_method.as_str() {
            "none" => Some(ChunkMethod::None),
            "paragraph" => Some(ChunkMethod::Paragraph),
            "sentence" => Some(ChunkMethod::Sentence),
            "word" => Some(ChunkMethod::Word),
            other => {
                return Err(crate::Error::Service(format!(
                    "Unknown chunk_method in response: {other}"
                )))
            }
        }
    };

    let sparse_embedding = mem.sparse_embedding.and_then(|sparse| {
        if sparse.indices.is_empty() {
            None
        } else {
            Some(crate::sparse_embeddings::StoredSparseEmbedding {
                indices: sparse.indices.into_iter().map(|v| v as usize).collect(),
                values: sparse.values,
            })
        }
    });

    Ok(Memory {
        id: Uuid::parse_str(&mem.id)
            .map_err(|e| crate::Error::Service(format!("Invalid memory id: {e}")))?,
        memory_type,
        content: mem.content,
        embedding: if mem.embedding.is_empty() {
            None
        } else {
            Some(mem.embedding)
        },
        sparse_embedding,
        metadata: serde_json::from_str(&mem.metadata_json)
            .unwrap_or_else(|_| serde_json::json!({})),
        importance: mem.importance,
        created_at: chrono::DateTime::parse_from_rfc3339(&mem.created_at)
            .map_err(|e| crate::Error::Service(format!("Invalid created_at: {e}")))?
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&mem.updated_at)
            .map_err(|e| crate::Error::Service(format!("Invalid updated_at: {e}")))?
            .with_timezone(&chrono::Utc),
        category: mem.category,
        tags: mem.tags,
        parent_id,
        chunk_index: if mem.chunk_index < 0 {
            None
        } else {
            Some(mem.chunk_index)
        },
        total_chunks: if mem.total_chunks < 0 {
            None
        } else {
            Some(mem.total_chunks)
        },
        chunk_method,
    })
}
