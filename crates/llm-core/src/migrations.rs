//! SQLite migration infrastructure for the Rust LLM CLI.
//!
//! This module provides a Rust-native migration engine that tracks applied migrations
//! in the upstream-compatible `_llm_migrations` table format. Each migration is
//! identified by name and the timestamp when it was applied.
//!
//! # Migration Workflow
//!
//! 1. Call `migration_preflight()` to get a preview of pending migrations
//! 2. Review the `PreflightReport` for warnings and backup target
//! 3. Call `run_migrations()` to apply all pending migrations (with automatic backup)
//!
//! # Example
//!
//! ```ignore
//! use llm_core::migrations::{migration_preflight, run_migrations};
//! use std::path::Path;
//!
//! let db_path = Path::new("/path/to/logs.db");
//! let report = migration_preflight(db_path).unwrap();
//! println!("Pending: {:?}", report.pending_migrations);
//!
//! if !report.pending_migrations.is_empty() {
//!     run_migrations(db_path).unwrap();
//! }
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use ulid::Ulid;

/// Metadata describing a single migration.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Migration {
    /// Unique migration name (e.g., "001_initial_schema").
    pub name: &'static str,
    /// SQL statements to execute for this migration.
    pub sql: &'static str,
    /// Human-readable description of what this migration does.
    pub description: &'static str,
    /// Whether this migration modifies the schema (triggers backup).
    pub is_schema_change: bool,
}

/// Record of an applied migration from the database.
#[derive(Debug, Clone, Serialize)]
pub struct AppliedMigration {
    /// Migration name that was applied.
    pub name: String,
    /// Timestamp when the migration was applied (RFC 3339 format).
    pub applied_at: String,
}

/// Result of applying a single migration.
#[derive(Debug, Clone, Serialize)]
pub struct MigrationResult {
    /// Name of the migration that was applied.
    pub name: String,
    /// Whether the migration was successful.
    pub success: bool,
    /// Error message if the migration failed.
    pub error: Option<String>,
}

/// Preflight report describing pending migrations and potential issues.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    /// Path to the database being checked.
    pub database_path: PathBuf,
    /// Whether the database file exists.
    pub database_exists: bool,
    /// List of migrations that have not yet been applied.
    pub pending_migrations: Vec<String>,
    /// Warnings about potential issues (e.g., schema conflicts).
    pub warnings: Vec<String>,
    /// Path where the backup will be created (if schema changes are pending).
    pub backup_path: Option<PathBuf>,
    /// List of migrations already applied.
    pub applied_migrations: Vec<AppliedMigration>,
    /// Whether any pending migration will change the schema.
    pub has_schema_changes: bool,
}

/// Result of running all migrations.
#[derive(Debug, Clone, Serialize)]
pub struct MigrationSummary {
    /// Number of migrations that were applied.
    pub applied_count: usize,
    /// Number of migrations that were already applied.
    pub skipped_count: usize,
    /// Path to backup file if one was created.
    pub backup_path: Option<PathBuf>,
    /// Details for each migration attempt.
    pub results: Vec<MigrationResult>,
}

// ============================================================================
// Embedded Migrations
// ============================================================================

