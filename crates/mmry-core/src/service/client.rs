use crate::config::ExternalApiConfig;
use crate::memory::ChunkMethod;
use crate::memory::Memory;
use crate::memory::MemoryType;
use crate::service::manager::ServiceManager;
use crate::Result;
use serde::Deserialize;
use serde::Serialize;

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

/// Request payload for HMLR memory enrichment via external API
#[derive(Debug, Serialize)]
pub struct EnrichMemoryRequest {
    /// Content of the memory to enrich
    pub content: String,
    /// Category for the memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Memory type: episodic, semantic, procedural
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<String>,
    /// Tags for the memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Importance score (1-10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importance: Option<i32>,
    /// Agent ID (UUID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Optional query/prompt that led to this memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Response from HMLR memory enrichment (create new memory)
#[derive(Debug, Deserialize)]
pub struct EnrichMemoryResponse {
    /// Created memory ID
    pub id: String,
    /// Memory type
    pub memory_type: String,
    /// Category
    pub category: String,
    /// Tags
    pub tags: Vec<String>,
    /// Importance
    pub importance: i32,
    /// Facts extracted (if HMLR enabled)
    pub facts_extracted: usize,
    /// Whether this started a new topic
    pub is_new_topic: bool,
    /// Created timestamp
    pub created_at: String,
}

/// Request payload for enriching an existing memory via external API
#[derive(Debug, Serialize)]
pub struct EnrichExistingMemoryRequest {
    /// ID of the existing memory to enrich
    pub memory_id: String,
    /// Agent ID (UUID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Optional query/prompt context for routing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Previous memories in conversation (for HMLR routing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_history: Option<Vec<String>>,
}

/// Response from enriching an existing memory
#[derive(Debug, Deserialize)]
pub struct EnrichExistingMemoryResponse {
    /// Memory ID that was enriched
    pub memory_id: String,
    /// Facts extracted
    pub facts_extracted: usize,
    /// Whether this started a new topic
    pub is_new_topic: bool,
}

pub struct DaemonClient {
    client: Option<EmbeddingServiceClient<Channel>>,
    http_client: reqwest::Client,
    manager: ServiceManager,
    api_config: Option<ExternalApiConfig>,
}

#[derive(Debug, Clone)]
pub struct DaemonSearchOptions<'a> {
    pub query: &'a str,
    pub category: Option<&'a str>,
    pub limit: i64,
    pub mode: crate::config::SearchMode,
    pub rerank: bool,
    pub include_expired: bool,
    pub store: Option<&'a str>,
    /// Filter by tags (memory must contain at least one)
    pub tags: Vec<String>,
    /// Filter by memory type
    pub memory_type: Option<String>,
    /// Minimum importance threshold
    pub min_importance: Option<i32>,
    /// Only return memories created after this time (RFC 3339)
    pub after: Option<String>,
    /// Only return memories created before this time (RFC 3339)
    pub before: Option<String>,
}

