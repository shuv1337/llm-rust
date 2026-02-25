//! Fragment loader abstraction.
//!
//! Fragment loaders can return one or more content fragments for a given
//! `prefix:key` reference.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// A content fragment loaded from a fragment source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fragment {
    /// Original source identifier for this fragment.
    pub source: String,
    /// Fragment text content.
    pub content: String,
    /// Deterministic content hash (sha256 hex).
    pub hash: String,
    /// Optional metadata emitted by loader implementations.
    pub metadata: Option<serde_json::Value>,
}

impl Fragment {
    /// Build a fragment from source and content, computing `hash` automatically.
    pub fn new(source: impl Into<String>, content: impl Into<String>) -> Self {
        let source = source.into();
        let content = content.into();
        let hash = fragment_hash(&content);
        Self {
            source,
            content,
            hash,
            metadata: None,
        }
    }

    /// Attach metadata to the fragment.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Compute a deterministic SHA-256 hash for fragment content.
pub fn fragment_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Metadata about an available fragment loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentLoader {
    /// Loader prefix used in `prefix:key` syntax.
    pub name: String,
    /// Human-readable description.
    pub description: String,
}

/// Execution trait implemented by fragment loaders.
pub trait FragmentLoaderImpl: Send + Sync {
    /// Prefix used for routing (`prefix:key`).
    fn prefix(&self) -> &str;

    /// Load one or more fragments for the given key.
    fn load(&self, key: &str) -> Result<Vec<Fragment>>;

    /// Human-readable loader description.
    fn description(&self) -> &str;
}

/// Dynamic registry for fragment loaders.
pub struct FragmentLoaderRegistry {
    builtin: RwLock<HashMap<String, Arc<dyn FragmentLoaderImpl>>>,
    plugin: RwLock<HashMap<String, Arc<dyn FragmentLoaderImpl>>>,
    collision_warnings: RwLock<Vec<String>>,
}

impl Default for FragmentLoaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FragmentLoaderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            builtin: RwLock::new(HashMap::new()),
            plugin: RwLock::new(HashMap::new()),
            collision_warnings: RwLock::new(Vec::new()),
        }
    }

    /// Register a builtin fragment loader.
    pub fn register_builtin(&self, loader: Arc<dyn FragmentLoaderImpl>) {
        let prefix = loader.prefix().to_string();
        let mut builtin = self.builtin.write().unwrap();
        if builtin.contains_key(&prefix) {
            self.record_collision_warning(&format!(
                "builtin fragment loader '{}' already registered; replacing",
                prefix
            ));
        }
        builtin.insert(prefix, loader);
    }

    /// Register a plugin-provided fragment loader.
    pub fn register_plugin(&self, loader: Arc<dyn FragmentLoaderImpl>) {
        let prefix = loader.prefix().to_string();

        {
            let builtin = self.builtin.read().unwrap();
            if builtin.contains_key(&prefix) {
                self.record_collision_warning(&format!(
                    "plugin attempted to register fragment loader '{}' which collides with builtin loader; builtin takes precedence",
                    prefix
                ));
            }
        }

        {
            let plugin = self.plugin.read().unwrap();
            if plugin.contains_key(&prefix) {
                self.record_collision_warning(&format!(
                    "plugin fragment loader '{}' already registered; replacing",
                    prefix
                ));
            }
        }

        let mut plugin = self.plugin.write().unwrap();
        plugin.insert(prefix, loader);
    }

    /// Load fragments through `prefix:key` routing.
    pub fn load(&self, prefix: &str, key: &str) -> Result<Vec<Fragment>> {
        {
            let builtin = self.builtin.read().unwrap();
            if let Some(loader) = builtin.get(prefix) {
                return loader.load(key);
            }
        }

        {
            let plugin = self.plugin.read().unwrap();
            if let Some(loader) = plugin.get(prefix) {
                return loader.load(key);
            }
        }

        bail!(
            "Unknown fragment loader prefix '{}'. Available loaders: {}",
            prefix,
            self.available_prefixes().join(", ")
        )
    }

    /// List loaders (builtin first, then plugin).
    pub fn list(&self) -> Vec<FragmentLoader> {
        let mut loaders = Vec::new();

        {
            let builtin = self.builtin.read().unwrap();
            let mut items: Vec<_> = builtin
                .iter()
                .map(|(prefix, loader)| FragmentLoader {
                    name: prefix.clone(),
                    description: loader.description().to_string(),
                })
                .collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            loaders.extend(items);
        }

        {
            let plugin = self.plugin.read().unwrap();
            let mut items: Vec<_> = plugin
                .iter()
                .map(|(prefix, loader)| FragmentLoader {
                    name: prefix.clone(),
                    description: loader.description().to_string(),
                })
                .collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            loaders.extend(items);
        }

        loaders
    }

    /// Return all available loader prefixes.
    pub fn available_prefixes(&self) -> Vec<String> {
        let mut prefixes = Vec::new();

        {
            let builtin = self.builtin.read().unwrap();
            prefixes.extend(builtin.keys().cloned());
        }

        {
            let plugin = self.plugin.read().unwrap();
            prefixes.extend(plugin.keys().cloned());
        }

        prefixes.sort();
        prefixes.dedup();
        prefixes
    }

    /// Return collision warnings.
    pub fn collision_warnings(&self) -> Vec<String> {
        self.collision_warnings.read().unwrap().clone()
    }

    fn record_collision_warning(&self, message: &str) {
        let mut warnings = self.collision_warnings.write().unwrap();
        let entry = message.to_string();
        if !warnings.contains(&entry) {
            eprintln!("warning: {}", message);
            warnings.push(entry);
        }
    }
}

