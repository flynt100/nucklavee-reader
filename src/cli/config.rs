//! CLI configuration (spec §8): read from `--config <path>` or, by default,
//! `~/.config/nucklavee/config.toml` (with legacy Forge-path fallback).
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
//! api_key_env = "OPENAI_API_KEY" # optional; preferred for secrets
//! use_env_proxy = true    # optional; default true
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use nucklavee::embedder::api::ApiEmbedderConfig;
use nucklavee::{Error, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub storage: StorageConfig,
    pub embedding: EmbeddingConfig,
}

#[derive(Deserialize)]
pub struct StorageConfig {
    pub database: PathBuf,
    pub vector_index: PathBuf,
}

#[derive(Deserialize)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub model: String,
    pub dimension: usize,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
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
                "could not read config '{}': {e} (pass --config or create ~/.config/nucklavee/config.toml)",
                path.display()
            ))
        })?;
        toml::from_str(&raw)
            .map_err(|e| Error::InvalidInput(format!("invalid config '{}': {e}", path.display())))
    }

    pub fn embedder_config(&self) -> ApiEmbedderConfig {
        let api_key = resolve_api_key(&self.embedding, |name| std::env::var(name).ok());
        ApiEmbedderConfig {
            endpoint: self.embedding.endpoint.clone(),
            model: self.embedding.model.clone(),
            api_key,
            dimension: self.embedding.dimension,
            use_env_proxy: self.embedding.use_env_proxy,
            timeout: Duration::from_secs(30),
        }
    }
}

fn default_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| {
            Error::InvalidInput("HOME/USERPROFILE is not set; pass --config <path>".to_string())
        })?;
    Ok(default_path_for_home(Path::new(&home)))
}

fn default_path_for_home(home: &Path) -> PathBuf {
    let config_root = home.join(".config");
    let preferred = config_root.join("nucklavee/config.toml");
    let legacy = config_root.join("forge/config.toml");
    if !preferred.exists() && legacy.exists() {
        eprintln!(
            "warning: using legacy config '{}'; move it to '{}'",
            legacy.display(),
            preferred.display()
        );
        return legacy;
    }
    preferred
}

fn resolve_api_key(
    config: &EmbeddingConfig,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    config
        .api_key_env
        .as_deref()
        .and_then(&lookup)
        .filter(|value| !value.is_empty())
        .or_else(|| lookup("NUCKLAVEE_EMBEDDING_API_KEY").filter(|value| !value.is_empty()))
        .or_else(|| config.api_key.clone().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedding(api_key: Option<&str>, api_key_env: Option<&str>) -> EmbeddingConfig {
        EmbeddingConfig {
            endpoint: "http://example.invalid/v1/embeddings".to_string(),
            model: "test".to_string(),
            dimension: 8,
            api_key: api_key.map(str::to_string),
            api_key_env: api_key_env.map(str::to_string),
            use_env_proxy: false,
        }
    }

    #[test]
    fn configured_environment_variable_has_highest_precedence() {
        let config = embedding(Some("file-secret"), Some("OPENAI_API_KEY"));
        let key = resolve_api_key(&config, |name| match name {
            "OPENAI_API_KEY" => Some("configured-env".to_string()),
            "NUCKLAVEE_EMBEDDING_API_KEY" => Some("standard-env".to_string()),
            _ => None,
        });
        assert_eq!(key.as_deref(), Some("configured-env"));
    }

    #[test]
    fn standard_environment_variable_precedes_legacy_file_value() {
        let config = embedding(Some("file-secret"), None);
        let key = resolve_api_key(&config, |name| {
            (name == "NUCKLAVEE_EMBEDDING_API_KEY").then(|| "standard-env".to_string())
        });
        assert_eq!(key.as_deref(), Some("standard-env"));
    }

    #[test]
    fn legacy_file_value_remains_a_fallback() {
        let config = embedding(Some("file-secret"), None);
        assert_eq!(
            resolve_api_key(&config, |_| None).as_deref(),
            Some("file-secret")
        );
    }

    #[test]
    fn missing_secret_means_unauthenticated() {
        let config = embedding(None, None);
        assert_eq!(resolve_api_key(&config, |_| None), None);
    }

    #[test]
    fn empty_environment_values_fall_through() {
        // An empty variable is treated as unset at every precedence level,
        // never as an empty bearer token.
        let config = embedding(Some("file-secret"), Some("OPENAI_API_KEY"));
        let key = resolve_api_key(&config, |_| Some(String::new()));
        assert_eq!(key.as_deref(), Some("file-secret"));

        let config = embedding(None, None);
        assert_eq!(resolve_api_key(&config, |_| Some(String::new())), None);
    }

    #[test]
    fn preferred_config_path_wins_over_legacy() {
        let home = std::env::temp_dir().join(format!("nucklavee_config_{}", uuid::Uuid::new_v4()));
        let preferred = home.join(".config/nucklavee/config.toml");
        let legacy = home.join(".config/forge/config.toml");
        std::fs::create_dir_all(preferred.parent().unwrap()).unwrap();
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&preferred, "preferred").unwrap();
        std::fs::write(&legacy, "legacy").unwrap();
        assert_eq!(default_path_for_home(&home), preferred);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn legacy_config_is_used_only_when_preferred_is_absent() {
        let home = std::env::temp_dir().join(format!("nucklavee_config_{}", uuid::Uuid::new_v4()));
        let legacy = home.join(".config/forge/config.toml");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "legacy").unwrap();
        assert_eq!(default_path_for_home(&home), legacy);
        let _ = std::fs::remove_dir_all(home);
    }
}
