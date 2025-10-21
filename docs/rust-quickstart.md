# Rust Rewrite Quick Start

The Rust workspace lives in the `rust/` directory and currently ships a stub CLI (`llm-cli`) that mirrors a few Python commands. You can experiment with it as follows:

1. **Build the workspace**
   ```bash
   cd rust
   cargo build
   ```

2. **Re-run the existing integration tests**
   ```bash
   cargo test
   ```

3. **Build a release binary**
   ```bash
   cargo build --release
   ```

4. **Install the binary for local use**
   ```bash
   cargo install --path crates/llm-cli --force
   ```
   This places an `llm-cli` executable in `~/.cargo/bin`. Ensure that directory is on your `PATH`, then invoke the CLI directly with `llm-cli --help`.

5. **Execute a prompt**
   ```bash
   cargo run -- "Hello from Rust"
   ```
   Output comes from the stub core for now.

6. **Inspect available models**
   ```bash
   cargo run -- models list
   # or, after installing:
   llm-cli models list
   ```
   Set the default model (stored in your user dir) with:
   ```bash
   llm-cli models default openai/gpt-4o-mini
   ```
   Models support the same aliases as the Python CLI (for example `llm-cli models default 4o`).

7. **List detected plugins (stubbed)**
   ```bash
   cargo run -- plugins
   ```

8. **Manage API keys**
   ```bash
   cargo run -- keys set openai --value YOUR_API_KEY
   cargo run -- keys list --json
   cargo run -- keys get openai
   ```
   Keys are stored in the same user directory as the Python CLI (`~/.config/io.datasette.llm/keys.json`, overridable by `LLM_USER_PATH`).

9. **Inspect logs database path**
   ```bash
   cargo run -- logs path
   ```

10. **Override configuration directory**
   ```bash
   cargo run -- keys list --json --env LLM_USER_PATH=/tmp/llm
   ```

> Tip: `cargo run -- --help` prints the current command tree so you can track progress as more features migrate from Python.

Until the rewrite reaches parity, you can continue to use the Python CLI (`pipx install llm`, `llm --help`) alongside this workspace.
