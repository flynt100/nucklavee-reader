//! OpenAI-compatible embedding client (spec §7.1).
//!
//! Blocking `reqwest` POST to a `/v1/embeddings`-style endpoint. Works with
//! OpenAI, local `llama.cpp --embedding`, and most third-party services that
//! speak the same request/response shape:
//!
//! ```json
//! // request  -> { "model": "...", "input": ["a", "b"] }
//! // response <- { "data": [ { "embedding": [..], "index": 0 }, .. ] }
//! ```
//!
//! Network I/O is isolated here; `use_env_proxy` lets tests reach a loopback
//! mock server directly (matching `crate::net`).

use std::time::Duration;

use serde::Deserialize;

use crate::embedder::Embedder;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct ApiEmbedderConfig {
    /// Full embeddings endpoint URL (e.g. `https://api.openai.com/v1/embeddings`).
    pub endpoint: String,
    pub model: String,
    /// Bearer token; `None` for local servers that need no auth.
    pub api_key: Option<String>,
    /// Expected embedding dimension. Returned vectors are validated against it.
    pub dimension: usize,
    pub use_env_proxy: bool,
    pub timeout: Duration,
}

impl ApiEmbedderConfig {
    /// Config with sensible defaults (auth off, env proxy on, 30s timeout).
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>, dimension: usize) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: None,
            dimension,
            use_env_proxy: true,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
}

pub struct ApiEmbedder {
    config: ApiEmbedderConfig,
    client: reqwest::blocking::Client,
}

impl ApiEmbedder {
    pub fn new(config: ApiEmbedderConfig) -> Result<Self> {
        let mut builder = reqwest::blocking::Client::builder().timeout(config.timeout);
        if !config.use_env_proxy {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|e| Error::Embedding(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { config, client })
    }
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    #[serde(default)]
    index: usize,
}

impl Embedder for ApiEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request_body = serde_json::json!({
            "model": self.config.model,
            "input": texts,
        })
        .to_string();

        let mut req = self
            .client
            .post(&self.config.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request_body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }

        let response = req
            .send()
            .map_err(|e| Error::Embedding(format!("embedding request failed: {e}")))?;

        let status = response.status();
        let body = response
            .text()
            .map_err(|e| Error::Embedding(format!("failed to read embedding response: {e}")))?;
        if !status.is_success() {
            return Err(Error::Embedding(format!(
                "embedding endpoint returned HTTP {}: {}",
                status.as_u16(),
                body.trim()
            )));
        }

        let parsed: EmbeddingResponse = serde_json::from_str(&body)
            .map_err(|e| Error::Embedding(format!("failed to parse embedding response: {e}")))?;

        if parsed.data.len() != texts.len() {
            return Err(Error::Embedding(format!(
                "expected {} embeddings, got {}",
                texts.len(),
                parsed.data.len()
            )));
        }

        // Order by the provider's `index` so results line up with `texts`.
        let mut data = parsed.data;
        data.sort_by_key(|d| d.index);

        let mut out = Vec::with_capacity(data.len());
        for (i, datum) in data.into_iter().enumerate() {
            if datum.embedding.len() != self.config.dimension {
                return Err(Error::Embedding(format!(
                    "embedding {i} has dimension {}, expected {}",
                    datum.embedding.len(),
                    self.config.dimension
                )));
            }
            out.push(datum.embedding);
        }
        Ok(out)
    }

    fn dimension(&self) -> usize {
        self.config.dimension
    }
}
