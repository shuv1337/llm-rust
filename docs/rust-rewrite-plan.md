# Rust Rewrite TODO

This document tracks the outstanding work needed to bring the Rust rewrite to feature parity with the Python `llm` CLI and supporting libraries.

Upstream baseline: `simonw/llm@6b84a0d36b0df1341a9b64ef7001d56eee5e9185`

Primary roadmap: `PLAN-llm-upstream-feature-parity-roadmap.md`
CLI command/status matrix: `docs/cli-parity-matrix.md`

## Cross-cutting Decisions (M0 blockers)
- [ ] Confirm ULID strategy for response/conversation IDs and document format + ordering assumptions.
- [ ] Confirm migration metadata table compatibility with upstream (`_llm_migrations`, `name`, `applied_at`).
- [ ] Confirm migration preflight/dry-run behavior (audit pending migrations + backup target before writes).
- [ ] Confirm plugin runtime architecture (dynamic command registry + model provider registry).
- [ ] Confirm continuation flag migration strategy:
  - `-c/--continue` (boolean) for latest conversation
  - `--cid/--conversation <id>` for explicit conversation
  - one-release legacy argv rewrite: `-c <id>` → `--cid <id>` with warning
- [ ] Confirm binary naming strategy for parity release (`llm` vs `llm-cli` alias/wrapper behavior).
- [ ] Confirm parity release scope: upstream core + default plugins are blocking; third-party plugin smoke remains non-blocking quality signal.
- [ ] Publish ADRs referenced by roadmap:
  - ADR-001 plugin runtime dispatch architecture
  - ADR-002 deterministic int-ID → ULID migration algorithm

## CLI Parity
- [ ] Expand `prompt` to support templates, fragments, attachments, tool execution, structured extraction, async runs, usage reporting, and continuation semantics matching Python.
  - [x] Accept `--system` flag for system prompts when invoking `llm` or `llm prompt`.
  - [x] Support explicit `--key` overrides for prompt execution (inline or alias).
  - [x] Support attachments via `-a/--attachment` and `--attachment-type`.
  - [x] Allow associating prompts with existing conversations via current `--conversation/--conversation-name/--conversation-model` flags.
- [ ] Align high-risk `prompt` option semantics with upstream:
  - [ ] `--save` saves templates (not response logs)
  - [ ] `--database` selects logs DB path
  - [ ] `--query` performs model lookup for prompt
  - [ ] `--async` uses async-model execution semantics
- [ ] Align continuation flags with upstream and ship/remove one-release compatibility shim.
- [ ] Implement `chat` with interactive session UI, conversation history management, tool execution, and streaming controls.
- [ ] Close `cmd` gaps: approval workflow, multi-line editing UX, logging toggles, shell safety prompts, and plugin hook integration.
- [ ] Port `aliases` (list/set/remove/path, query helpers, storage format) and integrate aliases into model resolution flows.
- [ ] Port collection and embeddings commands: `collections`, `embed`, `embed-models`, `embed-multi`, `similar`.
- [ ] Port prompt helpers: fragments (prompt/chat options), `templates`, `schemas` (including hidden `--path` overrides and DSL tooling).
- [ ] Implement `tools list` with `--json/--functions` and integrate native/plugin-provided tools.
- [ ] Finish `keys` parity: alias query flags, secure input UX, masking, legacy JSON output behavior.
- [ ] Fill out `logs list` parity: export flags (`-l`, `-t`, `-s`, `-u`, `-r`, `-x`, `--xl`, etc.), tool/schema/fragment filters, rich response metadata, conversation summaries.
- [ ] Extend `models` with `options` tree, per-provider refresh, async/schema/tool flags, and catalog metadata parity.
- [ ] Enhance `plugins` command with `--all`, `--hook`, detailed capability reporting.
- [ ] Restore package wrappers: `install`, `uninstall`, and plugin-provided commands such as `jq` once bridge exists.
- [ ] Wire plugin CLIs (`anyscale-endpoints`, `gemini`, `grok`, `mistral`, `openai`, `openrouter`, etc.) through bridge/native equivalents.

