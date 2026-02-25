# ADR-003: Unified Plugin Registry Architecture

**Status:** Accepted  
**Date:** 2026-02-25  
**Author:** llm-rust team  
**Amends:** [ADR-001](ADR-001-plugin-runtime-architecture.md)

## Context

ADR-001 established the plugin runtime architecture with `CommandRegistry` and `ProviderRegistry` for bridging Python plugins via pyo3. However, the project also needs a native Rust plugin system that:

1. Shares the same registries (no parallel systems)
2. Supports both compile-time and future dynamic loading
3. Provides a stable API surface for plugin authors
4. Enables incremental migration from Python to Rust plugins

This ADR extends ADR-001 by specifying:
- The unified registry model serving both native Rust plugins and the pyo3 bridge
- V1 compile-time plugin loading via Cargo feature flags
- V2 dynamic plugin loading via shared libraries (design only)
- The frozen `PluginEntrypoint` trait and registrar interfaces
- The `llm-plugin.toml` manifest schema

## Decision

### 1. Unified Registry Model

Native Rust plugins and Python bridge plugins use the **same registries** defined in ADR-001:

```
┌─────────────────────────────────────────────────────────┐
│                      llm-cli                            │
│  ┌─────────────────────────────────────────────────┐   │
│  │              CommandRegistry                     │   │
│  │  ┌──────────────┐  ┌───────────────────────┐    │   │
│  │  │ core_commands│  │   plugin_commands     │    │   │
│  │  │  (built-in)  │  │ (native + bridge)     │    │   │
│  │  └──────────────┘  └───────────────────────┘    │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                     llm-core                            │
│  ┌─────────────────────────────────────────────────┐   │
│  │              ProviderRegistry                    │   │
│  │  ┌──────────────┐  ┌───────────────────────┐    │   │
│  │  │    builtin   │  │       plugin          │    │   │
│  │  │ (OpenAI, etc)│  │ (native + bridge)     │    │   │
│  │  └──────────────┘  └───────────────────────┘    │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ TemplateLoaderRegistry + FragmentLoaderRegistry │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                   llm-embeddings                        │
│  ┌─────────────────────────────────────────────────┐   │
│  │             EmbeddingRegistry                    │   │
│  │  ┌──────────────┐  ┌───────────────────────┐    │   │
│  │  │    builtin   │  │       plugin          │    │   │
│  │  └──────────────┘  └───────────────────────┘    │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

**Key principle:** There is ONE set of registries. Native plugins, Python bridge plugins, and future dylib plugins all register into the same structures via the registrar traits.

### 2. Plugin Loading Mechanisms

#### V1: Compile-Time Feature-Gated Loading (Implemented)

Plugins are Rust crates compiled into the binary via Cargo feature flags:

```toml
# Cargo.toml (workspace root)
[features]
default = []
plugin-markov = ["dep:llm-plugin-markov"]
plugin-gemini = ["dep:llm-plugin-gemini"]
plugin-openrouter = ["dep:llm-plugin-openrouter"]
all-plugins = ["plugin-markov", "plugin-gemini", "plugin-openrouter"]

[dependencies]
llm-plugin-markov = { path = "crates/llm-plugin-markov", optional = true }
llm-plugin-gemini = { path = "crates/llm-plugin-gemini", optional = true }
llm-plugin-openrouter = { path = "crates/llm-plugin-openrouter", optional = true }
```

```rust
// crates/llm-plugin-host/src/lib.rs
use llm_plugin_api::PluginEntrypoint;

pub fn load_plugins() -> Vec<Box<dyn PluginEntrypoint>> {
    let mut plugins: Vec<Box<dyn PluginEntrypoint>> = Vec::new();
    
    #[cfg(feature = "plugin-markov")]
    plugins.push(Box::new(llm_plugin_markov::MarkovPlugin));
    
    #[cfg(feature = "plugin-gemini")]
    plugins.push(Box::new(llm_plugin_gemini::GeminiPlugin));
    
    #[cfg(feature = "plugin-openrouter")]
    plugins.push(Box::new(llm_plugin_openrouter::OpenRouterPlugin));
    
    plugins
}
```

**Benefits:**
- No ABI fragility or `dlopen` unsafety
- Rust's type system ensures compatibility at compile time
- Zero startup overhead (no plugin discovery)
- Dead code elimination for unused plugins

**Tradeoffs:**
- Users must recompile to add/remove plugins
- Binary size increases with each plugin

**Build commands:**
```bash
# Minimal build (no plugins)
cargo build --release

