//! Dynamic embedding model registry.
//!
//! This module implements `EmbeddingRegistry` for dynamic registration of
//! builtin and plugin embedding models, following the pattern from ADR-001.

use crate::provider::{EmbeddingModelInfo, EmbeddingProvider};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Registry holding builtin and plugin embedding models with clear resolution order.
///
/// Resolution order:
/// 1. Exact match against builtin model IDs
/// 2. Alias match against builtin models
/// 3. Exact match against plugin model IDs
/// 4. Alias match against plugin models
/// 5. Return None (not found)
///
/// Builtin models always take precedence over plugin models. When a plugin
/// attempts to register a model ID that collides with a builtin, a warning
/// is emitted and the plugin model is still registered but won't be matched.
pub struct EmbeddingRegistry {
    /// Built-in embedding models (OpenAI, etc.) keyed by model ID.
    builtin: RwLock<HashMap<String, EmbeddingModelInfo>>,
    /// Plugin-registered embedding models keyed by model ID.
    plugin: RwLock<HashMap<String, EmbeddingModelInfo>>,
    /// Executable plugin embedding providers keyed by model ID.
    plugin_providers: RwLock<HashMap<String, Arc<dyn EmbeddingProvider>>>,
    /// Track collision warnings to avoid duplicate messages.
    collision_warnings: RwLock<Vec<String>>,
}

