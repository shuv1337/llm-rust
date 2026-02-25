# PLAN — Rust-Native Plugin API + Python Plugin Converter

> **Revision 2** — Updated 2026-02-25 after plan review against codebase.
> Changes from v1: ADR-001 alignment, prerequisite milestones, reduced initial
> crate count, realistic timelines, plugin loading mechanism, manifest schema,
> async deferral, CI environment requirements. See "Review changelog" at end.

## Goal

Design and execute a **full Rust-native plugin system** plus a **conversion helper** that can ingest existing Python `llm-*` plugin repositories and generate high-fidelity Rust plugin implementations with parity validation.

This plan **amends and extends** the bridge-first strategy in ADR-001. Rather than maintaining two parallel plugin systems, this plan defines a **unified registry architecture** (documented in a new ADR-003) that serves both native Rust plugins and the future pyo3 bridge through the same `CommandRegistry`, `ProviderRegistry`, and hook registrars. The native plugin track is implemented first; the bridge plugs into the same registries later.

Deliverables:

1. A unified registry infrastructure in `llm-core` and `llm-cli` (prerequisite — shared with bridge path).
2. A fully compatible Rust plugin API (hooks, runtime behavior, command/model registration, loaders, tools, embeddings).
3. A converter pipeline that can extract and reproduce plugin logic into Rust templates/scaffolds (deferred to Phase 2).
4. A parity harness that proves behavior against upstream Python plugins.

## Hard Prerequisites (from AGENTS.md)

This plan **depends on** the following milestones being complete or in-flight from the main parity roadmap. These are not re-specified here but must be satisfied before the corresponding plugin milestone can begin:

| Prerequisite | Required before | Status | Reference |
|---|---|---|---|
| M0: ADR decisions locked (ADR-001, ADR-002) | This plan's M0 | ✅ Done | `docs/adr/ADR-001-plugin-runtime-architecture.md` |
| M1: `aliases.json`, `model_options.json` support | This plan's M2 | ✅ Done | `crates/llm-core/src/aliases.rs`, `model_options.rs` |
| M2a: Tool/function/schema execution in providers | This plan's M2 | ✅ Done | `crates/llm-core/src/providers/mod.rs` (types exist) |
| M2a: Conversation continuation (`-c`/`--cid`) | This plan's M2 | ✅ Done | `crates/llm-cli/src/main.rs` |
| **Template loader execution trait** | This plan's M2 | ✅ Done | `crates/llm-core/src/templates.rs` |
| **Fragment loader abstraction** | This plan's M2 | ✅ Done | `crates/llm-core/src/fragments.rs` |
| **Dynamic `ProviderRegistry` in llm-core** | This plan's M1 | ✅ Done | `crates/llm-core/src/registry.rs` |
| **Dynamic `CommandRegistry` in llm-cli** | This plan's M1 | ✅ Done | `crates/llm-cli/src/command_registry.rs` |
| **Dynamic `EmbeddingRegistry` in llm-embeddings** | This plan's M1 | ✅ Done | `crates/llm-embeddings/src/registry.rs` |

### Prerequisite tasks (must be completed before M1)

These are **new work items** that do not exist anywhere in the codebase today.

#### P1 — Dynamic `ProviderRegistry` in llm-core

**Current state:** `build_provider()` in `crates/llm-core/src/lib.rs:581` is a static `match` on `"openai" | "openai-compatible" | "anthropic"`. No dynamic dispatch.

**Required:** Implement the `ProviderRegistry` from ADR-001:

```rust
// crates/llm-core/src/registry.rs (new file)
pub struct ProviderRegistry {
    builtin: HashMap<String, Box<dyn PromptProvider>>,
    plugin: HashMap<String, Box<dyn PromptProvider>>,
}

impl ProviderRegistry {
    pub fn register_builtin(&mut self, prefix: &str, provider: impl PromptProvider + 'static);
    pub fn register_plugin(&mut self, model_id: &str, provider: impl PromptProvider + 'static);
    pub fn resolve(&self, model_name: &str) -> Result<&dyn PromptProvider, ModelNotFound>;
}
```

- [x] Create `crates/llm-core/src/registry.rs` with `ProviderRegistry`.
- [x] Refactor `build_provider()` to delegate to `ProviderRegistry` resolution.
- [x] Register OpenAI and Anthropic as builtin providers at startup.
- [x] Preserve existing model resolution order (aliases → builtin → plugin → error).
- [x] Add tests for registry resolution, collision, fallback, and registration metadata.

#### P2 — Dynamic `CommandRegistry` in llm-cli

**Current state:** Commands are a Clap `#[derive(Subcommand)]` enum in `crates/llm-cli/src/main.rs`. No runtime command injection.

**Required:**

```rust
// crates/llm-cli/src/command_registry.rs (new file)
pub struct CommandRegistry {
    core_commands: HashMap<String, CoreCommand>,
    plugin_commands: HashMap<String, PluginCommand>,
}
```

- [x] Create `crates/llm-cli/src/command_registry.rs`.
- [x] Add a post-parse dispatch path that checks plugin commands when Clap doesn't match.
- [x] Implement collision rules from ADR-001 (core wins, warning on collision).
- [x] Add tests for command dispatch, collision warnings, and plugin help rendering.

#### P3 — Dynamic `EmbeddingRegistry` in llm-embeddings

**Current state:** `BUILTIN_OPENAI_MODELS` in `crates/llm-embeddings/src/provider.rs` is a hardcoded `&[(&str, &[&str], usize)]` constant. No dynamic registration.

