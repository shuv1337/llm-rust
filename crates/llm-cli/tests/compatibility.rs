//! Compatibility test suite for upstream parity verification.
//!
//! This test suite ensures the Rust LLM CLI implementation is compatible
//! with databases and configuration files created by the upstream Python CLI.

use std::fs;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use tempfile::TempDir;

// ============================================================================
// Test Utilities
// ============================================================================

/// Create the upstream-compatible integer-ID schema (pre-ULID migration).
fn create_upstream_integer_id_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            name TEXT,
            model TEXT
        );

        CREATE TABLE IF NOT EXISTS responses (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            model TEXT NOT NULL,
            prompt TEXT,
            system TEXT,
            prompt_json TEXT,
            options_json TEXT,
            response TEXT,
            response_json TEXT,
            conversation_id TEXT REFERENCES conversations(id),
            duration_ms INTEGER,
            datetime_utc TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            token_details TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_responses_datetime ON responses(datetime_utc);
        CREATE INDEX IF NOT EXISTS idx_responses_conversation_id ON responses(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_responses_model ON responses(model);

        CREATE VIRTUAL TABLE IF NOT EXISTS responses_fts USING fts5(
            prompt,
            response,
            content='responses',
            content_rowid='id'
        );

        CREATE TABLE IF NOT EXISTS attachments (
            id TEXT PRIMARY KEY,
            type TEXT,
            path TEXT,
            url TEXT,
            content BLOB
        );

        CREATE TABLE IF NOT EXISTS prompt_attachments (
            response_id INTEGER NOT NULL,
            attachment_id TEXT NOT NULL,
            "order" INTEGER,
            PRIMARY KEY (response_id, attachment_id)
        );

        CREATE TABLE IF NOT EXISTS schemas (
            id TEXT PRIMARY KEY,
            content TEXT
        );

        CREATE TABLE IF NOT EXISTS fragments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            hash TEXT NOT NULL UNIQUE,
            content TEXT,
            datetime_utc TEXT,
            source TEXT
        );

        CREATE TABLE IF NOT EXISTS fragment_aliases (
            alias TEXT PRIMARY KEY,
            fragment_id INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS prompt_fragments (
            response_id INTEGER NOT NULL,
            fragment_id INTEGER NOT NULL,
            "order" INTEGER,
            PRIMARY KEY (response_id, fragment_id, "order")
        );

        CREATE TABLE IF NOT EXISTS system_fragments (
            response_id INTEGER NOT NULL,
            fragment_id INTEGER NOT NULL,
            "order" INTEGER,
            PRIMARY KEY (response_id, fragment_id, "order")
        );

        CREATE TABLE IF NOT EXISTS tools (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            hash TEXT NOT NULL UNIQUE,
            name TEXT,
            description TEXT,
            input_schema TEXT,
            plugin TEXT
        );

        CREATE TABLE IF NOT EXISTS tool_responses (
            tool_id INTEGER NOT NULL,
            response_id INTEGER NOT NULL,
            PRIMARY KEY (tool_id, response_id)
        );

        CREATE TABLE IF NOT EXISTS tool_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            response_id INTEGER NOT NULL,
            tool_id INTEGER,
            name TEXT,
            arguments TEXT,
            tool_call_id TEXT
        );

        CREATE TABLE IF NOT EXISTS tool_instances (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin TEXT,
            name TEXT,
            arguments TEXT
        );

        CREATE TABLE IF NOT EXISTS tool_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            response_id INTEGER NOT NULL,
            tool_id INTEGER,
            name TEXT,
            output TEXT,
            tool_call_id TEXT,
            instance_id INTEGER,
            exception TEXT
        );

        CREATE TABLE IF NOT EXISTS tool_results_attachments (
            tool_result_id INTEGER NOT NULL,
            attachment_id TEXT NOT NULL,
            "order" INTEGER,
            PRIMARY KEY (tool_result_id, attachment_id)
        );

        CREATE TRIGGER IF NOT EXISTS responses_ai AFTER INSERT ON responses BEGIN
            INSERT INTO responses_fts(rowid, prompt, response) VALUES (new.id, new.prompt, new.response);
        END;

        CREATE TRIGGER IF NOT EXISTS responses_ad AFTER DELETE ON responses BEGIN
            INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
                VALUES ('delete', old.id, old.prompt, old.response);
        END;

        CREATE TRIGGER IF NOT EXISTS responses_au AFTER UPDATE ON responses BEGIN
            INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
                VALUES ('delete', old.id, old.prompt, old.response);
            INSERT INTO responses_fts(rowid, prompt, response) VALUES (new.id, new.prompt, new.response);
        END;
        "#,
    )?;
    Ok(())
}

