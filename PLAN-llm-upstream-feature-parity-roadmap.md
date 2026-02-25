# PLAN-llm-upstream-feature-parity-roadmap.md

## Scope and review baseline

- **Goal:** reach practical feature parity with upstream `simonw/llm`.
- **Upstream baseline reviewed:** `simonw/llm` @ `6b84a0d36b0df1341a9b64ef7001d56eee5e9185`
- **Current Rust baseline reviewed:** `llm-rust` @ `ce44d15563445b9a32fba98e374f3fe7bd7ffd75`

## Inputs reviewed

### Internal code (Rust rewrite)
- `crates/llm-cli/src/main.rs`
- `crates/llm-cli/tests/cli.rs` (33 integration tests, all passing)
- `crates/llm-core/src/lib.rs`
- `crates/llm-core/src/logs.rs`
- `crates/llm-core/src/providers/mod.rs`
- `crates/llm-core/src/providers/openai.rs`
- `crates/llm-core/src/providers/anthropic.rs`
- `crates/llm-core/src/attachments.rs`
- `crates/llm-plugin-host/src/lib.rs` (stub only — returns single placeholder)
- `crates/llm-embeddings/src/lib.rs` (stub only — returns 0)
- `docs/cli-parity-matrix.md`
- `docs/rust-rewrite-plan.md`

### External upstream references
- CLI surface: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/cli.py
- Plugin system: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/plugins.py
- Hook specs: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/hookspecs.py
- Core models/responses/tools: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/models.py
- Log DB migrations: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/migrations.py
- Embeddings API: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/embeddings.py
- Embeddings migrations: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/embeddings_migrations.py
- Default OpenAI models plugin: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/default_plugins/openai_models.py
- Default tools plugin: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/default_plugins/default_tools.py

## Central task log (execution tracking)

Execution mode: multi-agent crew with subagent workers coordinated via `pi_messenger`.

**Overall status:** ✅ 20/20 crew tasks completed  
**Validation status:** ✅ `cargo test --workspace` passing after post-merge stabilization fixes

| Task | Status | Assignee | Commit |
|---|---|---|---|
| task-1 Lock M0 Decisions and Create ADRs | ✅ Done | GoldIce | `24f454d` |
| task-2 Config File Compatibility Layer | ✅ Done | ZenYak | `44cc869` |
| task-3 Migration Engine Scaffold | ✅ Done | UltraStorm | `0f66dd5` |
| task-4 Logs DB Schema Migration | ✅ Done | Crew worker | `bc2e77b` |
| task-5 String ID Migration (int→ULID) | ✅ Done | Crew worker | `707772f` |
| task-6 Provider Data Model Refactor | ✅ Done | CalmCastle | `083606f` |
| task-7 OpenAI Tool/Schema Support | ✅ Done | Crew worker | `968609a` |
| task-8 Anthropic Tool/Schema Support | ✅ Done | Crew worker | `c1717c2` |
| task-9 Conversation Continuation Infrastructure | ✅ Done | Crew worker | `7b19912` |
| task-10 Continuation Flag Migration Shim | ✅ Done | Crew worker | `a8e0ae6` |
| task-11 Prompt Option Parity (core semantics) | ✅ Done | Crew worker | `f6cb291` |
| task-12 Chat Command Implementation | ✅ Done | Crew worker | `e345e12` |
| task-13 Aliases Command Group | ✅ Done | Crew worker | `d787d70` |
| task-14 Templates Command Group | ✅ Done | Crew worker | `89d8330` |
| task-15 Logs List Extended Options | ✅ Done | TrueBear | `01dcac3` |
| task-16 Models Command Extended Options | ✅ Done | ZenMoon | `a784bab` |
| task-17 Schemas and Tools Command Groups | ✅ Done | EpicQuartz | `e382f21` |
| task-18 Embeddings Provider Abstraction | ✅ Done | SwiftDragon | `c1e0120` |
| task-19 Embeddings CLI Commands | ✅ Done | YoungYak | `de1b651` |
| task-20 Compatibility Test Suite | ✅ Done | NiceUnion | `766b6b2` |

Post-merge stabilization commit(s):
- `HEAD` (current working commit after crew waves): test-lock harmonization + template save path hardening to resolve parallel test flakiness.

---

## Parity snapshot (current)

### Top-level command parity

