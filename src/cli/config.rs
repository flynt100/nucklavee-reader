//! CLI configuration (spec §8): read from `--config <path>` or, by default,
//! `~/.config/forge/config.toml`.
//!
//! ```toml
//! [storage]
//! database = "/path/to/library.sqlite"
//! vector_index = "/path/to/library.usearch"
//!
//! [embedding]
//! endpoint = "https://api.openai.com/v1/embeddings"
//! model = "text-embedding-3-small"
//! dimension = 1536
//! api_key = "sk-..."      # optional; omit for local servers
//! use_env_proxy = true    # optional; default true
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use nucklavee::embedder::api::ApiEmbedderConfig;
use nucklavee::{Error, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub storage: StorageConfig,
    pub embedding: EmbeddingConfig,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub database: PathBuf,
    pub vector_index: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub model: String,
    pub dimension: usize,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_true")]
    pub use_env_proxy: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Load config from an explicit path, or the default location.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => default_path()?,
        };
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            Error::InvalidInput(format!(
                "could not read config '{}': {e} (pass --config or create ~/.config/forge/config.toml)",
                path.display()
            ))
        })?;
        toml::from_str(&raw)
            .map_err(|e| Error::InvalidInput(format!("invalid config '{}': {e}", path.display())))
    }

    pub fn embedder_config(&self) -> ApiEmbedderConfig {
        ApiEmbedderConfig {
            endpoint: self.embedding.endpoint.clone(),
            model: self.embedding.model.clone(),
            api_key: self.embedding.api_key.clone(),
            dimension: self.embedding.dimension,
            use_env_proxy: self.embedding.use_env_proxy,
            timeout: Duration::from_secs(30),
        }
    }
}

fn default_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| Error::InvalidInput("HOME is not set; pass --config <path>".to_string()))?;
    Ok(PathBuf::from(home).join(".config/forge/config.toml"))
}
