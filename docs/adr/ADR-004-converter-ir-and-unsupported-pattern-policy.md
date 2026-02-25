# ADR-004: Converter IR Schema and Unsupported-Pattern Policy

**Status:** Accepted  
**Date:** 2026-02-25  
**Author:** llm-rust team  
**Related:** [ADR-003](ADR-003-unified-plugin-registry.md)

## Context

Phase 2 introduces `llm-plugin-convert`, which analyzes Python `llm-*` plugins and scaffolds Rust-native plugin crates.

Without a strict intermediate representation (IR) and explicit unsupported-pattern policy, conversion can silently lose behavior. Silent loss is unacceptable for parity-driven migration.

We need:

1. A deterministic IR format used by analyze/scaffold/parity commands.
2. Clear unsupported-pattern classifications with fail-fast rules.
3. Machine-readable and human-readable reports that explain conversion risk.

## Decision

## 1) Converter IR (`conversion-report.json`)

The converter IR is a versioned JSON document.

```json
{
  "schema_version": "1.0.0",
  "source": {
    "repo": "https://github.com/simonw/llm-openrouter",
    "revision": "<git-sha>",
    "analyzed_at": "<utc-rfc3339>"
  },
  "plugin": {
    "id": "llm-openrouter",
    "version": "0.5.0",
    "entrypoints": ["llm"]
  },
  "hooks": {
    "register_models": true,
    "register_embedding_models": false,
    "register_commands": true,
    "register_template_loaders": false,
    "register_fragment_loaders": false,
    "register_tools": false
  },
  "models": [],
  "embedding_models": [],
  "commands": [],
  "tools": [],
  "template_loaders": [],
  "fragment_loaders": [],
  "dependencies": [],
  "tests": [],
  "unsupported": [],
  "summary": {
    "complexity": "M",
    "risk_score": 0,
    "can_scaffold": true,
    "requires_manual_work": false
  }
}
```

### Determinism rules

- Sort all arrays by stable keys (name/path/order index).
- Normalize paths to POSIX separators.
- Redact machine-local absolute paths.
- Do not embed nondeterministic IDs.
- Record stable source revision (`git sha`) when available.

## 2) Unsupported-pattern classification

Unsupported findings are explicit records:

```json
{
  "code": "U201",
  "severity": "error",
  "location": "plugin.py:84",
  "summary": "dynamic command graph built via runtime introspection",
  "detail": "Click command tree depends on runtime API response",
  "suggested_action": "manual port required",
  "blocks_scaffold": true
}
```

### Severity levels

- `error` — cannot safely scaffold equivalent behavior; scaffolding must fail by default.
- `warning` — scaffold allowed, but TODOs are injected and parity tests must cover gap.
- `info` — notable pattern; no blocking behavior.

### Initial unsupported taxonomy

- `U1xx` dynamic execution/reflection (`eval`, `exec`, runtime code generation)
- `U2xx` dynamic plugin/command graph mutation
- `U3xx` metaclass/descriptor-heavy behavior not representable in templates
- `U4xx` unsupported async/runtime semantics requiring architecture changes
- `U5xx` external side effects requiring manual safety review

## 3) Fail-fast policy

- If any `unsupported.severity == "error"` and `--allow-unsafe` is not set:
  - `analyze` exits non-zero.
  - `scaffold` exits non-zero.
- No hook/function/model may be dropped silently.
- For warning-level findings, scaffolding must emit explicit `TODO(converter:<code>)` markers.

## 4) Output artifacts

The converter produces both:

1. `conversion-report.json` (machine-readable, canonical input for tooling)
2. `conversion-report.md` (human summary with risk table and next actions)

A scaffold run also writes:

- `TODO-CONVERSION.md` listing unresolved warnings/errors.
- Generated parity test stubs mapped to IR entities.

## 5) Parity gate expectations

A generated plugin is not considered parity-ready until:

- it compiles,
- converter TODO list is empty or explicitly waived,
- parity tests pass against golden fixtures (and live Python parity when available).

## Consequences

### Positive

- Deterministic conversion artifacts for CI and review.
- No silent behavior loss.
- Clear contract between extractor, scaffolder, and parity runner.

### Negative

- More upfront strictness means some plugins fail fast instead of generating partial output.
- Manual conversion remains necessary for high-dynamism plugins.

### Neutral

- Taxonomy can expand over time with new `Uxxx` codes without changing schema version semantics (non-breaking additions).

## References

- `PLAN-rust-native-plugin-api-and-converter.md`
- `docs/adr/ADR-003-unified-plugin-registry.md`
