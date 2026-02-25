//! Rust-native plugin API for the LLM workspace.
//!
//! This crate defines the core plugin entrypoint contract and registrar traits.

use std::sync::Arc;

use anyhow::Result;

pub use llm_core::providers::{
    FinishReason, JsonSchema, MessageRole, PromptCompletion, PromptMessage, PromptProvider,
    PromptRequest, ResponseFormat, StreamSink, ToolCall, ToolChoice, ToolDefinition, ToolResult,
    UsageInfo,
};
pub use llm_core::registry::ProviderFactory;
pub use llm_core::{Fragment, FragmentLoaderImpl, Template, TemplateLoaderImpl};
pub use llm_embeddings::{EmbeddingModelInfo, EmbeddingProvider};

/// Result type used across plugin API traits.
pub type PluginResult<T = ()> = Result<T>;

/// Handler function signature for plugin commands.
pub type CommandHandler = Arc<dyn Fn(&[String]) -> PluginResult<()> + Send + Sync>;

/// Command registration payload.
#[derive(Clone)]
pub struct PluginCommand {
    /// Command name (for example `markov`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Plugin identifier that owns this command.
    pub plugin_name: String,
    /// Command execution callback.
    pub handler: CommandHandler,
}

impl std::fmt::Debug for PluginCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("plugin_name", &self.plugin_name)
            .field("handler", &"<fn>")
            .finish()
    }
}

impl PluginCommand {
    /// Construct a plugin command descriptor.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        plugin_name: impl Into<String>,
        handler: CommandHandler,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            plugin_name: plugin_name.into(),
            handler,
        }
    }

    /// Execute the command with CLI arguments.
    pub fn execute(&self, args: &[String]) -> PluginResult<()> {
        (self.handler)(args)
    }
}

/// Capabilities declared by plugins.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PluginCapability {
    Models,
    EmbeddingModels,
    Commands,
    TemplateLoaders,
    FragmentLoaders,
    Tools,
}

/// Required plugin metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin identifier, for example `llm-markov`.
    pub id: String,
    /// SemVer plugin version.
    pub version: String,
    /// Declared capabilities.
    pub capabilities: Vec<PluginCapability>,
    /// Optional minimum host version requirement.
    pub min_host_version: Option<String>,
    /// Optional human-readable description.
    pub description: Option<String>,
}

/// Registrar used by plugins to contribute CLI commands.
pub trait CommandRegistrar {
    fn register_command(&mut self, command: PluginCommand) -> PluginResult<()>;
}

/// Registrar used by plugins to contribute prompt-model providers.
pub trait ModelRegistrar {
    fn register_model_factory(
        &mut self,
        model_id: &str,
        factory: Box<dyn ProviderFactory>,
    ) -> PluginResult<()>;
}

/// Registrar used by plugins to contribute embedding models.
pub trait EmbeddingRegistrar {
    fn register_embedding_model(&mut self, model: EmbeddingModelInfo) -> PluginResult<()>;
}

/// Registrar used by plugins to contribute template loaders.
pub trait TemplateLoaderRegistrar {
    fn register_template_loader(&mut self, loader: Arc<dyn TemplateLoaderImpl>)
        -> PluginResult<()>;
}

/// Registrar used by plugins to contribute fragment loaders.
pub trait FragmentLoaderRegistrar {
    fn register_fragment_loader(&mut self, loader: Arc<dyn FragmentLoaderImpl>)
        -> PluginResult<()>;
}

/// Registrar used by plugins to contribute tools.
pub trait ToolRegistrar {
    fn register_tool(&mut self, tool: ToolDefinition) -> PluginResult<()>;
}

/// Core trait implemented by every Rust-native plugin.
pub trait PluginEntrypoint: Send + Sync {
    /// Static metadata for this plugin.
    fn metadata(&self) -> PluginMetadata;

    /// Register CLI commands provided by this plugin.
    fn register_commands(&self, _reg: &mut dyn CommandRegistrar) -> PluginResult<()> {
        Ok(())
    }

    /// Register prompt models provided by this plugin.
    fn register_models(&self, _reg: &mut dyn ModelRegistrar) -> PluginResult<()> {
        Ok(())
    }

    /// Register embedding models provided by this plugin.
    fn register_embedding_models(&self, _reg: &mut dyn EmbeddingRegistrar) -> PluginResult<()> {
        Ok(())
    }

    /// Register template loaders provided by this plugin.
    fn register_template_loaders(
        &self,
        _reg: &mut dyn TemplateLoaderRegistrar,
    ) -> PluginResult<()> {
        Ok(())
    }

    /// Register fragment loaders provided by this plugin.
    fn register_fragment_loaders(
        &self,
        _reg: &mut dyn FragmentLoaderRegistrar,
    ) -> PluginResult<()> {
        Ok(())
    }

    /// Register tools provided by this plugin.
    fn register_tools(&self, _reg: &mut dyn ToolRegistrar) -> PluginResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin;

    impl PluginEntrypoint for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: "test-plugin".to_string(),
                version: "0.1.0".to_string(),
                capabilities: vec![PluginCapability::Commands],
                min_host_version: None,
                description: Some("test".to_string()),
            }
        }
    }

    struct RecordingCommandRegistrar {
        commands: Vec<String>,
    }

    impl RecordingCommandRegistrar {
        fn new() -> Self {
            Self {
                commands: Vec::new(),
            }
        }
    }

    impl CommandRegistrar for RecordingCommandRegistrar {
        fn register_command(&mut self, command: PluginCommand) -> PluginResult<()> {
            self.commands.push(command.name);
            Ok(())
        }
    }

    #[test]
    fn plugin_metadata_roundtrip() {
        let plugin = TestPlugin;
        let metadata = plugin.metadata();
        assert_eq!(metadata.id, "test-plugin");
        assert_eq!(metadata.version, "0.1.0");
        assert_eq!(metadata.capabilities, vec![PluginCapability::Commands]);
    }

    #[test]
    fn default_hook_methods_are_noops() {
        let plugin = TestPlugin;
        let mut registrar = RecordingCommandRegistrar::new();
        plugin
            .register_commands(&mut registrar)
            .expect("default command registration should succeed");
        assert!(registrar.commands.is_empty());
    }

    #[test]
    fn plugin_command_executes_handler() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen_clone = Arc::clone(&seen);

        let command = PluginCommand::new(
            "demo",
            "demo command",
            "test-plugin",
            Arc::new(move |args| {
                *seen_clone.lock().unwrap() = args.to_vec();
                Ok(())
            }),
        );

        command
            .execute(&["one".to_string(), "two".to_string()])
            .expect("handler should succeed");

        assert_eq!(seen.lock().unwrap().as_slice(), ["one", "two"]);
    }
}
