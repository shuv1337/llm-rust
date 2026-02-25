//! Model options management with merge precedence.
//!
//! User-defined default options for models are stored in `model_options.json`
//! within the user directory. Options follow merge precedence: CLI > stored defaults.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::user_dir;

/// Options that can be set for a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelOptions {
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Top-p (nucleus) sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// System prompt to prepend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

impl ModelOptions {
    /// Create empty options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if all fields are None.
    pub fn is_empty(&self) -> bool {
        self.temperature.is_none()
            && self.max_tokens.is_none()
            && self.top_p.is_none()
            && self.frequency_penalty.is_none()
            && self.presence_penalty.is_none()
            && self.stop.is_none()
            && self.system.is_none()
    }

    /// Merge with another set of options, preferring values from `other` (CLI overrides stored).
    ///
    /// Values from `other` take precedence when present.
    pub fn merge_with(&self, other: &ModelOptions) -> ModelOptions {
        ModelOptions {
            temperature: other.temperature.or(self.temperature),
            max_tokens: other.max_tokens.or(self.max_tokens),
            top_p: other.top_p.or(self.top_p),
            frequency_penalty: other.frequency_penalty.or(self.frequency_penalty),
            presence_penalty: other.presence_penalty.or(self.presence_penalty),
            stop: other.stop.clone().or_else(|| self.stop.clone()),
            system: other.system.clone().or_else(|| self.system.clone()),
        }
    }
}

/// Stored options mapping model name to options.
pub type StoredModelOptions = HashMap<String, ModelOptions>;

/// Return the path to `model_options.json` within the user directory.
pub fn model_options_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("model_options.json");
    Ok(path)
}

/// Load model options from `model_options.json`, returning an empty map if the file is missing.
pub fn load_model_options() -> Result<StoredModelOptions> {
    let path = model_options_path()?;
    if !path.exists() {
        return Ok(StoredModelOptions::new());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(StoredModelOptions::new());
    }
    let parsed: StoredModelOptions = serde_json::from_str(&contents)
        .with_context(|| format!("Invalid JSON in {}", path.display()))?;
    Ok(parsed)
}

