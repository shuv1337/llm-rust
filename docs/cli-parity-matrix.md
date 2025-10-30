# CLI Parity Matrix (Draft)

Status legend: `[ ]` not started, `[~]` in progress, `[x]` complete.

## Global Options
- [~] `--version`, `-h/--help` – confirm behavior and output parity.

## Command Reference

### `prompt`
- Status `[~]` – minimal execution path is working but parity gaps remain.
- Implemented options: `--model`, `--temperature`, `--max-tokens`, `--retries`, `--retry-backoff-ms`, `--no-stream`, `--log`, `--no-log`, `--system`, `--key`, `--conversation`, `--conversation-name`, `--conversation-model`.
- Outstanding: fragment/template handling, attachments, database/template lookups, tool execution controls, structured extraction, async/save/usage reporting, conversation continuation and logging toggles that mirror Python semantics.

### `chat`
- Status `[ ]`.
- Not yet implemented; requires interactive session UI, conversation continuation, and tool integration.

### `cmd`
- Status `[~]`.
- Implemented options: prompt options above plus `--system`, `--key`, `--conversation`, `--conversation-name`, `--conversation-model`.
- Outstanding: approval/auto-approve parity, logging toggles beyond `--log/--no-log`, multi-command/tool integrations, plugin hook fidelity, and post-processing UX matching Python.

### `aliases`
- Status `[ ]`.
- Subcommands and storage layer not yet ported.

### `collections`
- Status `[ ]`.
- Embeddings database management commands pending.

### `embed`
- Status `[ ]`.
- Embeddings generation/store/retrieval pipeline not yet implemented.

### `embed-models`
- Status `[ ]`.
- Needs model catalogue, default management, and parity flags.

### `embed-multi`
- Status `[ ]`.
- Bulk ingestion workflows (files/SQL/etc.) not yet available.

### `fragments`
- Status `[ ]`.
- Fragment CRUD and loader discovery pending.

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
- Outstanding: alias query helpers (`set -q`), secure input UX parity, masking options, and any extended JSON output semantics.

### `logs`
- Status `[~]`.
- Implemented subcommands: `list`, `backup`, `on`, `off`, `status`, `path`.
- Implemented filters: `--count/--json/--model/--query/--conversation/--id-gt/--id-gte/--since/--before/--database` (plus hidden `--path`).
- Outstanding: tool/schema extraction flags, usage/token metadata population, export helpers, conversation summaries, and parity output formatting.

### `models`
- Status `[~]`.
- Implemented subcommands: `list` (with `--json`), `default` (get/set).
- Outstanding: `models options` tree, per-provider refresh, async/schema/tool flags, and richer catalog metadata.

### `plugins`
- Status `[~]`.
- Implemented options: `--json`.
- Outstanding: `--all`, `--hook`, plugin capability summaries, and integration with Python bridge once available.

### `install`
- Status `[ ]`.
- Pip-wrapper command not yet ported.

### `uninstall`
- Status `[ ]`.
- Removal workflow pending.

### `jq`
- Status `[ ]`.
- Plugin-provided command absent without Python bridge.

### Plugin-Provided Commands
- Status `[ ]`.
- Requires Python plugin bridge and parity wrappers for `anyscale-endpoints`, `gemini`, `grok`, `mistral`, `openai`, `openrouter`, `cmd`, `jq`, etc.

## Notes
- Populate behavioral notes, environment variables, and test references per command.
- Identify hidden commands/flags by inspecting source (`llm/cli.py`) once parity matrix skeleton is ready.
