#[cfg(feature = "remote-http")]
use std::future::Future;
#[cfg(feature = "remote-http")]
use std::pin::Pin;

#[cfg(feature = "remote-http")]
use reqwest::header::AUTHORIZATION;
#[cfg(feature = "remote-http")]
use serde_json::Value;

#[cfg(feature = "remote-http")]
use crate::Result;

#[cfg(feature = "remote-http")]
pub type PostJsonFuture =
    Pin<Box<dyn Future<Output = Result<(reqwest::StatusCode, Value)>> + Send + 'static>>;

#[cfg(feature = "remote-http")]
pub trait JsonHttpClient: Send + Sync {
    fn post_json(
        &self,
        url: String,
        api_key: Option<String>,
        timeout: std::time::Duration,
        body: Value,
    ) -> PostJsonFuture;
}

#[cfg(feature = "remote-http")]
#[derive(Debug, Default, Clone)]
pub struct ReqwestJsonHttpClient;

#[cfg(feature = "remote-http")]
impl JsonHttpClient for ReqwestJsonHttpClient {
    fn post_json(
        &self,
        url: String,
        api_key: Option<String>,
        timeout: std::time::Duration,
        body: Value,
    ) -> PostJsonFuture {
        Box::pin(async move {
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|e| crate::Error::Service(format!("Failed to build HTTP client: {e}")))?;

            let mut request = client.post(url).json(&body);
            if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
                request = request.header(AUTHORIZATION, format!("Bearer {key}"));
            }

            let response = request
                .send()
                .await
                .map_err(|e| crate::Error::Service(format!("Remote request failed: {e}")))?;
            let status = response.status();
            let value = response.json::<Value>().await.map_err(|e| {
                crate::Error::Service(format!("Failed to parse remote JSON response: {e}"))
            })?;
            Ok((status, value))
        })
    }
}