/// Save model options to `model_options.json`.
pub fn save_model_options(options: &StoredModelOptions) -> Result<()> {
    let path = model_options_path()?;
    let json = serde_json::to_string_pretty(options)? + "\n";
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Get stored options for a specific model.
pub fn get_model_options(model: &str) -> Result<Option<ModelOptions>> {
    let options = load_model_options()?;
    Ok(options.get(model).cloned())
}

/// Set options for a specific model.
pub fn set_model_options(model: &str, opts: &ModelOptions) -> Result<()> {
    let mut all_options = load_model_options()?;
    if opts.is_empty() {
        all_options.remove(model);
    } else {
        all_options.insert(model.to_string(), opts.clone());
    }
    save_model_options(&all_options)?;
    Ok(())
}

/// Remove stored options for a specific model.
///
/// Returns `Ok(true)` if options were removed, `Ok(false)` if model had no stored options.
pub fn remove_model_options(model: &str) -> Result<bool> {
    let mut all_options = load_model_options()?;
    let removed = all_options.remove(model).is_some();
    if removed {
        save_model_options(&all_options)?;
    }
    Ok(removed)
}

/// List all models with stored options.
pub fn list_model_options() -> Result<Vec<(String, ModelOptions)>> {
    let options = load_model_options()?;
    let mut entries: Vec<(String, ModelOptions)> = options.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Resolve effective options for a model by merging stored defaults with CLI overrides.
///
/// Precedence: CLI options > stored model defaults
pub fn resolve_model_options(model: &str, cli_options: &ModelOptions) -> Result<ModelOptions> {
    let stored = get_model_options(model)?.unwrap_or_default();
    Ok(stored.merge_with(cli_options))
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
    fn model_options_merge_precedence() {
        let stored = ModelOptions {
            temperature: Some(0.7),
            max_tokens: Some(1000),
            top_p: Some(0.9),
            frequency_penalty: None,
            presence_penalty: None,
            stop: Some(vec!["END".to_string()]),
            system: Some("You are helpful.".to_string()),
        };

        let cli = ModelOptions {
            temperature: Some(0.5),       // Override stored
            max_tokens: None,             // Keep stored
            top_p: None,                  // Keep stored
            frequency_penalty: Some(0.1), // New value
            presence_penalty: None,
            stop: None,   // Keep stored
            system: None, // Keep stored
        };

        let merged = stored.merge_with(&cli);

        assert_eq!(merged.temperature, Some(0.5)); // CLI wins
        assert_eq!(merged.max_tokens, Some(1000)); // Stored preserved
        assert_eq!(merged.top_p, Some(0.9)); // Stored preserved
        assert_eq!(merged.frequency_penalty, Some(0.1)); // CLI adds
        assert_eq!(merged.presence_penalty, None);
        assert_eq!(merged.stop, Some(vec!["END".to_string()])); // Stored preserved
        assert_eq!(merged.system, Some("You are helpful.".to_string())); // Stored preserved
    }

    #[test]
    fn model_options_is_empty() {
        assert!(ModelOptions::new().is_empty());
        assert!(!ModelOptions {
            temperature: Some(0.5),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn load_model_options_missing_file() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            let options = load_model_options().expect("options");
            assert!(options.is_empty());
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_and_get_model_options() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let opts = ModelOptions {
                temperature: Some(0.8),
                max_tokens: Some(2000),
                ..Default::default()
            };
            set_model_options("openai/gpt-4o", &opts).expect("set options");

            let loaded = get_model_options("openai/gpt-4o")
                .expect("get options")
                .unwrap();
            assert_eq!(loaded.temperature, Some(0.8));
            assert_eq!(loaded.max_tokens, Some(2000));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn remove_model_options_existing() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let opts = ModelOptions {
                temperature: Some(0.5),
                ..Default::default()
            };
            set_model_options("test-model", &opts).expect("set options");

            let removed = remove_model_options("test-model").expect("remove options");
            assert!(removed);

            let loaded = get_model_options("test-model").expect("get options");
            assert!(loaded.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn remove_model_options_nonexistent() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let removed = remove_model_options("nonexistent").expect("remove options");
            assert!(!removed);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn list_model_options_sorted() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_model_options(
                "zebra-model",
                &ModelOptions {
                    temperature: Some(0.1),
                    ..Default::default()
                },
            )
            .expect("set options");
            set_model_options(
                "alpha-model",
                &ModelOptions {
                    temperature: Some(0.2),
                    ..Default::default()
                },
            )
            .expect("set options");
            set_model_options(
                "beta-model",
                &ModelOptions {
                    temperature: Some(0.3),
                    ..Default::default()
                },
            )
            .expect("set options");

            let list = list_model_options().expect("list options");
            let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["alpha-model", "beta-model", "zebra-model"]);

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn resolve_model_options_with_stored() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Set up stored defaults
            let stored = ModelOptions {
                temperature: Some(0.7),
                max_tokens: Some(1000),
                system: Some("Default system prompt.".to_string()),
                ..Default::default()
            };
            set_model_options("openai/gpt-4o", &stored).expect("set options");

            // CLI overrides temperature
            let cli = ModelOptions {
                temperature: Some(0.3),
                ..Default::default()
            };

            let resolved = resolve_model_options("openai/gpt-4o", &cli).expect("resolve");
            assert_eq!(resolved.temperature, Some(0.3)); // CLI wins
            assert_eq!(resolved.max_tokens, Some(1000)); // Stored preserved
            assert_eq!(resolved.system, Some("Default system prompt.".to_string())); // Stored preserved

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn resolve_model_options_no_stored() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let cli = ModelOptions {
                temperature: Some(0.5),
                max_tokens: Some(500),
                ..Default::default()
            };

            let resolved = resolve_model_options("no-stored-options", &cli).expect("resolve");
            assert_eq!(resolved.temperature, Some(0.5));
            assert_eq!(resolved.max_tokens, Some(500));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_empty_options_removes_entry() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let opts = ModelOptions {
                temperature: Some(0.5),
                ..Default::default()
            };
            set_model_options("test-model", &opts).expect("set options");

            // Setting empty options should remove the entry
            set_model_options("test-model", &ModelOptions::new()).expect("set empty");

            let loaded = get_model_options("test-model").expect("get options");
            assert!(loaded.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }
}
