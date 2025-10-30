# Next Steps for Rust Rewrite

1. **Harden SQLite logging and CLI parity**
   - Persist conversation rows, token usage, and options metadata when writing responses so the schema matches Python's `logs.db`.
   - Add logging toggles (`--log`/`--no-log`) to `prompt`/`cmd`, align `logs list` filters (conversation IDs, id thresholds, time bounds), and polish TTY vs `--json` output.
   - Expand integration coverage for logging on/off sentinel handling, advanced filter combinations, and backup/restore flows.

2. **Enrich provider streaming metadata**
   - Extend the provider abstraction to emit usage counters, tool calls, and structured metadata alongside text deltas.
   - Surface metadata through `stream_prompt_internal` so both streaming and buffered paths can log usage and populate `LogEntry`.

3. **Prototype the Python plugin bridge**
   - Stand up a `pyo3`-based bridge that can load Python plugins, execute `register_*` hooks, and surface their models/commands.
   - Use `llm-markov` (or another minimal plugin) as an initial integration test and document any compatibility gaps.

4. **Reconcile architectural documentation references**
   - Restore or recreate the missing reference files noted in `docs/rust-rewrite-plan.md` (e.g., module mapping, scaffold docs), or update the plan to point at their new homes.
   - Keep AGENTS.md and related docs in sync so future sessions land on accurate guidance.

5. **Expand automated parity testing**
   - Translate high-priority rows from `docs/cli-parity-matrix.md` into integration tests that compare Rust CLI behavior with the Python original.
   - Prioritize coverage for embeddings, templates, tool invocation, and plugin-provided commands to guard upcoming feature work.
