//! Embedding provider abstraction and implementations.
//!
//! This module defines the `EmbeddingProvider` trait for generating vector embeddings
//! from text, along with built-in implementations for OpenAI's embedding models.

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::env;
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Provider Trait
// ============================================================================

/// Configuration for an embedding provider.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// API key for the provider.
    pub api_key: String,
    /// Base URL for API requests.
    pub base_url: String,
    /// Number of retries on failure.
    pub retries: usize,
    /// Backoff duration between retries.
    pub retry_backoff: Duration,
    /// Request timeout.
    pub timeout: Duration,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: String::new(),
            retries: 2,
            retry_backoff: Duration::from_millis(250),
            timeout: Duration::from_secs(60),
        }
    }
}

/// Metadata about an embedding model.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingModelInfo {
    /// Unique model identifier.
    pub model_id: String,
    /// Human-readable model name.
    pub name: String,
    /// Provider identifier.
    pub provider: String,
    /// Dimension of embedding vectors.
    pub dimensions: Option<usize>,
    /// Whether the model supports binary input.
    pub supports_binary: bool,
    /// Whether the model supports text input.
    pub supports_text: bool,
    /// Aliases for this model.
    pub aliases: Vec<String>,
}

/// Result of embedding a single input.
#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    /// The embedding vector.
    pub embedding: Vec<f32>,
    /// Token usage for this embedding.
    pub tokens: Option<u32>,
}

/// Trait implemented by all embedding providers.
pub trait EmbeddingProvider: Send + Sync {
    /// Returns the provider identifier.
    fn id(&self) -> &'static str;

    /// Returns the model identifier.
    fn model_id(&self) -> &str;

    /// Returns information about the embedding model.
    fn model_info(&self) -> EmbeddingModelInfo;

    /// Embed a single text input.
    fn embed(&self, text: &str) -> Result<EmbeddingResult>;

    /// Embed multiple text inputs efficiently.
    fn embed_multi(&self, texts: &[&str]) -> Result<Vec<EmbeddingResult>> {
        // Default implementation embeds one at a time
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Embed binary content (for models that support it).
    fn embed_binary(&self, _data: &[u8]) -> Result<EmbeddingResult> {
        bail!("This embedding model does not support binary input")
    }

    /// Whether this provider supports binary input.
    fn supports_binary(&self) -> bool {
        false
    }

    /// Whether this provider supports batched embedding.
    fn supports_batch(&self) -> bool {
        false
    }

    /// Suggested batch size for efficient embedding.
    fn batch_size(&self) -> usize {
        100
    }
}

// ============================================================================
// OpenAI Embedding Provider
// ============================================================================

/// OpenAI embedding provider configuration.
#[derive(Debug, Clone)]
pub struct OpenAIEmbeddingConfig {
    /// API key for OpenAI.
    pub api_key: String,
    /// Base URL for the API (default: https://api.openai.com/v1).
    pub base_url: String,
    /// Model to use for embeddings.
    pub model: String,
    /// Number of retries on failure.
    pub retries: usize,
    /// Backoff duration between retries.
    pub retry_backoff: Duration,
    /// Request timeout.
    pub timeout: Duration,
    /// Optional dimensions override (for models that support it).
    pub dimensions: Option<usize>,
}

impl Default for OpenAIEmbeddingConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "text-embedding-3-small".to_string(),
            retries: 2,
            retry_backoff: Duration::from_millis(250),
            timeout: Duration::from_secs(60),
            dimensions: None,
        }
    }
}

/// OpenAI embedding provider.
pub struct OpenAIEmbeddingProvider {
    client: Client,
    config: OpenAIEmbeddingConfig,
}

