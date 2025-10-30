# Rust LLM Rewrite Implementation Plan
**Created:** 2025-10-24  
**Status:** Active Implementation Plan

## Project Overview

This plan documents the complete rewrite of the Python `llm` CLI tool in Rust, maintaining full compatibility while improving performance and maintainability. The project is currently ~60% complete with core functionality implemented.

### Current Architecture

```
llm-rust/
├── crates/
│   ├── llm-core/           # Core library (providers, models, keys, logging)
│   ├── llm-cli/            # CLI interface with Clap
│   ├── llm-plugin-host/    # Python plugin bridge (placeholder)
│   └── llm-embeddings/     # Embeddings support (placeholder)
├── docs/                   # Documentation and parity tracking
└── CONTEXT/               # This plan and related context
```

### Completed Features

- ✅ Core provider abstraction (OpenAI, OpenAI-compatible, Anthropic)
- ✅ CLI with `prompt`, `keys`, `logs`, `models`, `plugins`, `cmd` subcommands
- ✅ Key storage and resolution system
- ✅ Model catalog with aliases
- ✅ Streaming support with `--no-stream` opt-out
- ✅ Conversation persistence
- ✅ Basic logging infrastructure
- ✅ Integration test suite

## Implementation Roadmap

### Phase 1: Core Infrastructure Completion (Priority: High)

#### 1.1 SQLite Logging Database Enhancement

**Files to modify:**
- `crates/llm-core/src/logs.rs` - Complete logging implementation
- `crates/llm-core/src/lib.rs` - Update `log_prompt_result` function

**Tasks:**
- [ ] Implement full SQLite schema matching Python version
- [ ] Add token usage tracking (input/output tokens)
- [ ] Implement conversation row persistence
- [ ] Add tool call metadata storage
- [ ] Complete `logs list` with all filter options:
  - [ ] `-l/--limit` count handling
  - [ ] `-t/--time` time-based filters  
  - [ ] `-s/--search` text search
  - [ ] `-u/--usage` token usage filters
  - [ ] `-r/--reverse` ordering
  - [ ] `-x/--export` export formats
  - [ ] `--xl` extended listing
- [ ] Add conversation summary generation
- [ ] Implement backup/restore functionality

**Validation:**
```bash
# Test logging persistence
cargo run -- prompt "test message" --log
cargo run -- logs list --count 1 --json
# Verify database schema matches Python version
sqlite3 ~/.config/io.datasette.llm/logs.db ".schema"
```

#### 1.2 Provider Metadata Enhancement

**Files to modify:**
- `crates/llm-core/src/providers/mod.rs` - Extend `StreamSink` trait
- `crates/llm-core/src/providers/openai.rs` - Add usage tracking
- `crates/llm-core/src/providers/anthropic.rs` - Add usage tracking
- `crates/llm-core/src/lib.rs` - Update `stream_prompt_internal`

**Tasks:**
- [ ] Extend `StreamSink` with metadata callbacks:
  ```rust
  fn handle_usage(&mut self, input_tokens: u32, output_tokens: u32) -> Result<()>;
  fn handle_tool_call(&mut self, tool: ToolCall) -> Result<()>;
  ```
- [ ] Implement token counting for OpenAI responses
- [ ] Implement token counting for Anthropic responses
- [ ] Add tool call detection and parsing
- [ ] Update logging to capture usage metadata
- [ ] Add cost calculation based on model pricing

**Validation:**
```bash
# Test usage tracking
LLM_OPENAI_API_KEY=test cargo run -- prompt "test" --debug
# Verify usage appears in logs
cargo run -- logs list --count 1 | grep "Token usage"
```

### Phase 2: Python Plugin Bridge (Priority: High)

#### 2.1 PyO3 Integration Foundation

**Files to create/modify:**
- `crates/llm-plugin-host/src/lib.rs` - Main plugin host
- `crates/llm-plugin-host/src/python.rs` - Python bridge
- `crates/llm-plugin-host/Cargo.toml` - Add PyO3 dependencies

**Dependencies to add:**
```toml
[dependencies]
pyo3 = { version = "0.20", features = ["extension-module"] }
pyo3-ffi = "0.20"
```

**Tasks:**
- [ ] Set up PyO3 Python interpreter initialization
- [ ] Implement plugin discovery system
- [ ] Create Python module loader
- [ ] Add pluggy hook compatibility layer
- [ ] Implement plugin manifest parsing
- [ ] Add error handling for Python exceptions

**External References:**
- https://github.com/PyO3/pyo3 - PyO3 documentation
- https://github.com/pytest-dev/pluggy - Plugin system reference

#### 2.2 Plugin Hook Implementation

**Files to modify:**
- `crates/llm-plugin-host/src/hooks.rs` - Hook definitions
- `crates/llm-plugin-host/src/models.rs` - Model registration

**Tasks:**
- [ ] Implement `register_models` hook
- [ ] Implement `register_commands` hook  
- [ ] Implement `register_tools` hook
- [ ] Add plugin lifecycle management
- [ ] Implement plugin configuration system
- [ ] Add plugin dependency resolution

