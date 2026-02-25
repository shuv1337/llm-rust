# Test Fixtures

This directory contains test fixtures for compatibility testing between the Rust
and Python implementations of the LLM CLI.

## Databases

### upstream_logs.db
A SQLite database created with the upstream Python schema, containing:
- Sample conversations and responses with integer IDs (pre-ULID migration)
- FTS5 full-text search table
- Sample attachments, schemas, tools, and fragments

### upstream_logs_migrated.db
Same data as upstream_logs.db but after ULID migration (string IDs).

### upstream_embeddings.db
An embeddings database with:
- Sample collections
- Sample embeddings with content hashes

## Config Files

### keys.json
Sample keys file with the upstream warning note format.

### aliases.json
Sample model aliases in upstream format.

### model_options.json
Sample model options in upstream format.

### default_model.txt / default-model.txt
Default model configuration (both naming conventions for fallback testing).

## Regenerating Fixtures

To regenerate these fixtures using the upstream Python CLI:

```bash
# Install upstream llm
pip install llm

# Set user path to a temp directory
export LLM_USER_PATH=/tmp/llm-fixtures

# Create some logs
llm -m gpt-4o-mini "Hello world" --system "Be helpful"
llm -c "Follow up question"
llm --conversation test-conv "New conversation" 

# Create aliases
llm aliases set mymodel gpt-4o

# Copy fixtures
cp $LLM_USER_PATH/logs.db tests/fixtures/upstream_logs.db
cp $LLM_USER_PATH/aliases.json tests/fixtures/aliases.json
```

For the Rust tests, we generate synthetic fixtures that match the upstream schema
without requiring the Python CLI to be installed.