- Upstream commands: `prompt`, `aliases`, `chat`, `collections`, `embed`, `embed-models`, `embed-multi`, `install`, `keys`, `logs`, `models`, `openai`, `plugins`, `schemas`, `similar`, `templates`, `tools`, `uninstall`
- Rust commands today: `prompt`, `plugins`, `models`, `keys`, `logs`, `cmd`, `version`
- **Overlap:** 5/18 upstream commands (plus Rust-only `cmd`/`version`)
- **Note:** upstream `fragments` is not a standalone command — fragment support lives inside the `prompt` command options (`--fragment/-f`, `--system-fragment/--sf`). A `fragments` command group does not exist upstream.

### Major parity gaps identified

1. **Large CLI surface missing** — `chat`, `aliases`, `templates`, `schemas`, `tools`, embeddings family (`embed`, `embed-models`, `embed-multi`, `similar`, `collections`), `install`/`uninstall`, `openai` command group.
2. **Prompt feature + semantics gap** — tools/functions, schema extraction, templates/fragments, async, save, usage/extract controls, model-query selection, and conversation continuation are incomplete; additionally several option semantics differ from upstream (`--save` template save behavior, `--database` logs DB path, `--query` model lookup, `--async` async-model execution).
3. **Core data model gap** — `PromptRequest` (in `providers/mod.rs`) lacks fields for tools, functions, schemas, response_format. This blocks all tool/schema work.
4. **Conversation continuation gap** — code writes conversation rows to DB but has zero infrastructure to load previous messages back for `--continue`/`--cid` continuation.
5. **Plugin architecture gap** — `llm-plugin-host` is stub-only (returns single placeholder).
6. **Storage compatibility gap**:
   - Rust logs DB schema is simplified (2 tables, ~14 columns) vs upstream (~15+ tables).
   - Rust default model path uses `default-model.txt`; upstream uses `default_model.txt`.
   - No aliases/model-options file support yet.
   - No migration engine.
7. **Embeddings stack missing** — `llm-embeddings` crate returns `0`.
8. **Model options + tool calling + schema support** — not present in provider/runtime flow.

### Current test baseline

- 46 tests total: 33 integration (`llm-cli`), 13 unit (`llm-core`), all passing.
- Tests use stub mode (`LLM_PROMPT_STUB=1`) and temp `LLM_USER_PATH` sandboxes.

### Rust-only extensions (intentional non-parity)

These exist in Rust but not upstream. They will be retained behind clear compatibility docs:
- `cmd` subcommand (interactive command generation/execution)
- `version` subcommand
- `--retries` / `--retry-backoff-ms` prompt options
- `--debug` / `--info` global logging flags
- `keys resolve` subcommand

---

## Roadmap strategy

Use a two-lane strategy:

- **Lane A (Parity-first):** match upstream CLI behavior and storage contracts quickly.
- **Lane B (Rust-native):** keep provider abstractions performant/idiomatic, but only after behavior is green against parity tests.

This avoids shipping a fast but incompatible CLI.

### Effort sizing legend

- **S** = ≤2 hours
- **M** = half-day to 1 day
- **L** = 2–4 days
- **XL** = 1–2 weeks

---

## M0 — Lock decisions and baseline (Week 1)

### Objectives
- Resolve cross-cutting architectural decisions that gate all subsequent milestones.
- Establish a lightweight parity baseline (not a full harness).

### Prerequisite decisions (must resolve before M1)

