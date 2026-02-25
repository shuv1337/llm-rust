//! Plugin host for Rust-native LLM plugins.
//!
//! V1 uses compile-time feature-gated plugin loading. Plugins are compiled into
//! the binary and registered against the unified registries in `llm-core` and
//! `llm-embeddings`.

use anyhow::{bail, Context, Result};
use llm_core::providers::ToolDefinition;
use llm_core::{
    core_version, fragment_loader_registry, provider_registry, template_loader_registry,
};
use llm_embeddings::global_registry as embedding_registry;
use llm_plugin_api::{
    CommandRegistrar, EmbeddingRegistrar, FragmentLoaderRegistrar, ModelRegistrar,
    PluginCapability, PluginCommand, PluginEntrypoint, PluginMetadata, TemplateLoaderRegistrar,
    ToolRegistrar,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Public metadata returned by `load_plugins()`.
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub version: String,
    pub description: Option<String>,
    pub capabilities: Vec<PluginCapability>,
    pub min_host_version: Option<String>,
}

impl From<PluginMetadata> for PluginInfo {
    fn from(metadata: PluginMetadata) -> Self {
        Self {
            id: metadata.id,
            version: metadata.version,
            description: metadata.description,
            capabilities: metadata.capabilities,
            min_host_version: metadata.min_host_version,
        }
    }
}

#[derive(Clone)]
struct PluginHostState {
    plugins: Vec<PluginInfo>,
    commands: Vec<PluginCommand>,
    tools: Vec<ToolDefinition>,
}

static HOST_STATE: OnceLock<PluginHostState> = OnceLock::new();

/// Load and register all compiled plugins, returning metadata for display.
pub fn load_plugins() -> Result<Vec<PluginInfo>> {
    Ok(host_state()?.plugins.clone())
}

/// Return plugin commands registered during host initialization.
pub fn load_plugin_commands() -> Result<Vec<PluginCommand>> {
    Ok(host_state()?.commands.clone())
}

/// Return plugin tools registered during host initialization.
pub fn load_plugin_tools() -> Result<Vec<ToolDefinition>> {
    Ok(host_state()?.tools.clone())
}

fn host_state() -> Result<&'static PluginHostState> {
    if let Some(state) = HOST_STATE.get() {
        return Ok(state);
    }

    let initialized = initialize_plugins()?;
    let _ = HOST_STATE.set(initialized);

    Ok(HOST_STATE
        .get()
        .expect("plugin host state must be initialized"))
}

fn initialize_plugins() -> Result<PluginHostState> {
    let mut infos = Vec::new();

    let mut command_registrar = HostCommandRegistrar::default();
    let mut model_registrar = HostModelRegistrar;
    let mut embedding_registrar = HostEmbeddingRegistrar;
    let mut template_loader_registrar = HostTemplateLoaderRegistrar;
    let mut fragment_loader_registrar = HostFragmentLoaderRegistrar;
    let mut tool_registrar = HostToolRegistrar::default();

    for plugin in discover_plugins() {
        let metadata = plugin.metadata();
        validate_metadata(&metadata)?;
        enforce_min_host_version(&metadata)?;

        plugin
            .register_commands(&mut command_registrar)
            .with_context(|| format!("plugin '{}' register_commands failed", metadata.id))?;
        plugin
            .register_models(&mut model_registrar)
            .with_context(|| format!("plugin '{}' register_models failed", metadata.id))?;
        plugin
            .register_embedding_models(&mut embedding_registrar)
            .with_context(|| {
                format!("plugin '{}' register_embedding_models failed", metadata.id)
            })?;
        plugin
            .register_template_loaders(&mut template_loader_registrar)
            .with_context(|| {
                format!("plugin '{}' register_template_loaders failed", metadata.id)
            })?;
        plugin
            .register_fragment_loaders(&mut fragment_loader_registrar)
            .with_context(|| {
                format!("plugin '{}' register_fragment_loaders failed", metadata.id)
            })?;
        plugin
            .register_tools(&mut tool_registrar)
            .with_context(|| format!("plugin '{}' register_tools failed", metadata.id))?;

        infos.push(metadata.into());
    }

    Ok(PluginHostState {
        plugins: infos,
        commands: command_registrar.commands,
        tools: tool_registrar.tools,
    })
}

