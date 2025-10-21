# Rust Rewrite TODO

## Meta
- [x] Create and version the Rust rewrite TODO tracker.

## Discovery & Targets
- [x] Catalogue every CLI command, subcommand, flag, hidden option into a parity matrix with priority and coverage notes.
  - [x] Capture initial top-level command list (`docs/cli-parity-matrix.md`).
  - [x] Record built-in command option snapshots (`docs/cli-parity-matrix.md`).
  - [x] Document installed plugin command trees (`docs/cli-parity-matrix.md`).
  - [x] Note hidden/path options for advanced usage (`docs/cli-parity-matrix.md`).
- [x] Audit plugin usage across first-party and third-party packages, recording entry points, dependencies, and hook usage.
  - [x] Snapshot currently installed plugins and exposed hooks (`docs/plugin-inventory.md`).
  - [x] Capture plugin dependency requirements and extras (`docs/plugin-inventory.md`).
  - [x] Summarize runtime behaviors, caching, and CLI additions (`docs/plugin-behaviors.md`).
- [ ] Profile current performance bottlenecks (CLI startup, streaming latency, SQLite operations) and capture baseline metrics.
  - [x] Record initial CLI startup timing (`docs/performance-baseline.md`).
  - [x] Capture plugin enumeration timing (`docs/performance-baseline.md`).
- [ ] Decide packaging targets and document success criteria for latency, parity, and API compatibility.
  - [x] Capture current distribution channels and documentation workflow (`docs/current-distribution.md`).

## Architecture & Scaffolding
- [ ] Map existing Python modules to Rust crate structure (`core`, `cli`, `plugins`, `storage`, `providers`, `tools`, `python-bridge`).
  - [x] Draft initial Python → Rust module mapping (`docs/module-mapping.md`).
  - [x] Outline workspace scaffold and crate responsibilities (`docs/workspace-scaffold.md`).
- [ ] Finalize foundational crate selections and document configuration/keyring strategy.
  - [x] Draft foundational crate candidates (`docs/foundational-crates.md`).
  - [x] Document current configuration and key storage behavior (`docs/config-and-keys.md`).
- [ ] Specify plugin ABI roadmap covering Rust-native plugins, Python bridge, and installation metadata.
  - [x] Draft ABI migration outline (`docs/plugin-abi-roadmap.md`).
- [ ] Define error handling, logging/tracing approach, and structured output formats to retain CLI/library behavior.
  - [x] Outline error/logging strategy (`docs/error-logging-strategy.md`).

## Plugin Ecosystem & Translation
- [ ] Implement Python plugin host with `pyo3`, including virtualenv management and Pluggy compatibility layer.
- [ ] Deliver compatibility tests ensuring lifecycle hooks execute correctly for Python plugins.
- [ ] Build automated translation pipeline to scaffold Rust adapters from Python plugin metadata/AST.
- [ ] Create developer tooling (`llm migrate-plugin`) to drive translation workflow and track plugin migration status.
- [ ] Document long-term plugin migration strategy (Rust crate index, signed manifests, compatibility timeline).
  - [x] Draft plugin bridge + translation roadmap (`docs/plugin-translation-plan.md`).

## Library/API Compatibility
- [ ] Design Rust library API mirroring critical Python functions and document deliberate differences.
  - [x] Survey current Python public API (`docs/library-api-survey.md`).
- [ ] Provide Python wrapper (`pyo3`/`maturin`) that preserves `import llm` behavior with legacy signatures.
  - [x] Draft Python wrapper integration plan (`docs/python-wrapper-plan.md`).
- [ ] Write migration guidance for downstream Python consumers, highlighting any deprecated surfaces.

## Incremental Implementation
- [ ] Scaffold Rust workspace and minimal prompt execution path with parity verification against Python CLI.
  - [x] Create initial workspace skeleton and stub CLI (`rust/` workspace, `llm-cli` -> `llm-core`).
- [ ] Port configuration, key management, and SQLite schema/migrations with embedded SQL runner.
  - [x] Implement user directory + key resolution stubs in Rust (`llm-core`, `llm-cli keys`).
  - [x] Expose logs database path via CLI placeholder (`llm-cli logs path`).
  - [x] Implement `keys list/get/path/resolve/set` parity with secure input and JSON option (`llm-cli`).
- [ ] Implement provider abstraction layer with async traits, streaming support, and retries plus mocks.
  - [x] Draft provider trait design and requirements (`docs/provider-abstraction.md`).
- [ ] Rebuild prompt execution pipeline (templates, fragments, system prompts, structured extraction) backed by fixtures.
- [ ] Port embeddings subsystem ensuring database compatibility and similarity search parity.
- [ ] Re-implement tools execution sandbox with schema validation and plugin-provided tools support.
- [ ] Integrate plugin loader/bridge including translation outputs and native Rust plugins.
- [ ] Harden concurrency, cancellation, and resource cleanup paths.

## Testing, CI & Benchmarks
- [ ] Translate CLI parity matrix into automated tests (unit, integration, golden) comparing Python and Rust CLIs.
- [ ] Add streaming-specific integration tests using SSE/WebSocket mocks validating chunk timing.
- [ ] Test plugin bridge with real Python plugins and translation pipeline smoke tests in CI.
- [ ] Build performance benchmarking suite comparing latency/throughput against Python baseline with regression alerts.
- [ ] Configure formatting/linting/security checks and cross-platform CI for native and Python-bridged modes.
  - [x] Draft comprehensive testing/CI plan (`docs/testing-strategy.md`).

## Docs, Tooling & Packaging
- [ ] Decide doc generation system and update README/Sphinx workflow accordingly.
  - [x] Document documentation strategy options (`docs/docs-strategy.md`).
- [ ] Update developer docs for Rust setup, plugin translation guide, and CLI parity table.
- [ ] Package binaries (e.g., `cargo dist`) and maintain pip wrapper or wheel bundling strategy.
  - [x] Draft packaging plan across channels (`docs/packaging-plan.md`).
- [ ] Produce migration guide, plugin author playbook, and release checklist for alpha/beta/GA phases.
  - [x] Draft migration guide outline (`docs/migration-guide-outline.md`).

## Rollout & Maintenance
- [ ] Plan staged release timeline (internal alpha, community beta, GA) with feedback channels.
  - [x] Draft rollout plan (`docs/rollout-plan.md`).
- [ ] Track parity matrix, API compatibility, and plugin migration status on dashboards.
- [ ] Communicate deprecation timeline for Python core once parity thresholds are met.
- [ ] Establish governance for ongoing maintenance, issue triage, and plugin review processes.