- [ ] **ID strategy:** adopt upstream ULID-style string IDs for conversations/responses now, not later. Document the format in this roadmap + `docs/rust-rewrite-plan.md`. **[S]**
- [ ] **Plugin strategy:** confirm short-term pyo3 bridge approach. Document that native Rust plugin ABI is post-parity. Note: `openai` command group and tool-providing plugins are bridge-dependent and cannot ship natively in M3. **[S]**
- [ ] **Plugin runtime architecture strategy:** define how plugin-provided commands/models are dispatched in Rust (dynamic command registry + model provider registry), since current CLI/provider paths are static. Capture in ADR before M5 implementation starts. **[M]**
- [ ] **Parity scope:** release blocker is upstream core + default plugins parity. Third-party plugin smoke coverage is tracked as a non-blocking quality gate. **[S]**
- [ ] **Command naming policy:** retain Rust extras (`cmd`, `version`, `--retries`, `--retry-backoff-ms`, `--debug`, `--info`) as documented non-parity extensions. **[S]**
- [ ] **Binary naming policy:** decide whether release ships `llm` binary name directly (or `llm` alias/wrapper over `llm-cli`) and document migration/install behavior. Include this in release scope, not as follow-up. **[S]**
- [ ] **Prompt flag compatibility policy:** align with upstream by using `-c/--continue` for continuation and `--cid/--conversation` for explicit conversation ID; implement a one-release shim that rewrites legacy `-c <conversation-id>` argv usage to `--cid <id>` with deprecation warning before Clap parsing, then remove next release. **[M]**
- [ ] **Migration engine strategy:** use Rust-native SQLite migrations (not Python's sqlite-utils). Track applied migrations in upstream-compatible `_llm_migrations` table, codify upstream's final schema state, and migrate toward it directly. Document in this roadmap + `docs/rust-rewrite-plan.md`. **[M]**

### Baseline tasks

- [ ] Record upstream target commit/tag in `docs/cli-parity-matrix.md` header. **[S]**
- [ ] Add a prompt/logs/models option-by-option parity contract table with exact upstream semantics + source line references in `docs/cli-parity-matrix.md`. **[M]**
- [ ] Correct `docs/cli-parity-matrix.md` command surface to match upstream (remove standalone `fragments` command group, keep fragment support under `prompt` flags). **[S]**
- [ ] Add a lightweight shell script (`scripts/parity-diff.sh`) that compares `--help` output between upstream and Rust binaries — not a full harness, just a quick diff. **[M]**
- [ ] Create 3–5 golden fixtures for critical command behaviors (prompt, logs list, models list) in temp sandboxes. **[M]**
- [ ] Define "parity done" rubric (commands, options, storage compat, core tests) in this roadmap + `docs/rust-rewrite-plan.md`. **[S]**
- [ ] Add ADR-001: plugin command/model runtime architecture (dynamic CLI command registration + provider routing). **[S]**
- [ ] Add ADR-002: deterministic integer-ID to ULID migration algorithm + ordering guarantees. **[S]**
- [ ] Add binary naming compatibility note (`llm` vs `llm-cli`) to parity docs + release checklist. **[S]**

### Internal references
- `docs/cli-parity-matrix.md`
- `crates/llm-cli/tests/cli.rs`

### Exit criteria
- [ ] All 8 prerequisite decisions documented and committed.
- [ ] Flag-semantics parity contract doc committed with upstream source references.
- [ ] Parity diff script runs and produces readable output.
- [ ] ADR-001/ADR-002 committed and linked from `docs/rust-rewrite-plan.md`.
- [ ] Binary naming scope (`llm` vs `llm-cli`) explicitly decided and documented.
- [ ] Parity rubric committed.

---

## M1 — Storage and config compatibility foundation (Weeks 2–3)

### Objectives
- Make Rust read/write user state in formats upstream understands.
- This milestone is the foundation for everything else — get it right.

### Tasks

#### Config file compatibility
- [ ] **Default model filename:** support upstream `default_model.txt` path. Add backward-compat read fallback for existing Rust `default-model.txt` and optional one-time rename-on-write behavior. Resolve in `default_model_path()` at `crates/llm-core/src/lib.rs`. **[S]**
- [ ] **Aliases file + resolution:** add `aliases.json` read/write support and API helpers. Integrate aliases into model resolution paths (`normalize_model_name()`, `resolve_model_name()`, `models default`, `logs list --model`). **[M]**
- [ ] **Model options file:** add `model_options.json` read/set/clear per model with merge precedence matching upstream (CLI option overrides stored defaults). **[M]**

#### Logs DB schema upgrade
- [ ] Implement **backup-first migration behavior** (create timestamped backup of `logs.db` before first schema-changing migration). **[M]**
- [ ] Implement Rust-native migration engine (apply numbered SQL migrations, track version in upstream-compatible `_llm_migrations` table with `name` + `applied_at`). **[L]**
- [ ] Add migration preflight/dry-run audit mode (report pending migrations, compatibility warnings, and backup target before any schema write). **[M]**
- [ ] Write migrations to evolve from current 2-table schema to upstream-compatible schema:
  - `conversations` (add missing columns)
  - `responses` (add missing columns, including upstream-compatible string ID PK)
  - New tables: `schemas`, `attachments`, `prompt_attachments`, `tools`, `tool_responses`, `tool_calls`, `tool_results`, `tool_instances`, `tool_results_attachments`, `fragments`, `fragment_aliases`, `prompt_fragments`, `system_fragments` **[XL]**
- [ ] Add FTS parity migrations/triggers for responses search behavior (compatible with upstream `logs list -q` ranking and filtering semantics). **[M]**
- [ ] Switch response IDs / conversation IDs to ULID-style string IDs end-to-end (schema + runtime + CLI parsing). **[L]**
  - Convert `responses.id` from integer autoincrement to text ID.
  - Use a deterministic conversion algorithm for legacy integer IDs: iterate legacy rows in ascending `(datetime_utc, id)` order and assign monotonic ULIDs so lexical order preserves chronological/logical ordering.
  - Keep a temporary migration map (`legacy_integer_id -> new_ulid`) during migration and update all foreign keys/joins via that map.
  - Migrate `logs list --id-gt/--id-gte` filters and internal types to string IDs.
  - Add compatibility tests that assert old integer ordering/filter behavior is preserved after migration.
- [ ] Update `LogRecord` and `LogEntry` structs to match new schema columns and ID types. **[M]**
- [ ] Update `list_logs()` query to join new tables where relevant and to use string-ID filters. **[M]**

### Internal references
- `crates/llm-core/src/lib.rs` — `default_model_path()`, `normalize_model_name()`
- `crates/llm-core/src/logs.rs` — `ensure_schema()`, `LogRecord`, `LogEntry`, `record_log_entry()`, `list_logs()`

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/migrations.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/__init__.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/models.py

### Exit criteria
- [ ] Existing upstream `logs.db` opens and lists without loss.
- [ ] New Rust logs are readable by upstream `llm logs list`.
- [ ] Migration metadata is tracked in upstream-compatible `_llm_migrations` shape.
- [ ] Migration preflight/dry-run mode is available and validated in tests.
- [ ] String-ID migration is complete (`responses.id` + `conversation_id` + `--id-gt/--id-gte` behavior) with deterministic legacy ordering guarantees.
- [ ] Config files (`keys.json`, `aliases.json`, `default_model.txt`, `model_options.json`) round-trip across both CLIs (with read fallback for legacy `default-model.txt`).
- [ ] Migration test suite passes against real upstream fixture DBs (including FTS/search behavior checks).
- [ ] All 46 existing tests still pass (no regressions).

---

## M2a — Core runtime refactor + prompt parity (Weeks 3–6)

### Objectives
- Refactor the provider data model to support tools, schemas, and richer metadata.
- Bring `prompt` command close to upstream behavior.

**Rationale for splitting M2:** the original M2 combined prompt parity + chat into one 3-week milestone. That's 6-8 weeks of actual work. Splitting into M2a (prompt) and M2b (chat) gives realistic targets and a useful intermediate checkpoint.

### Tasks

#### Provider data model refactor (prerequisite for all tool/schema work)
- [ ] Add to `PromptRequest` (`providers/mod.rs`): `tools: Vec<ToolDefinition>`, `functions: Vec<FunctionDefinition>`, `response_format: Option<ResponseFormat>`, `schema: Option<JsonSchema>`. **[L]**
- [ ] Add `ToolDefinition`, `FunctionDefinition`, `ResponseFormat`, `JsonSchema` types to `llm-core`. **[L]**
- [ ] Extend `PromptCompletion` with: `usage: Option<UsageInfo>`, `tool_calls: Vec<ToolCall>`, `finish_reason: Option<String>`. **[M]**
- [ ] Update OpenAI provider to serialize/deserialize tool calls in requests and responses. **[L]**
- [ ] Update Anthropic provider for tool_use blocks. **[L]**
- [ ] Persist tool calls, results, and usage to logs DB via updated `LogRecord`. **[M]**

#### Conversation continuation infrastructure + flag migration
- [ ] Implement `load_conversation_messages(conversation_id) -> Vec<PromptMessage>` in `logs.rs` that reads previous messages from DB. **[M]**
- [ ] Adopt upstream flag semantics exactly:
  - `-c/--continue` = continue most recent conversation
  - `--cid/--conversation` = continue specific conversation ID **[M]**
- [ ] Add one-release compatibility shim for current Rust `-c <conversation-id>` usage with deprecation warning, then remove. **[M]**
  - Implement argv pre-processing before Clap parse: when legacy pattern `-c <non-flag-token>` is detected, rewrite to `--cid <token>` and emit deprecation warning.
  - Keep `-c`/`--continue` as boolean with no value in Clap so future behavior is unambiguous.
  - Add migration tests for: `-c` alone, `--continue`, `--cid <id>`, and legacy `-c <id>` rewrite path.
- [ ] Ensure conversation continuation works across process restarts. **[M]**

#### Prompt command option parity (upstream semantics)
- [ ] `--tool/-T`, `--functions`, `--tools-debug`, `--tools-approve`, `--chain-limit` **[L]**
- [ ] `--option` (arbitrary key-value model options) **[M]**
- [ ] `--schema`, `--schema-multi` (structured extraction) **[L]**
- [ ] `--fragment/-f`, `--system-fragment/--sf` (resolve from alias/hash/path/URL) **[L]**
- [ ] Persist prompt/system fragment links into `prompt_fragments` and `system_fragments` tables introduced in M1. **[M]**
- [ ] `--template/-t`, `--param/-p` (template evaluation and variable handling) **[L]**
- [ ] `--save` (save prompt/system/template metadata as a template; enforce upstream disallowed combinations such as with `--template`, `--continue`, `--cid`). **[M]**
- [ ] `--async` (execute using async model path and output behavior matching upstream; not a background-job/response-ID API). **[M]**
- [ ] `--usage` (print token usage after response) **[S]**
- [ ] `--extract/-x`, `--extract-last/--xl` (extract fenced code blocks using upstream behavior) **[M]**
- [ ] `--database` (override logs database path for prompt execution). **[S]**
- [ ] `--query` (model discovery query terms; select shortest matching model ID when `--model` is omitted). **[M]**
- [ ] Implement stdin prompt merge behavior: when stdin is piped and positional prompt is also provided, upstream concatenates them with stdin first. Match this. **[S]**

### Internal references
- `crates/llm-core/src/providers/mod.rs` — `PromptRequest`, `PromptCompletion`, `PromptProvider`
- `crates/llm-core/src/providers/openai.rs` — streaming + request serialization
- `crates/llm-core/src/providers/anthropic.rs` — tool_use block format
- `crates/llm-core/src/lib.rs` — `execute_prompt_with_messages()`, `stream_prompt_with_messages()`
- `crates/llm-core/src/logs.rs` — `LogRecord`, `record_log_entry()`
- `crates/llm-cli/src/main.rs` — `PromptInputArgs`, `run_prompt()`

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/cli.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/models.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/templates.py

### Exit criteria
- [ ] Prompt option parity diff is green for all shared flags.
- [ ] `--save`, `--database`, `--query`, and `--async` semantics match upstream behavior (verified by golden tests).
- [ ] Tool call chains respect chain limit and approval/debug flows.
- [ ] Conversation continuation works across process restarts using upstream flag semantics (`-c` vs `--cid`), with tested one-release legacy `-c <id>` rewrite + warning behavior.
- [ ] Fragment usage persistence (`prompt_fragments`/`system_fragments`) is working.
- [ ] Token usage metadata persisted to logs DB.
- [ ] No regressions in existing 46 tests + new tests for each feature.

---

## M2b — Chat command parity (Weeks 6–8)

### Objectives
- Deliver the `chat` interactive command with full upstream behavior.

### Dependencies
- **Requires M2a complete:** conversation continuation, tool infrastructure, fragment resolution.

### Tasks
- [ ] Implement `chat` command with interactive readline loop. **[L]**
- [ ] Add `!multi` command (multi-line input mode). **[M]**
- [ ] Add `!edit` command (open editor for input). **[M]**
- [ ] Add `!fragment` command (inline fragment insertion). **[M]**
- [ ] Support conversation continuation and DB-backed history inflation (reuse M2a infrastructure). **[M]**
- [ ] Support chat with tool chaining and approvals. **[M]**
- [ ] Streaming output during chat responses. **[S]**
- [ ] Exit handling (Ctrl+C, Ctrl+D, `!exit`). **[S]**

### Internal references
- `crates/llm-cli/src/main.rs` — new `Chat` command variant

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/cli.py (search for `chat_` functions)

### Exit criteria
- [ ] Interactive chat loop works with conversation persistence.
- [ ] All `!` commands functional.
- [ ] Tool chaining works within chat sessions.

---

## M3 — Command-surface completion (Weeks 7–10)

### Objectives
- Fill missing command groups that do NOT depend on plugin bridge.

**Important:** tasks that depend on the plugin bridge (e.g., `openai models`, plugin-provided commands) are moved to M5. Only natively implementable commands belong here.

### Tasks

#### New command groups (no plugin dependency)
- [ ] `aliases` (`list/set/remove/path`) — reads/writes `aliases.json` from M1. **[M]**
- [ ] `templates` (`list/show/edit/path/loaders`) — needs template file storage + loader registry. **[L]**
- [ ] `schemas` (`list/show/dsl`) — reads from logs DB schema tables. **[M]**
- [ ] `tools` (`list --json --functions`) — lists natively registered tools. **[M]**

#### Existing command parity upgrades
- [ ] `logs list` full option set: `--response/-r`, `--extract/-x`, `--extract-last/--xl`, `--data*`, `--short`, `--truncate`, fragment/tool/schema filters, `--current`, `--latest`. **[L]**
  - **Note:** fragment filters depend on M2a fragment persistence; tool/schema filters depend on M2a tool execution being in place.
- [ ] `models list` filters and capability flags: `--options`, `--async`, `--schemas`, `--tools`, `--query`, `--model`. **[M]**
- [ ] `models options` subcommands: `list/show/set/clear` — reads/writes `model_options.json` from M1. **[M]**
- [ ] `plugins` options: `--all`, `--hook` — richer metadata (limited without bridge). **[S]**
- [ ] `keys` UX parity: hidden-input prompts, masking, alias query flags. **[S]**

### Internal references
- `crates/llm-cli/src/main.rs` — add new `Command` variants
- `crates/llm-core/src/logs.rs` — extend `list_logs()` for new filters

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/cli.py

### Exit criteria
- [ ] Top-level command diff reduced to: plugin-dependent commands (`openai`, `install`, `uninstall`) **plus embeddings family deferred to M4** (`collections`, `embed`, `embed-models`, `embed-multi`, `similar`) + intentional Rust extras (`cmd`, `version`).
- [ ] Help-option diff passes for all implemented groups.
- [ ] New tests for each new command/subcommand.

---

## M4 — Embeddings and collections parity (Weeks 9–12)

### Objectives
- Deliver full embeddings command family and storage compatibility.

### Tasks
- [ ] Implement `llm-embeddings` crate: collection abstraction, encode/decode vectors, cosine similarity. **[XL]**
- [ ] Port embeddings DB migrations and ensure schema compatibility with upstream. **[L]**
- [ ] Add embeddings provider abstraction in `llm-core` (request/response types, provider trait, retry/timeouts, key/env resolution). **[L]**
- [ ] Implement built-in embeddings providers/models needed for parity baseline (at minimum OpenAI-compatible embeddings path), with default-model selection behavior matching upstream. **[L]**
- [ ] Add embeddings model registry integration (`embed-models list/default`) that works natively first and can be extended by plugin `register_embedding_models` in M5. **[M]**
- [ ] Implement commands:
  - `embed` **[M]**
  - `embed-multi` **[L]**
  - `similar` **[M]**
  - `embed-models list/default` **[M]**
  - `collections list/path/delete` **[M]**
- [ ] Implement binary/text content modes, metadata JSON, and output formats. **[M]**
- [ ] Implement file/SQL ingestion flows for `embed-multi`. **[L]**

### Internal references
- `crates/llm-embeddings/src/lib.rs` — currently returns `0`
- `crates/llm-cli/src/main.rs`

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/embeddings.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/embeddings_migrations.py

### Exit criteria
- [ ] Rust-created `embeddings.db` can be consumed by upstream and vice versa.
- [ ] Embedding provider/model execution path works end-to-end (real provider + stub test modes).
- [ ] Similarity results and output formatting match expected behavior.
- [ ] All embedding commands have integration tests.
- [ ] After M4 completion, top-level command diff excludes only plugin-dependent commands (`openai`, `install`, `uninstall`) + intentional Rust extras.

---

## M5 — Plugin bridge and ecosystem parity (Weeks 10–14)

### Objectives
- Replace plugin stub with real upstream-compatible plugin execution path.
- Add dynamic command/model dispatch infrastructure required for plugin-provided commands/models.
- Ship plugin-dependent commands that were deferred from M3.

### Known risks (must plan for)
- **Python environment management:** pyo3 requires a specific Python version linked at build time. Document minimum Python version (3.10+) and provide build instructions.
- **Cross-platform linking:** Windows linking with Python is notoriously fragile. Target Linux/macOS first; Windows is stretch goal.
- **setuptools entrypoint discovery:** upstream plugins register via `[project.entry-points."llm"]` in pyproject.toml. The bridge must discover these via Python's `importlib.metadata`, not by reimplementing setuptools.
- **Build time impact:** pyo3 adds significant build complexity. Make the bridge a cargo feature (`--features python-bridge`) so pure-Rust builds remain fast.

### Tasks

#### Bridge infrastructure
- [ ] Implement `pyo3` bridge in `llm-plugin-host` behind `python-bridge` cargo feature. **[XL]**
- [ ] Implement Python environment detection and version validation. **[M]**
- [ ] Implement `importlib.metadata` entrypoint discovery for `llm` group. **[L]**
- [ ] Implement dynamic command registry in `llm-cli` so plugin commands can be registered/executed at runtime (while preserving static core command UX/help output). **[XL]**
- [ ] Implement command-collision and precedence rules (core commands win; plugin command conflicts emit deterministic warnings/errors). **[M]**
- [ ] Implement model provider registry in `llm-core` so plugin models route through registered providers instead of hardcoded provider `match` only. **[L]**

#### Hook support
- [ ] `register_commands` **[L]**
- [ ] `register_models` **[L]**
- [ ] `register_embedding_models` **[M]**
- [ ] `register_template_loaders` **[M]**
- [ ] `register_fragment_loaders` **[M]**
- [ ] `register_tools` **[L]**

#### Plugin loading behavior
- [ ] `LLM_LOAD_PLUGINS` environment variable support. **[S]**
- [ ] Built-in default plugins loading order. **[M]**

#### Deferred plugin-dependent commands
- [ ] `openai models` command group (provided by default OpenAI plugin). **[M]**
- [ ] `install` / `uninstall` (pip-wrapper commands). **[M]**
- [ ] Tool metadata capture (plugin name, schemas, calls/results). **[M]**

### Internal references
- `crates/llm-plugin-host/src/lib.rs` — currently returns single stub
- `crates/llm-cli/src/main.rs`
- `crates/llm-core/src/lib.rs`

### External references
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/plugins.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/hookspecs.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/default_plugins/openai_models.py
- https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/default_plugins/default_tools.py

### Exit criteria
- [ ] `llm plugins --all` parity for default plugins.
- [ ] Dynamic plugin command/model dispatch works (commands/models discovered and executable at runtime).
- [ ] Plugin-provided commands/models/tools appear and execute.
- [ ] Third-party plugin smoke test runs in CI as a **non-blocking quality gate** (tracked/reported, not required for parity release sign-off).
- [ ] Pure-Rust build without `python-bridge` feature still compiles and runs (graceful degradation).

---

## M6 — Provider/runtime feature completion (Weeks 12–15)

### Objectives
- Close runtime behavior gaps not solved by CLI surface parity.

### Tasks
- [ ] Add richer response metadata capture: usage (input/output/details), resolved model, response/prompt JSON payloads with redaction. **[M]**
- [ ] Add schema-aware responses (`response_format` / JSON schema mode where supported). **[L]**
- [ ] Add model option validation and persistence integration (`model_options.json`). **[M]**
- [ ] Harden async execution path after M2a parity landing (timeouts, cancellation, retry/telemetry behavior under async flows). **[L]**
- [ ] Align key/env precedence with upstream for all supported providers. **[S]**
- [ ] Document Rust-only enhancements (`--retries`, `--retry-backoff-ms`, `cmd`, `--debug`, `--info`, `keys resolve`) in `docs/cli-parity-matrix.md` and `README.md`. **[S]**

### Internal references
- `crates/llm-core/src/providers/openai.rs`
- `crates/llm-core/src/providers/anthropic.rs`
- `crates/llm-core/src/lib.rs`
- `crates/llm-core/src/logs.rs`

### Exit criteria
- [ ] Logs include usage and model metadata comparable to upstream.
- [ ] Tool call flows are persisted and queryable via `logs list` filters.
- [ ] `--async` behavior matches documented expectations.

---

## M7 — Test parity, docs, and release readiness (Weeks 14–16)

### Objectives
- Convert roadmap completion into reliable parity signal and release process.

### Tasks
- [ ] Build command-level parity test suite comparing Rust vs upstream outputs (normalize IDs/timestamps). **[L]**
- [ ] Add compatibility fixture suites using real upstream-created databases (`logs.db`, `embeddings.db`) for read/write/migration round-trips. **[M]**
- [ ] Port/translate key upstream tests by behavior category:
  - prompt/options/attachments/chat **[L]**
  - logs and schemas **[M]**
  - tools and plugins **[L]**
  - embeddings commands **[M]**
- [ ] Add CI matrix for:
  - pure Rust tests (no Python) **[M]**
  - Python bridge integration tests (requires Python env) **[M]**
  - compatibility tests against pinned upstream baseline **[M]**
  - third-party plugin smoke test as non-blocking/reporting job **[S]**
- [ ] Update public docs with parity status, migration notes, and binary naming guidance (`llm` vs `llm-cli`). **[M]**
- [ ] Add release checklist for parity regression sign-off. **[S]**

### External test references
- https://github.com/simonw/llm/tree/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/tests

### Exit criteria
- [ ] Parity CI passes on Linux/macOS for core scenarios.
- [ ] Third-party plugin smoke job publishes status without blocking parity release.
- [ ] Documented exceptions from upstream are explicit and intentional.
- [ ] Release checklist signed off.

---

## Dependency graph

```
M0 (decisions + baseline)
 │
 ├── M1 (storage/config compat)
 │    │
 │    ├── M2a (provider refactor + prompt parity)
 │    │    │
 │    │    ├── M2b (chat command)
 │    │    │
 │    │    ├── M3 (command surface — non-plugin)
 │    │    │    │
 │    │    │    └── M6 (runtime completion)
 │    │    │
 │    │    └── M4 (embeddings) ← can start after M1, parallel to M2b/M3
 │    │
 │    └── M5 (plugin bridge + dynamic runtime) ← can start infrastructure after M1
 │         │
 │         └── M3 deferred items (openai group, install/uninstall)
 │
 └── M7 (test parity + release) ← after M3, M4, M5, M6
```

### Parallelization opportunities
- **M4** (embeddings) can start after M1 completes, in parallel with M2a/M2b.
- **M5** bridge infrastructure can start after M1 (after ADR-approved dynamic registry scaffold), but hook support requires M2a types.
- **M3** non-plugin commands can start after M2a is far enough along for tool/schema types to exist.

---

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Python bridge complexity delays parity | High | High | Ship native parity for non-plugin commands first (M2a–M4). Bridge is M5. Make it a cargo feature so core CLI works without Python. |
| Dynamic plugin command/model dispatch design adds unexpected refactor cost | High | High | Land ADR + minimal dynamic registry scaffold in M0/M1, then layer bridge hooks in M5 instead of coupling both changes late. |
| DB compatibility regressions corrupt user data | Medium | Critical | Migration tests with real upstream fixture DBs. Backup-first behavior on schema upgrades. |
| Option-level drift across model plugins | Medium | Medium | Snapshot tests on `llm models list --options` and `prompt --help`. Pin upstream baseline. |
| CLI flag migration confusion (`-c` semantics change) | Medium | High | One-release compatibility shim, explicit deprecation warnings, and targeted migration tests/docs before removal. |
| pyo3 cross-platform build failures | High | Medium | Target Linux/macOS first. Windows is explicit stretch goal. CI covers both bridge and non-bridge builds. |
| Timeline slippage on M2a (scope is large) | High | Medium | M2a is the largest milestone. Track weekly against task list. Keep base semantic parity for `--save`/`--database`/`--query`/`--async` in M2a; if needed, defer only non-critical edge cases (advanced extract behaviors, async hardening) to M6. |
| Upstream breaking changes during implementation | Low | Medium | Pin upstream commit. Periodic rebase checks (monthly). |

---

## Definition of done (feature parity)

- [ ] Command tree parity achieved for upstream core + default plugins.
- [ ] Option parity achieved for prompt/logs/models/plugins/tools/embeddings families, including semantic parity for high-risk flags (`--save`, `--database`, `--query`, `--async`, `-c/--cid`).
- [ ] Upstream-compatible config and database files are interoperable in both directions.
- [ ] Plugin hooks and default plugins function via bridge (or graceful degradation without Python).
- [ ] Third-party plugin smoke coverage is reported in CI as a non-blocking quality gate.
- [ ] Binary naming compatibility decision (`llm` vs `llm-cli`) is implemented and documented for users.
- [ ] Parity test suite is green against pinned upstream baseline.
- [ ] Remaining differences are documented as intentional extensions in `docs/cli-parity-matrix.md` and `README.md`.

---

## Suggested immediate next actions (next 5 PRs)

1. [ ] **PR-1 (M0):** Lock architectural decisions + parity diff script + parity rubric + flag-semantics contract doc + parity-matrix cleanup (including fragments command-surface correction) + ADR-001/ADR-002 + binary naming decision note. Target: 2–3 days.
2. [ ] **PR-2a (M1 config):** Config compatibility — `default_model.txt` transition + `aliases.json` + `model_options.json` + alias-aware model resolution integration. Target: 3–4 days.
3. [ ] **PR-2b (M1 migrations):** Migration engine scaffold using `_llm_migrations` + backup-first behavior + migration preflight/dry-run + full logs schema migration + FTS parity + deterministic ULID string-ID conversion in DB/runtime/CLI types. Target: 4–6 days.
4. [ ] **PR-2c (M1 validation):** Real upstream fixture DB compatibility tests (read/write/migrate) + regression updates for string-ID filters and ordering semantics + dry-run coverage. Target: 2–3 days.
5. [ ] **PR-3 (M2a start):** Provider data model refactor + conversation loading infrastructure + semantic alignment for `--save`/`--database`/`--query`/`--async` and `-c`/`--cid` (including legacy `-c <id>` rewrite tests). Target: 1 week.