fn discover_plugins() -> Vec<Box<dyn PluginEntrypoint>> {
    let mut plugins: Vec<Box<dyn PluginEntrypoint>> = Vec::new();

    #[cfg(feature = "plugin-markov")]
    {
        plugins.push(Box::new(llm_plugin_markov::MarkovPlugin));
    }

    plugins
}

fn validate_metadata(metadata: &PluginMetadata) -> Result<()> {
    if metadata.id.trim().is_empty() {
        bail!("plugin metadata.id cannot be empty");
    }
    if metadata.version.trim().is_empty() {
        bail!("plugin '{}' has empty version", metadata.id);
    }
    Ok(())
}

fn enforce_min_host_version(metadata: &PluginMetadata) -> Result<()> {
    if let Some(min) = &metadata.min_host_version {
        if !version_gte(core_version(), min) {
            bail!(
                "plugin '{}' requires host version >= {}, current host is {}",
                metadata.id,
                min,
                core_version()
            );
        }
    }
    Ok(())
}

fn version_gte(current: &str, minimum: &str) -> bool {
    let cur = parse_semver(current);
    let min = parse_semver(minimum);

    match (cur, min) {
        (Some(cur), Some(min)) => cur >= min,
        // If either side is non-semver, fail open for now.
        _ => true,
    }
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parse_semver_part(parts.next()?)?;
    let minor = parse_semver_part(parts.next().unwrap_or("0"))?;
    let patch = parse_semver_part(parts.next().unwrap_or("0"))?;
    Some((major, minor, patch))
}