**Required:**

- [x] Create `EmbeddingRegistry` struct with `register_builtin()` and `register_plugin()`.
- [x] Refactor `list_embedding_models()` and `resolve_embedding_model()` to use registry.
- [x] Add tests.

#### P4 — Template loader execution trait

**Current state:** `TemplateLoader` in `crates/llm-core/src/templates.rs` is a metadata-only struct (name + description). `list_template_loaders()` returns a single hardcoded "filesystem" entry with no execution capability.

**Required:**

```rust
pub trait TemplateLoaderImpl: Send + Sync {
    fn prefix(&self) -> &str;
    fn load(&self, key: &str) -> Result<Template>;
    fn description(&self) -> &str;
}
```

- [x] Define `TemplateLoaderImpl` trait with `prefix()`, `load()`, `description()`.
- [x] Implement `FilesystemTemplateLoader` as the built-in.
- [x] Update `load_template()` to route prefix-based lookups through registered loaders.
- [x] Add tests.

#### P5 — Fragment loader abstraction

**Current state:** DB schema has `fragments` tables (`crates/llm-core/src/migrations.rs:245`) but there is no loader abstraction, no `FragmentLoader` trait, no fragment loading code.

**Required:**

```rust
pub trait FragmentLoaderImpl: Send + Sync {
    fn prefix(&self) -> &str;
    fn load(&self, key: &str) -> Result<Vec<Fragment>>;
    fn description(&self) -> &str;
}
```

- [x] Define `FragmentLoaderImpl` trait.
- [x] Define `Fragment` data struct (source, content, hash, metadata).
- [x] Add fragment loader registry.
- [x] Add tests.

**Estimated time for all prerequisites: 2–3 weeks.**

## Baseline (historical at plan creation)

- `crates/llm-plugin-host/src/lib.rs` started as a stub (`load_plugins()` returned `llm-default-plugin-stub`).
- `llm-cli plugins` initially listed only that stub.
- ADR-001 registry direction existed but registries had not yet been implemented.
- `PromptProvider` remained synchronous blocking (still true for V1).

## Progress Snapshot (2026-02-25)

- Dynamic registries are implemented:
  - `ProviderRegistry` (`crates/llm-core/src/registry.rs`)
  - `CommandRegistry` (`crates/llm-cli/src/command_registry.rs`)
  - `EmbeddingRegistry` (`crates/llm-embeddings/src/registry.rs`)
- Loader abstractions are implemented:
  - `TemplateLoaderImpl` + `TemplateLoaderRegistry`
  - `FragmentLoaderImpl` + `FragmentLoaderRegistry`
- Native plugin API crate exists: `crates/llm-plugin-api`.
- Plugin host is feature-gated and lifecycle-aware: `crates/llm-plugin-host`.
- First canary plugin is hand-converted: `crates/llm-plugin-markov` (+ golden test).
- `llm-cli plugins --json` reports real plugin metadata.
- `llm models list` includes plugin-registered models; prompt execution resolves plugin models.

## Success Criteria

- [x] Unified registries (`ProviderRegistry`, `CommandRegistry`, `EmbeddingRegistry`) operational in existing crates.
- [x] Native plugin API supports all upstream hook surfaces:
  - `register_commands`
  - `register_models`
  - `register_embedding_models`
  - `register_template_loaders`
  - `register_fragment_loaders`
  - `register_tools`
- [x] Converted Rust plugins can be discovered and executed without Python (validated with `llm-markov`).
- [ ] Hand-converted canary plugins pass golden-output tests:
  - [x] `llm-markov` (smoke)
  - [ ] `llm-gemini`
  - [ ] `llm-cmd`
  - [ ] `llm-openrouter`
- [ ] Converter (Phase 2) produces a deterministic conversion report with zero silent drops.
- [ ] Parity harness compares Python plugin behavior to Rust plugin behavior and publishes gaps.

## Constraints and Reality Check

- Python plugins are arbitrary Python code; some dynamic/metaprogrammed behavior cannot be perfectly auto-translated.
- Therefore "full conversion" must be defined as:
  1. **Full extraction** (AST + metadata + hook inventory + dependency graph + test inventory), and
  2. **Deterministic Rust generation** with explicit TODOs for non-translatable sections, and
  3. **Parity tests** that fail on behavior drift.
- **Async model execution is deferred.** The current `PromptProvider` trait is synchronous blocking. Upstream `AsyncModel` support will be added in a future milestone after the core plugin API stabilizes. V1 plugins implement sync models only; async will be added as a trait extension later.
- No silent behavior loss is acceptable.

## Proposed Architecture

### 1) Crate plan — phased introduction

**Phase 1 (this plan, V1):**

- [x] `crates/llm-plugin-api`
  - Stable trait/data contract for native plugins.
  - `PluginEntrypoint` trait, registrar traits, plugin metadata types.
  - Request/response/tool/loader/embedding types re-exported from `llm-core`.
- [x] `crates/llm-plugin-host` (enhanced — already exists)
  - Discovery, manifest loading, version checks.
  - Plugin lifecycle (load → register → ready).
  - Integration with unified registries in `llm-core`/`llm-cli`/`llm-embeddings`.
  - Compile-time plugin loading for V1 (see "Plugin loading mechanism").

**Phase 2 (deferred — after canary plugins validated):**

- [ ] `crates/llm-plugin-sdk`
  - Author ergonomics (`#[llm_plugin]` macro, registrars, helper derives).
  - Deferred because canary plugins validate the trait API without macros.
  - Add when there are 10+ plugins and the API is stable.