impl DaemonClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: None,
            http_client: reqwest::Client::new(),
            manager: ServiceManager::new()?,
            api_config: None,
        })
    }

    /// Create a new client with external API configuration
    pub fn with_api_config(api_config: ExternalApiConfig) -> Result<Self> {
        Ok(Self {
            client: None,
            http_client: reqwest::Client::new(),
            manager: ServiceManager::new()?,
            api_config: Some(api_config),
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

    /// Get the external API base URL
    fn get_api_url(&self) -> Result<String> {
        let config = self
            .api_config
            .as_ref()
            .ok_or_else(|| crate::Error::Service("External API not configured".into()))?;

        if !config.enabled {
            return Err(crate::Error::Service("External API not enabled".into()));
        }

        Ok(format!("http://{}:{}", config.host, config.port))
    }

    /// Build authorization header if API key is configured
    fn get_auth_header(&self) -> Option<String> {
        self.api_config
            .as_ref()
            .and_then(|c| c.api_key.as_ref())
            .filter(|k| !k.is_empty())
            .map(|k| format!("Bearer {k}"))
    }

    /// Enrich a memory using the external API's HMLR pipeline (with LLM support)
    ///
    /// This calls the service's /v1/agents/memories endpoint which uses the
    /// configured LLM analyzer for fact extraction and routing.
    pub async fn enrich_memory(
        &self,
        request: EnrichMemoryRequest,
    ) -> Result<EnrichMemoryResponse> {
        let base_url = self.get_api_url()?;
        let url = format!("{base_url}/v1/agents/memories");

        let mut req = self.http_client.post(&url).json(&request);

        if let Some(auth) = self.get_auth_header() {
            req = req.header("Authorization", auth);
        }

        let response = req
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("HTTP request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::Error::Service(format!(
                "API error ({status}): {body}"
            )));
        }

        response
            .json::<EnrichMemoryResponse>()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to parse response: {e}")))
    }

    /// Enrich an existing memory using the external API's HMLR pipeline (with LLM support)
    ///
    /// This calls the service's /v1/agents/enrich endpoint which uses the
    /// configured LLM analyzer for fact extraction and routing on an existing memory.
    /// Unlike enrich_memory(), this does NOT create a new memory.
    pub async fn enrich_existing_memory(
        &self,
        request: EnrichExistingMemoryRequest,
    ) -> Result<EnrichExistingMemoryResponse> {
        let base_url = self.get_api_url()?;
        let url = format!("{base_url}/v1/agents/enrich");

        let mut req = self.http_client.post(&url).json(&request);

        if let Some(auth) = self.get_auth_header() {
            req = req.header("Authorization", auth);
        }

        let response = req
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("HTTP request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::Error::Service(format!(
                "API error ({status}): {body}"
            )));
        }

        response
            .json::<EnrichExistingMemoryResponse>()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to parse response: {e}")))
    }

    /// Check if the external API is available
    pub async fn is_api_available(&self) -> bool {
        let Ok(base_url) = self.get_api_url() else {
            return false;
        };

        // Try to connect to the API (a simple health check would be better,
        // but for now we just check if we can establish a connection)
        let url = format!("{base_url}/v1/embeddings");
        let mut req = self.http_client.post(&url).json(&serde_json::json!({
            "input": "test"
        }));

        if let Some(auth) = self.get_auth_header() {
            req = req.header("Authorization", auth);
        }

        // We don't care about the response, just that we can connect
        req.send().await.is_ok()
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

    pub async fn search(&mut self, opts: DaemonSearchOptions<'_>) -> Result<Vec<Memory>> {
        self.connect().await?;

        let mut client = self
            .client
            .as_mut()
            .ok_or_else(|| crate::Error::Service("Not connected".into()))?
            .clone();

        let request = SearchRequest {
            query: opts.query.to_string(),
            limit: opts.limit,
            category: opts.category.unwrap_or_default().to_string(),
            mode: search_mode_to_proto(opts.mode) as i32,
            rerank: opts.rerank,
            store: opts.store.unwrap_or_default().to_string(),
            include_expired: opts.include_expired,
            tags: opts.tags,
            memory_type: opts.memory_type.unwrap_or_default(),
            min_importance: opts.min_importance.unwrap_or(0),
            after: opts.after.unwrap_or_default(),
            before: opts.before.unwrap_or_default(),
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
    let memory_type = match mem.memory_type.to_lowercase().as_str() {
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

    let expires_at = if mem.expires_at.is_empty() {
        None
    } else {
        Some(
            chrono::DateTime::parse_from_rfc3339(&mem.expires_at)
                .map_err(|e| crate::Error::Service(format!("Invalid expires_at: {e}")))?
                .with_timezone(&chrono::Utc),
        )
    };

    let expired_at = if mem.expired_at.is_empty() {
        None
    } else {
        Some(
            chrono::DateTime::parse_from_rfc3339(&mem.expired_at)
                .map_err(|e| crate::Error::Service(format!("Invalid expired_at: {e}")))?
                .with_timezone(&chrono::Utc),
        )
    };

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
        expires_at,
        expired_at,
        helpful_count: 0,
        harmful_count: 0,
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
        bridge_block_id: None,
    })
}