## Storage, Migrations & Compatibility
- [ ] Implement Rust-native migration engine with deterministic ordered migrations.
- [ ] Track migration state using upstream-compatible `_llm_migrations` table.
- [ ] Implement backup-first behavior before first schema-changing migration on user DBs.
- [ ] Implement migration preflight/dry-run mode (pending migration report + compatibility warnings + backup location preview).
- [ ] Port logs schema to upstream-compatible structure, including tools/schemas/attachments/fragment-link tables.
- [ ] Port FTS/triggers behavior for upstream-compatible `logs list -q` behavior.
- [ ] Migrate IDs to upstream-compatible ULID-style string IDs end-to-end:
  - [ ] DB schema (`responses.id`, foreign keys)
  - [ ] deterministic conversion path for legacy integer IDs
  - [ ] CLI/API types for `--id-gt/--id-gte`
  - [ ] ordering/filter semantics + compatibility tests
- [ ] Support `default_model.txt` with backward-compatible read fallback for legacy `default-model.txt`.
- [ ] Add `model_options.json` persistence + merge precedence compatible with upstream.

## Provider, Logging & Telemetry
- [ ] Capture provider usage metadata (tokens, costs, tool calls) during streaming/non-streaming and persist to `logs.db`.
- [ ] Support conversation persistence (IDs, names, message history) throughout core + CLI.
- [ ] Implement tool execution sandboxing, approvals, and schema validation consistent with Python.
- [ ] Add configurable retries/backoff per provider with env overrides and telemetry hooks.
- [ ] Introduce cancellation, timeout, and cleanup paths for long-running requests.
- [ ] Harden async execution after semantic parity lands (timeouts, cancellation, retries, telemetry consistency).

## Plugin Ecosystem & Runtime Architecture
- [ ] Implement Python plugin bridge with `pyo3`, environment management, pluggy hook compatibility, and entrypoint discovery.
- [ ] Implement dynamic CLI command registry so plugin commands can be registered/executed at runtime.
- [ ] Implement command collision/precedence rules (core commands win, deterministic warnings/errors for conflicts).
- [ ] Implement model provider registry in `llm-core` for plugin model dispatch (not only hardcoded provider matches).
- [ ] Provide native Rust plugin loader APIs mirroring Python lifecycle hooks.
- [ ] Build automated tests that exercise default plugins through bridge and parity wrappers.
- [ ] Keep third-party plugin smoke test in CI as non-blocking/reporting quality gate.
- [ ] Document plugin authoring, migration strategy, signing, and compatibility guarantees.

## Embeddings & Data Stores
- [ ] Port embeddings database schema, migrations, and query helpers.
- [ ] Add embeddings provider abstraction in core (request/response/provider trait + key/env/retry wiring).
- [ ] Implement baseline built-in embeddings providers/models required for parity.
- [ ] Reimplement similarity search, collection management, and multi-file ingestion workflows.
- [ ] Ensure interoperability with existing Python-created embedding databases.

## Testing & Validation
- [ ] Derive automated parity tests from the CLI matrix (command-level integration, golden outputs, failure cases).
- [ ] Add compatibility fixture suites using real upstream-created databases (`logs.db`, `embeddings.db`) for read/write/migration round-trips.
- [ ] Add streaming-specific tests (chunk timing, SSE/WebSocket mocks).
- [ ] Establish regression tests for logging, embeddings, tools, plugins, and continuation flag migration behavior.
- [ ] Add migration tests for dry-run/preflight and deterministic int-ID → ULID conversion ordering.
- [ ] Build performance baselines (startup, prompt latency, SQLite operations) and monitor regressions.
- [ ] Stand up CI covering native builds, plugin bridge scenarios, compatibility checks, and third-party plugin smoke reporting.

## Packaging & Rollout
- [ ] Decide binary packaging/distribution strategy (`cargo dist`, installers) plus pip/pyproject wrappers.
- [ ] Implement/document binary naming decision (`llm` vs `llm-cli`) for user-facing parity release.
- [ ] Provide Python wrapper (`import llm`) delegating to Rust core while preserving legacy signatures where required.
- [ ] Prepare migration guides, release checklist, and staged rollout plan (alpha → GA).
- [ ] Define governance for issue triage, plugin review, and long-term maintenance after parity.