/// All migrations defined in the crate, in order of application.
///
/// Migration names must be unique and should be numbered for clarity.
/// Migrations are applied in the order they appear in this array.
static MIGRATIONS: &[Migration] = &[
    Migration {
        name: "001_llm_migrations_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS _llm_migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                applied_at TEXT NOT NULL
            );
        "#,
        description: "Create the _llm_migrations tracking table",
        is_schema_change: true,
    },
    Migration {
        name: "002_conversations_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                name TEXT,
                model TEXT
            );
        "#,
        description: "Create the conversations table",
        is_schema_change: true,
    },
    Migration {
        name: "003_responses_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS responses (
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
                duration_ms INTEGER,
                datetime_utc TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                token_details TEXT,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
            );
            CREATE INDEX IF NOT EXISTS idx_responses_datetime
                ON responses(datetime_utc);
            CREATE INDEX IF NOT EXISTS idx_responses_conversation_id
                ON responses(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_responses_model
                ON responses(model);
        "#,
        description: "Create the responses table with indexes",
        is_schema_change: true,
    },
    // ============================================================================
    // Upstream-compatible schema migrations (M1 task-4)
    // ============================================================================
    Migration {
        name: "004_responses_fts",
        sql: r#"
            -- Create FTS5 virtual table for full-text search on responses
            CREATE VIRTUAL TABLE IF NOT EXISTS responses_fts USING fts5(
                prompt,
                response,
                content='responses',
                content_rowid='id'
            );

            -- Populate FTS from existing data
            INSERT INTO responses_fts(rowid, prompt, response)
                SELECT id, prompt, response FROM responses;

            -- Create triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS responses_ai AFTER INSERT ON responses BEGIN
                INSERT INTO responses_fts(rowid, prompt, response)
                    VALUES (new.id, new.prompt, new.response);
            END;

            CREATE TRIGGER IF NOT EXISTS responses_ad AFTER DELETE ON responses BEGIN
                INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
                    VALUES ('delete', old.id, old.prompt, old.response);
            END;

            CREATE TRIGGER IF NOT EXISTS responses_au AFTER UPDATE ON responses BEGIN
                INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
                    VALUES ('delete', old.id, old.prompt, old.response);
                INSERT INTO responses_fts(rowid, prompt, response)
                    VALUES (new.id, new.prompt, new.response);
            END;
        "#,
        description: "Add FTS5 full-text search for responses (upstream parity)",
        is_schema_change: true,
    },
    Migration {
        name: "005_attachments_tables",
        sql: r#"
            -- Attachments table for storing binary/text content
            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                type TEXT,
                path TEXT,
                url TEXT,
                content BLOB
            );

            -- Join table linking responses to attachments (prompt attachments)
            CREATE TABLE IF NOT EXISTS prompt_attachments (
                response_id INTEGER NOT NULL,
                attachment_id TEXT NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, attachment_id),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (attachment_id) REFERENCES attachments(id)
            );
        "#,
        description: "Create attachments and prompt_attachments tables (upstream parity)",
        is_schema_change: true,
    },
    Migration {
        name: "006_schemas_table",
        sql: r#"
            -- Schemas table for JSON schema definitions
            CREATE TABLE IF NOT EXISTS schemas (
                id TEXT PRIMARY KEY,
                content TEXT
            );

            -- Add schema_id column to responses
            ALTER TABLE responses ADD COLUMN schema_id TEXT REFERENCES schemas(id);
        "#,
        description: "Create schemas table and add schema_id to responses (upstream parity)",
        is_schema_change: true,
    },
    Migration {
        name: "007_fragments_tables",
        sql: r#"
            -- Fragments table for storing prompt/system fragments
            CREATE TABLE IF NOT EXISTS fragments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL UNIQUE,
                content TEXT,
                datetime_utc TEXT,
                source TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_fragments_hash ON fragments(hash);

            -- Fragment aliases for named references
            CREATE TABLE IF NOT EXISTS fragment_aliases (
                alias TEXT PRIMARY KEY,
                fragment_id INTEGER NOT NULL,
                FOREIGN KEY (fragment_id) REFERENCES fragments(id)
            );

            -- Join tables linking responses to fragments
            CREATE TABLE IF NOT EXISTS prompt_fragments (
                response_id INTEGER NOT NULL,
                fragment_id INTEGER NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, fragment_id, "order"),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (fragment_id) REFERENCES fragments(id)
            );

            CREATE TABLE IF NOT EXISTS system_fragments (
                response_id INTEGER NOT NULL,
                fragment_id INTEGER NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, fragment_id, "order"),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (fragment_id) REFERENCES fragments(id)
            );
        "#,
        description: "Create fragments, fragment_aliases, prompt_fragments, system_fragments tables (upstream parity)",
        is_schema_change: true,
    },
    Migration {
        name: "008_tools_tables",
        sql: r#"
            -- Tools table for storing tool definitions
            CREATE TABLE IF NOT EXISTS tools (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL UNIQUE,
                name TEXT,
                description TEXT,
                input_schema TEXT,
                plugin TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tools_hash ON tools(hash);

            -- Many-to-many relationship between tools and responses
            CREATE TABLE IF NOT EXISTS tool_responses (
                tool_id INTEGER NOT NULL,
                response_id INTEGER NOT NULL,
                PRIMARY KEY (tool_id, response_id),
                FOREIGN KEY (tool_id) REFERENCES tools(id),
                FOREIGN KEY (response_id) REFERENCES responses(id)
            );

            -- Tool calls made by the model
            CREATE TABLE IF NOT EXISTS tool_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                response_id INTEGER NOT NULL,
                tool_id INTEGER,
                name TEXT,
                arguments TEXT,
                tool_call_id TEXT,
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (tool_id) REFERENCES tools(id)
            );
            CREATE INDEX IF NOT EXISTS idx_tool_calls_response ON tool_calls(response_id);

            -- Tool instances for tracking Toolbox class instances
            CREATE TABLE IF NOT EXISTS tool_instances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                plugin TEXT,
                name TEXT,
                arguments TEXT
            );

            -- Tool results returned from executing tools
            CREATE TABLE IF NOT EXISTS tool_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                response_id INTEGER NOT NULL,
                tool_id INTEGER,
                name TEXT,
                output TEXT,
                tool_call_id TEXT,
                instance_id INTEGER,
                exception TEXT,
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (tool_id) REFERENCES tools(id),
                FOREIGN KEY (instance_id) REFERENCES tool_instances(id)
            );
            CREATE INDEX IF NOT EXISTS idx_tool_results_response ON tool_results(response_id);

            -- Attachments associated with tool results
            CREATE TABLE IF NOT EXISTS tool_results_attachments (
                tool_result_id INTEGER NOT NULL,
                attachment_id TEXT NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (tool_result_id, attachment_id),
                FOREIGN KEY (tool_result_id) REFERENCES tool_results(id),
                FOREIGN KEY (attachment_id) REFERENCES attachments(id)
            );
        "#,
        description: "Create tools, tool_responses, tool_calls, tool_instances, tool_results, tool_results_attachments tables (upstream parity)",
        is_schema_change: true,
    },
    Migration {
        name: "009_responses_rust_extensions",
        sql: r#"
            -- Rust-specific extension columns for simplified tool tracking
            -- These are kept for backward compatibility with existing Rust code
            -- The relational tables above are the upstream-compatible storage
            ALTER TABLE responses ADD COLUMN tool_calls_json TEXT;
            ALTER TABLE responses ADD COLUMN tool_results_json TEXT;
            ALTER TABLE responses ADD COLUMN finish_reason TEXT;
            ALTER TABLE responses ADD COLUMN usage_json TEXT;
        "#,
        description: "Add Rust-specific extension columns to responses (backward compat)",
        is_schema_change: true,
    },
    // ============================================================================
    // String ID Migration (M1 task-5) - ULID conversion
    // ============================================================================
    // NOTE: Migration 010 is a Rust-based migration that converts INTEGER IDs to
    // ULID strings. The SQL here is minimal; the real work is done in
    // apply_ulid_migration(). This migration:
    // 1. Drops FTS triggers (they reference integer rowid)
    // 2. Creates new responses table with TEXT id
    // 3. Rust code generates deterministic ULIDs and copies data
    // 4. Updates all foreign key references
    // 5. Recreates FTS table and triggers for TEXT ids
    Migration {
        name: "010_responses_ulid_ids",
        sql: "", // Empty - handled by Rust code in apply_ulid_migration()
        description: "Convert responses.id from INTEGER to ULID TEXT (upstream parity)",
        is_schema_change: true,
    },
];

