# Plugin Manifest Schema (`llm-plugin.toml`)

This document defines the TOML schema for native Rust plugin manifests. The manifest provides metadata for plugin discovery, version compatibility checking, and loading configuration.

## Schema Version

**Current Version:** 1.0.0

The schema follows [SemVer](https://semver.org/). Breaking changes increment the major version.

## Required Sections

### `[plugin]` — Core Metadata

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | String | Yes | Unique plugin identifier (e.g., `"llm-markov"`). Convention: use `llm-` prefix for official plugins. |
| `version` | String | Yes | SemVer version string (e.g., `"1.2.3"`). |
| `description` | String | No | Human-readable description shown in `llm plugins list`. |
| `min_host_version` | String | No | Minimum required host version. Plugin won't load if host version is lower. |
| `authors` | Array[String] | No | Plugin author(s). |
| `homepage` | String | No | URL to plugin homepage or repository. |
| `license` | String | No | SPDX license identifier (e.g., `"MIT"`, `"Apache-2.0"`). |

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
```

### `[capabilities]` — Feature Advertisement

Declares which hook methods the plugin implements. Used for optimization (skip calling hooks the plugin doesn't implement).

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `models` | Boolean | No | `false` | Plugin provides prompt models via `register_models()` |
| `embedding_models` | Boolean | No | `false` | Plugin provides embedding models via `register_embedding_models()` |
| `commands` | Boolean | No | `false` | Plugin provides CLI commands via `register_commands()` |
| `template_loaders` | Boolean | No | `false` | Plugin provides template loaders via `register_template_loaders()` |
| `fragment_loaders` | Boolean | No | `false` | Plugin provides fragment loaders via `register_fragment_loaders()` |
| `tools` | Boolean | No | `false` | Plugin provides tools via `register_tools()` |

**Example:**
```toml
[capabilities]
models = true
embedding_models = false
commands = false
template_loaders = false
fragment_loaders = false
tools = false
```

## V1 Loading (Compile-Time)

### `[rust]` — Compile-Time Integration

Used for V1 feature-gated loading where plugins are compiled into the binary.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `crate_name` | String | Yes | Cargo crate name (e.g., `"llm-plugin-markov"`). |
| `entry_type` | String | Yes | Fully qualified path to the type implementing `PluginEntrypoint` (e.g., `"llm_plugin_markov::MarkovPlugin"`). |

**Example:**
```toml
[rust]
crate_name = "llm-plugin-markov"
entry_type = "llm_plugin_markov::MarkovPlugin"
```

## V2 Loading (Dynamic — Future)

> **Status:** Design only. Not yet implemented.

### `[dylib]` — Dynamic Library Loading

Used for V2 dynamic loading where plugins are loaded at runtime from shared libraries.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | String | Yes | Relative path to shared library (e.g., `"libllm_plugin_markov.so"`). |
| `abi_version` | Integer | Yes | ABI version for compatibility checking. Current ABI version: `1`. |

**Platform library naming conventions:**
- Linux: `libllm_plugin_<name>.so`
- macOS: `libllm_plugin_<name>.dylib`
- Windows: `llm_plugin_<name>.dll`

**Example:**
```toml
[dylib]
path = "libllm_plugin_markov.so"
abi_version = 1
```

## Complete Examples

### Minimal Plugin (Models Only)

```toml
[plugin]
id = "llm-markov"
version = "0.1.0"

[capabilities]
models = true

[rust]
crate_name = "llm-plugin-markov"
entry_type = "llm_plugin_markov::MarkovPlugin"
```

### Full-Featured Plugin

```toml
[plugin]
id = "llm-gemini"
version = "1.0.0"
description = "Google Gemini models with vision, tools, and embeddings"
min_host_version = "1.0.0"
authors = ["llm-rust team"]
homepage = "https://github.com/example/llm-gemini"
license = "Apache-2.0"

[capabilities]
models = true
embedding_models = true
commands = true
template_loaders = false
fragment_loaders = false
tools = false

[rust]
crate_name = "llm-plugin-gemini"
entry_type = "llm_plugin_gemini::GeminiPlugin"
```

### Command-Only Plugin

```toml
[plugin]
id = "llm-cmd"
version = "0.5.0"
description = "Interactive command execution with LLM assistance"
min_host_version = "1.0.0"
authors = ["Simon Willison"]
homepage = "https://github.com/simonw/llm-cmd"
license = "Apache-2.0"

[capabilities]
models = false
embedding_models = false
commands = true
template_loaders = false
fragment_loaders = false
tools = false

[rust]
crate_name = "llm-plugin-cmd"
entry_type = "llm_plugin_cmd::CmdPlugin"
```

### Fragment/Template Loader Plugin

```toml
[plugin]
id = "llm-fragments-github"
version = "0.2.0"
description = "Load fragments and templates from GitHub repositories"
min_host_version = "1.0.0"
authors = ["Simon Willison"]
homepage = "https://github.com/simonw/llm-fragments-github"
license = "Apache-2.0"

[capabilities]
models = false
embedding_models = false
commands = false
template_loaders = true
fragment_loaders = true
tools = false

[rust]
crate_name = "llm-plugin-fragments-github"
entry_type = "llm_plugin_fragments_github::GitHubPlugin"
```

### Future V2 Dylib Example

```toml
[plugin]
id = "llm-custom"
version = "1.0.0"
description = "Custom user plugin loaded dynamically"
min_host_version = "2.0.0"

[capabilities]
models = true
commands = true

[dylib]
path = "libllm_plugin_custom.so"
abi_version = 1
```

## Validation Rules

1. **`plugin.id`** must:
   - Be non-empty
   - Contain only lowercase alphanumeric characters, hyphens, and underscores
   - Start with a letter
   - Be unique across all loaded plugins

2. **`plugin.version`** must:
   - Be a valid SemVer string
   - Follow the pattern `MAJOR.MINOR.PATCH` with optional prerelease/build metadata

3. **`plugin.min_host_version`** (if present) must:
   - Be a valid SemVer string
   - Not exceed the current host version (or plugin is skipped)

4. **`capabilities`** section:
   - At least one capability must be `true`
   - Missing capabilities default to `false`

5. **`rust`** section (V1):
   - Required for compile-time loading
   - `entry_type` must be a valid Rust path

6. **`dylib`** section (V2):
   - Required for dynamic loading
   - `abi_version` must match host ABI version

## Version Compatibility

### Host Version Checking

When a plugin specifies `min_host_version`, the host compares versions using SemVer precedence:

```
min_host_version = "1.5.0"
host_version     = "1.4.0"  → Plugin SKIPPED with warning
host_version     = "1.5.0"  → Plugin LOADED
host_version     = "2.0.0"  → Plugin LOADED
```

### ABI Version Checking (V2)

The ABI version is an integer that tracks breaking changes to the plugin C ABI:

| ABI Version | Host Version | Compatible |
|-------------|--------------|------------|
| 1           | 1.x - 1.y    | Yes        |
| 1           | 2.x          | Maybe (depends on deprecation policy) |
| 2           | 1.x          | No         |
| 2           | 2.x          | Yes        |

**Policy:** ABI version bumps are rare and always accompanied by a major host version release.

## Discovery Locations

### V1 (Compile-Time)

Manifest is optional for V1 — metadata can come from `PluginEntrypoint::metadata()`. If present, the manifest is at:

```
crates/<plugin-crate-name>/llm-plugin.toml
```

### V2 (Dynamic — Future)

Plugins are discovered from:

```
$LLM_USER_PATH/plugins/<plugin-id>/llm-plugin.toml
~/.config/io.datasette.llm/plugins/<plugin-id>/llm-plugin.toml  (default)
```

The host scans each subdirectory for `llm-plugin.toml` and loads compatible plugins.

## Error Handling

| Error | Behavior |
|-------|----------|
| Missing `[plugin]` section | Skip, emit error |
| Missing `plugin.id` | Skip, emit error |
| Missing `plugin.version` | Skip, emit error |
| Invalid SemVer in `version` | Skip, emit error |
| `min_host_version` > host version | Skip, emit warning |
| No capabilities set to `true` | Skip, emit warning |
| `dylib.abi_version` mismatch | Skip, emit error |
| Shared library not found | Skip, emit error |

Errors and warnings are emitted to stderr. Plugin loading failures do not crash the host.

## Related Documents

- [ADR-003: Unified Plugin Registry Architecture](adr/ADR-003-unified-plugin-registry.md)
- [ADR-001: Plugin Runtime Architecture](adr/ADR-001-plugin-runtime-architecture.md)
