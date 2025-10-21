# AGENTS.md – Rust Rewrite Progress Notes

Use this file to resume work on the Rust rewrite of `llm`. It captures the current state, key commands, and next steps so you can pick up after a context reset.

## Current Status
- Workspace scaffolded under `rust/` with crates:
  - `llm-core` (user dir + key storage helpers, logs path stub, provider abstraction with OpenAI/OpenAI-compatible/Anthropic support + retries/alias normalization)
  - `llm-cli` (Clap-based CLI with `prompt`, `plugins`, `keys`, `logs`, `cmd` subcommands)
  - `llm-plugin-host`, `llm-embeddings` (placeholders)
- CLI parity implemented so far: `keys path/get/list/set/resolve`, `logs path`, `plugins`, `prompt` (provider dispatch w/ OpenAI + Anthropics; prompt options `--model/--temperature/--max-tokens/--retries/--retry-backoff-ms` + `--key` override, streaming on by default with `--no-stream` opt-out), `cmd` (interactive command generation/execution with rustyline + temp editor fallback), `models list/default` (OpenAI + Anthropic catalog + alias support + persisted default).
- Integration tests (`cargo test`) cover CLI behaviors including `cmd` auto-accept; unit tests in `llm-core` ensure key persistence and default-model validation for new providers.

## Quick Start Commands
```bash
cd rust
cargo build          # compile workspace
cargo test           # run unit + integration tests
cargo run -- --help  # top-level CLI help
cargo run -- "prompt text"
# Disable streaming when needed
cargo run -- --no-stream "prompt text"
cargo run -- keys list --json
cargo run -- keys set openai --value YOUR_API_KEY
cargo run -- logs path
# Auto-accept handy during non-interactive runs
LLM_CMD_AUTO_ACCEPT=1 cargo run -- cmd "undo last git commit"
```

## Configuration Notes
- User data directory mirrors Python CLI:
  - Default: `~/.config/io.datasette.llm`
  - Override with `LLM_USER_PATH=/path`
- Keys stored in `keys.json`, structured the same as Python version (includes `"// Note"` warning line).
- Provider key aliases / env overrides:
  - OpenAI: alias `openai`, env `OPENAI_API_KEY` (or `LLM_OPENAI_API_KEY`).
  - OpenAI-compatible: alias `openai-compatible`, env `OPENAI_COMPATIBLE_API_KEY` (fallback `LLM_OPENAI_COMPATIBLE_API_KEY`).
  - Anthropic: alias `anthropic`, env `ANTHROPIC_API_KEY` (fallback `LLM_ANTHROPIC_API_KEY`).
- Base URLs: `OPENAI_BASE_URL`, `OPENAI_COMPATIBLE_BASE_URL` (`LLM_OPENAI_COMPATIBLE_BASE_URL` fallback), `ANTHROPIC_BASE_URL` (`LLM_ANTHROPIC_BASE_URL`).
- `LLM_CMD_AUTO_ACCEPT=1` (or running in non-interactive STDIN) auto-accepts generated commands without launching the line editor.
- `--debug` now logs resolved model/provider + sampling/retry settings before execution (relies on `prompt_debug_info`).
- Logs path stub returns `<user_dir>/logs.db`.

## Dependencies
- Runtime crates: `clap`, `serde`, `tracing`, `rpassword`, `crossterm` (optional), `directories`, `reqwest`, `rustyline`, `shell-words`.
- Tests leverage `assert_cmd`, `predicates`, `tempfile` and stub envs (`LLM_PROMPT_STUB`, `LLM_CMD_AUTO_ACCEPT`).

## Outstanding Work
1. Port logging DB access (`logs list`) using SQLite.
2. Flesh out provider abstraction: richer streaming metadata (usage, tool calls) + TODO at `stream_prompt_internal`.
3. Integrate Python plugin bridge via `pyo3`.
4. Expand CLI parity (models list per provider refresh, embeddings, templates, non-OpenAI providers).
5. Document build/test flow in `README` once core parity improves.
6. Refresh Anthropic API key (current stored value returns 401) before next smoke test.

## Useful References
- Detailed plan tracker: `docs/rust-rewrite-plan.md`
- CLI parity checklist: `docs/cli-parity-matrix.md`
- Plugin inventory & behaviors: `docs/plugin-inventory.md`, `docs/plugin-behaviors.md`
- Quick start (Rust CLI): `docs/rust-quickstart.md`

Keep this file updated when major milestones change so future sessions can resume quickly.
