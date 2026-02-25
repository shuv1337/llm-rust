# LLM - Rust Edition

A fast, memory-safe reimplementation of the LLM CLI tool in Rust.

## What this is

This is a Rust port of the Python `llm` CLI tool. It provides:

- Fast performance with Rust's zero-cost abstractions
- Memory safety without garbage collection
- Compatibility with existing workflows
- Support for multiple LLM providers

## Features

### Currently implemented

- Multi-provider support (OpenAI, Anthropic, OpenAI-compatible endpoints)
- Streaming responses with real-time token delivery
- Key management for API credentials
- Conversation tracking
- Model catalog with aliases
- Shell command generation via `cmd` subcommand
- Logging and usage analytics

### In progress / next up

- Python plugin bridge for existing plugins
- Expanded third-party plugin command/model registration parity
- Broader provider coverage and model-catalog refresh workflows
- Continued tool-calling and structured-output parity hardening

## Getting Started

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs))
- API keys for your chosen providers

### Installation

```bash
git clone https://github.com/simonw/llm
cd llm-rust
cargo build --release
cargo run -- --help
```

## Usage Examples

### Basic prompts

```bash
# Ask a question
cargo run -- "What is the meaning of life?"

# Use a specific model
cargo run -- --model gpt-4 "Explain quantum computing"

# Disable streaming
cargo run -- --no-stream "Tell me a joke"
```

### Key management

```bash
# Set your OpenAI key
cargo run -- keys set openai --value sk-...

# List stored keys
cargo run -- keys list

# Show key storage location
cargo run -- keys path
```

### Command generation

```bash
# Generate shell commands
cargo run -- cmd "find all TypeScript files modified today"

# Auto-execute in non-interactive mode
LLM_CMD_AUTO_ACCEPT=1 cargo run -- cmd "show git status"
```

### Model management

```bash
# List available models
cargo run -- models list

# Set default model
cargo run -- models default gpt-4o-mini

# Check current default
cargo run -- models default
```

### Logging

```bash
# View recent prompts
cargo run -- logs list

# Export logs as JSON
cargo run -- logs list --json --count 10

# Disable logging globally
cargo run -- logs off
```

## Architecture

The project is organized as a Cargo workspace:

```
llm-rust/
├── crates/llm-core         - Core library with provider abstractions
├── crates/llm-cli          - Command-line interface
├── crates/llm-plugin-host  - Python plugin bridge
└── crates/llm-embeddings   - Vector embeddings support
```

## Configuration

### Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `LLM_USER_PATH` | Config directory | `~/.config/io.datasette.llm` |
| `OPENAI_API_KEY` | OpenAI API key | - |
| `ANTHROPIC_API_KEY` | Anthropic API key | - |
| `LLM_DEFAULT_MODEL` | Default model | `openai/gpt-4o-mini` |

## Contributing

Contributions are welcome. To contribute:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests (`cargo test`)
5. Push and open a pull request

### Development setup

```bash
# Run tests
cargo test

# Run with debug logging
cargo run -- --debug prompt "test"

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Documentation

For more detailed information:

- [Implementation Plan](CONTEXT/PLAN-rust-llm-rewrite-2025-10-24.md)
- [CLI Parity Matrix](docs/cli-parity-matrix.md)
- [Quick Start Guide](docs/rust-quickstart.md)
- [Python LLM Docs](https://llm.datasette.io/)

## Acknowledgments

- [Simon Willison](https://github.com/simonw) for the original Python LLM
- The Rust community for tools and libraries
- OpenAI & Anthropic for their APIs

## License

Licensed under Apache 2.0.

## Status

The Rust CLI is usable today for core workflows (prompting, chat, models, logs, embeddings, templates, and command generation). Remaining work is focused on plugin bridge parity and long-tail compatibility gaps with upstream Python `llm`.

For the most accurate parity status, see:

- `docs/cli-parity-matrix.md`
- `PLAN-llm-upstream-feature-parity-roadmap.md`