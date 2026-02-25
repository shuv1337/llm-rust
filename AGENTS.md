# AGENTS.md – Rust Rewrite Progress Notes

Use this file to resume work on the Rust rewrite of `llm` after context resets.

## Current Status
- Workspace crates in repo root:
  - `crates/llm-core`
  - `crates/llm-cli`
  - `crates/llm-embeddings`
  - `crates/llm-plugin-api`
  - `crates/llm-plugin-host`
  - `crates/llm-plugin-markov` (first hand-converted canary)
- Implemented parity surface so far:
  - `keys path/get/list/set/resolve`
  - `aliases path/list/set/remove`
  - `logs path/status/on/off/backup/list` (+ filters)
  - `prompt` with streaming + system prompt + key override + attachments + conversation metadata
  - continuation migration (`-c` / `--continue` / `--cid` rewrite path)
  - `models list/default/options`
  - templates + template loaders
  - embeddings commands + collection storage
  - Rust-only `cmd` and `version`
- Plugin architecture progress:
  - Dynamic `ProviderRegistry` in `llm-core`
  - Dynamic `CommandRegistry` in `llm-cli`
  - Dynamic `EmbeddingRegistry` in `llm-embeddings`
  - `TemplateLoaderImpl` + `FragmentLoaderImpl` registries
  - `llm-plugin-api` crate with `PluginEntrypoint` + registrar traits
  - feature-gated plugin host in `llm-plugin-host`
  - markov canary plugin wired into CLI (`--model markov`)

## Quick Start Commands
```bash
# from repo root
cargo build
cargo test
cargo run -- --help

# prompt smoke tests (stub mode)
LLM_PROMPT_STUB=1 cargo run -- "hello"
LLM_PROMPT_STUB=1 cargo run -- --no-stream "hello"

# markov plugin smoke test (no API key required)
cargo run -- --model markov --no-stream "the quick brown fox jumps over the lazy dog"

# key + logs helpers
cargo run -- keys list --json
cargo run -- keys set openai --value YOUR_API_KEY
cargo run -- logs path
cargo run -- logs list --json --count 3

# plugin metadata
cargo run -- plugins --json
```

## Configuration Notes
- User directory mirrors Python CLI:
  - default: `~/.config/io.datasette.llm`
  - override: `LLM_USER_PATH=/path`
- Keys are stored in `keys.json` with the Python-compatible warning note entry.
- Default model file writes to `default_model.txt` and still reads legacy `default-model.txt` fallback.
- Logs DB migration alignment is in progress:
  - `_llm_migrations` metadata table present
  - response IDs migrated to ULID strings

## Immediate Priorities
1. **M3 hook parity completion**
   - embedding provider execution path for plugin-provided embeddings
   - tool invocation + DB logging integration for plugin tools
2. **M4 canaries**
   - hand-convert `llm-cmd`
   - hand-convert `llm-openrouter`
   - hand-convert `llm-gemini`
3. **Phase 2 prep**
   - converter extractor/scaffolder commands based on ADR-004 IR contract

## Useful References
- Detailed roadmap: `PLAN-llm-upstream-feature-parity-roadmap.md`
- Native plugin conversion plan: `PLAN-rust-native-plugin-api-and-converter.md`
- Rewrite backlog: `docs/rust-rewrite-plan.md`
- CLI parity matrix: `docs/cli-parity-matrix.md`
- Plugin docs: `docs/plugins/index.md`, `docs/plugins/plugin-hooks.md`
- ADRs: `docs/adr/ADR-001-plugin-runtime-architecture.md`, `docs/adr/ADR-003-unified-plugin-registry.md`, `docs/adr/ADR-004-converter-ir-and-unsupported-pattern-policy.md`

Keep this file updated when milestones change so future sessions can resume quickly.
