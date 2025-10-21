# Next Steps for Rust Rewrite

1. **Complete SQLite-backed logging pipeline**
   - Implement the remaining pieces in `llm-core::logs` to read/write SQLite entries and expose them through `list_logs`/`logs_status`.
   - Wire `llm-cli logs list/status` to the finished core API and add regression tests covering filters, JSON output, and backup behavior.

2. **Enrich provider streaming metadata**
   - Extend the provider abstraction to emit usage counters, tool calls, and other structured metadata during streaming.
   - Resolve the TODOs in `stream_prompt_internal`, ensuring metadata flows through both streaming and non-streaming code paths.

3. **Prototype the Python plugin bridge**
   - Stand up a `pyo3`-based bridge that can load Python plugins, execute `register_*` hooks, and surface their models/commands.
   - Use `llm-markov` (or another minimal plugin) as an initial integration test and document any compatibility gaps.

4. **Reconcile architectural documentation references**
   - Restore or recreate the missing reference files noted in `docs/rust-rewrite-plan.md` (e.g., module mapping, scaffold docs), or update the plan to point at their new homes.
   - Keep AGENTS.md and related docs in sync so future sessions land on accurate guidance.

5. **Expand automated parity testing**
   - Translate high-priority rows from `docs/cli-parity-matrix.md` into integration tests that compare Rust CLI behavior with the Python original.
   - Prioritize coverage for embeddings, templates, tool invocation, and plugin-provided commands to guard upcoming feature work.
