# Rust Rewrite TODO

This document tracks the outstanding work needed to bring the Rust rewrite to feature parity with the Python `llm` CLI and supporting libraries.

## CLI Parity
- [ ] Expand `prompt` to support system prompts, templates, fragments, attachments, tool execution, structured extraction, async runs, conversation continuation, explicit key overrides, usage reporting, and log suppression/force semantics matching Python.
  - [x] Accept `--system` flag for system prompts when invoking `llm` or `llm prompt`.
  - [x] Support explicit `--key` overrides for prompt execution (inline or alias).
  - [x] Allow associating prompts with existing conversations via `--conversation/--conversation-name/--conversation-model`.
- [ ] Implement `chat` with interactive session UI, conversation history management, tool execution, and streaming controls.
- [ ] Close `cmd` gaps: align approval workflow, multi-line editing UX, logging toggles, shell safety prompts, and plugin hook integration.
- [ ] Port `aliases` (list/set/remove/path, query helpers, storage format).
- [ ] Port collection and embeddings commands: `collections`, `embed`, `embed-models`, `embed-multi`, `similar`, ensuring SQLite schema compatibility and binary payload handling.
- [ ] Port prompt library helpers: `fragments`, `templates`, `schemas` (including hidden `--path` overrides and DSL tooling).
- [ ] Implement `tools list` with `--json/--functions` and integrate with native/plugin-provided tools.
- [ ] Finish `keys` parity by adding alias query flags, secure input UX, masking, and legacy JSON output behavior.
- [ ] Fill out `logs list` parity: support export flags (`-l`, `-t`, `-s`, `-u`, `-r`, `-x`, `--xl`, etc.), tool/schema filters, rich response metadata, and conversation summaries.
- [ ] Extend `models` with the `options` subcommand tree, per-provider refresh, async/schema/tool flags, and catalog metadata parity.
- [ ] Enhance `plugins` command with `--all`, `--hook`, and detailed capability reporting sourced from the plugin bridge.
- [ ] Restore package management wrappers: `install`, `uninstall`, and plugin-provided commands such as `jq` once the bridge exists.
- [ ] Wire up plugin CLIs (`anyscale-endpoints`, `gemini`, `grok`, `mistral`, `openai`, `openrouter`, etc.) through the Python bridge or native equivalents.

## Provider, Logging & Telemetry
- [ ] Capture provider usage metadata (tokens, costs, tool calls) during streaming and non-streaming flows and persist to `logs.db`.
- [ ] Support conversation persistence (IDs, names, message history) throughout the core library and CLI commands.
- [ ] Implement tool execution sandboxing, approvals, and schema validation consistent with Python.
- [ ] Add configurable retries/backoff per provider with environment overrides and telemetry hooks.
- [ ] Introduce cancellation, timeout, and resource cleanup paths for long-running requests.

## Plugin Ecosystem
- [ ] Implement the Python plugin bridge with `pyo3`, including environment management, pluggy hook compatibility, and manifest discovery.
- [ ] Provide native Rust plugin loader APIs and registration interfaces mirroring Python’s lifecycle hooks.
- [ ] Deliver translation tooling (`llm migrate-plugin`) to scaffold Rust adapters from Python metadata/AST and track migration status.
- [ ] Build automated tests that exercise real plugins through the bridge (smoke tests plus golden outputs).
- [ ] Document plugin authoring, migration strategy, signing, and compatibility guarantees.

## Embeddings & Data Stores
- [ ] Port embeddings database schema, migrations, and query helpers.
- [ ] Reimplement similarity search, collection management, and multi-file ingestion workflows.
- [ ] Ensure interoperability with existing Python-created databases (indexes, metadata, binary columns).

## Testing & Validation
- [ ] Derive automated parity tests from the updated CLI matrix (command-level integration, golden outputs, failure cases).
- [ ] Add streaming-specific tests (chunk timing, SSE/WebSocket mocks).
- [ ] Establish regression tests for logging, embeddings, tools, and plugin workflows.
- [ ] Build performance baselines (startup, prompt latency, SQLite operations) and monitor regressions.
- [ ] Stand up CI covering native builds, plugin bridge (Python) scenarios, formatting, linting, and security checks.

## Packaging & Rollout
- [ ] Decide binary packaging/distribution strategy (`cargo dist`, installers) plus pip/pyproject wrappers.
- [ ] Provide a Python wrapper (`import llm`) that delegates to the Rust core while preserving legacy signatures.
- [ ] Prepare migration guides, release checklist, and communication plan for staged rollout (alpha → GA).
- [ ] Define governance for issue triage, plugin review, and long-term maintenance once parity is achieved.
