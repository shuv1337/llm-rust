//! `llm-markov` Rust canary plugin.
//!
//! This plugin provides a tiny deterministic Markov-chain text model to
//! validate plugin lifecycle wiring end-to-end.

use anyhow::Result;
use llm_core::PromptConfig;
use llm_plugin_api::{
    ModelRegistrar, PluginCapability, PluginEntrypoint, PluginMetadata, PromptCompletion,
    PromptProvider, PromptRequest, ProviderFactory,
};

pub mod markov;

/// Native Rust implementation of the `llm-markov` canary plugin.
pub struct MarkovPlugin;

impl PluginEntrypoint for MarkovPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "llm-markov".to_string(),
            version: "0.1.0".to_string(),
            capabilities: vec![PluginCapability::Models],
            min_host_version: Some("1.0.0".to_string()),
            description: Some("Deterministic Markov chain text model".to_string()),
        }
    }

    fn register_models(&self, reg: &mut dyn ModelRegistrar) -> Result<()> {
        reg.register_model_factory("markov", Box::new(MarkovProviderFactory))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingModelRegistrar {
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

    #[test]
    fn metadata_is_correct() {
        let plugin = MarkovPlugin;
        let metadata = plugin.metadata();
        assert_eq!(metadata.id, "llm-markov");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(metadata.capabilities, vec![PluginCapability::Models]);
    }

    #[test]
    fn register_models_registers_markov() {
        let plugin = MarkovPlugin;
        let mut reg = RecordingModelRegistrar::new();
        plugin.register_models(&mut reg).unwrap();
        assert_eq!(reg.models, vec!["markov"]);
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
}
