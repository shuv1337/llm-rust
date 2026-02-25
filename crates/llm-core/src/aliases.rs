//! Alias management for model name resolution.
//!
//! User-defined aliases are stored in `aliases.json` within the user directory.
//! Aliases are resolved after built-in aliases but before treating the name as literal.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::user_dir;

/// Alias mapping from user-defined name to canonical model identifier.
pub type Aliases = HashMap<String, String>;

/// Return the path to `aliases.json` within the user directory.
pub fn aliases_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("aliases.json");
    Ok(path)
}

/// Load aliases from `aliases.json`, returning an empty map if the file is missing.
pub fn load_aliases() -> Result<Aliases> {
    let path = aliases_path()?;
    if !path.exists() {
        return Ok(Aliases::new());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(Aliases::new());
    }
    let parsed: Aliases = serde_json::from_str(&contents)
        .with_context(|| format!("Invalid JSON in {}", path.display()))?;
    Ok(parsed)
}

/// Save aliases to `aliases.json`.
pub fn save_aliases(aliases: &Aliases) -> Result<()> {
    let path = aliases_path()?;
    let json = serde_json::to_string_pretty(aliases)? + "\n";
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Set an alias mapping `name` to `model`.
///
/// If the alias already exists, it will be overwritten.
pub fn set_alias(name: &str, model: &str) -> Result<()> {
    let mut aliases = load_aliases()?;
    aliases.insert(name.to_string(), model.to_string());
    save_aliases(&aliases)?;
    Ok(())
}

/// Remove an alias by name.
///
/// Returns `Ok(true)` if the alias was removed, `Ok(false)` if it didn't exist.
pub fn remove_alias(name: &str) -> Result<bool> {
    let mut aliases = load_aliases()?;
    let removed = aliases.remove(name).is_some();
    if removed {
        save_aliases(&aliases)?;
    }
    Ok(removed)
}

/// Get the target model for an alias, if it exists.
pub fn get_alias(name: &str) -> Result<Option<String>> {
    let aliases = load_aliases()?;
    Ok(aliases.get(name).cloned())
}

/// List all defined aliases sorted by name.
pub fn list_aliases() -> Result<Vec<(String, String)>> {
    let aliases = load_aliases()?;
    let mut entries: Vec<(String, String)> = aliases.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Resolve a name through user-defined aliases.
///
/// Returns the target model if the name is an alias, otherwise returns `None`.
pub fn resolve_user_alias(name: &str) -> Result<Option<String>> {
    let aliases = load_aliases()?;
    Ok(aliases.get(name).cloned())
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
    fn load_aliases_missing_file() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            let aliases = load_aliases().expect("aliases");
            assert!(aliases.is_empty());
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_and_get_alias() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_alias("fast", "openai/gpt-4o-mini").expect("set alias");
            let target = get_alias("fast").expect("get alias").unwrap();
            assert_eq!(target, "openai/gpt-4o-mini");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn remove_alias_existing() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_alias("slow", "openai/gpt-4").expect("set alias");
            let removed = remove_alias("slow").expect("remove alias");
            assert!(removed);
            let target = get_alias("slow").expect("get alias");
            assert!(target.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn remove_alias_nonexistent() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let removed = remove_alias("nonexistent").expect("remove alias");
            assert!(!removed);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn list_aliases_sorted() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_alias("zebra", "openai/gpt-4").expect("set alias");
            set_alias("alpha", "openai/gpt-3.5-turbo").expect("set alias");
            set_alias("beta", "anthropic/claude-3-opus").expect("set alias");

            let list = list_aliases().expect("list aliases");
            let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["alpha", "beta", "zebra"]);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn resolve_user_alias_found() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_alias("mymodel", "anthropic/claude-3.5-sonnet").expect("set alias");
            let resolved = resolve_user_alias("mymodel").expect("resolve").unwrap();
            assert_eq!(resolved, "anthropic/claude-3.5-sonnet");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn resolve_user_alias_not_found() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let resolved = resolve_user_alias("unknown").expect("resolve");
            assert!(resolved.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn alias_overwrite() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_alias("test", "openai/gpt-3.5-turbo").expect("set alias");
            set_alias("test", "openai/gpt-4o").expect("overwrite alias");
            let target = get_alias("test").expect("get alias").unwrap();
            assert_eq!(target, "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }
}
