//! Template management for prompt templates.
//!
//! User-defined templates are stored as individual files within the
//! `templates/` directory under the user directory. Each template is a
//! plain text file with the template content.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use crate::user_dir;

/// Metadata about a template.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Template {
    /// The template name (derived from filename without extension).
    pub name: String,
    /// The template content.
    pub content: String,
}

/// Information about a template loader.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateLoader {
    /// Loader prefix (for `prefix:key` resolution).
    pub name: String,
    /// Description of what the loader does.
    pub description: String,
}

/// Execution trait implemented by template loaders.
///
/// Loaders are selected by prefix when loading `prefix:key` templates.
pub trait TemplateLoaderImpl: Send + Sync {
    /// Prefix used to route template lookups (for example: `filesystem`).
    fn prefix(&self) -> &str;

    /// Load a template for the given key.
    fn load(&self, key: &str) -> Result<Template>;

    /// Human-readable loader description.
    fn description(&self) -> &str;
}

/// Built-in filesystem-backed template loader.
#[derive(Debug, Default)]
pub struct FilesystemTemplateLoader;

impl TemplateLoaderImpl for FilesystemTemplateLoader {
    fn prefix(&self) -> &str {
        "filesystem"
    }

    fn load(&self, key: &str) -> Result<Template> {
        load_template_from_filesystem(key)?.ok_or_else(|| anyhow!("Template '{}' not found", key))
    }

    fn description(&self) -> &str {
        "Loads templates from the templates directory"
    }
}

/// Dynamic registry for template loaders.
///
/// Resolution order:
/// 1. Builtin loaders
/// 2. Plugin loaders
pub struct TemplateLoaderRegistry {
    builtin: RwLock<HashMap<String, Arc<dyn TemplateLoaderImpl>>>,
    plugin: RwLock<HashMap<String, Arc<dyn TemplateLoaderImpl>>>,
    collision_warnings: RwLock<Vec<String>>,
}

impl Default for TemplateLoaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateLoaderRegistry {
    /// Create an empty loader registry.
    pub fn new() -> Self {
        Self {
            builtin: RwLock::new(HashMap::new()),
            plugin: RwLock::new(HashMap::new()),
            collision_warnings: RwLock::new(Vec::new()),
        }
    }

    /// Register a builtin loader.
    pub fn register_builtin(&self, loader: Arc<dyn TemplateLoaderImpl>) {
        let prefix = loader.prefix().to_string();
        let mut builtin = self.builtin.write().unwrap();
        if builtin.contains_key(&prefix) {
            self.record_collision_warning(&format!(
                "builtin template loader '{}' already registered; replacing",
                prefix
            ));
        }
        builtin.insert(prefix, loader);
    }

    /// Register a plugin loader.
    pub fn register_plugin(&self, loader: Arc<dyn TemplateLoaderImpl>) {
        let prefix = loader.prefix().to_string();

        {
            let builtin = self.builtin.read().unwrap();
            if builtin.contains_key(&prefix) {
                self.record_collision_warning(&format!(
                    "plugin attempted to register template loader '{}' which collides with builtin loader; builtin takes precedence",
                    prefix
                ));
            }
        }

        {
            let plugin = self.plugin.read().unwrap();
            if plugin.contains_key(&prefix) {
                self.record_collision_warning(&format!(
                    "plugin template loader '{}' already registered; replacing",
                    prefix
                ));
            }
        }

        let mut plugin = self.plugin.write().unwrap();
        plugin.insert(prefix, loader);
    }

