//! Placeholder crate for the plugin host and Python bridge.
//!
//! This will eventually embed the existing Python plugin ecosystem via `pyo3`
//! and expose a trait-based ABI for native Rust plugins. For now it just
//! exposes a stub API so other crates can compile against it.

use anyhow::Result;

/// Temporary representation of a plugin identifier.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
}

/// Stub function for loading plugins. Currently returns a placeholder entry.
pub fn load_plugins() -> Result<Vec<PluginInfo>> {
    Ok(vec![PluginInfo {
        name: "llm-default-plugin-stub".to_string(),
    }])
}