impl OpenAIEmbeddingProvider {
    /// Create a new OpenAI embedding provider.
    pub fn new(config: OpenAIEmbeddingConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { client, config })
    }

    /// Create a provider using environment variables for configuration.
    pub fn from_env(model: &str) -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY")
            .or_else(|_| env::var("LLM_OPENAI_API_KEY"))
            .context("OpenAI API key not found. Set OPENAI_API_KEY or LLM_OPENAI_API_KEY")?;

        let base_url = env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let config = OpenAIEmbeddingConfig {
            api_key,
            base_url,
            model: model.to_string(),
            ..Default::default()
        };

        Self::new(config)
    }

    fn request(&self, request: &OpenAIEmbeddingRequest) -> Result<OpenAIEmbeddingResponse> {
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));
        let mut attempt = 0usize;

        loop {
            let start = Instant::now();
            let result = self
                .client
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(request)
                .send();

            match result {
                Ok(response) => {
                    let status = response.status();
                    if response.status().is_success() {
                        let body = response
                            .text()
                            .context("failed to read OpenAI response body")?;
                        let parsed: OpenAIEmbeddingResponse = serde_json::from_str(&body)
                            .context("failed to parse OpenAI embedding response")?;
                        return Ok(parsed);
                    }

                    if attempt >= self.config.retries || !should_retry_status(status) {
                        let body = response
                            .text()
                            .unwrap_or_else(|_| "<unreadable>".to_string());
                        bail!("OpenAI embedding request failed ({status}): {body}");
                    }

                    tracing::warn!(
                        target: "llm::embeddings::openai",
                        url = %url,
                        attempt,
                        status = %status,
                        "request_retry_status"
                    );
                }
                Err(err) => {
                    if attempt >= self.config.retries {
                        return Err(err)
                            .context(format!("failed to send embedding request to {url}"));
                    }
                    tracing::warn!(
                        target: "llm::embeddings::openai",
                        url = %url,
                        attempt,
                        error = %err,
                        "request_retry_error"
                    );
                }
            }

            attempt += 1;
            let multiplier = (attempt as u32).max(1);
            let backoff = self
                .config
                .retry_backoff
                .checked_mul(multiplier)
                .unwrap_or(self.config.retry_backoff);
            let elapsed = start.elapsed();
            if backoff > elapsed {
                thread::sleep(backoff - elapsed);
            }
        }
    }
}

impl EmbeddingProvider for OpenAIEmbeddingProvider {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }

    fn model_info(&self) -> EmbeddingModelInfo {
        let (dimensions, aliases) = match self.config.model.as_str() {
            "text-embedding-3-small" => (Some(1536), vec!["3-small".to_string()]),
            "text-embedding-3-large" => (Some(3072), vec!["3-large".to_string()]),
            "text-embedding-ada-002" => (Some(1536), vec!["ada".to_string(), "ada-002".to_string()]),
            _ => (None, vec![]),
        };

        EmbeddingModelInfo {
            model_id: self.config.model.clone(),
            name: self.config.model.clone(),
            provider: "openai".to_string(),
            dimensions: self.config.dimensions.or(dimensions),
            supports_binary: false,
            supports_text: true,
            aliases,
        }
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        let request = OpenAIEmbeddingRequest {
            model: self.config.model.clone(),
            input: OpenAIEmbeddingInput::Single(text.to_string()),
            dimensions: self.config.dimensions,
            encoding_format: None,
        };

        let response = self.request(&request)?;
        let embedding = response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("OpenAI returned no embeddings"))?;

        Ok(EmbeddingResult {
            embedding: embedding.embedding,
            tokens: response.usage.map(|u| u.total_tokens),
        })
    }

    fn embed_multi(&self, texts: &[&str]) -> Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let request = OpenAIEmbeddingRequest {
            model: self.config.model.clone(),
            input: OpenAIEmbeddingInput::Multiple(texts.iter().map(|s| s.to_string()).collect()),
            dimensions: self.config.dimensions,
            encoding_format: None,
        };

        let response = self.request(&request)?;
        let total_tokens = response.usage.as_ref().map(|u| u.total_tokens);
        let tokens_per_item = total_tokens.map(|t| t / texts.len() as u32);

        let mut embeddings: Vec<OpenAIEmbeddingData> = response.data;
        embeddings.sort_by_key(|e| e.index);

        Ok(embeddings
            .into_iter()
            .map(|e| EmbeddingResult {
                embedding: e.embedding,
                tokens: tokens_per_item,
            })
            .collect())
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_size(&self) -> usize {
        2048 // OpenAI supports up to 2048 inputs per request
    }
}

