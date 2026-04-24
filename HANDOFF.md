# HANDOFF

## Objective
- Continue the Rust rewrite of `llm`, preserving parity with the Python CLI while advancing plugin hook support and canary plugin conversion.

## Current status
- Workspace crates currently called out by the user: `crates/llm-core`, `crates/llm-cli`, `crates/llm-embeddings`, `crates/llm-plugin-api`, `crates/llm-plugin-host`, and `crates/llm-plugin-markov`.
- Implemented parity surface so far includes keys, aliases, logs, prompt streaming/non-streaming with metadata and attachments, continuation migration, models, templates/loaders, embeddings commands/storage, Rust-only `cmd`, and `version`.
- Plugin architecture currently has dynamic provider, command, embedding, template-loader, and fragment-loader registries; `llm-plugin-api`; feature-gated `llm-plugin-host`; the Markov canary wired as `--model markov`; and a Markov embedding canary wired as `embed --model markov-embed`.
- M3 plugin-provided embedding execution is implemented: plugins can register executable embedding providers, and `embed`, `embed-multi`, and `similar` route through the embedding provider factory.

## Key context
- User guidance is in root `AGENTS.md` content provided in the conversation. Keep that file updated when rewrite milestones change.
- User expects concrete end-to-end delivery when asked to implement: inspect current repo state, make changes, validate with commands, and report exact blockers.
- Default user config path mirrors Python CLI: `~/.config/io.datasette.llm`; override with `LLM_USER_PATH=/path`.
- Keys are stored in `keys.json` with the Python-compatible warning note entry.
- Default model writes to `default_model.txt` and still reads legacy `default-model.txt`.
- Logs DB migration alignment is still in progress; `_llm_migrations` exists and response IDs have been migrated to ULID strings.

## Important files
- `AGENTS.md` — repo guidance and current rewrite progress notes.
- `PLAN-llm-upstream-feature-parity-roadmap.md` — detailed parity roadmap.
- `PLAN-rust-native-plugin-api-and-converter.md` — native plugin conversion plan.
- `docs/rust-rewrite-plan.md` — rewrite backlog.
- `docs/cli-parity-matrix.md` — parity tracking.
- `docs/plugins/index.md` and `docs/plugins/plugin-hooks.md` — plugin docs.
- `docs/adr/ADR-001-plugin-runtime-architecture.md`, `docs/adr/ADR-003-unified-plugin-registry.md`, `docs/adr/ADR-004-converter-ir-and-unsupported-pattern-policy.md` — architecture constraints.

## Next steps
1. Inspect current repo status and the roadmap/plans before editing; do not assume the prompt snapshot is fully current.
2. Finish remaining M3 hook parity: tool invocation plus DB logging integration for plugin tools.
3. Start M4 canaries after M3: hand-convert `llm-cmd`, `llm-openrouter`, and `llm-gemini`.
4. Prepare Phase 2 converter extractor/scaffolder commands against the ADR-004 IR contract.

## Validation
- Current turn validation:
  - `cargo test -p llm-embeddings`
  - `cargo test -p llm-plugin-host hook_registrars_cover_full_plugin_lifecycle`
  - `cargo test -p llm-plugin-markov`
  - `cargo test -p llm-cli embed_accepts_plugin_embedding_model_without_api_key`
- Useful baseline commands from repo root:
  - `cargo build`
  - `cargo test`
  - `cargo run -- --help`
  - `LLM_PROMPT_STUB=1 cargo run -- "hello"`
  - `LLM_PROMPT_STUB=1 cargo run -- --no-stream "hello"`
  - `cargo run -- --model markov --no-stream "the quick brown fox jumps over the lazy dog"`
  - `cargo run -- embed --model markov-embed --json "the quick brown fox"`
  - `cargo run -- plugins --json`

## Risks / open questions
- Prompt-provided status may be slightly stale; verify against the checkout before planning implementation.
- Remaining plugin hook parity is tool invocation and DB logging integration; this likely touches cross-crate contracts.
- DB logging and tool invocation changes need focused tests because they affect runtime behavior and persisted log shape.

## Resume prompt
- Pick up in `/home/shuv/repos/llm-rust`, read `AGENTS.md` plus the two `PLAN-*.md` files, verify current `git status`, then continue remaining M3 tool invocation/logging parity with validation.