- [ ] `crates/llm-plugin-convert`
  - Converter CLI: analyze/extract/scaffold/parity commands.
  - Deferred because hand-converting canaries is faster for V1 and validates the API better.
- [ ] `crates/llm-plugin-testkit`
  - Golden fixtures + side-by-side runner (Python plugin vs Rust plugin).
  - Deferred because golden-output tests can live in plugin crate `tests/` initially.

**Rationale:** The workspace has grown from 4 to 6 crates after Phase 1 additions. The remaining 3 plugin-related crates are deferred until the API is proven by real canary plugins.

### 2) Plugin loading mechanism

**V1: Compile-time feature-gated loading.**

Plugins are Rust crates compiled into the binary via Cargo feature flags:

```toml
# Cargo.toml (workspace root)
[features]
default = []
plugin-markov = ["llm-plugin-markov"]
plugin-gemini = ["llm-plugin-gemini"]
plugin-openrouter = ["llm-plugin-openrouter"]
all-plugins = ["plugin-markov", "plugin-gemini", "plugin-openrouter"]
```

```rust
// crates/llm-plugin-host/src/lib.rs
pub fn load_plugins() -> Vec<Box<dyn PluginEntrypoint>> {
    let mut plugins: Vec<Box<dyn PluginEntrypoint>> = Vec::new();
    #[cfg(feature = "plugin-markov")]
    plugins.push(Box::new(llm_plugin_markov::MarkovPlugin));
    #[cfg(feature = "plugin-gemini")]
    plugins.push(Box::new(llm_plugin_gemini::GeminiPlugin));
    // ...
    plugins
}
```

**Benefits:** No ABI fragility, no `dlopen` unsafety, Rust's type system ensures compatibility at compile time, zero startup overhead.

**Tradeoff:** Users must recompile to add/remove plugins. Acceptable for V1 because the primary audience is the project itself converting known plugins.

**Future (V2): Dynamic shared library loading.**

A future ADR will define `dlopen`-based loading with ABI stability contracts. This requires:
- Stable C ABI wrapper around the `PluginEntrypoint` trait.
- Plugin discovery from `~/.config/io.datasette.llm/plugins/` directory.
- Version compatibility checks via `llm-plugin.toml` manifest.

This is explicitly **out of scope** for V1 but the trait design should not preclude it.

### 3) Native hook contract (Rust)

- [x] Define registrars mirroring upstream semantics:

```rust
// crates/llm-plugin-api/src/lib.rs

/// Core trait that every native plugin must implement.
pub trait PluginEntrypoint: Send + Sync {
    /// Plugin metadata.
    fn metadata(&self) -> PluginMetadata;

    /// Register CLI commands provided by this plugin.
    fn register_commands(&self, reg: &mut dyn CommandRegistrar) -> Result<()> { Ok(()) }

    /// Register prompt models provided by this plugin.
    fn register_models(&self, reg: &mut dyn ModelRegistrar) -> Result<()> { Ok(()) }

    /// Register embedding models provided by this plugin.
    fn register_embedding_models(&self, reg: &mut dyn EmbeddingRegistrar) -> Result<()> { Ok(()) }

    /// Register template loaders provided by this plugin.
    fn register_template_loaders(&self, reg: &mut dyn TemplateLoaderRegistrar) -> Result<()> { Ok(()) }

    /// Register fragment loaders provided by this plugin.
    fn register_fragment_loaders(&self, reg: &mut dyn FragmentLoaderRegistrar) -> Result<()> { Ok(()) }

    /// Register tools available to models during prompting.
    fn register_tools(&self, reg: &mut dyn ToolRegistrar) -> Result<()> { Ok(()) }
}

/// Required metadata for every plugin.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Unique plugin identifier (e.g., "llm-markov").
    pub id: String,
    /// SemVer version string.
    pub version: String,
    /// Capabilities this plugin provides.
    pub capabilities: Vec<PluginCapability>,
    /// Minimum host version required.
    pub min_host_version: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginCapability {
    Models,
    EmbeddingModels,
    Commands,
    TemplateLoaders,
    FragmentLoaders,
    Tools,
}
```

- [x] Preserve ADR-001 collision policy (core commands win; deterministic warnings).
- [x] Registrar traits are defined in `llm-plugin-api` and implemented by the unified registries.
- [x] The `PluginEntrypoint` trait uses default method implementations so plugins only override the hooks they need.

### 4) Plugin manifest format (`llm-plugin.toml`)

Strawman schema for V1 — used for metadata and future dynamic loading:

```toml
[plugin]
id = "llm-markov"
version = "0.1.0"
description = "Markov chain text generation model"
min_host_version = "1.0.0"

[capabilities]
models = true
embedding_models = false
commands = false
template_loaders = false
fragment_loaders = false
tools = false

[rust]
# Compile-time integration (V1)
crate_name = "llm-plugin-markov"
entry_type = "llm_plugin_markov::MarkovPlugin"

# Future dynamic loading (V2)
# dylib = "libllm_plugin_markov.so"
```

- [x] Define and document the manifest schema.
- [ ] `llm-plugin-host` reads manifests for metadata/version checks.
- [x] Manifests are optional in V1 (metadata can come from `PluginMetadata` trait method).
- [x] Required for V2 dynamic loading.

### 5) Data model parity layer

