//! Dynamic provider registry for prompt execution.
//!
//! This module implements the `ProviderRegistry` from ADR-001, enabling dynamic
//! registration of builtin and plugin providers with clear resolution order.

use crate::providers::{PromptProvider, PromptRequest};
use crate::PromptConfig;
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::RwLock;

/// Factory trait for creating provider instances on demand.
///
/// Providers require configuration (API keys, retry settings, etc.) that varies
/// per request, so we store factories rather than provider instances.
pub trait ProviderFactory: Send + Sync {
    /// Create a provider instance with the given configuration.
    fn create(
        &self,
        request: &PromptRequest,
        config: &PromptConfig<'_>,
    ) -> Result<Box<dyn PromptProvider>>;

    /// Return the provider identifier (e.g., "openai", "anthropic").
    fn id(&self) -> &str;

    /// Return a human-readable description of the provider.
    fn description(&self) -> &str {
        self.id()
    }
}

/// Metadata for a registered provider entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistration {
    /// Key used for resolution (`prefix` for builtins, model ID/prefix for plugins).
    pub key: String,
    /// Provider identifier returned by the underlying factory.
    pub provider_id: String,
    /// Human-readable description from the factory.
    pub description: String,
}

/// Registry holding builtin and plugin providers with clear resolution order.
///
/// Resolution order (per ADR-001):
/// 1. Check user aliases (handled externally via `resolve_user_alias`)
/// 2. Match against builtin provider prefixes (e.g., `openai/gpt-4`)
/// 3. Match against plugin-registered models
/// 4. Return actionable error with suggestions
pub struct ProviderRegistry {
    /// Built-in providers (OpenAI, Anthropic, etc.) keyed by prefix.
    builtin: RwLock<HashMap<String, Box<dyn ProviderFactory>>>,
    /// Plugin-registered providers keyed by model ID or prefix.
    plugin: RwLock<HashMap<String, Box<dyn ProviderFactory>>>,
    /// Track collision warnings to avoid duplicate messages.
    collision_warnings: RwLock<Vec<String>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            builtin: RwLock::new(HashMap::new()),
            plugin: RwLock::new(HashMap::new()),
            collision_warnings: RwLock::new(Vec::new()),
        }
    }

    /// Register a builtin provider factory.
    ///
    /// Builtin providers are keyed by their prefix (e.g., "openai", "anthropic").
    /// These take precedence over plugin-registered providers.
    pub fn register_builtin(&self, prefix: &str, factory: Box<dyn ProviderFactory>) {
        let mut builtin = self.builtin.write().unwrap();
        if builtin.contains_key(prefix) {
            self.record_collision_warning(&format!(
                "builtin provider '{}' already registered; replacing",
                prefix
            ));
        }
        builtin.insert(prefix.to_string(), factory);
    }

    /// Register a plugin-provided provider factory.
    ///
    /// Plugin providers can register either by prefix or by specific model ID.
    /// If a plugin tries to register a prefix that conflicts with a builtin,
    /// a warning is emitted and the plugin provider is still registered but
    /// won't be matched (builtin takes precedence).
    pub fn register_plugin(&self, model_id: &str, factory: Box<dyn ProviderFactory>) {
        // Check for builtin collision
        {
            let builtin = self.builtin.read().unwrap();
            let prefix = model_id.split('/').next().unwrap_or(model_id);
            if builtin.contains_key(prefix) {
                self.record_collision_warning(&format!(
                    "plugin attempted to register '{}' which collides with builtin provider '{}'; \
                     builtin takes precedence",
                    model_id, prefix
                ));
            }
        }

        // Check for plugin-vs-plugin collision
        {
            let plugin = self.plugin.read().unwrap();
            if plugin.contains_key(model_id) {
                self.record_collision_warning(&format!(
                    "plugin provider '{}' already registered; replacing with new registration",
                    model_id
                ));
            }
        }

        let mut plugin = self.plugin.write().unwrap();
        plugin.insert(model_id.to_string(), factory);
    }

    /// Create a provider instance for the given model.
    ///
    /// This combines resolution and creation into a single convenient method.
    /// The caller is responsible for alias resolution before calling this method.
    pub fn create_provider(
        &self,
        model_name: &str,
        request: &PromptRequest,
        config: &PromptConfig<'_>,
    ) -> Result<Box<dyn PromptProvider>> {
        // Extract provider prefix
        let prefix = model_name
            .split('/')
            .next()
            .unwrap_or(model_name)
            .to_lowercase();

        // 1. Try builtin providers first
        {
            let builtin = self.builtin.read().unwrap();
            if let Some(factory) = builtin.get(&prefix) {
                return factory.create(request, config);
            }
        }

        // 2. Try plugin providers (exact model ID match first)
        {
            let plugin = self.plugin.read().unwrap();
            if let Some(factory) = plugin.get(model_name) {
                return factory.create(request, config);
            }
            // Fallback to prefix match for plugins
            if let Some(factory) = plugin.get(&prefix) {
                return factory.create(request, config);
            }
        }

        // 3. No match found
        bail!(
            "Unsupported provider for model '{}'. \
             Available builtin providers: {}. \
             Use `llm models list` to see available models.",
            model_name,
            self.list_builtin_prefixes().join(", ")
        )
    }

    /// List all registered builtin provider prefixes.
    pub fn list_builtin_prefixes(&self) -> Vec<String> {
        let builtin = self.builtin.read().unwrap();
        let mut prefixes: Vec<String> = builtin.keys().cloned().collect();
        prefixes.sort();
        prefixes
    }

    /// List all registered plugin model IDs.
    pub fn list_plugin_models(&self) -> Vec<String> {
        let plugin = self.plugin.read().unwrap();
        let mut models: Vec<String> = plugin.keys().cloned().collect();
        models.sort();
        models
    }

    /// List metadata for builtin provider registrations.
    pub fn list_builtin_registrations(&self) -> Vec<ProviderRegistration> {
        let builtin = self.builtin.read().unwrap();
        let mut entries: Vec<ProviderRegistration> = builtin
            .iter()
            .map(|(key, factory)| ProviderRegistration {
                key: key.clone(),
                provider_id: factory.id().to_string(),
                description: factory.description().to_string(),
            })
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        entries
    }

    /// List metadata for plugin provider registrations.
    pub fn list_plugin_registrations(&self) -> Vec<ProviderRegistration> {
        let plugin = self.plugin.read().unwrap();
        let mut entries: Vec<ProviderRegistration> = plugin
            .iter()
            .map(|(key, factory)| ProviderRegistration {
                key: key.clone(),
                provider_id: factory.id().to_string(),
                description: factory.description().to_string(),
            })
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        entries
    }

    /// Check if a builtin provider is registered for the given prefix.
    pub fn has_builtin(&self, prefix: &str) -> bool {
        let builtin = self.builtin.read().unwrap();
        builtin.contains_key(prefix)
    }

    /// Check if a plugin provider is registered for the given model ID.
    pub fn has_plugin(&self, model_id: &str) -> bool {
        let plugin = self.plugin.read().unwrap();
        plugin.contains_key(model_id)
    }

    /// Get any collision warnings that have been recorded.
    pub fn collision_warnings(&self) -> Vec<String> {
        let warnings = self.collision_warnings.read().unwrap();
        warnings.clone()
    }

    /// Clear recorded collision warnings.
    pub fn clear_collision_warnings(&self) {
        let mut warnings = self.collision_warnings.write().unwrap();
        warnings.clear();
    }

    fn record_collision_warning(&self, message: &str) {
        let mut warnings = self.collision_warnings.write().unwrap();
        if !warnings.contains(&message.to_string()) {
            eprintln!("warning: {}", message);
            warnings.push(message.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{PromptCompletion, PromptProvider, StreamSink};

    /// Test provider that returns static responses.
    struct TestProvider {
        id: &'static str,
        response: String,
    }

    impl PromptProvider for TestProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn complete(&self, _request: PromptRequest) -> Result<PromptCompletion> {
            Ok(PromptCompletion::text(&self.response))
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        fn stream(&self, request: PromptRequest, sink: &mut dyn StreamSink) -> Result<()> {
            let completion = self.complete(request)?;
            sink.handle_text_delta(&completion.text)?;
            sink.handle_done()
        }
    }

    /// Test factory that creates TestProvider instances.
    struct TestFactory {
        id: &'static str,
        response: String,
    }

    impl TestFactory {
        fn new(id: &'static str, response: &str) -> Self {
            Self {
                id,
                response: response.to_string(),
            }
        }
    }

    impl ProviderFactory for TestFactory {
        fn create(
            &self,
            _request: &PromptRequest,
            _config: &PromptConfig<'_>,
        ) -> Result<Box<dyn PromptProvider>> {
            Ok(Box::new(TestProvider {
                id: self.id,
                response: self.response.clone(),
            }))
        }

        fn id(&self) -> &str {
            self.id
        }
    }

    #[test]
    fn test_empty_registry() {
        let registry = ProviderRegistry::new();
        assert!(registry.list_builtin_prefixes().is_empty());
        assert!(registry.list_plugin_models().is_empty());
    }

    #[test]
    fn test_register_builtin() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("test", Box::new(TestFactory::new("test", "hello")));

        assert!(registry.has_builtin("test"));
        assert!(!registry.has_builtin("other"));
        assert_eq!(registry.list_builtin_prefixes(), vec!["test"]);
    }

    #[test]
    fn test_register_plugin() {
        let registry = ProviderRegistry::new();
        registry.register_plugin(
            "custom/model-1",
            Box::new(TestFactory::new("custom", "plugin response")),
        );

        assert!(registry.has_plugin("custom/model-1"));
        assert!(!registry.has_plugin("other/model"));
        assert_eq!(registry.list_plugin_models(), vec!["custom/model-1"]);
    }

    #[test]
    fn test_resolve_builtin() {
        let registry = ProviderRegistry::new();
        registry.register_builtin(
            "openai",
            Box::new(TestFactory::new("openai", "openai response")),
        );

        let config = PromptConfig::default();
        let request = PromptRequest::user_only("openai/gpt-4".to_string(), "hello".to_string());

        let provider = registry.create_provider("openai/gpt-4", &request, &config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn test_resolve_plugin() {
        let registry = ProviderRegistry::new();
        registry.register_plugin(
            "custom/special-model",
            Box::new(TestFactory::new("custom", "plugin response")),
        );

        let config = PromptConfig::default();
        let request =
            PromptRequest::user_only("custom/special-model".to_string(), "hello".to_string());

        let provider = registry.create_provider("custom/special-model", &request, &config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.id(), "custom");
    }

    #[test]
    fn test_builtin_takes_precedence() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("openai", Box::new(TestFactory::new("openai", "builtin")));
        registry.register_plugin(
            "openai/gpt-4",
            Box::new(TestFactory::new("plugin-openai", "plugin")),
        );

        // Should have a collision warning
        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("collides with builtin"));

        let config = PromptConfig::default();
        let request = PromptRequest::user_only("openai/gpt-4".to_string(), "hello".to_string());

        // Builtin should be used, not plugin
        let provider = registry
            .create_provider("openai/gpt-4", &request, &config)
            .unwrap();
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn test_plugin_collision_warning() {
        let registry = ProviderRegistry::new();
        registry.register_plugin(
            "custom/model",
            Box::new(TestFactory::new("custom1", "first")),
        );
        registry.register_plugin(
            "custom/model",
            Box::new(TestFactory::new("custom2", "second")),
        );

        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("already registered")));

        // Second registration should win
        let config = PromptConfig::default();
        let request = PromptRequest::user_only("custom/model".to_string(), "hello".to_string());
        let provider = registry
            .create_provider("custom/model", &request, &config)
            .unwrap();
        assert_eq!(provider.id(), "custom2");
    }

    #[test]
    fn test_resolve_not_found() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("openai", Box::new(TestFactory::new("openai", "response")));

        let config = PromptConfig::default();
        let request = PromptRequest::user_only("unknown/model".to_string(), "hello".to_string());

        let result = registry.create_provider("unknown/model", &request, &config);
        assert!(result.is_err());

        let error = result.err().expect("expected error");
        assert!(error.to_string().contains("Unsupported provider"));
        assert!(error.to_string().contains("openai")); // Should suggest available providers
    }

    #[test]
    fn test_fallback_to_plugin_prefix() {
        let registry = ProviderRegistry::new();
        // Register plugin with prefix only
        registry.register_plugin(
            "custom",
            Box::new(TestFactory::new("custom", "prefix fallback")),
        );

        let config = PromptConfig::default();
        let request = PromptRequest::user_only("custom/any-model".to_string(), "hello".to_string());

        // Should match via prefix fallback
        let provider = registry.create_provider("custom/any-model", &request, &config);
        assert!(provider.is_ok());
    }

    #[test]
    fn test_clear_collision_warnings() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("test", Box::new(TestFactory::new("test1", "first")));
        registry.register_builtin("test", Box::new(TestFactory::new("test2", "second")));

        assert!(!registry.collision_warnings().is_empty());

        registry.clear_collision_warnings();
        assert!(registry.collision_warnings().is_empty());
    }

    #[test]
    fn test_list_registrations_include_metadata() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("openai", Box::new(TestFactory::new("openai", "builtin")));
        registry.register_plugin(
            "markov",
            Box::new(TestFactory::new("markov", "deterministic markov")),
        );

        let builtin = registry.list_builtin_registrations();
        assert_eq!(builtin.len(), 1);
        assert_eq!(builtin[0].key, "openai");
        assert_eq!(builtin[0].provider_id, "openai");

        let plugin = registry.list_plugin_registrations();
        assert_eq!(plugin.len(), 1);
        assert_eq!(plugin[0].key, "markov");
        assert_eq!(plugin[0].provider_id, "markov");
        assert_eq!(plugin[0].description, "markov");
    }

    #[test]
    fn test_provider_execution() {
        let registry = ProviderRegistry::new();
        registry.register_builtin("test", Box::new(TestFactory::new("test", "hello world")));

        let config = PromptConfig::default();
        let request = PromptRequest::user_only("test/model".to_string(), "prompt".to_string());

        let provider = registry
            .create_provider("test/model", &request, &config)
            .unwrap();
        let completion = provider.complete(request).unwrap();

        assert_eq!(completion.text, "hello world");
    }
}
