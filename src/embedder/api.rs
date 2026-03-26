#[derive(Debug, Clone)]
pub struct ApiEmbedderConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiEmbedder {
    pub config: ApiEmbedderConfig,
}