- [ ] Model execution parity: streaming, **sync-only for V1**, tool calls/results, schema mode, attachments, usage, resolved model.
- [x] Command parity: option parsing, help text fidelity, output channels (stdout/stderr), exit codes.
- [x] Loader parity: template/fragment prefix registration and error semantics.
- [ ] Tool parity: schema generation, invocation lifecycle, side effects, logging metadata.
- [ ] Embedding parity: batching semantics, dimension checks, model defaults.

**Async model execution** is explicitly deferred. The `PluginEntrypoint` trait and `ModelRegistrar` will be designed to allow a future `register_async_models()` method without breaking changes, but V1 plugins implement sync models only.

### 6) Plugin packaging and discovery

- [x] V1: Compile-time feature flags (no runtime discovery needed).
- [ ] V2 (future): Define discovery roots (`~/.config/io.datasette.llm/plugins/` + bundled plugins).
- [x] Add compatibility/version gate checks (manifest `min_host_version` vs `core_version()`).
- [ ] Add plugin cache invalidation on install/uninstall/refresh (V2 only).

## Converter Design (`llm-plugin-convert`) — Phase 2

> **This entire section is deferred to Phase 2.** Phase 1 validates the plugin API
> by hand-converting canary plugins. The converter is built after the API is proven
> stable and the hand-conversion patterns are well-understood.

### CLI surface

- [ ] `llm-cli plugins convert analyze <repo-or-path>`
- [ ] `llm-cli plugins convert scaffold <repo-or-path> --out <dir>`
- [ ] `llm-cli plugins convert parity <python-plugin> <rust-plugin>`
- [ ] `llm-cli plugins convert doctor <generated-plugin-dir>`

### Pipeline stages

#### Stage A — Extract (2–3 weeks)
- [ ] Clone/fetch repository.
- [ ] Parse `pyproject.toml` / entrypoints.
- [ ] Parse Python AST for `@llm.hookimpl` functions (using `rustpython-parser` or similar).
- [ ] Build IR:
  - hooks implemented
  - model classes and inheritance (`Model`, `AsyncModel`, `KeyModel`, etc.)
  - command definitions (Click graph)
  - tool functions/classes
  - template/fragment loaders
  - embedding models
  - dependency imports and external SDKs
  - test inventory
- [ ] Emit `conversion-report.json` and markdown summary.

#### Stage B — Classify (1 week)
- [ ] Classify plugin into one or more archetypes:
  - Remote API model
  - Local runtime model
  - Embedding model
  - Tool plugin
  - Loader plugin
  - Command plugin
  - Hybrid
- [ ] Compute complexity score and unresolved-risk score.

#### Stage C — Generate (2–3 weeks)
- [ ] Generate Rust plugin crate scaffold:
  - `Cargo.toml`
  - `src/lib.rs`
  - `src/models/*.rs`
  - `src/commands/*.rs`
  - `src/tools/*.rs`
  - `src/loaders/*.rs`
  - `llm-plugin.toml`
  - `tests/parity/*.rs`
- [ ] Populate mapped logic for known templates.
- [ ] Insert explicit TODO blocks for non-translatable Python constructs.

#### Stage D — Validate (1 week)
- [ ] Compile generated plugin.
- [ ] Run static checks (`fmt`, `clippy`, unit tests).
- [ ] Run side-by-side parity tests against upstream Python plugin.
- [ ] Emit `parity-report.md` with pass/fail per scenario.

**Total estimated Phase 2 converter timeline: 6–8 weeks.**

## Template Library (required for converter, useful for hand-conversion)

Templates are ordered by coverage impact. The first two alone cover ~60% of the plugin directory (30 of 52 are OpenAI-compatible remote API models).

**Tier 1 — High coverage (implement first, useful even before converter):**

- [ ] `template:model-openai-compatible` — Covers OpenAI-derived providers (OpenRouter, DeepSeek, Groq, Fireworks, Perplexity, Together, etc.)
- [ ] `template:model-http-sse` — SSE streaming for providers with non-OpenAI streaming formats.
- [ ] `template:embedding-http-batch` — Remote embedding providers (Jina, OpenAI-compatible).

**Tier 2 — Medium coverage:**

- [ ] `template:model-http-json` — Non-streaming HTTP JSON providers.
- [ ] `template:tool-function` — Simple single-function tool plugins.
- [ ] `template:command-click-basic` — Single command plugins.
- [ ] `template:command-click-group` — Command group plugins.
- [ ] `template:cache-file-json` — JSON file caching pattern (model lists, etc.).

**Tier 3 — Specialized:**

- [ ] `template:tool-class/toolbox` — Multi-tool class plugins.
- [ ] `template:fragment-loader-remote` — Remote fragment loaders (GitHub, PyPI, etc.).
- [ ] `template:template-loader-remote` — Remote template loaders (GitHub, Fabric, etc.).

Each template must include:
- [ ] Mapping rules from Python IR nodes → Rust codegen fragments.
- [ ] Required manual checkpoints.
- [ ] Known caveats.

## Canary conversion program (first wave)

**Phase 1 canaries are hand-converted** to validate the plugin API. The converter (Phase 2) will later be validated by reproducing these conversions automatically.

Recommended pre-canary smoke plugin:
- **`llm-markov`** — ~50 lines Python, deterministic output, exercises `register_models` only. Ideal for validating the full plugin lifecycle end-to-end before tackling complex plugins.

Required canary plugins (used as design anchors):

1. **`llm-gemini`**
   - Hybrid plugin: models + embeddings + commands.
   - Exercises: streaming parsing, schema/tools/attachments, usage metadata, model catalog commands.
