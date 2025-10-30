use crate::{logs_db_path, user_dir};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Value, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Internal representation of a log record to be persisted.
pub(crate) struct LogRecord {
    pub model: String,
    pub resolved_model: String,
    pub prompt: Option<String>,
    pub system: Option<String>,
    pub prompt_json: Option<String>,
    pub options_json: Option<String>,
    pub response: String,
    pub response_json: Option<String>,
    pub conversation_id: Option<String>,
    pub conversation_name: Option<String>,
    pub conversation_model: Option<String>,
    pub duration_ms: Option<u128>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub token_details: Option<String>,
}

/// Summary describing the current logs status.
#[derive(Debug, Clone)]
pub struct LogsStatus {
    pub logging_enabled: bool,
    pub database_path: PathBuf,
    pub database_exists: bool,
    pub conversations: u64,
    pub responses: u64,
    pub file_size_bytes: Option<u64>,
}

/// Canonical representation of a stored log entry.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub model: String,
    pub resolved_model: Option<String>,
    pub prompt: Option<String>,
    pub system: Option<String>,
    pub prompt_json: Option<String>,
    pub options_json: Option<String>,
    pub response: Option<String>,
    pub response_json: Option<String>,
    pub datetime_utc: Option<String>,
    pub conversation_id: Option<String>,
    pub conversation_name: Option<String>,
    pub conversation_model: Option<String>,
    pub duration_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub token_details: Option<String>,
}