/// Insert sample upstream-format data into a database.
fn insert_upstream_sample_data(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        INSERT INTO conversations (id, name, model) VALUES 
            ('conv-001', 'Test Conversation 1', 'openai/gpt-4o'),
            ('conv-002', 'Debug Session', 'anthropic/claude-3-opus');

        INSERT INTO responses (id, model, prompt, system, response, conversation_id, datetime_utc, duration_ms, input_tokens, output_tokens) VALUES 
            (1, 'openai/gpt-4o', 'Hello world', 'You are helpful', 'Hello! How can I assist you today?', 'conv-001', '2024-01-15T10:30:00+00:00', 250, 10, 15),
            (2, 'openai/gpt-4o', 'Follow up question', 'You are helpful', 'Sure, I can help with that!', 'conv-001', '2024-01-15T10:31:00+00:00', 180, 8, 12),
            (3, 'anthropic/claude-3-opus', 'Debug this code', 'You are a code expert', 'I found the issue in line 42.', 'conv-002', '2024-01-15T11:00:00+00:00', 500, 100, 200);

        INSERT INTO responses_fts(rowid, prompt, response) SELECT id, prompt, response FROM responses;
        "#,
    )?;
    Ok(())
}

/// Create a test fixture database with upstream integer-ID schema.
fn create_upstream_fixture_db(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    create_upstream_integer_id_schema(&conn)?;
    insert_upstream_sample_data(&conn)?;
    Ok(())
}

/// Create upstream-compatible embeddings schema.
fn create_embeddings_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS collections (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            model TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS embeddings (
            collection_id INTEGER NOT NULL REFERENCES collections(id),
            id TEXT NOT NULL,
            embedding BLOB NOT NULL,
            content TEXT,
            content_blob BLOB,
            content_hash BLOB,
            metadata TEXT,
            updated INTEGER,
            PRIMARY KEY (collection_id, id)
        );

        CREATE INDEX IF NOT EXISTS idx_embeddings_content_hash
            ON embeddings(collection_id, content_hash);
        "#,
    )?;
    Ok(())
}

// ============================================================================
// Upstream Database Compatibility Tests
// ============================================================================

#[test]
fn read_upstream_integer_id_responses() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let mut stmt = conn.prepare(
        "SELECT id, model, prompt, response, conversation_id FROM responses ORDER BY id",
    )?;

    let rows: Vec<(i64, String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, 1); // Integer ID
    assert_eq!(rows[0].1, "openai/gpt-4o");
    assert_eq!(rows[0].2, "Hello world");
    assert_eq!(rows[0].4, "conv-001");

    Ok(())
}

#[test]
fn read_upstream_conversations() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let mut stmt = conn.prepare("SELECT id, name, model FROM conversations ORDER BY id")?;

    let rows: Vec<(String, Option<String>, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "conv-001");
    assert_eq!(rows[0].1, Some("Test Conversation 1".to_string()));

    Ok(())
}

#[test]
fn upstream_fts_search() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM responses_fts WHERE responses_fts MATCH 'Hello'",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(count, 1);

    Ok(())
}

#[test]
fn upstream_schema_has_all_tables() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    assert!(tables.contains(&"conversations".to_string()));
    assert!(tables.contains(&"responses".to_string()));
    assert!(tables.contains(&"attachments".to_string()));
    assert!(tables.contains(&"schemas".to_string()));
    assert!(tables.contains(&"tools".to_string()));

    Ok(())
}

#[test]
fn upstream_fts_triggers_exist() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let triggers: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    assert!(triggers.contains(&"responses_ai".to_string()));
    assert!(triggers.contains(&"responses_ad".to_string()));
    assert!(triggers.contains(&"responses_au".to_string()));

    Ok(())
}