2. **`llm-cmd`**
   - Command-heavy interactive UX plugin.
   - Exercises: command registration, terminal editing behavior, subprocess execution semantics.
3. **`llm-openrouter`**
   - OpenAI-derived model plugin + command group + model metadata caching.
   - Exercises: provider inheritance translation (uses `template:model-openai-compatible`), remote model list + cache behavior.

## Milestones and Implementation Order

### M0 — Spec lock + architecture decisions (1–2 weeks)

- [x] **Write ADR-003: Unified plugin registry architecture.**
  - Amends ADR-001 to define a single registry serving both native and bridge plugins.
  - Specifies V1 loading mechanism (compile-time feature flags).
  - Specifies V2 loading mechanism (dynamic shared libraries — design only, not implemented).
  - Defines ABI boundary considerations for future dylib support.
- [x] Write ADR-004: Converter IR schema and unsupported-pattern policy (Phase 2 reference).
- [x] Freeze hook/trait surface in `llm-plugin-api` v0 (the `PluginEntrypoint` trait above).
- [x] Freeze `llm-plugin.toml` manifest schema.
- [x] Define parity acceptance rubric for converted plugins (golden-output match criteria).

### M1 — Prerequisite: Dynamic registries (2–3 weeks)

> **This milestone builds the unified registries that ADR-001 designed but were never implemented.
> Both this plan and the future pyo3 bridge depend on this work.**

- [x] **P1:** Implement `ProviderRegistry` in `llm-core` (see prerequisite P1 above).
- [x] **P2:** Implement `CommandRegistry` in `llm-cli` (see prerequisite P2 above).
- [x] **P3:** Implement `EmbeddingRegistry` in `llm-embeddings` (see prerequisite P3 above).
- [x] **P4:** Implement `TemplateLoaderImpl` trait in `llm-core` (see prerequisite P4 above).
- [x] **P5:** Implement `FragmentLoaderImpl` trait and `Fragment` data model in `llm-core` (see prerequisite P5 above).
- [x] Verify all existing tests still pass after registry refactor.
- [x] Add registry-specific tests (resolve, collision, fallback, empty).

### M2 — Plugin API crate + enhanced plugin host (2 weeks)

- [x] Create `crates/llm-plugin-api`:
  - `PluginEntrypoint` trait with default-method hooks.
  - `PluginMetadata` and `PluginCapability` types.
  - Registrar trait definitions (`CommandRegistrar`, `ModelRegistrar`, `EmbeddingRegistrar`, `TemplateLoaderRegistrar`, `FragmentLoaderRegistrar`, `ToolRegistrar`).
  - Re-export necessary types from `llm-core` (`PromptProvider`, `EmbeddingProvider`, `ToolDefinition`, etc.).
- [x] Enhance `crates/llm-plugin-host`:
  - Replace stub `load_plugins()` with feature-gated plugin loading.
  - Add plugin lifecycle: load → metadata check → version gate → register hooks.
  - Wire registrars into unified registries from M1.
  - Implement collision diagnostics (ADR-001 rules: core wins, warning on collision).
  - Add `llm-plugin.toml` manifest parsing (optional — metadata can come from trait).
- [x] Update `Cargo.toml` workspace: add `llm-plugin-api` member, update `llm-plugin-host` dependencies.
- [x] Update `llm-cli plugins list` to show real plugin metadata.

### M3 — Hook parity + execution semantics (2–3 weeks)

- [x] **Commands:** Plugin commands dispatch through `CommandRegistry`. Plugin provides argument spec + handler function. Help text forwarded correctly.
- [ ] **Models (sync only):** Plugin models implement `PromptProvider`. Streaming via `StreamSink`. Tool calls/results, schema mode, attachments, usage metadata.
- [ ] **Embeddings:** Plugin embedding models implement `EmbeddingProvider`. Batch semantics, dimension declaration.
- [x] **Template loaders:** Plugin loaders implement `TemplateLoaderImpl`. Prefix-based routing.
- [x] **Fragment loaders:** Plugin loaders implement `FragmentLoaderImpl`. Multi-fragment returns, hash computation.
- [ ] **Tools:** Plugin tools implement schema generation + invocation handler. Tool results logged to DB.
- [x] Contract tests for each hook family (one test per registrar that exercises the full lifecycle).

### M4 — Canary hand-conversions (3–4 weeks)

> Hand-convert each canary plugin as a separate crate. No automated converter — the goal is to
> validate the API and build the template patterns that the converter will later automate.

- [x] **`llm-markov`** (smoke — 1–2 days)
  - Create `crates/llm-plugin-markov/`.
  - Implement `PluginEntrypoint` with `register_models` only.
  - Markov chain model implementing `PromptProvider` (sync, no streaming).
  - Golden-output test matching Python `llm-markov` output for deterministic seeds.
- [ ] **`llm-cmd`** (1 week)
  - Create `crates/llm-plugin-cmd/`.
  - Implement `register_commands` with interactive terminal UX.
  - Subprocess execution semantics, exit code handling.
  - Integration test with pseudo-TTY interaction snapshots.
- [ ] **`llm-openrouter`** (1 week)
  - Create `crates/llm-plugin-openrouter/`.
  - Implement `register_models` + `register_commands`.
  - OpenAI-compatible provider (validates `template:model-openai-compatible` pattern).
  - Remote model list + JSON file cache.
  - Golden-output test for model list and prompt execution.