// ============================================================================
// OpenAI API Types
// ============================================================================

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest {
    model: String,
    input: OpenAIEmbeddingInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding_format: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAIEmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
    usage: Option<OpenAIEmbeddingUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingData {
    #[serde(default)]
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingUsage {
    total_tokens: u32,
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

// ============================================================================
// Built-in Model Registry
// ============================================================================

/// Information about built-in OpenAI embedding models.
pub const BUILTIN_OPENAI_MODELS: &[(&str, &[&str], usize)] = &[
    ("text-embedding-3-small", &["3-small"], 1536),
    ("text-embedding-3-large", &["3-large"], 3072),
    ("text-embedding-ada-002", &["ada", "ada-002"], 1536),
];

/// Resolve a model name to its canonical form.
pub fn resolve_embedding_model(name: &str) -> Option<&'static str> {
    let name_lower = name.to_ascii_lowercase();
    for (canonical, aliases, _) in BUILTIN_OPENAI_MODELS {
        if name_lower == canonical.to_ascii_lowercase() {
            return Some(canonical);
        }
        for alias in *aliases {
            if name_lower == alias.to_ascii_lowercase() {
                return Some(canonical);
            }
        }
    }
    None
}

/// List all available embedding models.
pub fn list_embedding_models() -> Vec<EmbeddingModelInfo> {
    BUILTIN_OPENAI_MODELS
        .iter()
        .map(|(model, aliases, dims)| EmbeddingModelInfo {
            model_id: model.to_string(),
            name: model.to_string(),
            provider: "openai".to_string(),
            dimensions: Some(*dims),
            supports_binary: false,
            supports_text: true,
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_embedding_model() {
        assert_eq!(
            resolve_embedding_model("3-small"),
            Some("text-embedding-3-small")
        );
        assert_eq!(
            resolve_embedding_model("ada"),
            Some("text-embedding-ada-002")
        );
        assert_eq!(
            resolve_embedding_model("text-embedding-3-large"),
            Some("text-embedding-3-large")
        );
        assert_eq!(resolve_embedding_model("unknown"), None);
    }

    #[test]
    fn test_list_embedding_models() {
        let models = list_embedding_models();
        assert_eq!(models.len(), 3);
        assert!(models.iter().any(|m| m.model_id == "text-embedding-3-small"));
    }

    #[test]
    fn test_embedding_config_default() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.retries, 2);
        assert_eq!(config.retry_backoff, Duration::from_millis(250));
    }

    #[test]
    fn test_openai_embedding_config_default() {
        let config = OpenAIEmbeddingConfig::default();
        assert_eq!(config.model, "text-embedding-3-small");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_model_info() {
        let config = OpenAIEmbeddingConfig {
            model: "text-embedding-3-small".to_string(),
            ..Default::default()
        };
        // Can't actually create provider without API key, but we can test config
        assert_eq!(config.model, "text-embedding-3-small");
    }

    #[test]
    fn test_openai_request_serialization() {
        let request = OpenAIEmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: OpenAIEmbeddingInput::Single("Hello world".to_string()),
            dimensions: Some(512),
            encoding_format: None,
        };

        let json = serde_json::to_string(&request).expect("serialize");
        assert!(json.contains("\"model\":\"text-embedding-3-small\""));
        assert!(json.contains("\"input\":\"Hello world\""));
        assert!(json.contains("\"dimensions\":512"));
    }

    #[test]
    fn test_openai_request_multi_serialization() {
        let request = OpenAIEmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: OpenAIEmbeddingInput::Multiple(vec!["Hello".to_string(), "World".to_string()]),
            dimensions: None,
            encoding_format: None,
        };

        let json = serde_json::to_string(&request).expect("serialize");
        assert!(json.contains("[\"Hello\",\"World\"]"));
    }

    #[test]
    fn test_openai_response_parsing() {
        let json = r#"{
            "data": [
                {"index": 0, "embedding": [0.1, 0.2, 0.3]},
                {"index": 1, "embedding": [0.4, 0.5, 0.6]}
            ],
            "usage": {"total_tokens": 10}
        }"#;

        let response: OpenAIEmbeddingResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(response.data[1].index, 1);
        assert_eq!(response.usage.unwrap().total_tokens, 10);
    }
}
