//! SQLite migrations for the embeddings database.
//!
//! This module provides schema migrations compatible with the upstream
//! Python LLM embeddings database format.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;

// ============================================================================
// Migration Definitions
// ============================================================================

/// Metadata describing a single migration.
#[derive(Debug, Clone, Serialize)]
pub struct Migration {
    /// Unique migration name.
    pub name: &'static str,
    /// SQL to execute.
    pub sql: &'static str,
    /// Human-readable description.
    pub description: &'static str,
}

/// All embeddings migrations in order.
static EMBEDDINGS_MIGRATIONS: &[Migration] = &[
    Migration {
        name: "001_collections_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS collections (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                model TEXT NOT NULL
            );
        "#,
        description: "Create collections table",
    },
    Migration {
        name: "002_embeddings_table",
        sql: r#"
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
        description: "Create embeddings table with content hash index",
    },
];

// ============================================================================
// Migration Tracking
// ============================================================================

/// Record of an applied migration.
#[derive(Debug, Clone, Serialize)]
pub struct AppliedMigration {
    pub name: String,
    pub applied_at: String,
}

/// Ensure the migrations tracking table exists.
fn ensure_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS _llm_embeddings_migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL
        );
        "#,
    )
    .context("failed to create embeddings migrations table")?;
    Ok(())
}

/// Check if a migration has been applied.
fn is_migration_applied(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM _llm_embeddings_migrations WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count > 0)
}

/// Record a migration as applied.
fn record_migration(conn: &Connection, name: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO _llm_embeddings_migrations (name, applied_at) VALUES (?1, ?2)",
        params![name, now],
    )
    .context("failed to record migration")?;
    Ok(())
}

// ============================================================================
// Public API
// ============================================================================

/// Run all pending embeddings migrations.
pub fn run_embeddings_migrations(conn: &Connection) -> Result<usize> {
    ensure_migrations_table(conn)?;

    let mut applied = 0;
    for migration in EMBEDDINGS_MIGRATIONS {
        if is_migration_applied(conn, migration.name)? {
            continue;
        }

        tracing::debug!(
            target: "llm::embeddings::migrations",
            migration = migration.name,
            "applying migration"
        );

        conn.execute_batch(migration.sql)
            .with_context(|| format!("failed to apply migration '{}'", migration.name))?;

        record_migration(conn, migration.name)?;
        applied += 1;
    }

    Ok(applied)
}

/// List all applied embeddings migrations.
pub fn list_applied_migrations(conn: &Connection) -> Result<Vec<AppliedMigration>> {
    ensure_migrations_table(conn)?;

    let mut stmt = conn.prepare(
        "SELECT name, applied_at FROM _llm_embeddings_migrations ORDER BY id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(AppliedMigration {
            name: row.get(0)?,
            applied_at: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().context("failed to list migrations")
}

/// List pending embeddings migrations.
pub fn list_pending_migrations(conn: &Connection) -> Result<Vec<&'static str>> {
    ensure_migrations_table(conn)?;

    let mut pending = Vec::new();
    for migration in EMBEDDINGS_MIGRATIONS {
        if !is_migration_applied(conn, migration.name)? {
            pending.push(migration.name);
        }
    }
    Ok(pending)
}

/// Get all defined migrations.
pub fn all_migrations() -> &'static [Migration] {
    EMBEDDINGS_MIGRATIONS
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        Connection::open_in_memory().expect("in-memory db")
    }

    #[test]
    fn test_run_migrations() {
        let conn = in_memory_db();
        let applied = run_embeddings_migrations(&conn).expect("run migrations");
        assert_eq!(applied, EMBEDDINGS_MIGRATIONS.len());

        // Running again should apply nothing
        let applied2 = run_embeddings_migrations(&conn).expect("run again");
        assert_eq!(applied2, 0);
    }

    #[test]
    fn test_list_applied_migrations() {
        let conn = in_memory_db();
        run_embeddings_migrations(&conn).expect("run");

        let applied = list_applied_migrations(&conn).expect("list");
        assert_eq!(applied.len(), EMBEDDINGS_MIGRATIONS.len());
        assert_eq!(applied[0].name, "001_collections_table");
    }

    #[test]
    fn test_list_pending_migrations() {
        let conn = in_memory_db();
        
        // Before running, all are pending
        let pending = list_pending_migrations(&conn).expect("list");
        assert_eq!(pending.len(), EMBEDDINGS_MIGRATIONS.len());

        // After running, none are pending
        run_embeddings_migrations(&conn).expect("run");
        let pending2 = list_pending_migrations(&conn).expect("list");
        assert!(pending2.is_empty());
    }

    #[test]
    fn test_schema_created() {
        let conn = in_memory_db();
        run_embeddings_migrations(&conn).expect("run");

        // Check collections table exists
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='collections'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists);

        // Check embeddings table exists
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='embeddings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists);
    }

    #[test]
    fn test_schema_columns() {
        let conn = in_memory_db();
        run_embeddings_migrations(&conn).expect("run");

        // Insert test data to verify schema
        conn.execute(
            "INSERT INTO collections (name, model) VALUES ('test', 'model')",
            [],
        )
        .expect("insert collection");

        conn.execute(
            r#"
            INSERT INTO embeddings (collection_id, id, embedding, content, content_hash, metadata, updated)
            VALUES (1, 'item1', X'0000803F', 'hello', X'1234', '{"key": "value"}', 12345)
            "#,
            [],
        )
        .expect("insert embedding");

        // Query back
        let (id, content, metadata): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT id, content, metadata FROM embeddings WHERE collection_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query");

        assert_eq!(id, "item1");
        assert_eq!(content, Some("hello".to_string()));
        assert!(metadata.unwrap().contains("key"));
    }

    #[test]
    fn test_all_migrations() {
        let migrations = all_migrations();
        assert!(!migrations.is_empty());
        assert!(migrations.iter().all(|m| !m.name.is_empty()));
        assert!(migrations.iter().all(|m| !m.sql.is_empty()));
    }
}