- [ ] **`llm-gemini`** (1–2 weeks)
  - Create `crates/llm-plugin-gemini/`.
  - Implement `register_models` + `register_embedding_models` + `register_commands`.
  - Streaming SSE parsing, tool calls, schema mode, attachments, usage metadata.
  - Embedding provider with batch support.
  - Golden-output tests for model catalog, prompt, and embedding.
- [ ] Document patterns discovered during hand-conversion for each template archetype.
- [ ] All canary plugins pass `cargo test` and golden-output parity checks.

### M5 — Converter pipeline (Phase 2, 6–8 weeks)

> **Deferred until after M4 is complete and the plugin API is stable.**

#### M5a — Extractor + IR (3–4 weeks)
- [ ] Build extractor for `pyproject.toml` + Python AST + test inventory.
- [ ] Build IR schema (hook inventory, model classes, Click graphs, dependencies).
- [ ] Build conversion report generation (`conversion-report.json` + markdown summary).
- [ ] Build unsupported-pattern detector and fail-fast rules.
- [ ] Validate extractor against all 4 canary plugin repos.

#### M5b — Scaffolder + templates (3–4 weeks)
- [ ] Build scaffold generator using template engine.
- [ ] Implement Tier 1 templates (`model-openai-compatible`, `model-http-sse`, `embedding-http-batch`).
- [ ] Implement Tier 2 templates.
- [ ] Verify scaffolder reproduces canary plugins (compare to hand-converted versions).
- [ ] Build `doctor` command for post-scaffold validation.

### M6 — Directory-scale rollout (ongoing waves)
- [ ] Convert P1 bucket (12 plugins).
- [ ] Convert P2 bucket (26 plugins).
- [ ] Convert P3 bucket / long-tail (6 plugins).
- [ ] Publish compatibility dashboard.

### M7 — Productization and maintenance
- [ ] Add `install/uninstall` flow for native plugins.
- [ ] Implement V2 dynamic plugin loading (dylib) per ADR-003.
- [ ] Implement `llm-plugin-sdk` proc-macro crate (`#[llm_plugin]`).
- [ ] Add CI jobs for converter regressions and parity drift checks.
- [ ] Add plugin author docs and converter playbook.

## CI and Test Environment Requirements

### Standard CI (runs on every PR)
- `cargo build --workspace` — all crates compile.
- `cargo test --workspace` — all unit + integration tests pass.
- `cargo clippy --workspace` — no warnings.
- `cargo fmt --check` — formatting clean.
- Canary plugin golden-output tests (committed fixtures, no Python required).

### Parity CI (runs on schedule or manual trigger)

The side-by-side parity harness requires a Python environment:

- [ ] Python 3.10+ installed.
- [ ] `pip install llm` (upstream tool).
- [ ] `pip install llm-markov llm-cmd llm-openrouter llm-gemini` (canary plugins).
- [ ] Parity runner invokes both Python and Rust plugins with identical inputs.
- [ ] Output normalized (strip nondeterminism: timestamps, ULIDs, etc.).
- [ ] Diff published as `parity-report.md` artifact.

**Alternative for V1:** Capture golden outputs from Python once, commit as test fixtures. Run Rust plugins against fixtures in standard CI. Only run live parity checks periodically.

## Validation plan

- [ ] Golden-output tests per converted plugin (committed fixtures, no Python needed).
- [ ] Side-by-side Python vs Rust snapshot tests — normalized nondeterminism (Parity CI only).
- [ ] Contract tests for each hook family (one per registrar trait).
- [ ] Registry resolution tests (builtin vs plugin, collision, fallback).
- [ ] Performance sanity checks (startup latency with 0/5/20 plugins compiled in).
- [ ] Security checks for tool and command plugins (sandboxing considerations for V2 dylib).

## Risks and mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| **Arbitrary Python dynamism** | High | Strict unsupported detector + TODO emission + parity fail. Defer metaprogrammed plugins to P3. |
| **Provider SDK differences** | Medium | Template library + provider abstraction adapters. Tier 1 templates cover 60% of plugins. |
| **Terminal UX drift (`llm-cmd`)** | Medium | Interaction snapshots + integration tests in pseudo-TTY. |
| **Long-tail plugin entropy** | Low | Tiered priority rollout and quality gates. |
| **Async model execution gap** | Medium | Explicitly deferred. `PluginEntrypoint` designed to allow future `register_async_models()` without breaking changes. |
| **Registry refactor breaks existing tests** | Medium | M1 registry work is prerequisite with full test coverage before any plugin code. |
| **V2 dylib ABI instability** | High | V1 uses compile-time loading. V2 ADR will define ABI stability contract before implementation. |
| **Converter timeline overrun** | High | Converter deferred to Phase 2. Hand-converted canaries prove the API in Phase 1. |
| **CI environment complexity for parity** | Low | Golden-output fixtures committed to repo. Live parity runs only on schedule. |

## Internal code references

- `crates/llm-plugin-host/src/lib.rs` — feature-gated plugin host with lifecycle registrars
- `crates/llm-plugin-host/Cargo.toml` — current dependencies
- `crates/llm-cli/src/main.rs` — Clap command dispatch, `list_plugins()` at line 1690
- `crates/llm-core/src/lib.rs` — `build_provider()` at line 581, `BUILTIN_MODELS` static dispatch
- `crates/llm-core/src/providers/mod.rs` — `PromptProvider` trait, tool/schema types
- `crates/llm-core/src/templates.rs` — `TemplateLoader` metadata struct, `list_template_loaders()`
- `crates/llm-core/src/migrations.rs:245` — fragment DB tables (no loader code)
- `crates/llm-embeddings/src/provider.rs` — `EmbeddingProvider` trait, `BUILTIN_OPENAI_MODELS` constant
- `docs/adr/ADR-001-plugin-runtime-architecture.md` — registry design (not yet implemented)
- `docs/adr/ADR-002-id-migration-strategy.md`
- `docs/rust-rewrite-plan.md`
- `PLAN-llm-upstream-feature-parity-roadmap.md`
- `AGENTS.md` — current milestone status and priorities