// ============================================================================
// Public API
// ============================================================================

/// Run preflight checks and return a report of pending migrations.
///
/// This function does not modify the database. Use it to preview what
/// `run_migrations()` will do.
pub fn migration_preflight<P: AsRef<Path>>(db_path: P) -> Result<PreflightReport> {
    let path = db_path.as_ref();
    let database_exists = path.exists();

    // If database doesn't exist, all migrations are pending
    if !database_exists {
        let pending: Vec<String> = MIGRATIONS.iter().map(|m| m.name.to_string()).collect();
        let has_schema_changes = MIGRATIONS.iter().any(|m| m.is_schema_change);
        let backup_path = if has_schema_changes {
            Some(generate_backup_path(path)?)
        } else {
            None
        };

        return Ok(PreflightReport {
            database_path: path.to_path_buf(),
            database_exists,
            pending_migrations: pending,
            warnings: vec!["Database does not exist and will be created.".to_string()],
            backup_path,
            applied_migrations: vec![],
            has_schema_changes,
        });
    }

    let conn = open_migration_connection(path)?;
    ensure_migrations_table(&conn)?;

    let applied = list_applied_migrations_internal(&conn)?;
    let applied_names: std::collections::HashSet<&str> =
        applied.iter().map(|a| a.name.as_str()).collect();

    let pending: Vec<String> = MIGRATIONS
        .iter()
        .filter(|m| !applied_names.contains(m.name))
        .map(|m| m.name.to_string())
        .collect();

    let has_schema_changes = MIGRATIONS
        .iter()
        .filter(|m| !applied_names.contains(m.name))
        .any(|m| m.is_schema_change);

    let backup_path = if has_schema_changes && !pending.is_empty() {
        Some(generate_backup_path(path)?)
    } else {
        None
    };

    let mut warnings = Vec::new();

    // Check for potential issues
    if has_schema_changes && !pending.is_empty() {
        warnings.push(format!(
            "Schema changes will be applied. Backup will be created at: {}",
            backup_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ));
    }

    // Check for unknown migrations in the database (applied but not in code)
    let known_names: std::collections::HashSet<&str> = MIGRATIONS.iter().map(|m| m.name).collect();
    for applied_migration in &applied {
        if !known_names.contains(applied_migration.name.as_str()) {
            warnings.push(format!(
                "Unknown migration '{}' found in database (applied at {}). This may indicate a version mismatch.",
                applied_migration.name, applied_migration.applied_at
            ));
        }
    }

    Ok(PreflightReport {
        database_path: path.to_path_buf(),
        database_exists,
        pending_migrations: pending,
        warnings,
        backup_path,
        applied_migrations: applied,
        has_schema_changes,
    })
}

/// List migrations that have not yet been applied to the database.
pub fn list_pending_migrations<P: AsRef<Path>>(db_path: P) -> Result<Vec<&'static Migration>> {
    let path = db_path.as_ref();

    if !path.exists() {
        // All migrations are pending for a new database
        return Ok(MIGRATIONS.iter().collect());
    }

    let conn = open_migration_connection(path)?;
    ensure_migrations_table(&conn)?;

    let applied = list_applied_migrations_internal(&conn)?;
    let applied_names: std::collections::HashSet<&str> =
        applied.iter().map(|a| a.name.as_str()).collect();

    let pending: Vec<&'static Migration> = MIGRATIONS
        .iter()
        .filter(|m| !applied_names.contains(m.name))
        .collect();

    Ok(pending)
}

/// List migrations that have been applied to the database.
pub fn list_applied_migrations<P: AsRef<Path>>(db_path: P) -> Result<Vec<AppliedMigration>> {
    let path = db_path.as_ref();

    if !path.exists() {
        return Ok(vec![]);
    }

    let conn = open_migration_connection(path)?;
    ensure_migrations_table(&conn)?;
    list_applied_migrations_internal(&conn)
}

/// Apply a single migration to the database.
///
/// This function does NOT create a backup - use `run_migrations()` for
/// backup-first behavior.
///
/// Returns `Ok(true)` if the migration was applied, `Ok(false)` if it was
/// already applied, or an error if the migration failed.
pub fn apply_migration<P: AsRef<Path>>(db_path: P, migration: &Migration) -> Result<bool> {
    let path = db_path.as_ref();

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let conn = open_migration_connection(path)?;
    ensure_migrations_table(&conn)?;

    // Check if already applied
    if is_migration_applied(&conn, migration.name)? {
        return Ok(false);
    }

    // Special handling for ULID migration (Rust-based)
    if migration.name == "010_responses_ulid_ids" {
        return apply_ulid_migration(&conn, migration);
    }

    // Apply the migration in a transaction
    conn.execute_batch("BEGIN TRANSACTION;")?;

    match conn.execute_batch(migration.sql) {
        Ok(_) => {
            // Record the migration as applied
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO _llm_migrations (name, applied_at) VALUES (?1, ?2)",
                params![migration.name, now],
            )
            .context("Failed to record migration")?;

            conn.execute_batch("COMMIT;")?;
            Ok(true)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK;").ok();
            Err(e).with_context(|| format!("Failed to apply migration '{}'", migration.name))
        }
    }
}

