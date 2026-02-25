# Rust Rewrite Quick Start

The Rust workspace is at the repository root and provides a working CLI implementation in `crates/llm-cli`.

1. **Build the workspace**
   ```bash
   cargo build
   ```

2. **Run tests**
   ```bash
   cargo test
   ```

3. **Build a release binary**
   ```bash
   cargo build --release
   ```

4. **Install the CLI for local use**
   ```bash
   cargo install --path crates/llm-cli --force
   ```
   This installs `llm-cli` to `~/.cargo/bin`.

   Optional convenience alias:
   ```bash
   ln -sf ~/.cargo/bin/llm-cli ~/.cargo/bin/llm
   ```

5. **Show help**
   ```bash
   llm-cli --help
   # or, if you created the alias above:
   llm --help
   ```

6. **Execute a prompt**
   ```bash
   cargo run -- "Hello from Rust"
   # or after install:
   llm-cli "Hello from Rust"
   ```

7. **Inspect models**
   ```bash
   llm-cli models list
   llm-cli models default openai/gpt-4o-mini
   llm-cli models default
   ```

8. **List plugins**
   ```bash
   llm-cli plugins
   ```

9. **Manage API keys**
   ```bash
   llm-cli keys set openai --value YOUR_API_KEY
   llm-cli keys list --json
   llm-cli keys get openai
   ```

10. **Inspect logs**
   ```bash
   llm-cli logs path
   llm-cli logs list --count 3
   llm-cli logs list --json --count 3
   ```

11. **Override configuration directory**
   ```bash
   LLM_USER_PATH=/tmp/llm llm-cli keys list --json
   ```

> Tip: `cargo run -- --help` prints the current command tree.

If you also use the Python CLI, both tools can coexist because they share compatible config and logs locations by default.
