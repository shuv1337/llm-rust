# ADR-001: Plugin Runtime Architecture

**Status:** Accepted  
**Date:** 2026-02-24  
**Author:** llm-rust team

## Context

The Rust rewrite of `llm` must support the Python plugin ecosystem via a pyo3 bridge while also enabling future native Rust plugins. Currently, the CLI (`llm-cli`) and provider layer (`llm-core`) use static dispatch—commands are compiled-in enum variants and providers are matched by hardcoded strings. This approach cannot accommodate dynamically discovered plugin commands or models.

Upstream `llm` uses:
- `pluggy` hooks via `@hookimpl` decorators for `register_commands`, `register_models`, `register_embedding_models`, `register_template_loaders`, `register_fragment_loaders`, and `register_tools`
- `importlib.metadata` entrypoint discovery under the `llm` group
- Click command groups for dynamic command attachment

The Rust CLI must integrate these hooks without sacrificing startup performance or requiring Python for non-plugin operations.

## Decision

### 1. Dynamic CLI Command Registry

A `CommandRegistry` will be introduced in `llm-cli`:

```rust
pub struct CommandRegistry {
    /// Core commands (compiled-in, always available)
    core_commands: HashMap<String, CoreCommand>,
    /// Plugin-provided commands (discovered at runtime)
    plugin_commands: HashMap<String, PluginCommand>,
}
```

**Command resolution order:**
1. Core commands are registered at build time and always take precedence.
2. Plugin commands are discovered and registered at startup (if Python bridge enabled).
3. On collision: core command wins; a warning is emitted to stderr:
   ```
   warning: plugin 'foo' attempted to register command 'logs' which is a core command; skipped
   ```

**Collision rules:**
- Core vs plugin: core wins, warning emitted.
- Plugin vs plugin: first registered wins, warning emitted naming both plugins.
- Collisions are logged but do not cause failure (deterministic, non-breaking).

### 2. Model Provider Registry

A `ProviderRegistry` will be introduced in `llm-core`:

```rust
pub struct ProviderRegistry {
    /// Built-in providers (OpenAI, Anthropic, etc.)
    builtin: HashMap<String, Box<dyn PromptProvider>>,
    /// Plugin-registered providers
    plugin: HashMap<String, Box<dyn PromptProvider>>,
}
```

**Model resolution order:**
1. Check aliases (`aliases.json`) first.
2. Match against builtin provider prefixes (e.g., `gpt-4`, `claude-`).
3. Match against plugin-registered models.
4. If ambiguous or unresolved, emit actionable error with suggestions.

**Registration API (internal):**
```rust
impl ProviderRegistry {
    pub fn register_builtin(&mut self, prefix: &str, provider: impl PromptProvider);
    pub fn register_plugin(&mut self, model_id: &str, provider: impl PromptProvider);
    pub fn resolve(&self, model_name: &str) -> Result<&dyn PromptProvider, ModelNotFound>;
}
```

### 3. Bridge Integration Points

The pyo3 bridge (`llm-plugin-host` crate, behind `python-bridge` cargo feature) will:

1. **Entrypoint discovery:** Use Python's `importlib.metadata.entry_points(group='llm')` to find installed plugins.
2. **Hook invocation:** Call each plugin's registered hooks in loading order.
3. **Command forwarding:** For plugin-provided commands, serialize argv and delegate to Python, capturing stdout/stderr.
4. **Model forwarding:** For plugin-provided models, serialize `PromptRequest` → Python → deserialize `PromptCompletion`.

### 4. Feature Gating

- **Default build:** `cargo build` produces a pure-Rust binary. Plugin commands/models are unavailable; `plugins list` shows `(bridge not available)`.
- **Bridge build:** `cargo build --features python-bridge` links pyo3 and enables full plugin discovery.
- **Graceful degradation:** Core commands always work. Missing bridge produces clear user messaging, not crashes.

### 5. Startup Performance

To avoid Python initialization on every invocation:
- Parse argv first; if command is a core command with no plugin dependencies, skip bridge init.
- Lazy-initialize bridge only when needed (plugin commands, plugin models, `plugins --all`).
- Cache plugin metadata after first discovery (invalidate on `install`/`uninstall`).

## Consequences

### Positive
- Plugin ecosystem compatibility without forking upstream plugins.
- Clear command/model precedence rules avoid user confusion.
- Pure-Rust fast path for common operations.
- Future native Rust plugin API can reuse the same registries.

### Negative
- Added complexity in CLI dispatch path.
- pyo3 dependency increases build time when bridge is enabled.
- Plugin command help output may differ slightly from native (subprocess vs in-process).

### Neutral
- Third-party plugin compatibility is a non-blocking quality gate (smoke tested in CI, not required for release).

## References

- Upstream plugins.py: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/plugins.py
- Upstream hookspecs.py: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/hookspecs.py
- Roadmap M5: Plugin bridge implementation milestone
