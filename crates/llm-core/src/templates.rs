//! Template management for prompt templates.
//!
//! User-defined templates are stored as individual files within the
//! `templates/` directory under the user directory. Each template is a
//! plain text file with the template content.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;

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
    /// The loader name.
    pub name: String,
    /// Description of what the loader does.
    pub description: String,
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
pub fn load_template(name: &str) -> Result<Option<Template>> {
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

/// Get a template by name, returning an error if not found.
pub fn get_template(name: &str) -> Result<Template> {
    load_template(name)?.ok_or_else(|| anyhow::anyhow!("Template '{}' not found", name))
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

/// List available template loaders.
///
/// Currently returns the built-in filesystem loader. Plugin-based loaders
/// would be registered here in future versions.
pub fn list_template_loaders() -> Vec<TemplateLoader> {
    vec![TemplateLoader {
        name: "filesystem".to_string(),
        description: "Loads templates from the templates directory".to_string(),
    }]
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
}