fn parse_semver_part(part: &str) -> Option<u64> {
    let digits: String = part.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

#[derive(Default)]
struct HostCommandRegistrar {
    commands: Vec<PluginCommand>,
}

impl CommandRegistrar for HostCommandRegistrar {
    fn register_command(&mut self, command: PluginCommand) -> llm_plugin_api::PluginResult<()> {
        if self
            .commands
            .iter()
            .any(|existing| existing.name == command.name)
        {
            let owner = self
                .commands
                .iter()
                .find(|existing| existing.name == command.name)
                .map(|existing| existing.plugin_name.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "warning: plugin '{}' attempted to register command '{}' which is already registered by plugin '{}'; skipped",
                command.plugin_name, command.name, owner
            );
            return Ok(());
        }

        self.commands.push(command);
        Ok(())
    }
}

struct HostModelRegistrar;

impl ModelRegistrar for HostModelRegistrar {
    fn register_model_factory(
        &mut self,
        model_id: &str,
        factory: Box<dyn llm_plugin_api::ProviderFactory>,
    ) -> llm_plugin_api::PluginResult<()> {
        provider_registry().register_plugin(model_id, factory);
        Ok(())
    }
}

struct HostEmbeddingRegistrar;

impl EmbeddingRegistrar for HostEmbeddingRegistrar {
    fn register_embedding_model(
        &mut self,
        model: llm_plugin_api::EmbeddingModelInfo,
    ) -> llm_plugin_api::PluginResult<()> {
        embedding_registry().register_plugin(model);
        Ok(())
    }
}

struct HostTemplateLoaderRegistrar;

impl TemplateLoaderRegistrar for HostTemplateLoaderRegistrar {
    fn register_template_loader(
        &mut self,
        loader: std::sync::Arc<dyn llm_plugin_api::TemplateLoaderImpl>,
    ) -> llm_plugin_api::PluginResult<()> {
        template_loader_registry().register_plugin(loader);
        Ok(())
    }
}

struct HostFragmentLoaderRegistrar;

impl FragmentLoaderRegistrar for HostFragmentLoaderRegistrar {
    fn register_fragment_loader(
        &mut self,
        loader: std::sync::Arc<dyn llm_plugin_api::FragmentLoaderImpl>,
    ) -> llm_plugin_api::PluginResult<()> {
        fragment_loader_registry().register_plugin(loader);
        Ok(())
    }
}

#[derive(Default)]
struct HostToolRegistrar {
    tools: Vec<ToolDefinition>,
}

impl ToolRegistrar for HostToolRegistrar {
    fn register_tool(&mut self, tool: ToolDefinition) -> llm_plugin_api::PluginResult<()> {
        if self
            .tools
            .iter()
            .any(|existing| existing.function.name == tool.function.name)
        {
            eprintln!(
                "warning: plugin attempted to register tool '{}' which is already registered; keeping first",
                tool.function.name
            );
            return Ok(());
        }
        self.tools.push(tool);
        Ok(())
    }
}

/// Parsed `llm-plugin.toml` representation.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub plugin: ManifestPlugin,
    pub capabilities: Option<ManifestCapabilities>,
    pub rust: Option<ManifestRust>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestPlugin {
    pub id: String,
    pub version: String,
    pub description: Option<String>,
    pub min_host_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestCapabilities {
    pub models: Option<bool>,
    pub embedding_models: Option<bool>,
    pub commands: Option<bool>,
    pub template_loaders: Option<bool>,
    pub fragment_loaders: Option<bool>,
    pub tools: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestRust {
    pub crate_name: Option<String>,
    pub entry_type: Option<String>,
    pub dylib: Option<String>,
}

/// Parse a plugin manifest from disk.
pub fn parse_manifest(path: impl AsRef<Path>) -> Result<PluginManifest> {
    let path = path.as_ref();
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read plugin manifest {}", path.display()))?;
    let manifest: PluginManifest = toml::from_str(&raw)
        .with_context(|| format!("failed to parse plugin manifest {}", path.display()))?;
    Ok(manifest)
}

/// Load a manifest when present (returns `Ok(None)` when missing).
pub fn load_manifest_if_exists(path: impl AsRef<Path>) -> Result<Option<PluginManifest>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    parse_manifest(path).map(Some)
}

/// Build a default manifest path for a plugin root.
pub fn default_manifest_path(plugin_root: impl AsRef<Path>) -> PathBuf {
    plugin_root.as_ref().join("llm-plugin.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_core::providers::{
        FunctionDefinition, PromptCompletion, PromptProvider, PromptRequest,
    };
    use llm_core::{Fragment, FragmentLoaderImpl, PromptConfig, Template, TemplateLoaderImpl};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique(prefix: &str) -> String {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}-{n}")
    }

    struct HookProvider;

    impl PromptProvider for HookProvider {
        fn id(&self) -> &'static str {
            "hook"
        }

        fn complete(&self, _request: PromptRequest) -> Result<PromptCompletion> {
            Ok(PromptCompletion::text("hook plugin response"))
        }
    }

    struct HookProviderFactory;

    impl llm_plugin_api::ProviderFactory for HookProviderFactory {
        fn create(
            &self,
            _request: &PromptRequest,
            _config: &PromptConfig<'_>,
        ) -> Result<Box<dyn PromptProvider>> {
            Ok(Box::new(HookProvider))
        }

        fn id(&self) -> &str {
            "hook"
        }

        fn description(&self) -> &str {
            "Hook test provider"
        }
    }

    struct HookTemplateLoader {
        prefix: String,
    }

    impl TemplateLoaderImpl for HookTemplateLoader {
        fn prefix(&self) -> &str {
            &self.prefix
        }

        fn load(&self, key: &str) -> Result<Template> {
            Ok(Template {
                name: key.to_string(),
                content: format!("template:{key}"),
            })
        }

        fn description(&self) -> &str {
            "Hook template loader"
        }
    }

    struct HookFragmentLoader {
        prefix: String,
    }

    impl FragmentLoaderImpl for HookFragmentLoader {
        fn prefix(&self) -> &str {
            &self.prefix
        }

        fn load(&self, key: &str) -> Result<Vec<Fragment>> {
            Ok(vec![Fragment::new(
                format!("{}:{key}", self.prefix),
                format!("fragment:{key}"),
            )])
        }

        fn description(&self) -> &str {
            "Hook fragment loader"
        }
    }

    struct HookTestPlugin {
        command_name: String,
        model_id: String,
        embedding_model_id: String,
        template_prefix: String,
        fragment_prefix: String,
        tool_name: String,
    }

    impl HookTestPlugin {
        fn new() -> Self {
            Self {
                command_name: unique("hook-cmd"),
                model_id: unique("hook-model"),
                embedding_model_id: unique("hook-embed"),
                template_prefix: unique("hook-template"),
                fragment_prefix: unique("hook-fragment"),
                tool_name: unique("hook-tool"),
            }
        }
    }

    impl PluginEntrypoint for HookTestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: "hook-test-plugin".to_string(),
                version: "0.1.0".to_string(),
                capabilities: vec![
                    PluginCapability::Commands,
                    PluginCapability::Models,
                    PluginCapability::EmbeddingModels,
                    PluginCapability::TemplateLoaders,
                    PluginCapability::FragmentLoaders,
                    PluginCapability::Tools,
                ],
                min_host_version: None,
                description: Some("Hook contract test plugin".to_string()),
            }
        }

        fn register_commands(
            &self,
            reg: &mut dyn CommandRegistrar,
        ) -> llm_plugin_api::PluginResult<()> {
            reg.register_command(PluginCommand::new(
                self.command_name.clone(),
                "hook command",
                "hook-test-plugin",
                Arc::new(|_args| Ok(())),
            ))
        }

        fn register_models(
            &self,
            reg: &mut dyn ModelRegistrar,
        ) -> llm_plugin_api::PluginResult<()> {
            reg.register_model_factory(&self.model_id, Box::new(HookProviderFactory))
        }

        fn register_embedding_models(
            &self,
            reg: &mut dyn EmbeddingRegistrar,
        ) -> llm_plugin_api::PluginResult<()> {
            reg.register_embedding_model(llm_plugin_api::EmbeddingModelInfo {
                model_id: self.embedding_model_id.clone(),
                name: "Hook Embedding".to_string(),
                provider: "hook".to_string(),
                dimensions: Some(8),
                supports_binary: false,
                supports_text: true,
                aliases: vec![],
            })
        }

        fn register_template_loaders(
            &self,
            reg: &mut dyn TemplateLoaderRegistrar,
        ) -> llm_plugin_api::PluginResult<()> {
            reg.register_template_loader(Arc::new(HookTemplateLoader {
                prefix: self.template_prefix.clone(),
            }))
        }

        fn register_fragment_loaders(
            &self,
            reg: &mut dyn FragmentLoaderRegistrar,
        ) -> llm_plugin_api::PluginResult<()> {
            reg.register_fragment_loader(Arc::new(HookFragmentLoader {
                prefix: self.fragment_prefix.clone(),
            }))
        }

        fn register_tools(&self, reg: &mut dyn ToolRegistrar) -> llm_plugin_api::PluginResult<()> {
            reg.register_tool(ToolDefinition::function(
                FunctionDefinition::new(self.tool_name.clone())
                    .with_description("Hook tool for contract tests"),
            ))
        }
    }

    #[test]
    fn semver_compare_basic() {
        assert!(version_gte("1.0.0", "1.0.0"));
        assert!(version_gte("1.2.0", "1.1.9"));
        assert!(!version_gte("0.9.0", "1.0.0"));
    }

    #[test]
    fn semver_parser_handles_suffixes() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("invalid"), None);
    }

    #[test]
    fn validate_metadata_rejects_empty_fields() {
        let bad = PluginMetadata {
            id: "".to_string(),
            version: "1.0.0".to_string(),
            capabilities: vec![],
            min_host_version: None,
            description: None,
        };
        assert!(validate_metadata(&bad).is_err());

        let bad_version = PluginMetadata {
            id: "x".to_string(),
            version: "".to_string(),
            capabilities: vec![],
            min_host_version: None,
            description: None,
        };
        assert!(validate_metadata(&bad_version).is_err());
    }

    #[test]
    fn command_registrar_deduplicates_names() {
        let mut reg = HostCommandRegistrar::default();
        let cmd_a = PluginCommand::new("hello", "a", "plugin-a", std::sync::Arc::new(|_| Ok(())));
        let cmd_b = PluginCommand::new("hello", "b", "plugin-b", std::sync::Arc::new(|_| Ok(())));

        reg.register_command(cmd_a).unwrap();
        reg.register_command(cmd_b).unwrap();
        assert_eq!(reg.commands.len(), 1);
        assert_eq!(reg.commands[0].plugin_name, "plugin-a");
    }

    #[test]
    fn hook_registrars_cover_full_plugin_lifecycle() {
        let plugin = HookTestPlugin::new();

        let mut command_registrar = HostCommandRegistrar::default();
        let mut model_registrar = HostModelRegistrar;
        let mut embedding_registrar = HostEmbeddingRegistrar;
        let mut template_loader_registrar = HostTemplateLoaderRegistrar;
        let mut fragment_loader_registrar = HostFragmentLoaderRegistrar;
        let mut tool_registrar = HostToolRegistrar::default();

        plugin
            .register_commands(&mut command_registrar)
            .expect("register commands");
        plugin
            .register_models(&mut model_registrar)
            .expect("register models");
        plugin
            .register_embedding_models(&mut embedding_registrar)
            .expect("register embedding models");
        plugin
            .register_template_loaders(&mut template_loader_registrar)
            .expect("register template loaders");
        plugin
            .register_fragment_loaders(&mut fragment_loader_registrar)
            .expect("register fragment loaders");
        plugin
            .register_tools(&mut tool_registrar)
            .expect("register tools");

        // Commands
        assert_eq!(command_registrar.commands.len(), 1);
        assert_eq!(command_registrar.commands[0].name, plugin.command_name);
        command_registrar.commands[0]
            .execute(&[])
            .expect("plugin command executes");

        // Models
        assert!(provider_registry().has_plugin(&plugin.model_id));
        let request = PromptRequest::user_only(plugin.model_id.clone(), "hello".to_string());
        let provider = provider_registry()
            .create_provider(&plugin.model_id, &request, &PromptConfig::default())
            .expect("resolve plugin provider");
        let completion = provider.complete(request).expect("complete");
        assert_eq!(completion.text, "hook plugin response");

        // Embeddings
        assert_eq!(
            embedding_registry().resolve(&plugin.embedding_model_id),
            Some(plugin.embedding_model_id.clone())
        );

        // Template loaders
        let template = template_loader_registry()
            .load(&plugin.template_prefix, "example")
            .expect("template load");
        assert_eq!(template.content, "template:example");

        // Fragment loaders
        let fragments = fragment_loader_registry()
            .load(&plugin.fragment_prefix, "example")
            .expect("fragment load");
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].content, "fragment:example");

        // Tools
        assert!(tool_registrar
            .tools
            .iter()
            .any(|tool| tool.function.name == plugin.tool_name));
    }

    #[test]
    fn parse_markov_manifest_fixture() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("llm-plugin-markov")
            .join("llm-plugin.toml");

        let manifest = parse_manifest(path).expect("parse manifest");
        assert_eq!(manifest.plugin.id, "llm-markov");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(manifest.plugin.min_host_version.as_deref(), Some("1.0.0"));
        let caps = manifest.capabilities.expect("capabilities");
        assert_eq!(caps.models, Some(true));
    }
}