**Validation:**
```bash
# Test with existing plugin
cargo run -- plugins --json
# Should show llm-markov plugin loaded
```

#### 2.3 Plugin Command Integration

**Files to modify:**
- `crates/llm-cli/src/main.rs` - Add plugin command handling
- `crates/llm-cli/src/commands/` - New plugin command modules

**Tasks:**
- [ ] Add dynamic command registration
- [ ] Implement plugin command execution
- [ ] Add plugin help text integration
- [ ] Implement plugin error handling
- [ ] Add plugin version compatibility checking

### Phase 3: Missing CLI Commands (Priority: Medium)

#### 3.1 Interactive Chat Implementation

**Files to create:**
- `crates/llm-cli/src/commands/chat.rs` - Chat command
- `crates/llm-cli/src/chat/` - Chat session management

**Tasks:**
- [ ] Implement interactive session UI with rustyline
- [ ] Add conversation history management
- [ ] Implement multi-turn conversation persistence
- [ ] Add chat-specific options (continuation, export)
- [ ] Implement chat session resumption
- [ ] Add chat-specific logging

**Validation:**
```bash
cargo run -- chat
# Test interactive conversation
# Test conversation resumption
cargo run -- chat --conversation-id <id>
```

#### 3.2 Aliases Command Implementation

**Files to create:**
- `crates/llm-cli/src/commands/aliases.rs` - Aliases command
- `crates/llm-core/src/aliases.rs` - Alias storage

**Tasks:**
- [ ] Implement alias storage in JSON format
- [ ] Add `aliases list` command
- [ ] Add `aliases set` command with validation
- [ ] Add `aliases remove` command
- [ ] Add `aliases path` command
- [ ] Implement alias resolution in model lookup

**Validation:**
```bash
cargo run -- aliases list
cargo run -- aliases set gpt4 openai/gpt-4
cargo run -- prompt "test" --model gpt4
```

#### 3.3 Embeddings System Implementation

**Files to modify:**
- `crates/llm-embeddings/src/lib.rs` - Core embeddings
- `crates/llm-cli/src/commands/embeddings.rs` - Embeddings commands

**Tasks:**
- [ ] Implement embeddings database schema
- [ ] Add `embed` command for single text embedding
- [ ] Add `embed-multi` for batch processing
- [ ] Add `collections` command for collection management
- [ ] Add `embed-models` command for model listing
- [ ] Add `similar` command for similarity search
- [ ] Implement vector storage and retrieval
- [ ] Add cosine similarity calculation

**External References:**
- https://github.com/openai/openai-python - OpenAI embeddings API reference

### Phase 4: Template and Fragment System (Priority: Medium)

#### 4.1 Template Implementation

**Files to create:**
- `crates/llm-cli/src/commands/templates.rs` - Templates command
- `crates/llm-core/src/templates.rs` - Template engine

**Tasks:**
- [ ] Implement Jinja2-like template engine
- [ ] Add template discovery system
- [ ] Add `templates list` command
- [ ] Add `templates show` command
- [ ] Add `templates edit` command
- [ ] Add `templates path` command
- [ ] Implement template variable substitution
- [ ] Add template inheritance support

#### 4.2 Fragment Implementation

**Files to create:**
- `crates/llm-cli/src/commands/fragments.rs` - Fragments command
- `crates/llm-core/src/fragments.rs` - Fragment storage

**Tasks:**
- [ ] Implement fragment storage system
- [ ] Add `fragments list` command
- [ ] Add `fragments add` command
- [ ] Add `fragments get` command
- [ ] Add `fragments remove` command
- [ ] Implement fragment inclusion in prompts
- [ ] Add fragment versioning

### Phase 5: Advanced Features (Priority: Low)

#### 5.1 Tool System Implementation

**Files to create:**
- `crates/llm-cli/src/commands/tools.rs` - Tools command
- `crates/llm-core/src/tools.rs` - Tool execution framework

**Tasks:**
- [ ] Implement tool definition schema
- [ ] Add `tools list` command
- [ ] Add `tools --functions` export
- [ ] Implement tool execution sandbox
- [ ] Add tool approval workflow
- [ ] Implement tool result handling
- [ ] Add custom tool registration

#### 5.2 Schema System Implementation

**Files to create:**
- `crates/llm-cli/src/commands/schemas.rs` - Schemas command
- `crates/llm-core/src/schemas.rs` - Schema validation

**Tasks:**
- [ ] Implement JSON schema validation
- [ ] Add `schemas list` command
- [ ] Add `schemas show` command
- [ ] Add `schemas validate` command
- [ ] Implement structured extraction
- [ ] Add schema-based response parsing

#### 5.3 Package Management

**Files to create:**
- `crates/llm-cli/src/commands/install.rs` - Install command
- `crates/llm-cli/src/commands/uninstall.rs` - Uninstall command

**Tasks:**
- [ ] Implement pip wrapper for package installation
- [ ] Add plugin dependency management
- [ ] Add version constraint handling
- [ ] Implement plugin update mechanism
- [ ] Add plugin signing verification

## Technical Specifications

### Data Models