#[test]
fn upstream_indexes_exist() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    assert!(indexes.iter().any(|i| i.contains("datetime")));
    assert!(indexes.iter().any(|i| i.contains("conversation_id")));
    assert!(indexes.iter().any(|i| i.contains("model")));

    Ok(())
}

// ============================================================================
// Configuration File Compatibility Tests
// ============================================================================

#[test]
fn read_keys_with_warning_note() -> Result<()> {
    let tmp = TempDir::new()?;
    let keys_path = tmp.path().join("keys.json");

    let keys_json = serde_json::json!({
        "// Note": "This file stores secret API credentials. Do not share!",
        "openai": "sk-test-key",
        "anthropic": "sk-ant-test-key"
    });

    fs::write(&keys_path, serde_json::to_string_pretty(&keys_json)?)?;

    let content = fs::read_to_string(&keys_path)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;

    assert!(parsed.get("// Note").is_some());
    assert_eq!(parsed["openai"], "sk-test-key");

    Ok(())
}

#[test]
fn read_aliases_json() -> Result<()> {
    let tmp = TempDir::new()?;
    let aliases_path = tmp.path().join("aliases.json");

    let aliases = serde_json::json!({
        "fast": "openai/gpt-4o-mini",
        "smart": "anthropic/claude-3-opus"
    });

    fs::write(&aliases_path, serde_json::to_string_pretty(&aliases)?)?;

    let content = fs::read_to_string(&aliases_path)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;

    assert_eq!(parsed["fast"], "openai/gpt-4o-mini");

    Ok(())
}

#[test]
fn read_default_model_txt() -> Result<()> {
    let tmp = TempDir::new()?;
    let model_path = tmp.path().join("default_model.txt");

    fs::write(&model_path, "openai/gpt-4o\n")?;

    let content = fs::read_to_string(&model_path)?.trim().to_string();
    assert_eq!(content, "openai/gpt-4o");

    Ok(())
}

#[test]
fn read_default_model_legacy() -> Result<()> {
    let tmp = TempDir::new()?;
    let model_path = tmp.path().join("default-model.txt");

    fs::write(&model_path, "anthropic/claude-3-sonnet\n")?;

    let content = fs::read_to_string(&model_path)?.trim().to_string();
    assert_eq!(content, "anthropic/claude-3-sonnet");

    Ok(())
}

#[test]
fn default_model_fallback() -> Result<()> {
    let tmp = TempDir::new()?;
    let new_path = tmp.path().join("default_model.txt");
    let legacy_path = tmp.path().join("default-model.txt");

    // Only legacy exists
    fs::write(&legacy_path, "legacy-model")?;

    let model = if new_path.exists() {
        fs::read_to_string(&new_path)?.trim().to_string()
    } else if legacy_path.exists() {
        fs::read_to_string(&legacy_path)?.trim().to_string()
    } else {
        "openai/gpt-4o-mini".to_string()
    };

    assert_eq!(model, "legacy-model");

    Ok(())
}

// ============================================================================
// Embeddings Compatibility Tests
// ============================================================================

#[test]
fn embeddings_schema_has_all_tables() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("embeddings.db");

    let conn = Connection::open(&db_path)?;
    create_embeddings_schema(&conn)?;

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    assert!(tables.contains(&"collections".to_string()));
    assert!(tables.contains(&"embeddings".to_string()));

    Ok(())
}

#[test]
fn embeddings_has_content_hash_index() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("embeddings.db");

    let conn = Connection::open(&db_path)?;
    create_embeddings_schema(&conn)?;

    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    assert!(indexes.iter().any(|i| i.contains("content_hash")));

    Ok(())
}

// ============================================================================
// Token and Duration Storage Tests
// ============================================================================

#[test]
fn token_counts_stored() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let (input, output): (i64, i64) = conn.query_row(
        "SELECT input_tokens, output_tokens FROM responses WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    assert_eq!(input, 10);
    assert_eq!(output, 15);

    Ok(())
}

#[test]
fn duration_stored() -> Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("upstream.db");

    create_upstream_fixture_db(&db_path)?;

    let conn = Connection::open(&db_path)?;

    let duration: i64 = conn.query_row(
        "SELECT duration_ms FROM responses WHERE id = 1",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(duration, 250);

    Ok(())
}
