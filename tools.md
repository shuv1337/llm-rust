# Available Tools

## File Operations
- **read** - Read file contents with optional line offset/limit
- **write** - Write/overwrite files (requires reading first for existing files)
- **edit** - Make exact string replacements in files
- **list** - List files and directories at a path

## Search & Navigation
- **grep** - Search file contents using regex patterns
- **glob** - Find files matching glob patterns (e.g., `**/*.rs`)
- **gh_grep_searchGitHub** - Search real-world code examples from public GitHub repos

## Execution & Development
- **bash** - Execute bash commands with timeout support
  - Use for `cargo build`, `cargo test`, `cargo run`, etc.
  - Supports parallel execution with `;` or `&&` separators
  - Required timeout parameter (default 120s, max 600s)
- **task** - Launch specialized agents:
  - `general` - Research, code search, multi-step tasks
  - `review` - Code quality and best practices review

## Task Management
- **todoread** - Read current todo list
- **todowrite** - Create/update todo list with task tracking
  - Use for multi-step tasks (3+ steps)
  - Track status: pending, in_progress, completed, cancelled

## Web
- **webfetch** - Fetch and analyze web content (HTML to markdown)

## Common Patterns for This Project
- Build: `cargo build` or `cargo build --release`
- Test: `cargo test` (runs unit + integration tests)
- Run CLI: `cargo run -- <args>` (e.g., `cargo run -- --help`)
- Search code: Use `grep` with `*.rs` pattern or `glob` for `**/*.rs`
- Multi-step work: Use `todowrite` to track progress