#### Log Entry Schema
```sql
CREATE TABLE log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    resolved_model TEXT,
    prompt TEXT,
    system TEXT,
    prompt_json TEXT,
    options_json TEXT,
    response TEXT,
    response_json TEXT,
    conversation_id TEXT,
    conversation_name TEXT,
    conversation_model TEXT,
    duration_ms INTEGER,
    input_tokens INTEGER,
    output_tokens INTEGER,
    token_details TEXT,
    timestamp_utc TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);

CREATE TABLE conversation (
    id TEXT PRIMARY KEY,
    name TEXT,
    model TEXT,
    created_utc TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
    updated_utc TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);
```

#### Key Storage Format
```json
{
  "// Note": "This file stores secret API credentials. Do not share!",
  "openai": "sk-...",
  "anthropic": "sk-ant-...",
  "custom-alias": "custom-key-value"
}
```

#### Model Configuration
```rust
pub struct ModelInfo {
    pub name: String,
    pub provider: String,
    pub description: String,
    pub is_default: bool,
    pub aliases: Vec<String>,
}
```

### API Endpoints

#### OpenAI Compatible
- Base URL: `https://api.openai.com/v1`
- Chat endpoint: `/chat/completions`
- Embeddings endpoint: `/embeddings`

#### Anthropic
- Base URL: `https://api.anthropic.com/v1`
- Messages endpoint: `/messages`

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `LLM_USER_PATH` | User data directory override | `~/.config/io.datasette.llm` |
| `LLM_DEFAULT_MODEL` | Default model override | `openai/gpt-4o-mini` |
| `OPENAI_API_KEY` | OpenAI API key | None |
| `ANTHROPIC_API_KEY` | Anthropic API key | None |
| `LLM_CMD_AUTO_ACCEPT` | Auto-accept generated commands | `false` |
| `LLM_PROMPT_STUB` | Enable test stub mode | `false` |

## Testing Strategy

### Unit Tests
- Provider implementations
- Model resolution logic
- Key storage and retrieval
- Template rendering
- Schema validation

### Integration Tests
- CLI command execution
- Plugin loading and execution
- Database operations
- Streaming functionality
- Error handling

### Performance Tests
- Startup time benchmarks
- Memory usage profiling
- Concurrent request handling
- Large file processing

### Compatibility Tests
- Python CLI parity verification
- Database schema compatibility
- Configuration file compatibility
- Plugin API compatibility

## Validation Criteria

### Phase 1 Completion
- [ ] All logging filters working correctly
- [ ] Token usage accurately tracked
- [ ] Database schema matches Python version
- [ ] Performance meets or exceeds Python version

### Phase 2 Completion  
- [ ] At least one Python plugin loads successfully
- [ ] Plugin commands execute correctly
- [ ] Plugin models appear in model list
- [ ] Error handling robust for malformed plugins

### Phase 3 Completion
- [ ] All missing CLI commands implemented
- [ ] Interactive chat functional
- [ ] Embeddings system working
- [ ] Aliases system operational

### Phase 4 Completion
- [ ] Template system functional
- [ ] Fragment system operational
- [ ] Both integrate with prompt execution

### Phase 5 Completion
- [ ] Tool system functional
- [ ] Schema validation working
- [ ] Package management operational
- [ ] Full CLI parity achieved

## External Dependencies

### Rust Crates
- `clap` - CLI argument parsing
- `serde` - Serialization/deserialization
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `sqlx` - Database operations
- `pyo3` - Python integration
- `rustyline` - Interactive input
- `anyhow` - Error handling

### Python References
- https://github.com/simonw/llm - Original Python implementation
- https://github.com/simonw/llm-embeddings - Embeddings plugin
- https://github.com/simonw/llm-markov - Example plugin

### API Documentation
- https://platform.openai.com/docs/api-reference - OpenAI API
- https://docs.anthropic.com/claude/reference - Anthropic API

## Risk Mitigation

### Technical Risks
- **Plugin Bridge Complexity**: Start with simple plugin loading, expand gradually
- **Database Migration**: Implement schema versioning and migration scripts
- **Performance**: Profile early and optimize hot paths

### Compatibility Risks
- **CLI Parity**: Maintain comprehensive test suite comparing outputs
- **Configuration**: Ensure backward compatibility with existing config files
- **Plugin Ecosystem**: Provide migration guide for plugin authors

### Project Risks
- **Scope Creep**: Focus on core parity first, advanced features later
- **Resource Allocation**: Prioritize high-impact features
- **Documentation**: Update docs continuously with implementation

## Success Metrics

### Functional Metrics
- 100% CLI command parity with Python version
- 90%+ plugin compatibility
- Sub-second startup time
- Memory usage < 50MB for typical operations

### Quality Metrics
- 95%+ test coverage
- Zero critical security vulnerabilities
- Performance equal to or better than Python version
- Comprehensive documentation

### Adoption Metrics
- Seamless migration from Python version
- Active plugin ecosystem
- Community contributions
- Regular releases

---

**Next Steps:**
1. Begin Phase 1.1 - SQLite logging enhancement
2. Set up automated testing pipeline
3. Create migration guide for existing users
4. Establish release cadence

This plan will be updated weekly with progress and any discovered blockers or scope changes.