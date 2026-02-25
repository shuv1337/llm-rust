# AGENTS.md – Rust Rewrite Progress Notes

Use this file to resume work on the Rust rewrite of `llm` after context resets.

## Current Status
- Workspace crates in repo root:
  - `crates/llm-core` (provider abstraction + key/config helpers + logging DB access)
  - `crates/llm-cli` (Clap CLI with `prompt`, `plugins`, `models`, `keys`, `logs`, `cmd`, `version`)
  - `crates/llm-plugin-host` (stub plugin host)
  - `crates/llm-embeddings` (stub embeddings crate)
- Implemented parity surface so far:
  - `keys path/get/list/set/resolve`
  - `logs path/status/on/off/backup/list`
  - `prompt` with streaming + system prompt + key override + attachments + conversation metadata
  - `models list/default`
  - Rust-only `cmd` and `version`
- Test baseline:
  - `llm-cli` integration tests: 33
  - `llm-core` unit tests: 13
  - all passing

## Quick Start Commands
```bash
# from repo root
cargo build
cargo test
cargo run -- --help

# prompt smoke tests (stub mode)
LLM_PROMPT_STUB=1 cargo run -- "hello"
LLM_PROMPT_STUB=1 cargo run -- --no-stream "hello"

# key + logs helpers
cargo run -- keys list --json
cargo run -- keys set openai --value YOUR_API_KEY
cargo run -- logs path
cargo run -- logs list --json --count 3

# cmd auto-accept for non-interactive runs
LLM_PROMPT_STUB=1 LLM_CMD_AUTO_ACCEPT=1 cargo run -- cmd "undo last git commit"
```

## Configuration Notes
- User directory mirrors Python CLI:
  - default: `~/.config/io.datasette.llm`
  - override: `LLM_USER_PATH=/path`
- Keys are stored in `keys.json` with the Python-compatible warning note entry.
- Current default model file in code is still legacy `default-model.txt` (planned migration target: `default_model.txt` with fallback).
- Logs DB migration alignment work is pending:
  - current schema is simplified vs upstream
  - target migration metadata table is `_llm_migrations`
  - target response IDs are ULID-style strings

## Immediate Priorities (from roadmap)
1. **M0 decisions/docs lock-in**
   - ID strategy + deterministic legacy-ID migration ADR
   - plugin runtime architecture ADR (dynamic command/model registries)
   - continuation flag migration policy (`-c`/`--cid` + one-release rewrite shim)
   - binary naming policy (`llm` vs `llm-cli`)
2. **M1 config/storage compatibility**
   - `default_model.txt` fallback behavior
   - `aliases.json`, `model_options.json`
   - migration preflight/dry-run + backup-first behavior
   - `_llm_migrations` compatibility + ULID conversion
3. **M2a prompt/runtime parity**
   - tools/functions/schema model refactor
   - conversation loading for continuation
   - semantic alignment for `--save`, `--database`, `--query`, `--async`
4. **M4 embeddings parity**
   - embeddings provider/model execution layer + commands + DB compatibility
5. **M5 plugin bridge parity**
   - pyo3 bridge + hook support + runtime registration plumbing

## Useful References
- Detailed roadmap: `PLAN-llm-upstream-feature-parity-roadmap.md`
- Native plugin conversion plan: `PLAN-rust-native-plugin-api-and-converter.md`
- Rewrite backlog: `docs/rust-rewrite-plan.md`
- CLI parity matrix: `docs/cli-parity-matrix.md`
- Plugin docs: `docs/plugins/index.md`, `docs/plugins/plugin-hooks.md`
- Quick start: `docs/rust-quickstart.md`

Keep this file updated when milestones change so future sessions can resume quickly.
