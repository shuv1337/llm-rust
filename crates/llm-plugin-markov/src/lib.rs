//! `llm-markov` Rust canary plugin.
//!
//! This plugin provides a tiny deterministic Markov-chain text model to
//! validate plugin lifecycle wiring end-to-end.

use anyhow::Result;
use llm_core::PromptConfig;
use llm_plugin_api::{
    EmbeddingModelInfo, EmbeddingProvider, EmbeddingRegistrar, EmbeddingResult, ModelRegistrar,
    PluginCapability, PluginEntrypoint, PluginMetadata, PromptCompletion, PromptProvider,
    PromptRequest, ProviderFactory,
};
use std::sync::Arc;

pub mod markov;

/// Native Rust implementation of the `llm-markov` canary plugin.
pub struct MarkovPlugin;

impl PluginEntrypoint for MarkovPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "llm-markov".to_string(),
            version: "0.1.0".to_string(),
            capabilities: vec![PluginCapability::Models, PluginCapability::EmbeddingModels],
            min_host_version: Some("1.0.0".to_string()),
            description: Some("Deterministic Markov chain text and embedding model".to_string()),
        }
    }

    fn register_models(&self, reg: &mut dyn ModelRegistrar) -> Result<()> {
        reg.register_model_factory("markov", Box::new(MarkovProviderFactory))?;
        Ok(())
    }

    fn register_embedding_models(&self, reg: &mut dyn EmbeddingRegistrar) -> Result<()> {
        reg.register_embedding_provider(Arc::new(MarkovEmbeddingProvider))?;
        Ok(())
    }
}

struct MarkovProviderFactory;

impl ProviderFactory for MarkovProviderFactory {
    fn create(
        &self,
        _request: &PromptRequest,
        _config: &PromptConfig<'_>,
    ) -> Result<Box<dyn PromptProvider>> {
        Ok(Box::new(MarkovProvider))
    }

    fn id(&self) -> &str {
        "markov"
    }

    fn description(&self) -> &str {
        "Deterministic Markov chain provider"
    }
}

#[derive(Default)]
struct MarkovProvider;

impl PromptProvider for MarkovProvider {
    fn id(&self) -> &'static str {
        "markov"
    }

    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion> {
        let input = last_user_text(&request);
        let generated =
            markov::generate_markov_text(&input, request.max_tokens.unwrap_or(32) as usize);
        Ok(PromptCompletion::text(generated))
    }
}

fn last_user_text(request: &PromptRequest) -> String {
    request
        .messages
        .iter()
        .rev()
        .find(|msg| matches!(msg.role, llm_plugin_api::MessageRole::User))
        .map(|msg| msg.content.clone())
        .unwrap_or_default()
}

#[derive(Default)]
struct MarkovEmbeddingProvider;

impl EmbeddingProvider for MarkovEmbeddingProvider {
    fn id(&self) -> &'static str {
        "markov"
    }

    fn model_id(&self) -> &str {
        "markov-embedding"
    }

    fn model_info(&self) -> EmbeddingModelInfo {
        EmbeddingModelInfo {
            model_id: "markov-embedding".to_string(),
            name: "Markov deterministic embedding".to_string(),
            provider: "markov".to_string(),
            dimensions: Some(8),
            supports_binary: false,
            supports_text: true,
            aliases: vec!["markov-embed".to_string()],
        }
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        Ok(EmbeddingResult {
            embedding: deterministic_embedding(text),
            tokens: Some(text.split_whitespace().count() as u32),
        })
    }

    fn supports_batch(&self) -> bool {
        true
    }

    fn batch_size(&self) -> usize {
        128
    }
}

fn deterministic_embedding(text: &str) -> Vec<f32> {
    let mut buckets = [0.0f32; 8];
    for (index, byte) in text.bytes().enumerate() {
        let bucket = index % buckets.len();
        buckets[bucket] += (byte as f32) / 255.0;
    }
    let magnitude = buckets
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if magnitude > 0.0 {
        for value in &mut buckets {
            *value /= magnitude;
        }
    }
    buckets.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingModelRegistrar {
        models: Vec<String>,
    }

    struct RecordingEmbeddingRegistrar {
        models: Vec<String>,
    }

    impl RecordingModelRegistrar {
        fn new() -> Self {
            Self { models: Vec::new() }
        }
    }

    impl ModelRegistrar for RecordingModelRegistrar {
        fn register_model_factory(
            &mut self,
            model_id: &str,
            _factory: Box<dyn ProviderFactory>,
        ) -> llm_plugin_api::PluginResult<()> {
            self.models.push(model_id.to_string());
            Ok(())
        }
    }

    impl EmbeddingRegistrar for RecordingEmbeddingRegistrar {
        fn register_embedding_model(
            &mut self,
            model: EmbeddingModelInfo,
        ) -> llm_plugin_api::PluginResult<()> {
            self.models.push(model.model_id);
            Ok(())
        }
    }

    #[test]
    fn metadata_is_correct() {
        let plugin = MarkovPlugin;
        let metadata = plugin.metadata();
        assert_eq!(metadata.id, "llm-markov");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(
            metadata.capabilities,
            vec![PluginCapability::Models, PluginCapability::EmbeddingModels]
        );
    }

    #[test]
    fn register_models_registers_markov() {
        let plugin = MarkovPlugin;
        let mut reg = RecordingModelRegistrar::new();
        plugin.register_models(&mut reg).unwrap();
        assert_eq!(reg.models, vec!["markov"]);
    }

    #[test]
    fn register_embedding_models_registers_markov_embedding() {
        let plugin = MarkovPlugin;
        let mut reg = RecordingEmbeddingRegistrar { models: Vec::new() };
        plugin.register_embedding_models(&mut reg).unwrap();
        assert_eq!(reg.models, vec!["markov-embedding"]);
    }

    #[test]
    fn markov_generation_is_deterministic() {
        let prompt = "the quick brown fox jumps over the lazy dog";
        let a = markov::generate_markov_text(prompt, 12);
        let b = markov::generate_markov_text(prompt, 12);
        assert_eq!(a, b);
    }

    #[test]
    fn markov_generation_respects_max_tokens() {
        let prompt = "one two three four five";
        let output = markov::generate_markov_text(prompt, 5);
        assert_eq!(output.split_whitespace().count(), 5);
    }

    #[test]
    fn markov_embedding_is_deterministic() {
        let provider = MarkovEmbeddingProvider;
        let a = provider.embed("the quick brown fox").unwrap();
        let b = provider.embed("the quick brown fox").unwrap();
        assert_eq!(a.embedding, b.embedding);
        assert_eq!(a.embedding.len(), 8);
        assert_eq!(a.tokens, Some(4));
    }
}