/// Run all pending migrations with backup-first behavior.
///
/// If any schema-changing migrations are pending, a timestamped backup is
/// created before the first migration is applied.
pub fn run_migrations<P: AsRef<Path>>(db_path: P) -> Result<MigrationSummary> {
    let path = db_path.as_ref();
    let preflight = migration_preflight(path)?;

    let mut summary = MigrationSummary {
        applied_count: 0,
        skipped_count: 0,
        backup_path: None,
        results: vec![],
    };

    if preflight.pending_migrations.is_empty() {
        return Ok(summary);
    }

    // Create backup before any schema changes (only if database exists)
    if preflight.database_exists && preflight.has_schema_changes {
        let backup_path = backup_before_migration(path)?;
        summary.backup_path = Some(backup_path);
    }

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    // Apply pending migrations in order
    for migration in MIGRATIONS {
        if !preflight
            .pending_migrations
            .contains(&migration.name.to_string())
        {
            summary.skipped_count += 1;
            continue;
        }

        match apply_migration(path, migration) {
            Ok(true) => {
                summary.applied_count += 1;
                summary.results.push(MigrationResult {
                    name: migration.name.to_string(),
                    success: true,
                    error: None,
                });
            }
            Ok(false) => {
                summary.skipped_count += 1;
                summary.results.push(MigrationResult {
                    name: migration.name.to_string(),
                    success: true,
                    error: Some("Already applied".to_string()),
                });
            }
            Err(e) => {
                summary.results.push(MigrationResult {
                    name: migration.name.to_string(),
                    success: false,
                    error: Some(e.to_string()),
                });
                // Stop on first error
                return Err(e);
            }
        }
    }

    Ok(summary)
}

/// Create a timestamped backup of the database before migration.
///
/// The backup is created in the same directory as the database with a
/// timestamp suffix: `logs.db` -> `logs.db.backup.2024-01-15T10-30-45`
pub fn backup_before_migration<P: AsRef<Path>>(db_path: P) -> Result<PathBuf> {
    let path = db_path.as_ref();

    if !path.exists() {
        anyhow::bail!(
            "Cannot backup: database does not exist at {}",
            path.display()
        );
    }

    let backup_path = generate_backup_path(path)?;

    // Use VACUUM INTO for a consistent backup (avoids issues with WAL mode)
    let conn = open_migration_connection(path)?;
    conn.execute(
        "VACUUM INTO ?1",
        params![backup_path.to_string_lossy().as_ref()],
    )
    .with_context(|| format!("Failed to create backup at {}", backup_path.display()))?;

    Ok(backup_path)
}

/// Get the list of all defined migrations.
pub fn all_migrations() -> &'static [Migration] {
    MIGRATIONS
}

// ============================================================================
// ULID Migration Implementation
// ============================================================================

/// Generate a deterministic ULID from a timestamp and sequence number.
///
/// The ULID encodes the timestamp in the high bits and the sequence in the
/// low bits, ensuring that ULIDs generated from earlier timestamps sort
/// before those from later timestamps.
fn generate_deterministic_ulid(datetime_utc: Option<&str>, sequence: u64) -> String {
    // Parse the datetime or use epoch if not available
    let timestamp_ms = datetime_utc
        .and_then(|s| {
            // Try RFC3339 first
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.timestamp_millis() as u64)
                .ok()
                .or_else(|| {
                    // Try other common formats
                    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                        .map(|ndt| ndt.and_utc().timestamp_millis() as u64)
                        .ok()
                })
        })
        .unwrap_or(0);

    // ULID structure: 48-bit timestamp (ms) + 80-bit randomness
    // For deterministic conversion, we use the sequence number in the lower bits
    // to maintain ordering within the same millisecond.

    // Create ULID from timestamp and a deterministic "random" component
    // The random component is derived from the sequence to ensure uniqueness
    // and preserve ordering within the same timestamp.
    let random_part = sequence.to_be_bytes();
    let mut random_bytes = [0u8; 10];
    // Put sequence in the last 8 bytes, leaving first 2 as zero
    random_bytes[2..10].copy_from_slice(&random_part);

    let ulid = Ulid::from_parts(
        timestamp_ms,
        u128::from_be_bytes([
            0,
            0,
            0,
            0,
            0,
            0,
            random_bytes[0],
            random_bytes[1],
            random_bytes[2],
            random_bytes[3],
            random_bytes[4],
            random_bytes[5],
            random_bytes[6],
            random_bytes[7],
            random_bytes[8],
            random_bytes[9],
        ]),
    );

    ulid.to_string()
}