/// Selection criteria for listing logs.
#[derive(Debug, Default, Clone)]
pub struct ListLogsOptions {
    pub limit: Option<usize>,
    pub model: Option<String>,
    pub query: Option<String>,
    pub conversation_id: Option<String>,
    pub newest_first: bool,
    pub database_path: Option<PathBuf>,
    pub id_gt: Option<i64>,
    pub id_gte: Option<i64>,
    pub since: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

/// Return true when logging is enabled (default) and false when disabled.
pub fn logs_enabled() -> Result<bool> {
    let sentinel = user_dir()?.join("logs-off");
    Ok(!sentinel.exists())
}

/// Enable or disable logging by creating/removing the sentinel file.
pub fn set_logging_enabled(enabled: bool) -> Result<()> {
    let sentinel = user_dir()?.join("logs-off");
    if enabled {
        if sentinel.exists() {
            fs::remove_file(&sentinel)
                .with_context(|| format!("Failed to remove {}", sentinel.display()))?;
        }
    } else if !sentinel.exists() {
        fs::File::create(&sentinel)
            .with_context(|| format!("Failed to create {}", sentinel.display()))?;
    }
    Ok(())
}

/// Return high-level details about the logs database.
pub fn logs_status() -> Result<LogsStatus> {
    let path = logs_db_path()?;
    let logging_enabled = logs_enabled()?;
    if !path.exists() {
        return Ok(LogsStatus {
            logging_enabled,
            database_path: path,
            database_exists: false,
            conversations: 0,
            responses: 0,
            file_size_bytes: None,
        });
    }

    let conn = open_database(&path)?;
    let conversations = count_table(&conn, "conversations")?;
    let responses = count_table(&conn, "responses")?;
    let file_size_bytes = fs::metadata(&path).ok().map(|meta| meta.len());

    Ok(LogsStatus {
        logging_enabled,
        database_path: path,
        database_exists: true,
        conversations,
        responses,
        file_size_bytes,
    })
}

/// Copy the logs database into the provided destination using VACUUM INTO.
pub fn backup_logs<P: AsRef<Path>>(destination: P) -> Result<()> {
    let path = logs_db_path()?;
    if !path.exists() {
        bail!("No log database found at {}", path.display());
    }
    let dest = destination.as_ref();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let conn = open_database(&path)?;
    conn.execute(
        &format!("VACUUM INTO {}", bind_var(1)),
        params![dest.to_string_lossy().as_ref()],
    )
    .context("Failed to backup logs database")?;
    Ok(())
}

/// List log entries matching the provided filters.
pub fn list_logs(options: ListLogsOptions) -> Result<Vec<LogEntry>> {
    let default_path = logs_db_path()?;
    let path = options.database_path.unwrap_or(default_path);
    if !path.exists() {
        bail!("No log database found at {}", path.display());
    }

    let conn = open_database(&path)?;

    let mut sql = String::from(
        "SELECT responses.id, responses.model, responses.resolved_model, responses.prompt, \
         responses.system, responses.prompt_json, responses.options_json, responses.response, \
         responses.response_json, responses.datetime_utc, responses.conversation_id, \
         conversations.name, conversations.model, responses.duration_ms, \
         responses.input_tokens, responses.output_tokens, responses.token_details \
         FROM responses \
         LEFT JOIN conversations ON conversations.id = responses.conversation_id",
    );
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    if let Some(model) = options.model {
        conditions.push("responses.model = ?".to_string());
        params.push(Value::from(model));
    }
    if let Some(conversation_id) = options.conversation_id {
        conditions.push("responses.conversation_id = ?".to_string());
        params.push(Value::from(conversation_id));
    }
    if let Some(id_gt) = options.id_gt {
        conditions.push("responses.id > ?".to_string());
        params.push(Value::from(id_gt));
    }
    if let Some(id_gte) = options.id_gte {
        conditions.push("responses.id >= ?".to_string());
        params.push(Value::from(id_gte));
    }
    if let Some(query) = options.query {
        conditions.push("(responses.prompt LIKE ? OR responses.response LIKE ?)".to_string());
        let pattern = format!("%{}%", query);
        params.push(Value::from(pattern.clone()));
        params.push(Value::from(pattern));
    }
    if let Some(since) = options.since {
        conditions.push("responses.datetime_utc >= ?".to_string());
        params.push(Value::from(since.to_rfc3339()));
    }
    if let Some(before) = options.before {
        conditions.push("responses.datetime_utc < ?".to_string());
        params.push(Value::from(before.to_rfc3339()));
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY responses.id ");
    sql.push_str(if options.newest_first { "DESC" } else { "ASC" });

    if let Some(limit) = options.limit {
        if limit > 0 {
            sql.push_str(" LIMIT ?");
            params.push(Value::from(limit as i64));
        }
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        |row| -> Result<LogEntry, rusqlite::Error> {
            Ok(LogEntry {
                id: row.get(0)?,
                model: row.get(1)?,
                resolved_model: row.get(2)?,
                prompt: row.get(3)?,
                system: row.get(4)?,
                prompt_json: row.get(5)?,
                options_json: row.get(6)?,
                response: row.get(7)?,
                response_json: row.get(8)?,
                datetime_utc: row.get(9)?,
                conversation_id: row.get(10)?,
                conversation_name: row.get(11)?,
                conversation_model: row.get(12)?,
                duration_ms: row.get(13)?,
                input_tokens: row.get(14)?,
                output_tokens: row.get(15)?,
                token_details: row.get(16)?,
            })
        },
    )?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }

    Ok(entries)
}

/// Persist a log record if logging is enabled or forced.
pub(crate) fn record_log_entry(record: LogRecord, force_logging: bool) -> Result<()> {
    if !force_logging && !logs_enabled()? {
        return Ok(());
    }
    let path = logs_db_path()?;
    let conn = open_database(&path)?;
    let now = Utc::now().to_rfc3339();

    let LogRecord {
        model,
        resolved_model,
        prompt,
        system,
        prompt_json,
        options_json,
        response,
        response_json,
        conversation_id,
        conversation_name,
        conversation_model,
        duration_ms,
        input_tokens,
        output_tokens,
        token_details,
    } = record;

    if let Some(ref conversation_id_value) = conversation_id {
        conn.execute(
            "
            INSERT INTO conversations (id, name, model)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                name = COALESCE(excluded.name, conversations.name),
                model = COALESCE(excluded.model, conversations.model)
            ",
            params![
                conversation_id_value,
                conversation_name.as_deref(),
                conversation_model.as_deref()
            ],
        )
        .context("Failed to upsert conversation metadata")?;
    }

    conn.execute(
        "INSERT INTO responses \
         (model, resolved_model, prompt, system, prompt_json, options_json, \
          response, response_json, conversation_id, duration_ms, datetime_utc, \
          input_tokens, output_tokens, token_details) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            model,
            resolved_model,
            prompt,
            system,
            prompt_json,
            options_json,
            response,
            response_json,
            conversation_id,
            duration_ms.map(|ms| ms.min(i64::MAX as u128) as i64),
            now,
            input_tokens.map(|v| v as i64),
            output_tokens.map(|v| v as i64),
            token_details,
        ],
    )
    .context("Failed to insert row into logs database")?;
    Ok(())
}

fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            name TEXT,
            model TEXT
        );
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
        ",
    )
    .context("Failed to initialize logs schema")?;
    Ok(())
}

fn count_table(conn: &Connection, table: &str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) FROM {}", table);
    let count: i64 = conn
        .query_row(&sql, [], |row| row.get(0))
        .with_context(|| format!("Failed to count rows in {table}"))?;
    Ok(count as u64)
}

fn bind_var(index: usize) -> String {
    format!("?{}", index)
}