static FRAGMENT_LOADER_REGISTRY: OnceLock<FragmentLoaderRegistry> = OnceLock::new();

/// Return the global fragment loader registry.
pub fn fragment_loader_registry() -> &'static FragmentLoaderRegistry {
    FRAGMENT_LOADER_REGISTRY.get_or_init(FragmentLoaderRegistry::new)
}

/// List all registered fragment loaders.
pub fn list_fragment_loaders() -> Vec<FragmentLoader> {
    fragment_loader_registry().list()
}

/// Load fragments from a `prefix:key` selector.
pub fn load_fragments(selector: &str) -> Result<Vec<Fragment>> {
    let (prefix, key) = parse_fragment_selector(selector)
        .ok_or_else(|| anyhow!("Invalid fragment selector '{}'. Use prefix:key", selector))?;
    fragment_loader_registry().load(prefix, key)
}

fn parse_fragment_selector(selector: &str) -> Option<(&str, &str)> {
    let (prefix, key) = selector.split_once(':')?;
    if prefix.is_empty() || key.is_empty() {
        return None;
    }
    Some((prefix, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct MockFragmentLoader;

    impl FragmentLoaderImpl for MockFragmentLoader {
        fn prefix(&self) -> &str {
            "mock"
        }

        fn load(&self, key: &str) -> Result<Vec<Fragment>> {
            Ok(vec![
                Fragment::new(format!("mock:{}#1", key), "first"),
                Fragment::new(format!("mock:{}#2", key), "second"),
            ])
        }

        fn description(&self) -> &str {
            "Mock fragment loader"
        }
    }

    #[test]
    fn fragment_new_computes_hash() {
        let fragment = Fragment::new("source", "hello world");
        assert_eq!(fragment.source, "source");
        assert_eq!(fragment.content, "hello world");
        assert_eq!(fragment.hash, fragment_hash("hello world"));
        assert!(fragment.metadata.is_none());
    }

    #[test]
    fn fragment_hash_is_deterministic() {
        let a = fragment_hash("same");
        let b = fragment_hash("same");
        let c = fragment_hash("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn registry_loads_builtin_then_plugin() {
        let registry = FragmentLoaderRegistry::new();
        registry.register_builtin(Arc::new(MockFragmentLoader));

        let frags = registry.load("mock", "item").expect("load");
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].content, "first");

        // Plugin collision should not override builtin in resolution order
        registry.register_plugin(Arc::new(MockFragmentLoader));
        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("collides with builtin"));
    }

    #[test]
    fn registry_unknown_prefix_errors() {
        let registry = FragmentLoaderRegistry::new();
        let err = registry.load("missing", "key").expect_err("missing loader");
        assert!(err.to_string().contains("Unknown fragment loader prefix"));
    }

    #[test]
    fn parse_fragment_selector_works() {
        assert_eq!(parse_fragment_selector("mock:key"), Some(("mock", "key")));
        assert_eq!(parse_fragment_selector("mock:"), None);
        assert_eq!(parse_fragment_selector(":key"), None);
        assert_eq!(parse_fragment_selector("invalid"), None);
    }
}