# Single plugin
cargo build --release --features plugin-markov

# All plugins
cargo build --release --features all-plugins
```

#### V2: Dynamic Shared Library Loading (Future Design)

> **Status:** Design only. Implementation deferred until V1 plugin API is proven stable.

Dynamic loading enables plugin installation without recompilation:

```
~/.config/io.datasette.llm/
└── plugins/
    ├── llm-plugin-markov/
    │   ├── llm-plugin.toml
    │   └── libllm_plugin_markov.so  (or .dylib / .dll)
    └── llm-plugin-custom/
        ├── llm-plugin.toml
        └── libllm_plugin_custom.so
```

**Discovery algorithm:**
1. Scan `$LLM_USER_PATH/plugins/` for directories containing `llm-plugin.toml`
2. Parse manifest, check `min_host_version` compatibility
3. Load shared library specified in manifest
4. Call C ABI entry function to obtain `PluginEntrypoint` vtable

**C ABI contract:**
```rust
// Stable C ABI wrapper (future implementation)
#[repr(C)]
pub struct PluginVTable {
    pub metadata: extern "C" fn() -> *const PluginMetadataC,
    pub register_commands: extern "C" fn(*mut CommandRegistrarC) -> i32,
    pub register_models: extern "C" fn(*mut ModelRegistrarC) -> i32,
    // ... other hooks
}

#[no_mangle]
pub extern "C" fn llm_plugin_entry() -> *const PluginVTable {
    // Return pointer to static vtable
}
```

**ABI stability requirements:**
- Plugin ABI version in manifest (`abi_version = 1`)
- Host rejects plugins with incompatible ABI version
- Breaking changes require ABI version bump
- Data structures use `#[repr(C)]` for cross-compilation compatibility

**Security considerations:**
- Plugins execute arbitrary native code
- No sandboxing (same trust model as compiled-in plugins)
- Future: consider WASM-based sandboxed plugin runtime

### 3. Plugin Entrypoint Trait (Frozen)

The `PluginEntrypoint` trait is the primary interface between plugins and the host:

```rust
// crates/llm-plugin-api/src/lib.rs

use crate::registrars::*;
use crate::metadata::{PluginMetadata, PluginCapability};

/// Core trait that every native plugin must implement.
/// 
/// All hook methods have default no-op implementations, allowing plugins
/// to implement only the capabilities they provide.
pub trait PluginEntrypoint: Send + Sync {
    /// Returns plugin metadata including ID, version, and capabilities.
    /// 
    /// This method is called during plugin discovery before any registration.
    fn metadata(&self) -> PluginMetadata;

    /// Register CLI commands provided by this plugin.
    /// 
    /// Commands are invoked when the user runs `llm <command-name> [args]`.
    /// Plugin commands cannot shadow core commands (see collision rules).
    fn register_commands(&self, registrar: &mut dyn CommandRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }

    /// Register prompt models provided by this plugin.
    /// 
    /// Models are used when the user runs `llm "prompt" --model <model-id>`.
    fn register_models(&self, registrar: &mut dyn ModelRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }

    /// Register embedding models provided by this plugin.
    /// 
    /// Embedding models are used with `llm embed` commands.
    fn register_embedding_models(&self, registrar: &mut dyn EmbeddingRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }

    /// Register template loaders provided by this plugin.
    /// 
    /// Template loaders handle `prefix:key` syntax in template references.
    fn register_template_loaders(&self, registrar: &mut dyn TemplateLoaderRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }

    /// Register fragment loaders provided by this plugin.
    /// 
    /// Fragment loaders handle `prefix:key` syntax in fragment references.
    fn register_fragment_loaders(&self, registrar: &mut dyn FragmentLoaderRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }

    /// Register tools available to models during prompting.
    /// 
    /// Tools are invoked by models when tool calling is enabled.
    fn register_tools(&self, registrar: &mut dyn ToolRegistrar) -> Result<(), PluginError> {
        let _ = registrar;
        Ok(())
    }
}

/// Error type for plugin operations.
#[derive(Debug, Clone)]
pub struct PluginError {
    pub message: String,
    pub kind: PluginErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginErrorKind {
    /// Registration failed (e.g., invalid model definition)
    Registration,
    /// Plugin initialization failed
    Initialization,
    /// Version incompatibility
    VersionMismatch,
    /// Other plugin error
    Other,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::fmt::Display for PluginErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registration => write!(f, "registration error"),
            Self::Initialization => write!(f, "initialization error"),
            Self::VersionMismatch => write!(f, "version mismatch"),
            Self::Other => write!(f, "plugin error"),
        }
    }
}

impl std::error::Error for PluginError {}
```