## External references

- Upstream hook specs: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/hookspecs.py
- Upstream plugin runtime: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/plugins.py
- Plugin directory: https://llm.datasette.io/en/stable/plugins/directory.html#plugin-directory
- Canary repos:
  - https://github.com/simonw/llm-gemini
  - https://github.com/simonw/llm-cmd
  - https://github.com/simonw/llm-openrouter
  - https://github.com/simonw/llm-markov

## Full plugin inventory (extracted from plugin directory)

Extraction snapshot: **52 plugins**.

- Priority buckets: P0=8, P1=12, P2=26, P3=6
- Difficulty buckets: XL=10, L=15, M=17, S=9, XS=1

Legend:
- Priority: `P0` immediate canaries/foundation, `P1` high-value next wave, `P2` medium wave, `P3` long-tail.
- Difficulty: `XS` very small, `S` small, `M` medium, `L` large, `XL` extra-large.

| Priority | Difficulty | Plugin | Category | Hook surface | Why this bucket | Repo |
|---|---|---|---|---|---|---|
| P0 | XL | `llm-gemini` | Remote APIs | `register_models + register_embedding_models + register_commands` | Complex canary: tools, schema, attachments, streaming, embeddings, extra commands | https://github.com/simonw/llm-gemini |
| P0 | L | `llm-cmd` | Extra commands | `register_commands` | Interactive shell command UX and execution flow parity | https://github.com/simonw/llm-cmd |
| P0 | L | `llm-fragments-github` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | High-value fragment loader with multi-fragment behaviors | https://github.com/simonw/llm-fragments-github |
| P0 | L | `llm-openrouter` | Remote APIs | `register_models + register_commands` | OpenAI-derived model registry + cached catalog + command group | https://github.com/simonw/llm-openrouter |
| P0 | M | `llm-embed-jina` | Embedding models | `register_embedding_models` | Remote embedding canary for embedding hook + auth path | https://github.com/simonw/llm-embed-jina |
| P0 | S | `llm-templates-github` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Small template-loader canary with remote fetch | https://github.com/simonw/llm-templates-github |
| P0 | S | `llm-tools-simpleeval` | Tools | `register_tools` | Smallest tool plugin; ideal tool-registration baseline | https://github.com/simonw/llm-tools-simpleeval |
| P0 | XS | `llm-markov` | Just for fun | `register_models` | Tiny deterministic model canary for converter smoke tests | https://github.com/simonw/llm-markov |
| P1 | XL | `llm-anthropic` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-anthropic |
| P1 | XL | `llm-sentence-transformers` | Embedding models | `register_embedding_models` | Embedding batch semantics + vector DB compatibility | https://github.com/simonw/llm-sentence-transformers |
| P1 | L | `llm-cluster` | Extra commands | `register_commands` | Dynamic CLI command shape and UX parity | https://github.com/simonw/llm-cluster |
| P1 | L | `llm-mistral` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-mistral |
| P1 | M | `llm-fireworks` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-fireworks |
| P1 | M | `llm-jq` | Extra commands | `register_commands` | Dynamic CLI command shape and UX parity | https://github.com/simonw/llm-jq |
| P1 | M | `llm-ollama` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/taketwo/llm-ollama |
| P1 | M | `llm-python` | Extra commands | `register_commands` | Dynamic CLI command shape and UX parity | https://github.com/simonw/llm-python |
| P1 | M | `llm-tools-sqlite` | Tools | `register_tools` | Tool schema + execution sandbox behavior | https://github.com/simonw/llm-tools-sqlite |
| P1 | S | `llm-anyscale-endpoints` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-anyscale-endpoints |
| P1 | S | `llm-deepseek` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/abrasumente233/llm-deepseek |
| P1 | S | `llm-groq` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/angerman/llm-groq |
| P2 | XL | `llm-bedrock` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-bedrock |
| P2 | XL | `llm-bedrock-anthropic` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/sblakey/llm-bedrock-anthropic |
| P2 | XL | `llm-bedrock-meta` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/flabat/llm-bedrock-meta |
| P2 | XL | `llm-clip` | Embedding models | `register_embedding_models` | Embedding batch semantics + vector DB compatibility | https://github.com/simonw/llm-clip |
| P2 | L | `llm-embed-onnx` | Embedding models | `register_embedding_models` | Embedding batch semantics + vector DB compatibility | https://github.com/simonw/llm-embed-onnx |
| P2 | L | `llm-fragments-pdf` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/daturkel/llm-fragments-pdf |
| P2 | L | `llm-fragments-site-text` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/daturkel/llm-fragments-site-text |
| P2 | L | `llm-gguf` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-gguf |
| P2 | L | `llm-llamafile` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-llamafile |
| P2 | L | `llm-replicate` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-replicate |
| P2 | L | `llm-tools-quickjs` | Tools | `register_tools` | Tool schema + execution sandbox behavior | https://github.com/simonw/llm-tools-quickjs |
| P2 | M | `llm-cohere` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/Accudio/llm-cohere |
| P2 | M | `llm-command-r` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-command-r |
| P2 | M | `llm-grok` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/Hiepler/llm-grok |
| P2 | M | `llm-hacker-news` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/simonw/llm-hacker-news |
| P2 | M | `llm-perplexity` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/hex/llm-perplexity |
| P2 | M | `llm-reka` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-reka |
| P2 | M | `llm-templates-fabric` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/simonw/llm-templates-fabric |
| P2 | M | `llm-together` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/wearedevx/llm-together |
| P2 | M | `llm-tools-datasette` | Tools | `register_tools` | Tool schema + execution sandbox behavior | https://github.com/simonw/llm-tools-datasette |
| P2 | M | `llm-tools-exa` | Tools | `register_tools` | Tool schema + execution sandbox behavior | https://github.com/daturkel/llm-tools-exa |
| P2 | M | `llm-tools-rag` | Tools | `register_tools` | Tool schema + execution sandbox behavior | https://github.com/daturkel/llm-tools-rag |
| P2 | S | `llm-fragments-pypi` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/samueldg/llm-fragments-pypi |
| P2 | S | `llm-fragments-reader` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/simonw/llm-fragments-reader |
| P2 | S | `llm-lambda-labs` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/simonw/llm-lambda-labs |
| P2 | S | `llm-venice` | Remote APIs | `register_models` | Remote provider auth/options/streaming compatibility | https://github.com/ar-jan/llm-venice |
| P3 | XL | `llm-cmd-comp` | Extra commands | `register_commands` | Dynamic CLI command shape and UX parity | https://github.com/CGamesPlay/llm-cmd-comp |
| P3 | XL | `llm-mlx` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-mlx |
| P3 | XL | `llm-mpt30b` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-mpt30b |
| P3 | L | `llm-gpt4all` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-gpt4all |
| P3 | L | `llm-mlc` | Local models | `register_models` | Local runtime bindings, model packaging, and platform quirks | https://github.com/simonw/llm-mlc |
| P3 | L | `llm-video-frames` | Fragments and template loaders | `register_fragment_loaders / register_template_loaders` | Fragment/template loader semantics and remote I/O | https://github.com/simonw/llm-video-frames |