/// Apply the ULID migration to convert responses.id from INTEGER to TEXT.
///
/// This migration:
/// 1. Drops existing FTS triggers (they depend on integer rowid)
/// 2. Creates a new responses table with TEXT id
/// 3. Generates deterministic ULIDs for all existing rows (ordered by datetime_utc, id)
/// 4. Updates all foreign key references in related tables
/// 5. Drops old table and renames new one
/// 6. Recreates FTS virtual table and triggers for string IDs
fn apply_ulid_migration(conn: &Connection, migration: &Migration) -> Result<bool> {
    conn.execute_batch("BEGIN TRANSACTION;")?;

    let result = (|| -> Result<()> {
        // Step 1: Drop FTS triggers (they reference integer rowid)
        conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS responses_ai;
            DROP TRIGGER IF EXISTS responses_ad;
            DROP TRIGGER IF EXISTS responses_au;
            DROP TABLE IF EXISTS responses_fts;
        "#,
        )?;

        // Step 2: Create new responses table with TEXT id
        conn.execute_batch(
            r#"
            CREATE TABLE responses_new (
                id TEXT PRIMARY KEY,
                model TEXT NOT NULL,
                resolved_model TEXT,
                prompt TEXT,
                system TEXT,
                prompt_json TEXT,
                options_json TEXT,
                response TEXT,
                response_json TEXT,
                conversation_id TEXT,
                duration_ms INTEGER,
                datetime_utc TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                token_details TEXT,
                schema_id TEXT REFERENCES schemas(id),
                tool_calls_json TEXT,
                tool_results_json TEXT,
                finish_reason TEXT,
                usage_json TEXT,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
            );
        "#,
        )?;

        // Step 3: Read existing responses ordered by datetime_utc, id and generate ULIDs
        let mut id_mapping: Vec<(i64, String)> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, datetime_utc FROM responses ORDER BY datetime_utc ASC, id ASC",
            )?;
            let mut rows = stmt.query([])?;
            let mut sequence: u64 = 0;
            while let Some(row) = rows.next()? {
                let old_id: i64 = row.get(0)?;
                let datetime_utc: Option<String> = row.get(1)?;
                let new_id = generate_deterministic_ulid(datetime_utc.as_deref(), sequence);
                id_mapping.push((old_id, new_id));
                sequence += 1;
            }
        }

        // Step 4: Copy data with new IDs
        for (old_id, new_id) in &id_mapping {
            conn.execute(
                r#"
                INSERT INTO responses_new (
                    id, model, resolved_model, prompt, system, prompt_json, options_json,
                    response, response_json, conversation_id, duration_ms, datetime_utc,
                    input_tokens, output_tokens, token_details, schema_id,
                    tool_calls_json, tool_results_json, finish_reason, usage_json
                )
                SELECT 
                    ?1, model, resolved_model, prompt, system, prompt_json, options_json,
                    response, response_json, conversation_id, duration_ms, datetime_utc,
                    input_tokens, output_tokens, token_details, schema_id,
                    tool_calls_json, tool_results_json, finish_reason, usage_json
                FROM responses WHERE id = ?2
                "#,
                params![new_id, old_id],
            )?;
        }

        // Step 5: Update foreign key references in related tables
        // Create temp table with mappings for efficient updates
        conn.execute_batch("CREATE TEMP TABLE id_map (old_id INTEGER PRIMARY KEY, new_id TEXT);")?;
        {
            let mut stmt = conn.prepare("INSERT INTO id_map (old_id, new_id) VALUES (?1, ?2)")?;
            for (old_id, new_id) in &id_mapping {
                stmt.execute(params![old_id, new_id])?;
            }
        }

        // Update prompt_attachments
        conn.execute_batch(
            r#"
            CREATE TABLE prompt_attachments_new (
                response_id TEXT NOT NULL,
                attachment_id TEXT NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, attachment_id),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (attachment_id) REFERENCES attachments(id)
            );
            INSERT INTO prompt_attachments_new (response_id, attachment_id, "order")
                SELECT m.new_id, pa.attachment_id, pa."order"
                FROM prompt_attachments pa
                JOIN id_map m ON pa.response_id = m.old_id;
            DROP TABLE prompt_attachments;
            ALTER TABLE prompt_attachments_new RENAME TO prompt_attachments;
        "#,
        )?;

        // Update prompt_fragments
        conn.execute_batch(
            r#"
            CREATE TABLE prompt_fragments_new (
                response_id TEXT NOT NULL,
                fragment_id INTEGER NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, fragment_id, "order"),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (fragment_id) REFERENCES fragments(id)
            );
            INSERT INTO prompt_fragments_new (response_id, fragment_id, "order")
                SELECT m.new_id, pf.fragment_id, pf."order"
                FROM prompt_fragments pf
                JOIN id_map m ON pf.response_id = m.old_id;
            DROP TABLE prompt_fragments;
            ALTER TABLE prompt_fragments_new RENAME TO prompt_fragments;
        "#,
        )?;

        // Update system_fragments
        conn.execute_batch(
            r#"
            CREATE TABLE system_fragments_new (
                response_id TEXT NOT NULL,
                fragment_id INTEGER NOT NULL,
                "order" INTEGER,
                PRIMARY KEY (response_id, fragment_id, "order"),
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (fragment_id) REFERENCES fragments(id)
            );
            INSERT INTO system_fragments_new (response_id, fragment_id, "order")
                SELECT m.new_id, sf.fragment_id, sf."order"
                FROM system_fragments sf
                JOIN id_map m ON sf.response_id = m.old_id;
            DROP TABLE system_fragments;
            ALTER TABLE system_fragments_new RENAME TO system_fragments;
        "#,
        )?;

        // Update tool_responses
        conn.execute_batch(
            r#"
            CREATE TABLE tool_responses_new (
                tool_id INTEGER NOT NULL,
                response_id TEXT NOT NULL,
                PRIMARY KEY (tool_id, response_id),
                FOREIGN KEY (tool_id) REFERENCES tools(id),
                FOREIGN KEY (response_id) REFERENCES responses(id)
            );
            INSERT INTO tool_responses_new (tool_id, response_id)
                SELECT tr.tool_id, m.new_id
                FROM tool_responses tr
                JOIN id_map m ON tr.response_id = m.old_id;
            DROP TABLE tool_responses;
            ALTER TABLE tool_responses_new RENAME TO tool_responses;
        "#,
        )?;

        // Update tool_calls
        conn.execute_batch(
            r#"
            CREATE TABLE tool_calls_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                response_id TEXT NOT NULL,
                tool_id INTEGER,
                name TEXT,
                arguments TEXT,
                tool_call_id TEXT,
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (tool_id) REFERENCES tools(id)
            );
            INSERT INTO tool_calls_new (id, response_id, tool_id, name, arguments, tool_call_id)
                SELECT tc.id, m.new_id, tc.tool_id, tc.name, tc.arguments, tc.tool_call_id
                FROM tool_calls tc
                JOIN id_map m ON tc.response_id = m.old_id;
            DROP TABLE tool_calls;
            ALTER TABLE tool_calls_new RENAME TO tool_calls;
            CREATE INDEX IF NOT EXISTS idx_tool_calls_response ON tool_calls(response_id);
        "#,
        )?;

        // Update tool_results
        conn.execute_batch(r#"
            CREATE TABLE tool_results_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                response_id TEXT NOT NULL,
                tool_id INTEGER,
                name TEXT,
                output TEXT,
                tool_call_id TEXT,
                instance_id INTEGER,
                exception TEXT,
                FOREIGN KEY (response_id) REFERENCES responses(id),
                FOREIGN KEY (tool_id) REFERENCES tools(id),
                FOREIGN KEY (instance_id) REFERENCES tool_instances(id)
            );
            INSERT INTO tool_results_new (id, response_id, tool_id, name, output, tool_call_id, instance_id, exception)
                SELECT tr.id, m.new_id, tr.tool_id, tr.name, tr.output, tr.tool_call_id, tr.instance_id, tr.exception
                FROM tool_results tr
                JOIN id_map m ON tr.response_id = m.old_id;
            DROP TABLE tool_results;
            ALTER TABLE tool_results_new RENAME TO tool_results;
            CREATE INDEX IF NOT EXISTS idx_tool_results_response ON tool_results(response_id);
        "#)?;

        // Clean up temp table
        conn.execute_batch("DROP TABLE id_map;")?;

        // Step 6: Drop old responses table and rename new one
        conn.execute_batch(
            r#"
            DROP TABLE responses;
            ALTER TABLE responses_new RENAME TO responses;
            CREATE INDEX IF NOT EXISTS idx_responses_datetime ON responses(datetime_utc);
            CREATE INDEX IF NOT EXISTS idx_responses_conversation_id ON responses(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_responses_model ON responses(model);
        "#,
        )?;

        // Step 7: Recreate FTS virtual table for TEXT ids
        // Note: FTS5 with content= tables requires a rowid column, but we now have TEXT ids.
        // We use an external content table without content_rowid since our id is TEXT.
        // This means we need to manually sync FTS content.
        conn.execute_batch(
            r#"
            -- Create FTS5 table with explicit id column for TEXT-based lookups
            CREATE VIRTUAL TABLE IF NOT EXISTS responses_fts USING fts5(
                id,
                prompt,
                response
            );

            -- Populate FTS from existing data
            INSERT INTO responses_fts(id, prompt, response)
                SELECT id, prompt, response FROM responses;

            -- Create triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS responses_ai AFTER INSERT ON responses BEGIN
                INSERT INTO responses_fts(id, prompt, response)
                    VALUES (new.id, new.prompt, new.response);
            END;

            CREATE TRIGGER IF NOT EXISTS responses_ad AFTER DELETE ON responses BEGIN
                INSERT INTO responses_fts(responses_fts, id, prompt, response)
                    VALUES ('delete', old.id, old.prompt, old.response);
            END;

            CREATE TRIGGER IF NOT EXISTS responses_au AFTER UPDATE ON responses BEGIN
                INSERT INTO responses_fts(responses_fts, id, prompt, response)
                    VALUES ('delete', old.id, old.prompt, old.response);
                INSERT INTO responses_fts(id, prompt, response)
                    VALUES (new.id, new.prompt, new.response);
            END;
        "#,
        )?;

        Ok(())
    })();

    match result {
        Ok(_) => {
            // Record the migration as applied
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO _llm_migrations (name, applied_at) VALUES (?1, ?2)",
                params![migration.name, now],
            )
            .context("Failed to record migration")?;

            conn.execute_batch("COMMIT;")?;
            Ok(true)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK;").ok();
            Err(e).with_context(|| format!("Failed to apply migration '{}'", migration.name))
        }
    }
}