### 4. Registrar Trait Definitions (Frozen)

Registrars are the interface through which plugins add their capabilities to the host:

```rust
// crates/llm-plugin-api/src/registrars.rs

use llm_core::{PromptProvider, ToolDefinition};
use llm_core::templates::TemplateLoaderImpl;
use llm_core::fragments::FragmentLoaderImpl;
use llm_embeddings::EmbeddingProvider;

/// Registrar for plugin-provided CLI commands.
pub trait CommandRegistrar {
    /// Register a command with the given name and handler.
    /// 
    /// # Arguments
    /// * `name` - Command name (e.g., "my-command")
    /// * `description` - Short description for help text
    /// * `handler` - Function called when command is invoked
    /// 
    /// # Returns
    /// `Err` if command name is invalid or conflicts with core command.
    fn register_command(
        &mut self,
        name: &str,
        description: &str,
        handler: Box<dyn CommandHandler>,
    ) -> Result<(), RegistrationError>;
}

/// Handler for a plugin-provided command.
pub trait CommandHandler: Send + Sync {
    /// Execute the command with given arguments.
    /// 
    /// # Arguments
    /// * `args` - Command-line arguments (excluding the command name)
    /// 
    /// # Returns
    /// Exit code (0 for success).
    fn execute(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>>;
    
    /// Return help text for the command.
    fn help(&self) -> String;
}

/// Registrar for plugin-provided prompt models.
pub trait ModelRegistrar {
    /// Register a model with the given ID.
    /// 
    /// # Arguments
    /// * `model_id` - Unique model identifier (e.g., "markov", "gpt-4-turbo")
    /// * `provider` - Implementation of the PromptProvider trait
    /// 
    /// # Returns
    /// `Err` if model ID is invalid or already registered by another plugin.
    fn register_model(
        &mut self,
        model_id: &str,
        provider: Box<dyn PromptProvider>,
    ) -> Result<(), RegistrationError>;
    
    /// Register multiple models with a common prefix.
    /// 
    /// # Arguments
    /// * `prefix` - Model prefix (e.g., "gemini-")
    /// * `provider` - Provider handling all models with this prefix
    fn register_model_prefix(
        &mut self,
        prefix: &str,
        provider: Box<dyn PromptProvider>,
    ) -> Result<(), RegistrationError>;
}

/// Registrar for plugin-provided embedding models.
pub trait EmbeddingRegistrar {
    /// Register an embedding model.
    /// 
    /// # Arguments
    /// * `model_id` - Unique embedding model identifier
    /// * `provider` - Implementation of the EmbeddingProvider trait
    fn register_embedding_model(
        &mut self,
        model_id: &str,
        provider: Box<dyn EmbeddingProvider>,
    ) -> Result<(), RegistrationError>;
}

/// Registrar for plugin-provided template loaders.
pub trait TemplateLoaderRegistrar {
    /// Register a template loader for a given prefix.
    /// 
    /// # Arguments
    /// * `prefix` - Prefix for template references (e.g., "github" for "github:user/repo")
    /// * `loader` - Implementation of the TemplateLoaderImpl trait
    fn register_template_loader(
        &mut self,
        prefix: &str,
        loader: Box<dyn TemplateLoaderImpl>,
    ) -> Result<(), RegistrationError>;
}

/// Registrar for plugin-provided fragment loaders.
pub trait FragmentLoaderRegistrar {
    /// Register a fragment loader for a given prefix.
    /// 
    /// # Arguments
    /// * `prefix` - Prefix for fragment references (e.g., "github" for "github:user/repo/file.md")
    /// * `loader` - Implementation of the FragmentLoaderImpl trait
    fn register_fragment_loader(
        &mut self,
        prefix: &str,
        loader: Box<dyn FragmentLoaderImpl>,
    ) -> Result<(), RegistrationError>;
}

/// Registrar for plugin-provided tools.
pub trait ToolRegistrar {
    /// Register a tool available to models.
    /// 
    /// # Arguments
    /// * `tool` - Tool definition including name, schema, and handler
    fn register_tool(
        &mut self,
        tool: ToolDefinition,
    ) -> Result<(), RegistrationError>;
}

/// Error returned when registration fails.
#[derive(Debug, Clone)]
pub struct RegistrationError {
    pub message: String,
    pub kind: RegistrationErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegistrationErrorKind {
    /// Name/ID already registered
    Duplicate,
    /// Invalid name/ID format
    InvalidName,
    /// Conflicts with core functionality
    CoreConflict,
    /// Other registration error
    Other,
}

impl std::fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::fmt::Display for RegistrationErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate => write!(f, "duplicate registration"),
            Self::InvalidName => write!(f, "invalid name"),
            Self::CoreConflict => write!(f, "core conflict"),
            Self::Other => write!(f, "registration error"),
        }
    }
}

impl std::error::Error for RegistrationError {}
```

