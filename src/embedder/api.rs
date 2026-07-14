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
        // Shared crate-wide HTTP policy (timeout/proxy); see `crate::net`.
        let client = crate::net::build_blocking_client(config.timeout, config.use_env_proxy, None)
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
            // Never place an unbounded provider body into a user-facing error.
            return Err(Error::Embedding(format!(
                "embedding endpoint returned HTTP {}: {}",
                status.as_u16(),
                crate::net::truncate_for_diagnostics(body.trim())
            )));
        }

        let parsed: EmbeddingResponse = serde_json::from_str(&body).map_err(|e| {
            Error::Embedding(format!(
                "failed to parse embedding response: {e}: {}",
                crate::net::truncate_for_diagnostics(body.trim())
            ))
        })?;

        if parsed.data.len() != texts.len() {
            return Err(Error::Embedding(format!(
                "expected {} embeddings, got {}",
                texts.len(),
                parsed.data.len()
            )));
        }

        // Validate the provider's `index` fields as a complete permutation of
        // 0..texts.len(), placing each vector directly into its slot — count
        // equality alone does not prove correct input↔vector correspondence.
        let mut ordered: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        for datum in parsed.data {
            if datum.index >= ordered.len() {
                return Err(Error::Embedding(format!(
                    "embedding response index {} is out of range for {} inputs",
                    datum.index,
                    texts.len()
                )));
            }
            if ordered[datum.index].is_some() {
                return Err(Error::Embedding(format!(
                    "embedding response contains duplicate index {}",
                    datum.index
                )));
            }
            if datum.embedding.len() != self.config.dimension {
                return Err(Error::Embedding(format!(
                    "embedding for input {} has dimension {}, expected {}",
                    datum.index,
                    datum.embedding.len(),
                    self.config.dimension
                )));
            }
            if let Some(bad) = datum.embedding.iter().find(|v| !v.is_finite()) {
                return Err(Error::Embedding(format!(
                    "embedding for input {} contains a non-finite value ({bad})",
                    datum.index
                )));
            }
            ordered[datum.index] = Some(datum.embedding);
        }

        // With count equality, no out-of-range, and no duplicates, every slot
        // is provably filled; the expect is defensive.
        Ok(ordered
            .into_iter()
            .map(|slot| slot.expect("validated permutation covers every slot"))
            .collect())
    }

    fn dimension(&self) -> usize {
        self.config.dimension
    }

    /// Provider identity: endpoint + model + dimension distinguish vector
    /// spaces across providers and models. The API key is deliberately
    /// excluded — it does not affect the vector space and must not be
    /// persisted.
    ///
    /// The descriptor is versioned (`api-space-v1`) so a future format change
    /// cannot collide with today's identities. Inconsequential trailing
    /// slashes on the endpoint are normalized away; meaningful path
    /// differences are preserved.
    fn fingerprint(&self) -> String {
        let endpoint = self.config.endpoint.trim_end_matches('/');
        format!(
            "api-space-v1|endpoint={}|model={}|dimension={}",
            endpoint, self.config.model, self.config.dimension
        )
    }
}