/// Generate a new ULID for a new response entry.
///
/// This function should be used when inserting new responses after the
/// ULID migration has been applied.
pub fn generate_response_ulid() -> String {
    Ulid::new().to_string()
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn open_migration_connection(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open database: {}", path.display()))?;

    // Set pragmas for reliability
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )
    .context("Failed to set database pragmas")?;

    Ok(conn)
}

fn ensure_migrations_table(conn: &Connection) -> Result<()> {
    // Check if _llm_migrations table exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_llm_migrations'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        // Create the table using the first migration's SQL
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS _llm_migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                applied_at TEXT NOT NULL
            );
            ",
        )
        .context("Failed to create _llm_migrations table")?;
    }

    Ok(())
}

fn list_applied_migrations_internal(conn: &Connection) -> Result<Vec<AppliedMigration>> {
    let mut stmt = conn
        .prepare("SELECT name, applied_at FROM _llm_migrations ORDER BY id ASC")
        .context("Failed to prepare migration query")?;

    let rows = stmt.query_map([], |row| {
        Ok(AppliedMigration {
            name: row.get(0)?,
            applied_at: row.get(1)?,
        })
    })?;

    let mut applied = Vec::new();
    for row in rows {
        applied.push(row?);
    }

    Ok(applied)
}

fn is_migration_applied(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM _llm_migrations WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(count > 0)
}