    /// Load a template via a specific loader prefix.
    pub fn load(&self, prefix: &str, key: &str) -> Result<Template> {
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
            "Unknown template loader prefix '{}'. Available loaders: {}",
            prefix,
            self.available_prefixes().join(", ")
        )
    }

    /// List loader metadata (builtin first, then plugin).
    pub fn list(&self) -> Vec<TemplateLoader> {
        let mut loaders = Vec::new();

        {
            let builtin = self.builtin.read().unwrap();
            let mut items: Vec<_> = builtin
                .iter()
                .map(|(prefix, loader)| TemplateLoader {
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
                .map(|(prefix, loader)| TemplateLoader {
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

    /// Return recorded collision warnings.
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

static TEMPLATE_LOADER_REGISTRY: OnceLock<TemplateLoaderRegistry> = OnceLock::new();

/// Return the global template loader registry.
pub fn template_loader_registry() -> &'static TemplateLoaderRegistry {
    TEMPLATE_LOADER_REGISTRY.get_or_init(|| {
        let registry = TemplateLoaderRegistry::new();
        registry.register_builtin(Arc::new(FilesystemTemplateLoader));
        registry
    })
}

/// Return the path to the `templates/` directory within the user directory.
pub fn templates_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("templates");
    Ok(path)
}

/// Ensure the templates directory exists.
fn ensure_templates_dir() -> Result<PathBuf> {
    let path = templates_path()?;
    if !path.exists() {
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create templates directory: {}", path.display()))?;
    }
    Ok(path)
}

/// List all template names, sorted alphabetically.
pub fn list_templates() -> Result<Vec<String>> {
    let path = templates_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    let entries = fs::read_dir(&path)
        .with_context(|| format!("Failed to read templates directory: {}", path.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_path = entry.path();

        // Only consider files (not directories)
        if !file_path.is_file() {
            continue;
        }

        // Get the filename without extension as the template name
        if let Some(stem) = file_path.file_stem() {
            if let Some(name) = stem.to_str() {
                names.push(name.to_string());
            }
        }
    }

    names.sort();
    Ok(names)
}

/// Load a template by name.
///
/// Returns `Ok(Some(Template))` if found, `Ok(None)` if not found.
///
/// If `name` uses `prefix:key` syntax, the corresponding loader is used.
pub fn load_template(name: &str) -> Result<Option<Template>> {
    if let Some((prefix, key)) = parse_loader_name(name) {
        match template_loader_registry().load(prefix, key) {
            Ok(template) => return Ok(Some(template)),
            Err(err) => {
                if err.to_string().contains("not found") {
                    return Ok(None);
                }
                return Err(err);
            }
        }
    }

    load_template_from_filesystem(name)
}

/// Get a template by name, returning an error if not found.
pub fn get_template(name: &str) -> Result<Template> {
    load_template(name)?.ok_or_else(|| anyhow!("Template '{}' not found", name))
}

/// Save a template to disk.
///
/// Creates or overwrites the template file.
pub fn save_template(name: &str, content: &str) -> Result<()> {
    validate_template_name(name)?;

    // Build path from the ensured directory to avoid any path mismatch if
    // environment variables were mutated between multiple helper calls.
    let mut path = ensure_templates_dir()?;
    path.push(format!("{}.txt", name));

    fs::write(&path, content)
        .with_context(|| format!("Failed to write template: {}", path.display()))?;
    Ok(())
}

/// Delete a template by name.
///
/// Returns `Ok(true)` if the template was deleted, `Ok(false)` if it didn't exist.
pub fn delete_template(name: &str) -> Result<bool> {
    let path = template_file_path(name)?;
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path)
        .with_context(|| format!("Failed to delete template: {}", path.display()))?;
    Ok(true)
}

fn validate_template_name(name: &str) -> Result<()> {
    // Validate template name to prevent path traversal
    if name.is_empty() {
        bail!("Template name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("Template name contains invalid characters");
    }
    Ok(())
}

/// Get the file path for a template by name.
fn template_file_path(name: &str) -> Result<PathBuf> {
    validate_template_name(name)?;
    let mut path = templates_path()?;
    path.push(format!("{}.txt", name));
    Ok(path)
}

fn load_template_from_filesystem(name: &str) -> Result<Option<Template>> {
    let path = template_file_path(name)?;
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read template: {}", path.display()))?;

    Ok(Some(Template {
        name: name.to_string(),
        content,
    }))
}

fn parse_loader_name(name: &str) -> Option<(&str, &str)> {
    let (prefix, key) = name.split_once(':')?;
    if prefix.is_empty() || key.is_empty() {
        return None;
    }
    Some((prefix, key))
}

/// List available template loaders.
pub fn list_template_loaders() -> Vec<TemplateLoader> {
    template_loader_registry().list()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_user_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn with_env_lock<F: FnOnce()>(f: F) {
        let guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f();
        drop(guard);
    }

    #[test]
    fn templates_path_returns_directory() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let path = templates_path().expect("templates path");
            assert!(path.ends_with("templates"));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn list_templates_empty() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let templates = list_templates().expect("list templates");
            assert!(templates.is_empty());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn save_and_load_template() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let content = "Hello, {{ name }}!";
            save_template("greeting", content).expect("save template");

            let template = load_template("greeting").expect("load template").unwrap();
            assert_eq!(template.name, "greeting");
            assert_eq!(template.content, content);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn load_template_not_found() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let template = load_template("nonexistent").expect("load template");
            assert!(template.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn get_template_error_when_not_found() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let result = get_template("nonexistent");
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("not found"));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn delete_template_existing() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            save_template("to_delete", "content").expect("save template");
            let deleted = delete_template("to_delete").expect("delete template");
            assert!(deleted);

            let template = load_template("to_delete").expect("load template");
            assert!(template.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn delete_template_nonexistent() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let deleted = delete_template("nonexistent").expect("delete template");
            assert!(!deleted);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn list_templates_sorted() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            save_template("zebra", "z").expect("save");
            save_template("alpha", "a").expect("save");
            save_template("beta", "b").expect("save");

            let templates = list_templates().expect("list");
            assert_eq!(templates, vec!["alpha", "beta", "zebra"]);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn template_name_validation() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Empty name should fail
            let result = save_template("", "content");
            assert!(result.is_err());

            // Path traversal should fail
            let result = save_template("../evil", "content");
            assert!(result.is_err());

            // Slash in name should fail
            let result = save_template("foo/bar", "content");
            assert!(result.is_err());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn list_template_loaders_returns_filesystem() {
        let loaders = list_template_loaders();
        assert_eq!(loaders.len(), 1);
        assert_eq!(loaders[0].name, "filesystem");
    }

    #[test]
    fn template_overwrite() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            save_template("test", "original").expect("save");
            save_template("test", "updated").expect("overwrite");

            let template = load_template("test").expect("load").unwrap();
            assert_eq!(template.content, "updated");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn load_template_with_filesystem_prefix() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            save_template("hello", "world").expect("save");
            let template = load_template("filesystem:hello")
                .expect("load")
                .expect("template");
            assert_eq!(template.name, "hello");
            assert_eq!(template.content, "world");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn unknown_loader_prefix_errors() {
        let err = load_template("unknown:thing").expect_err("should fail for unknown prefix");
        assert!(err.to_string().contains("Unknown template loader prefix"));
    }

    #[derive(Debug)]
    struct MockLoader;

    impl TemplateLoaderImpl for MockLoader {
        fn prefix(&self) -> &str {
            "mock"
        }

        fn load(&self, key: &str) -> Result<Template> {
            Ok(Template {
                name: key.to_string(),
                content: "from-mock".to_string(),
            })
        }

        fn description(&self) -> &str {
            "Mock loader"
        }
    }

    #[test]
    fn template_loader_registry_resolves_builtin_then_plugin() {
        let registry = TemplateLoaderRegistry::new();
        registry.register_builtin(Arc::new(FilesystemTemplateLoader));
        registry.register_plugin(Arc::new(MockLoader));

        let prefixes = registry.available_prefixes();
        assert!(prefixes.contains(&"filesystem".to_string()));
        assert!(prefixes.contains(&"mock".to_string()));
    }

    #[test]
    fn template_loader_registry_detects_collisions() {
        let registry = TemplateLoaderRegistry::new();
        registry.register_builtin(Arc::new(MockLoader));
        registry.register_plugin(Arc::new(MockLoader));

        let warnings = registry.collision_warnings();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("collides with builtin"));
    }
}
