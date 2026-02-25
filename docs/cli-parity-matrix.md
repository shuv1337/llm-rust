# CLI Parity Matrix

Upstream baseline: `simonw/llm@6b84a0d36b0df1341a9b64ef7001d56eee5e9185`  
Upstream target commit locked: **2026-02-24**

Status legend: `[ ]` not started, `[~]` in progress, `[x]` complete.

## M0 Decisions (Locked)

### Parity Policy Alignment
- [x] Release blocker scope: upstream **core + default plugins** parity.
- [x] Third-party plugin smoke tests run in CI as **non-blocking/reporting** quality gates.
- [x] Migration metadata compatibility target: upstream `_llm_migrations` table (`name`, `applied_at`).
- [x] Response/log ID compatibility target: ULID-style string IDs (see [ADR-002](adr/ADR-002-id-migration-strategy.md)).
- [x] Dynamic runtime architecture for plugin commands/models documented (see [ADR-001](adr/ADR-001-plugin-runtime-architecture.md)).

### Binary Naming Decision
- [x] **Decision:** Ship as `llm-cli` with optional `llm` symlink/alias.
  - Primary binary name: `llm-cli`
  - Installation scripts create `llm` symlink when Python `llm` is not detected
  - When Python `llm` is present, users explicitly choose via `llm-cli` or by removing Python version
  - Rationale: Avoids PATH conflicts during migration; allows side-by-side testing
  - Future: After parity release is stable, consider shipping `llm` directly with Python version detection

### Continuation Flag Policy
- [x] **Decision:** Align with upstream flag semantics (see [rust-rewrite-plan.md](rust-rewrite-plan.md#continuation-flag-policy)).
  - `-c/--continue` = continue most recent conversation (boolean, no argument)
  - `--cid/--conversation <id>` = continue specific conversation by ID
  - One-release compatibility shim: detect legacy `-c <id>` usage, rewrite to `--cid <id>`, emit deprecation warning
  - Remove shim in following release

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
- **Note:** Fragment support (`--fragment/-f`, `--system-fragment/--sf`) is part of `prompt` options, not a standalone command.

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

## Rust-Only Extensions (Intentional Non-Parity)

These commands/options exist in Rust but not upstream. Retained as documented extensions:

| Extension | Description |
|-----------|-------------|
| `cmd` | Interactive command generation/execution |
| `version` | Version information subcommand |
| `keys resolve` | Key resolution debugging |
| `--retries` | Prompt retry count |
| `--retry-backoff-ms` | Retry backoff duration |
| `--debug` | Debug logging |
| `--info` | Info logging |

## Notes

- Upstream does **not** have a standalone `fragments` command; fragment support is part of `prompt`/`chat` options via `--fragment/-f` and `--system-fragment/--sf`.
- Keep this matrix synchronized with the roadmap milestones and flag-semantics contract.
- Parity diff script: `scripts/parity-diff.sh`