## Timeline summary

| Milestone | Duration | Phase | Cumulative |
|---|---|---|---|
| M0 — Spec lock + ADRs | 1–2 weeks | Phase 1 | 1–2 weeks |
| M1 — Dynamic registries (prerequisite) | 2–3 weeks | Phase 1 | 3–5 weeks |
| M2 — Plugin API + enhanced host | 2 weeks | Phase 1 | 5–7 weeks |
| M3 — Hook parity + execution | 2–3 weeks | Phase 1 | 7–10 weeks |
| M4 — Canary hand-conversions | 3–4 weeks | Phase 1 | 10–14 weeks |
| **Phase 1 total** | **10–14 weeks** | | |
| M5 — Converter pipeline | 6–8 weeks | Phase 2 | 16–22 weeks |
| M6 — Directory-scale rollout | ongoing | Phase 2 | — |
| M7 — Productization | ongoing | Phase 2 | — |

## Review changelog

**v1 → v2 (2026-02-25):**

1. **ADR-001 conflict resolved.** Plan now amends ADR-001 via new ADR-003 defining unified registries for both native and bridge plugins. No parallel registry systems.
2. **Dynamic registry prerequisite added.** New M1 milestone (2–3 weeks) to implement `ProviderRegistry`, `CommandRegistry`, and `EmbeddingRegistry` in existing crates — these were designed in ADR-001 but never built. Five concrete prerequisite tasks (P1–P5) with code references.
3. **Initial crate count reduced from 5 to 2.** Phase 1 creates only `llm-plugin-api` and enhances `llm-plugin-host`. `llm-plugin-sdk`, `llm-plugin-convert`, and `llm-plugin-testkit` deferred to Phase 2.
4. **Async model execution explicitly deferred.** V1 is sync-only. `PromptProvider` trait is synchronous blocking; no async runtime exists in workspace. Trait designed to allow future `register_async_models()` without breaking changes.
5. **Converter timeline fixed.** Moved from 2 weeks to 6–8 weeks and deferred to Phase 2. Phase 1 hand-converts canaries to validate API. Converter built after patterns are proven.
6. **Plugin loading mechanism specified.** V1: compile-time feature flags (no ABI fragility). V2 (future): `dlopen` dynamic loading with manifest-based discovery.
7. **`llm-plugin.toml` manifest schema drafted.** Concrete TOML strawman with plugin metadata, capabilities, and Rust crate entry point.
8. **Template/fragment loader gaps acknowledged.** Prerequisite tasks P4/P5 create the `TemplateLoaderImpl` and `FragmentLoaderImpl` execution traits that don't exist today.
9. **AGENTS.md prerequisites table added.** Explicit dependency on M0–M2a parity milestones with status tracking.
10. **CI environment requirements section added.** Standard CI (no Python needed) vs Parity CI (Python + plugins). Golden-output fixtures as V1 alternative to live parity runs.
11. **Template library reordered by coverage impact.** Tier 1 (OpenAI-compatible, HTTP-SSE, embedding-batch) covers ~60% of plugin directory. Implement first.
12. **Canary section restructured.** Explicitly "hand-converted" in Phase 1. `llm-markov` pre-canary with 1–2 day estimate. Per-canary time estimates added.
13. **Risk table expanded.** Added async gap, registry refactor risk, dylib ABI risk, converter timeline risk, CI complexity.
14. **Timeline summary table added.** Phase 1: 10–14 weeks. Phase 2: 6–8 weeks for converter.
