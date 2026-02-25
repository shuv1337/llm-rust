# CLI Parity Matrix (Draft)

Upstream baseline: `simonw/llm@6b84a0d36b0df1341a9b64ef7001d56eee5e9185`

Status legend: `[ ]` not started, `[~]` in progress, `[x]` complete.

## Parity Policy Alignment (M0)
- [ ] Release blocker scope: upstream **core + default plugins** parity.
- [ ] Third-party plugin smoke tests run in CI as **non-blocking/reporting** quality gates.
- [ ] Migration metadata compatibility target: upstream `_llm_migrations` table (`name`, `applied_at`).
- [ ] Response/log ID compatibility target: ULID-style string IDs (current Rust logs still integer IDs).
- [ ] Binary naming decision documented (`llm` vs `llm-cli` alias/wrapper behavior).
- [ ] Dynamic runtime architecture for plugin commands/models documented (registry-based dispatch).

## Global Options
- [~] `--version`, `-h/--help` – confirm behavior and output parity.
- [~] Rust-only extensions retained as intentional non-parity: `--info`, `--debug`.

## Command Reference

### `prompt`
- Status `[~]` – minimal execution path works, but major parity and semantic gaps remain.
- Implemented options: `--model`, `--temperature`, `--max-tokens`, `--retries`, `--retry-backoff-ms`, `--no-stream`, `--log`, `--no-log`, `--system`, `--key`, `-a/--attachment`, `--attachment-type`, `--conversation`, `--conversation-name`, `--conversation-model`.
- Outstanding feature parity: templates, fragments, tool execution controls, structured extraction/schema, async/usage, model query lookup, and full continuation semantics.
- **Semantic deltas to fix first:**
  - `-c/--continue` vs `--cid/--conversation` behavior (current Rust `-c` usage diverges)
  - `--save` should save templates (not response logs)
  - `--database` should select logs DB path
  - `--query` should perform model discovery for prompt command
  - `--async` should follow async-model execution semantics (not background-job semantics)

### `chat`
- Status `[ ]`.
- Not yet implemented; requires interactive loop, conversation continuation/history inflation, and tool integration.

### `cmd` (Rust extension)
- Status `[~]`.
- Implemented options: prompt options above plus `--system`, `--key`, `--conversation`, `--conversation-name`, `--conversation-model`.
- Outstanding: approval/auto-approve parity expectations, logging UX polish, and plugin/tool integration fidelity.

### `aliases`
- Status `[ ]`.
- Subcommands and storage layer not yet ported.
- Outstanding: integrate `aliases.json` into model resolution flows (`prompt`, `models default`, `logs list --model`).

### `collections`
- Status `[ ]`.
- Embeddings database management commands pending.

### `embed`
- Status `[ ]`.
- Embeddings generation/store/retrieval pipeline not yet implemented.

### `embed-models`
- Status `[ ]`.
- Needs model catalogue, default management, parity flags, and registry integration (native first, plugin-extendable).

### `embed-multi`
- Status `[ ]`.
- Bulk ingestion workflows (files/SQL/etc.) not yet available.

### `templates`
- Status `[ ]`.
- Template commands (list/show/edit/path/loaders) not yet ported.

### `schemas`
- Status `[ ]`.
- Schema registry, DSL tooling, and hidden path overrides outstanding.

### `similar`
- Status `[ ]`.
- Cosine similarity search and related options not yet exposed.

### `tools`
- Status `[ ]`.
- Tool listing and function export commands pending native/plugin integration.

### `keys`
- Status `[~]`.
- Implemented subcommands: `list`, `get`, `set`, `path`, `resolve`.
- Outstanding: alias query helpers (`set -q`), secure input UX parity, masking options, and extended JSON output behavior.

### `logs`
- Status `[~]`.
- Implemented subcommands: `list`, `backup`, `on`, `off`, `status`, `path`.
- Implemented filters: `--count/--json/--model/--query/--conversation/--id-gt/--id-gte/--since/--before/--database` (plus hidden `--path`).
- Outstanding: export/extract option parity (`--latest`, `--current`, `--response`, `--data*`, `--short`, `--truncate`, etc.), tool/schema/fragment filters, and usage/token metadata parity.
- **Schema mismatch note:** current Rust IDs are integer-based; upstream is ULID-style string IDs.
- **Migration note:** migration metadata must align with upstream `_llm_migrations`.

### `models`
- Status `[~]`.
- Implemented subcommands: `list` (with `--json`), `default` (get/set).
- Outstanding: `models options` tree, per-provider refresh, async/schema/tool filters, richer catalog metadata, and alias-aware resolution parity.

### `plugins`
- Status `[~]`.
- Implemented options: `--json` (stub plugin host).
- Outstanding: `--all`, `--hook`, plugin capability summaries, Python bridge integration, and dynamic runtime registration plumbing for plugin commands/models.

### `install`
- Status `[ ]`.
- Pip-wrapper command not yet ported (plugin-bridge milestone).

### `uninstall`
- Status `[ ]`.
- Removal workflow pending (plugin-bridge milestone).

### `jq`
- Status `[ ]`.
- Plugin-provided command absent without Python bridge.

### Plugin-Provided Commands
- Status `[ ]`.
- Requires Python plugin bridge and parity wrappers for `anyscale-endpoints`, `gemini`, `grok`, `mistral`, `openai`, `openrouter`, `cmd`, `jq`, etc.

## Notes
- Upstream does **not** have a standalone `fragments` command; fragment support is part of `prompt`/`chat` options.
- Keep this matrix synchronized with the roadmap milestones and flag-semantics contract.