### 5. Plugin Metadata Types (Frozen)

```rust
// crates/llm-plugin-api/src/metadata.rs

/// Required metadata for every plugin.
/// 
/// This information is used for:
/// - Plugin discovery and listing (`llm plugins list`)
/// - Version compatibility checking
/// - Capability advertisement
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Unique plugin identifier (e.g., "llm-markov").
    /// 
    /// Convention: use "llm-" prefix for official plugins.
    pub id: String,
    
    /// SemVer version string (e.g., "1.2.3").
    pub version: String,
    
    /// Capabilities this plugin provides.
    /// 
    /// Used for filtering and optimization (skip hook calls for
    /// capabilities the plugin doesn't provide).
    pub capabilities: Vec<PluginCapability>,
    
    /// Minimum host version required (e.g., "1.0.0").
    /// 
    /// If set, the plugin will not load on older host versions.
    pub min_host_version: Option<String>,
    
    /// Human-readable description for `llm plugins list`.
    pub description: Option<String>,
    
    /// Plugin author(s).
    pub authors: Option<Vec<String>>,
    
    /// Plugin homepage or repository URL.
    pub homepage: Option<String>,
    
    /// Plugin license (e.g., "MIT", "Apache-2.0").
    pub license: Option<String>,
}

impl PluginMetadata {
    /// Create minimal metadata with required fields only.
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            capabilities: Vec::new(),
            min_host_version: None,
            description: None,
            authors: None,
            homepage: None,
            license: None,
        }
    }
    
    /// Builder method to add a capability.
    pub fn with_capability(mut self, cap: PluginCapability) -> Self {
        self.capabilities.push(cap);
        self
    }
    
    /// Builder method to set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
    
    /// Builder method to set minimum host version.
    pub fn with_min_host_version(mut self, version: impl Into<String>) -> Self {
        self.min_host_version = Some(version.into());
        self
    }
}

/// Capabilities a plugin can provide.
/// 
/// Each capability corresponds to a registration hook in `PluginEntrypoint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginCapability {
    /// Plugin provides prompt models via `register_models`
    Models,
    /// Plugin provides embedding models via `register_embedding_models`
    EmbeddingModels,
    /// Plugin provides CLI commands via `register_commands`
    Commands,
    /// Plugin provides template loaders via `register_template_loaders`
    TemplateLoaders,
    /// Plugin provides fragment loaders via `register_fragment_loaders`
    FragmentLoaders,
    /// Plugin provides tools via `register_tools`
    Tools,
}

