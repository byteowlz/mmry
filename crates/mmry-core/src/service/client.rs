use crate::service::manager::ServiceManager;
use crate::Result;

// Include generated protobuf code
mod proto {
    tonic::include_proto!("mmry.embeddings");
}

use proto::embedding_service_client::EmbeddingServiceClient;
use proto::EmbedRequest;
use proto::PingRequest;
use tonic::transport::Channel;

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
        let addr = format!("http://127.0.0.1:{}", port);

        // Connect to service
        let channel = Channel::from_shared(addr)
            .map_err(|e| crate::Error::Service(format!("Invalid service address: {}", e)))?
            .connect()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to connect: {}", e)))?;

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
            .map_err(|e| crate::Error::Service(format!("Embed request failed: {}", e)))?;

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
}