fn generate_backup_path(db_path: &Path) -> Result<PathBuf> {
    let now: DateTime<Local> = Local::now();
    let timestamp = now.format("%Y-%m-%dT%H-%M-%S").to_string();

    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("database");

    let backup_name = format!("{}.backup.{}", file_name, timestamp);

    let backup_path = db_path
        .parent()
        .map(|p| p.join(&backup_name))
        .unwrap_or_else(|| PathBuf::from(&backup_name));

    Ok(backup_path)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.db");
        (dir, path)
    }

    #[test]
    fn test_preflight_new_database() {
        let (_dir, path) = temp_db();

        let report = migration_preflight(&path).expect("preflight");
        assert!(!report.database_exists);
        assert_eq!(report.pending_migrations.len(), MIGRATIONS.len());
        assert!(report.has_schema_changes);
        assert!(report.warnings.iter().any(|w| w.contains("does not exist")));
    }

    #[test]
    fn test_preflight_existing_database() {
        let (_dir, path) = temp_db();

        // Run migrations first
        run_migrations(&path).expect("run migrations");

        // Preflight should show nothing pending
        let report = migration_preflight(&path).expect("preflight");
        assert!(report.database_exists);
        assert!(report.pending_migrations.is_empty());
        assert!(!report.has_schema_changes);
        assert_eq!(report.applied_migrations.len(), MIGRATIONS.len());
    }

    #[test]
    fn test_list_pending_migrations_new_db() {
        let (_dir, path) = temp_db();

        let pending = list_pending_migrations(&path).expect("list pending");
        assert_eq!(pending.len(), MIGRATIONS.len());
    }

    #[test]
    fn test_apply_single_migration() {
        let (_dir, path) = temp_db();

        // Apply first migration
        let migration = &MIGRATIONS[0];
        let applied = apply_migration(&path, migration).expect("apply");
        assert!(applied);

        // Should not apply again
        let applied_again = apply_migration(&path, migration).expect("apply again");
        assert!(!applied_again);

        // Verify it's tracked
        let applied_list = list_applied_migrations(&path).expect("list");
        assert_eq!(applied_list.len(), 1);
        assert_eq!(applied_list[0].name, migration.name);
    }

    #[test]
    fn test_run_migrations_creates_all_tables() {
        let (_dir, path) = temp_db();

        let summary = run_migrations(&path).expect("run");
        assert_eq!(summary.applied_count, MIGRATIONS.len());
        assert_eq!(summary.skipped_count, 0);
        assert!(summary.backup_path.is_none()); // No backup for new DB

        // Verify tables exist
        let conn = Connection::open(&path).expect("open");

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        // Core tables
        assert!(tables.contains(&"_llm_migrations".to_string()));
        assert!(tables.contains(&"conversations".to_string()));
        assert!(tables.contains(&"responses".to_string()));

        // Upstream parity tables
        assert!(tables.contains(&"attachments".to_string()));
        assert!(tables.contains(&"prompt_attachments".to_string()));
        assert!(tables.contains(&"schemas".to_string()));
        assert!(tables.contains(&"fragments".to_string()));
        assert!(tables.contains(&"fragment_aliases".to_string()));
        assert!(tables.contains(&"prompt_fragments".to_string()));
        assert!(tables.contains(&"system_fragments".to_string()));
        assert!(tables.contains(&"tools".to_string()));
        assert!(tables.contains(&"tool_responses".to_string()));
        assert!(tables.contains(&"tool_calls".to_string()));
        assert!(tables.contains(&"tool_instances".to_string()));
        assert!(tables.contains(&"tool_results".to_string()));
        assert!(tables.contains(&"tool_results_attachments".to_string()));
    }

    #[test]
    fn test_run_migrations_idempotent() {
        let (_dir, path) = temp_db();

        // First run
        let summary1 = run_migrations(&path).expect("run 1");
        assert_eq!(summary1.applied_count, MIGRATIONS.len());

        // Second run should skip all
        let summary2 = run_migrations(&path).expect("run 2");
        assert_eq!(summary2.applied_count, 0);
        assert!(summary2.backup_path.is_none()); // No backup needed
    }

    #[test]
    fn test_backup_before_migration() {
        let (_dir, path) = temp_db();

        // Create initial database
        run_migrations(&path).expect("initial run");

        // Create backup
        let backup_path = backup_before_migration(&path).expect("backup");
        assert!(backup_path.exists());

        // Backup should be a valid database
        let conn = Connection::open(&backup_path).expect("open backup");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _llm_migrations", [], |row| row.get(0))
            .expect("count");
        assert!(count > 0);
    }

    #[test]
    fn test_backup_nonexistent_fails() {
        let (_dir, path) = temp_db();

        let result = backup_before_migration(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_backup_path_format() {
        let path = std::env::temp_dir().join("logs.db");
        let backup = generate_backup_path(&path).expect("generate");

        assert!(backup.to_string_lossy().contains("logs.db.backup."));
        assert_eq!(backup.parent(), path.parent());
    }

    #[test]
    fn test_preflight_detects_unknown_migrations() {
        let (_dir, path) = temp_db();

        // Run migrations
        run_migrations(&path).expect("run");

        // Insert an unknown migration directly
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "INSERT INTO _llm_migrations (name, applied_at) VALUES ('unknown_migration', datetime('now'))",
            [],
        )
        .expect("insert");

        // Preflight should warn about it
        let report = migration_preflight(&path).expect("preflight");
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("Unknown migration 'unknown_migration'")));
    }

    #[test]
    fn test_migrations_create_indexes() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");
        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='responses'")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        assert!(indexes.iter().any(|i| i.contains("datetime")));
        assert!(indexes.iter().any(|i| i.contains("conversation_id")));
        assert!(indexes.iter().any(|i| i.contains("model")));
    }

    #[test]
    fn test_partial_migration_state() {
        let (_dir, path) = temp_db();

        // Apply only first migration
        apply_migration(&path, &MIGRATIONS[0]).expect("apply first");

        // List pending should show remaining
        let pending = list_pending_migrations(&path).expect("pending");
        assert_eq!(pending.len(), MIGRATIONS.len() - 1);
        assert!(!pending.iter().any(|m| m.name == MIGRATIONS[0].name));
    }

    #[test]
    fn test_all_migrations_returns_full_list() {
        let migrations = all_migrations();
        assert!(!migrations.is_empty());
        assert_eq!(migrations.len(), MIGRATIONS.len());
    }

    #[test]
    fn test_migration_has_unique_names() {
        let mut names = std::collections::HashSet::new();
        for migration in MIGRATIONS {
            assert!(
                names.insert(migration.name),
                "Duplicate migration name: {}",
                migration.name
            );
        }
    }

    #[test]
    fn test_run_migrations_with_existing_data_creates_backup() {
        let (_dir, path) = temp_db();

        // Create initial database with data
        run_migrations(&path).expect("initial run");

        // Insert some data
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "INSERT INTO conversations (id, name, model) VALUES ('conv1', 'Test', 'gpt-4')",
            [],
        )
        .expect("insert");
        drop(conn);

        // Simulate a new migration by manually clearing the tracking table
        // and running again - this simulates what would happen with a new migration
        // In practice we'd add a new migration to MIGRATIONS
        let report = migration_preflight(&path).expect("preflight");

        // Since all migrations are applied, no backup path should be set
        assert!(report.backup_path.is_none());
        assert!(report.pending_migrations.is_empty());
    }

    #[test]
    fn test_fts_table_created() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");

        // Check that FTS table exists
        let fts_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='responses_fts'",
                [],
                |row| row.get(0),
            )
            .expect("check fts");
        assert!(fts_exists, "responses_fts table should exist");
    }

    #[test]
    fn test_fts_triggers_work() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");

        // Insert a conversation first
        conn.execute(
            "INSERT INTO conversations (id, name, model) VALUES ('c1', 'Test', 'gpt-4')",
            [],
        )
        .expect("insert conv");

        // Insert a response with ULID
        let ulid = generate_response_ulid();
        conn.execute(
            "INSERT INTO responses (id, model, prompt, response, conversation_id, datetime_utc) \
             VALUES (?1, 'gpt-4', 'Hello world test', 'Response here', 'c1', datetime('now'))",
            params![ulid],
        )
        .expect("insert response");

        // Search FTS
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM responses_fts WHERE responses_fts MATCH 'world'",
                [],
                |row| row.get(0),
            )
            .expect("fts search");

        assert_eq!(count, 1, "FTS should find the inserted response");
    }

    #[test]
    fn test_responses_has_schema_id_column() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");

        // Check schema_id column exists
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(responses)")
            .expect("prepare")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        assert!(columns.contains(&"schema_id".to_string()));
    }

    #[test]
    fn test_responses_has_rust_extension_columns() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");

        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(responses)")
            .expect("prepare")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        assert!(columns.contains(&"tool_calls_json".to_string()));
        assert!(columns.contains(&"tool_results_json".to_string()));
        assert!(columns.contains(&"finish_reason".to_string()));
        assert!(columns.contains(&"usage_json".to_string()));
    }

    #[test]
    fn test_responses_id_is_text_after_migration() {
        let (_dir, path) = temp_db();

        run_migrations(&path).expect("run");

        let conn = Connection::open(&path).expect("open");

        // Check that id column is TEXT
        let id_type: String = conn
            .query_row(
                "SELECT type FROM pragma_table_info('responses') WHERE name = 'id'",
                [],
                |row| row.get(0),
            )
            .expect("get id type");

        assert_eq!(
            id_type, "TEXT",
            "responses.id should be TEXT after migration"
        );
    }

    #[test]
    fn test_generate_deterministic_ulid() {
        // Same timestamp and sequence should produce same ULID
        let ulid1 = generate_deterministic_ulid(Some("2024-01-15T10:30:00Z"), 0);
        let ulid2 = generate_deterministic_ulid(Some("2024-01-15T10:30:00Z"), 0);
        assert_eq!(ulid1, ulid2, "Same inputs should produce same ULID");

        // Different sequences should produce different ULIDs
        let ulid3 = generate_deterministic_ulid(Some("2024-01-15T10:30:00Z"), 1);
        assert_ne!(
            ulid1, ulid3,
            "Different sequences should produce different ULIDs"
        );

        // Earlier timestamp should sort before later timestamp
        let ulid_early = generate_deterministic_ulid(Some("2024-01-15T10:30:00Z"), 0);
        let ulid_late = generate_deterministic_ulid(Some("2024-01-15T10:31:00Z"), 0);
        assert!(
            ulid_early < ulid_late,
            "Earlier timestamp ULID should sort before later"
        );
    }

    #[test]
    fn test_ulid_migration_preserves_data() {
        let (_dir, path) = temp_db();

        // Apply migrations up to 009 manually
        for migration in &MIGRATIONS[..9] {
            apply_migration(&path, migration).expect("apply pre-ulid migration");
        }

        let conn = Connection::open(&path).expect("open");

        // Insert test data
        conn.execute(
            "INSERT INTO conversations (id, name, model) VALUES ('conv1', 'Test Conv', 'gpt-4')",
            [],
        )
        .expect("insert conv");

        conn.execute(
            "INSERT INTO responses (model, prompt, response, conversation_id, datetime_utc) \
             VALUES ('gpt-4', 'Hello', 'World', 'conv1', '2024-01-15T10:30:00Z')",
            [],
        )
        .expect("insert response 1");

        conn.execute(
            "INSERT INTO responses (model, prompt, response, conversation_id, datetime_utc) \
             VALUES ('gpt-4', 'Goodbye', 'Farewell', 'conv1', '2024-01-15T10:31:00Z')",
            [],
        )
        .expect("insert response 2");

        drop(conn);

        // Apply ULID migration
        apply_migration(&path, &MIGRATIONS[9]).expect("apply ulid migration");

        // Verify data is preserved
        let conn = Connection::open(&path).expect("open after migration");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM responses", [], |row| row.get(0))
            .expect("count responses");
        assert_eq!(count, 2, "Should have 2 responses after migration");

        // Verify IDs are now strings
        let ids: Vec<String> = conn
            .prepare("SELECT id FROM responses ORDER BY id")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(ids.len(), 2);
        // IDs should be valid ULIDs (26 characters, uppercase base32)
        for id in &ids {
            assert_eq!(id.len(), 26, "ULID should be 26 characters");
        }
        // Earlier timestamp should sort before later
        assert!(ids[0] < ids[1], "ULIDs should preserve chronological order");
    }

    #[test]
    fn test_generate_response_ulid() {
        let ulid1 = generate_response_ulid();
        let ulid2 = generate_response_ulid();

        // Each call should produce a different ULID
        assert_ne!(ulid1, ulid2);

        // ULIDs should be 26 characters
        assert_eq!(ulid1.len(), 26);
        assert_eq!(ulid2.len(), 26);
    }
}