impl std::fmt::Display for PluginCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Models => write!(f, "models"),
            Self::EmbeddingModels => write!(f, "embedding_models"),
            Self::Commands => write!(f, "commands"),
            Self::TemplateLoaders => write!(f, "template_loaders"),
            Self::FragmentLoaders => write!(f, "fragment_loaders"),
            Self::Tools => write!(f, "tools"),
        }
    }
}
```

### 6. Collision Rules (Formalized)

Extending ADR-001, collision rules apply uniformly to all plugin sources:

| Collision Type | Resolution | Diagnostic |
|----------------|------------|------------|
| Core vs Plugin (any source) | Core wins | `warning: plugin '{plugin_id}' attempted to register {type} '{name}' which is a core {type}; skipped` |
| Native Plugin vs Native Plugin | First registered wins | `warning: plugin '{plugin_id}' attempted to register {type} '{name}' already registered by '{first_plugin}'; skipped` |
| Native Plugin vs Bridge Plugin | First registered wins | Same warning format |
| Bridge Plugin vs Bridge Plugin | First registered wins | Same warning format |

**Registration order:**
1. Core/builtin registrations (compile-time)
2. Native Rust plugins (in feature-flag order)
3. Python bridge plugins (in entrypoint discovery order)

**Diagnostic output:**
- Warnings are emitted to stderr
- `llm plugins list --verbose` shows collision history
- Collisions do not cause failure (deterministic, non-breaking)

### 7. Plugin Manifest Schema

The `llm-plugin.toml` manifest provides plugin metadata for discovery and loading.

See [docs/plugin-manifest-schema.md](../plugin-manifest-schema.md) for the complete schema specification.

**Example:**
```toml
[plugin]
id = "llm-markov"
version = "0.1.0"
description = "Markov chain text generation model"
min_host_version = "1.0.0"
authors = ["llm-rust team"]
homepage = "https://github.com/example/llm-markov"
license = "MIT"

[capabilities]
models = true
embedding_models = false
commands = false
template_loaders = false
fragment_loaders = false
tools = false

[rust]
crate_name = "llm-plugin-markov"
entry_type = "llm_plugin_markov::MarkovPlugin"

# Future V2 dylib loading (not yet implemented)
# [dylib]
# path = "libllm_plugin_markov.so"
# abi_version = 1
```

### 8. Plugin Lifecycle

```
┌──────────────────────────────────────────────────────────────────┐
│                        Plugin Lifecycle                          │
└──────────────────────────────────────────────────────────────────┘

1. Discovery
   ├── V1: Compile-time feature flags determine available plugins
   └── V2: Scan plugin directories for llm-plugin.toml manifests

2. Load
   ├── V1: Static linking (already loaded at compile time)
   └── V2: dlopen shared library, call entry function

3. Metadata Check
   ├── Call plugin.metadata()
   ├── Verify min_host_version compatibility
   └── Log plugin info to debug output

4. Version Gate
   ├── If min_host_version > current_host_version:
   │   └── Skip plugin, emit warning
   └── If compatible: continue

5. Register Hooks (in order)
   ├── register_commands()   → CommandRegistry
   ├── register_models()     → ProviderRegistry
   ├── register_embedding_models() → EmbeddingRegistry
   ├── register_template_loaders() → TemplateLoaderRegistry
   ├── register_fragment_loaders() → FragmentLoaderRegistry
   └── register_tools()      → ToolRegistry

6. Ready
   └── Plugin capabilities available for use

7. Shutdown (future)
   └── Optional cleanup hook (not in V1)
```

## Consequences

### Positive
- Single registry system serves all plugin sources
- Compile-time loading provides safety and performance
- Future dylib loading enables user plugin installation
- Frozen API provides stability for plugin authors
- Clear collision rules prevent user confusion

### Negative
- V1 requires recompilation to change plugins
- V2 dylib loading adds ABI stability burden
- Some Python plugin features may not map cleanly to Rust

### Neutral
- Async model support deferred to future ADR
- Plugin SDK macros deferred until API proven stable

## References

- [ADR-001: Plugin Runtime Architecture](ADR-001-plugin-runtime-architecture.md)
- [Plugin Manifest Schema](../plugin-manifest-schema.md)
- [PLAN-rust-native-plugin-api-and-converter.md](../../PLAN-rust-native-plugin-api-and-converter.md)
- Upstream hookspecs.py: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/hookspecs.py