impl Default for EmbeddingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            builtin: RwLock::new(HashMap::new()),
            plugin: RwLock::new(HashMap::new()),
            plugin_providers: RwLock::new(HashMap::new()),
            collision_warnings: RwLock::new(Vec::new()),
        }
    }

    /// Create a registry pre-populated with builtin OpenAI models.
    pub fn with_defaults() -> Self {
        let registry = Self::new();
        registry.register_openai_builtins();
        registry
    }

    /// Register the builtin OpenAI embedding models.
    pub fn register_openai_builtins(&self) {
        // text-embedding-3-small
        self.register_builtin(EmbeddingModelInfo {
            model_id: "text-embedding-3-small".to_string(),
            name: "text-embedding-3-small".to_string(),
            provider: "openai".to_string(),
            dimensions: Some(1536),
            supports_binary: false,
            supports_text: true,
            aliases: vec!["3-small".to_string()],
        });

        // text-embedding-3-large
        self.register_builtin(EmbeddingModelInfo {
            model_id: "text-embedding-3-large".to_string(),
            name: "text-embedding-3-large".to_string(),
            provider: "openai".to_string(),
            dimensions: Some(3072),
            supports_binary: false,
            supports_text: true,
            aliases: vec!["3-large".to_string()],
        });

        // text-embedding-ada-002
        self.register_builtin(EmbeddingModelInfo {
            model_id: "text-embedding-ada-002".to_string(),
            name: "text-embedding-ada-002".to_string(),
            provider: "openai".to_string(),
            dimensions: Some(1536),
            supports_binary: false,
            supports_text: true,
            aliases: vec!["ada".to_string(), "ada-002".to_string()],
        });
    }

    /// Register a builtin embedding model.
    ///
    /// Builtin models take precedence over plugin models in resolution.
    pub fn register_builtin(&self, model: EmbeddingModelInfo) {
        let mut builtin = self.builtin.write().unwrap();
        if builtin.contains_key(&model.model_id) {
            self.record_collision_warning(&format!(
                "builtin embedding model '{}' already registered; replacing",
                model.model_id
            ));
        }
        builtin.insert(model.model_id.clone(), model);
    }

    /// Register a plugin-provided embedding model.
    ///
    /// If a plugin tries to register a model ID that conflicts with a builtin,
    /// a warning is emitted and the plugin model is still registered but
    /// won't be matched (builtin takes precedence).
    pub fn register_plugin(&self, model: EmbeddingModelInfo) {
        // Check for builtin collision
        {
            let builtin = self.builtin.read().unwrap();
            if builtin.contains_key(&model.model_id) {
                self.record_collision_warning(&format!(
                    "plugin attempted to register embedding model '{}' which collides with \
                     builtin model; builtin takes precedence",
                    model.model_id
                ));
            }
            // Also check aliases
            for alias in &model.aliases {
                if self.resolve_alias_in_map(&builtin, alias).is_some() {
                    self.record_collision_warning(&format!(
                        "plugin embedding model '{}' has alias '{}' which collides with \
                         a builtin model alias; builtin takes precedence",
                        model.model_id, alias
                    ));
                }
            }
        }

        // Check for plugin-vs-plugin collision
        {
            let plugin = self.plugin.read().unwrap();
            if plugin.contains_key(&model.model_id) {
                self.record_collision_warning(&format!(
                    "plugin embedding model '{}' already registered; replacing with new registration",
                    model.model_id
                ));
            }
        }

        let mut plugin = self.plugin.write().unwrap();
        plugin.insert(model.model_id.clone(), model);
    }

    /// Register an executable plugin-provided embedding provider.
    ///
    /// The provider's [`EmbeddingProvider::model_info`] metadata is registered
    /// alongside the executable provider so list/resolve and runtime execution
    /// use the same model identifier.
    pub fn register_plugin_provider(&self, provider: Arc<dyn EmbeddingProvider>) {
        let model = provider.model_info();
        let model_id = model.model_id.clone();
        self.register_plugin(model);

        let mut providers = self.plugin_providers.write().unwrap();
        providers.insert(model_id, provider);
    }

    /// Resolve a model name to its canonical model ID.
    ///
    /// Checks exact matches first, then aliases. Builtin models take precedence.
    pub fn resolve(&self, name: &str) -> Option<String> {
        let name_lower = name.to_ascii_lowercase();

        // 1. Check builtin models (exact match, then aliases)
        {
            let builtin = self.builtin.read().unwrap();

            // Exact match
            for (model_id, _) in builtin.iter() {
                if model_id.to_ascii_lowercase() == name_lower {
                    return Some(model_id.clone());
                }
            }

            // Alias match
            if let Some(model_id) = self.resolve_alias_in_map(&builtin, &name_lower) {
                return Some(model_id);
            }
        }

        // 2. Check plugin models (exact match, then aliases)
        {
            let plugin = self.plugin.read().unwrap();

            // Exact match
            for (model_id, _) in plugin.iter() {
                if model_id.to_ascii_lowercase() == name_lower {
                    return Some(model_id.clone());
                }
            }

            // Alias match
            if let Some(model_id) = self.resolve_alias_in_map(&plugin, &name_lower) {
                return Some(model_id);
            }
        }

        None
    }

    /// Get model info by exact model ID.
    pub fn get(&self, model_id: &str) -> Option<EmbeddingModelInfo> {
        // Check builtin first
        {
            let builtin = self.builtin.read().unwrap();
            if let Some(model) = builtin.get(model_id) {
                return Some(model.clone());
            }
        }

        // Then check plugin
        {
            let plugin = self.plugin.read().unwrap();
            if let Some(model) = plugin.get(model_id) {
                return Some(model.clone());
            }
        }

        None
    }

    /// Get an executable plugin provider by exact model ID.
    pub fn get_plugin_provider(&self, model_id: &str) -> Option<Arc<dyn EmbeddingProvider>> {
        let providers = self.plugin_providers.read().unwrap();
        providers.get(model_id).cloned()
    }

    /// List all registered embedding models (builtin first, then plugin).
    pub fn list(&self) -> Vec<EmbeddingModelInfo> {
        let mut models = Vec::new();

        // Add builtins first
        {
            let builtin = self.builtin.read().unwrap();
            let mut builtin_models: Vec<_> = builtin.values().cloned().collect();
            builtin_models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
            models.extend(builtin_models);
        }

        // Then plugins
        {
            let plugin = self.plugin.read().unwrap();
            let mut plugin_models: Vec<_> = plugin.values().cloned().collect();
            plugin_models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
            models.extend(plugin_models);
        }

        models
    }

    /// List only builtin model IDs.
    pub fn list_builtin_ids(&self) -> Vec<String> {
        let builtin = self.builtin.read().unwrap();
        let mut ids: Vec<String> = builtin.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// List only plugin model IDs.
    pub fn list_plugin_ids(&self) -> Vec<String> {
        let plugin = self.plugin.read().unwrap();
        let mut ids: Vec<String> = plugin.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Check if a builtin model is registered with the given ID.
    pub fn has_builtin(&self, model_id: &str) -> bool {
        let builtin = self.builtin.read().unwrap();
        builtin.contains_key(model_id)
    }

    /// Check if a plugin model is registered with the given ID.
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

    fn resolve_alias_in_map(
        &self,
        map: &HashMap<String, EmbeddingModelInfo>,
        name_lower: &str,
    ) -> Option<String> {
        for (model_id, info) in map.iter() {
            for alias in &info.aliases {
                if alias.to_ascii_lowercase() == name_lower {
                    return Some(model_id.clone());
                }
            }
        }
        None
    }
}

// ============================================================================
// Global Registry
// ============================================================================

use std::sync::OnceLock;

/// Global embedding registry instance.
static GLOBAL_REGISTRY: OnceLock<EmbeddingRegistry> = OnceLock::new();

/// Get the global embedding registry, initializing with defaults if needed.
pub fn global_registry() -> &'static EmbeddingRegistry {
    GLOBAL_REGISTRY.get_or_init(EmbeddingRegistry::with_defaults)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::EmbeddingResult;
    use anyhow::Result;

    fn test_model(id: &str, aliases: &[&str]) -> EmbeddingModelInfo {
        EmbeddingModelInfo {
            model_id: id.to_string(),
            name: id.to_string(),
            provider: "test".to_string(),
            dimensions: Some(256),
            supports_binary: false,
            supports_text: true,
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
        }
    }

    struct TestEmbeddingProvider {
        model: EmbeddingModelInfo,
    }

    impl EmbeddingProvider for TestEmbeddingProvider {
        fn id(&self) -> &'static str {
            "test"
        }

        fn model_id(&self) -> &str {
            &self.model.model_id
        }

        fn model_info(&self) -> EmbeddingModelInfo {
            self.model.clone()
        }

        fn embed(&self, text: &str) -> Result<EmbeddingResult> {
            Ok(EmbeddingResult {
                embedding: vec![text.len() as f32],
                tokens: None,
            })
        }
    }

    #[test]
    fn test_empty_registry() {
        let registry = EmbeddingRegistry::new();
        assert!(registry.list_builtin_ids().is_empty());
        assert!(registry.list_plugin_ids().is_empty());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_with_defaults() {
        let registry = EmbeddingRegistry::with_defaults();
        let ids = registry.list_builtin_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"text-embedding-3-small".to_string()));
        assert!(ids.contains(&"text-embedding-3-large".to_string()));
        assert!(ids.contains(&"text-embedding-ada-002".to_string()));
    }

    #[test]
    fn test_register_builtin() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("test-model", &["test"]));

        assert!(registry.has_builtin("test-model"));
        assert!(!registry.has_builtin("other"));
        assert_eq!(registry.list_builtin_ids(), vec!["test-model"]);
    }

    #[test]
    fn test_register_plugin() {
        let registry = EmbeddingRegistry::new();
        registry.register_plugin(test_model("plugin-model", &["pm"]));

        assert!(registry.has_plugin("plugin-model"));
        assert!(!registry.has_plugin("other"));
        assert_eq!(registry.list_plugin_ids(), vec!["plugin-model"]);
    }

    #[test]
    fn test_resolve_exact_match() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("text-embedding-3-small", &["3-small"]));

        assert_eq!(
            registry.resolve("text-embedding-3-small"),
            Some("text-embedding-3-small".to_string())
        );
    }

    #[test]
    fn test_resolve_case_insensitive() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("text-embedding-3-small", &["3-small"]));

        assert_eq!(
            registry.resolve("TEXT-EMBEDDING-3-SMALL"),
            Some("text-embedding-3-small".to_string())
        );
        assert_eq!(
            registry.resolve("3-SMALL"),
            Some("text-embedding-3-small".to_string())
        );
    }

    #[test]
    fn test_resolve_alias() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("text-embedding-ada-002", &["ada", "ada-002"]));

        assert_eq!(
            registry.resolve("ada"),
            Some("text-embedding-ada-002".to_string())
        );
        assert_eq!(
            registry.resolve("ada-002"),
            Some("text-embedding-ada-002".to_string())
        );
    }

    #[test]
    fn test_resolve_not_found() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("test-model", &[]));

        assert_eq!(registry.resolve("unknown"), None);
    }

    #[test]
    fn test_builtin_takes_precedence() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("shared-model", &["shared"]));
        registry.register_plugin(test_model("shared-model", &["shared"]));

        // Should have collision warnings
        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("collides with builtin")));

        // Builtin should be resolved
        let model = registry.get("shared-model");
        assert!(model.is_some());
        // Both are registered, but get() returns builtin first
        assert!(registry.has_builtin("shared-model"));
        assert!(registry.has_plugin("shared-model"));
    }

    #[test]
    fn test_plugin_alias_collision() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("builtin-model", &["bm"]));
        registry.register_plugin(test_model("plugin-model", &["bm"])); // Same alias!

        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings
            .iter()
            .any(|w| w.contains("alias 'bm' which collides")));

        // Builtin alias takes precedence
        assert_eq!(registry.resolve("bm"), Some("builtin-model".to_string()));
    }

    #[test]
    fn test_plugin_collision_warning() {
        let registry = EmbeddingRegistry::new();
        registry.register_plugin(test_model("plugin-model", &[]));
        registry.register_plugin(test_model("plugin-model", &[])); // Duplicate

        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("already registered")));
    }

    #[test]
    fn test_list_ordering() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("b-builtin", &[]));
        registry.register_builtin(test_model("a-builtin", &[]));
        registry.register_plugin(test_model("b-plugin", &[]));
        registry.register_plugin(test_model("a-plugin", &[]));

        let models = registry.list();
        let ids: Vec<_> = models.iter().map(|m| m.model_id.as_str()).collect();

        // Builtins first (sorted), then plugins (sorted)
        assert_eq!(ids, vec!["a-builtin", "b-builtin", "a-plugin", "b-plugin"]);
    }

    #[test]
    fn test_get_model_info() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(EmbeddingModelInfo {
            model_id: "test-model".to_string(),
            name: "Test Model".to_string(),
            provider: "test-provider".to_string(),
            dimensions: Some(512),
            supports_binary: true,
            supports_text: true,
            aliases: vec!["tm".to_string()],
        });

        let model = registry.get("test-model").unwrap();
        assert_eq!(model.model_id, "test-model");
        assert_eq!(model.name, "Test Model");
        assert_eq!(model.provider, "test-provider");
        assert_eq!(model.dimensions, Some(512));
        assert!(model.supports_binary);
        assert!(model.supports_text);
        assert_eq!(model.aliases, vec!["tm"]);
    }

    #[test]
    fn test_clear_collision_warnings() {
        let registry = EmbeddingRegistry::new();
        registry.register_builtin(test_model("test", &[]));
        registry.register_builtin(test_model("test", &[])); // Collision

        assert!(!registry.collision_warnings().is_empty());

        registry.clear_collision_warnings();
        assert!(registry.collision_warnings().is_empty());
    }

    #[test]
    fn test_resolve_plugin_model() {
        let registry = EmbeddingRegistry::new();
        registry.register_plugin(test_model("custom-embed", &["custom"]));

        assert_eq!(
            registry.resolve("custom-embed"),
            Some("custom-embed".to_string())
        );
        assert_eq!(registry.resolve("custom"), Some("custom-embed".to_string()));
    }

    #[test]
    fn test_plugin_provider_registration_supports_execution() {
        let registry = EmbeddingRegistry::new();
        registry.register_plugin_provider(Arc::new(TestEmbeddingProvider {
            model: test_model("custom-exec-embed", &["custom-exec"]),
        }));

        assert_eq!(
            registry.resolve("custom-exec"),
            Some("custom-exec-embed".to_string())
        );
        let provider = registry
            .get_plugin_provider("custom-exec-embed")
            .expect("registered provider");
        let result = provider.embed("abcd").expect("embed");
        assert_eq!(result.embedding, vec![4.0]);
    }

    #[test]
    fn test_openai_builtins() {
        let registry = EmbeddingRegistry::new();
        registry.register_openai_builtins();

        // Check all models are registered
        assert!(registry.has_builtin("text-embedding-3-small"));
        assert!(registry.has_builtin("text-embedding-3-large"));
        assert!(registry.has_builtin("text-embedding-ada-002"));

        // Check aliases work
        assert_eq!(
            registry.resolve("3-small"),
            Some("text-embedding-3-small".to_string())
        );
        assert_eq!(
            registry.resolve("3-large"),
            Some("text-embedding-3-large".to_string())
        );
        assert_eq!(
            registry.resolve("ada"),
            Some("text-embedding-ada-002".to_string())
        );
        assert_eq!(
            registry.resolve("ada-002"),
            Some("text-embedding-ada-002".to_string())
        );

        // Check dimensions
        let small = registry.get("text-embedding-3-small").unwrap();
        assert_eq!(small.dimensions, Some(1536));

        let large = registry.get("text-embedding-3-large").unwrap();
        assert_eq!(large.dimensions, Some(3072));
    }
}
