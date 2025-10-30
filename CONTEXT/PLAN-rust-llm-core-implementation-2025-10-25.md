# LLM Rust Core Implementation Plan

## Overview
Plan for implementing remaining core functionality in the Rust LLM rewrite, focusing on SQLite logging, provider metadata, and plugin infrastructure.

## Current State
- Core workspace structure established
- Basic CLI commands operational
- Provider abstraction foundations in place
- Initial integration tests running

## Phase 1: SQLite Logging Infrastructure

### Requirements
- Match Python `logs.db` schema
- Support all filtering options from CLI matrix
- Capture full response metadata
- Handle streaming and buffered paths

### Implementation Tasks
- [ ] Database Schema & Migration
  - [ ] Review Python schema in `llm/db.py`
  - [ ] Define Rust structs in `llm-core/src/logs.rs`
  - [ ] Implement SQLite migrations
  - [ ] Add schema version tracking

- [ ] Core Logging API
  - [ ] Create `LogEntry` struct with all metadata fields
  - [ ] Implement transaction management
  - [ ] Add backup/restore functionality
  - [ ] Build query builder for filters

- [ ] CLI Integration  
  - [ ] Add `--log/--no-log` flags to commands
  - [ ] Implement all `logs list` filters
  - [ ] Add JSON output formatting
  - [ ] Support conversation tracking

### Validation
```rust
// Test cases to implement
#[test]
fn test_log_entry_persistence() {
    // Verify all fields round-trip
}

#[test]
fn test_conversation_tracking() {
    // Verify message threading
}

#[test] 
fn test_filter_combinations() {
    // Test complex filter queries
}
```

## Phase 2: Provider Metadata Streaming

### Requirements
- Emit usage counters during streaming
- Track tool calls and function invocations
- Support both streaming and buffered modes
- Persist metadata to logs

### Implementation Tasks
- [ ] Provider Trait Enhancement
  - [ ] Add metadata types to `llm-core/src/providers/mod.rs`
  - [ ] Extend streaming response type
  - [ ] Update provider impls for OpenAI/Anthropic

- [ ] Metadata Collection
  - [ ] Implement usage counter tracking
  - [ ] Add tool call capture
  - [ ] Create metadata aggregation

- [ ] Integration
  - [ ] Update `stream_prompt_internal`
  - [ ] Connect to logging system
  - [ ] Add CLI reporting options

### Validation
```rust
#[test]
fn test_streaming_metadata() {
    // Verify counters update during stream
}

#[test]
fn test_tool_call_capture() {
    // Verify tool calls recorded
}
```

## Phase 3: Python Plugin Bridge

### Requirements
- Load Python plugins via PyO3
- Execute registration hooks
- Support model/command plugins
- Maintain compatibility with existing plugins

### Implementation Tasks
- [ ] PyO3 Integration
  - [ ] Add PyO3 dependencies
  - [ ] Create Python runtime wrapper
  - [ ] Implement plugin loader

- [ ] Hook System
  - [ ] Port Python hook definitions
  - [ ] Create hook registry
  - [ ] Add plugin lifecycle management

- [ ] Plugin Support
  - [ ] Implement model plugins
  - [ ] Add command plugins
  - [ ] Create compatibility layer

### Testing
```rust
#[test]
fn test_plugin_loading() {
    // Verify plugin discovery
}

#[test]
fn test_hook_execution() {
    // Test hook registration
}
```

## Dependencies
- External Libraries:
  - PyO3: https://github.com/PyO3/pyo3
  - Rusqlite: https://github.com/rusqlite/rusqlite
  - Tokio: https://github.com/tokio-rs/tokio

- Reference Code:
  - Python LLM: https://github.com/simonw/llm
  - SQLite schema: https://github.com/simonw/llm/blob/main/llm/db.py

## Success Criteria
- All Python CLI parity tests passing
- Plugin system loads existing plugins
- Logging captures all metadata
- Performance benchmarks meet/exceed Python

## Timeline & Milestones
1. SQLite Logging (2 weeks)
   - Schema migration working
   - Basic logging functional
   - Filters implemented

2. Provider Metadata (1 week)
   - Usage tracking working
   - Tool calls captured
   - Tests passing

3. Plugin Bridge (2 weeks)
   - PyO3 integration complete
   - Basic plugins loading
   - Hook system operational

## Configuration Requirements
```toml
# Required Cargo.toml dependencies
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
pyo3 = { version = "0.20", features = ["auto-initialize"] }
tokio = { version = "1.40", features = ["full"] }
```

## Risk Mitigation
- Maintain Python test parity
- Regular integration testing
- Performance benchmarking
- Compatibility testing with popular plugins

## References
- Project Files:
  - `crates/llm-core/src/lib.rs`
  - `crates/llm-core/src/providers/mod.rs`
  - `crates/llm-cli/src/main.rs`
  - `crates/llm-plugin-host/src/lib.rs`

- Documentation:
  - `docs/rust-rewrite-plan.md`
  - `docs/cli-parity-matrix.md`
  - `AGENTS.md`