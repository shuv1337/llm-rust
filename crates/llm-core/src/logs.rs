use crate::providers::{MessageRole, PromptMessage};
use crate::{logs_db_path, user_dir};
use crate::migrations::{run_migrations, generate_response_ulid};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Value, Connection, OptionalExtension};
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
    // Schema support (upstream parity)
    /// ID of the JSON schema used for this response (if any).
    pub schema_id: Option<String>,
    // Rust extension fields for simplified tool tracking
    /// JSON-serialized list of tool calls made by the model.
    pub tool_calls_json: Option<String>,
    /// JSON-serialized list of tool results returned.
    pub tool_results_json: Option<String>,
    /// Why the model stopped generating (stop, length, tool_calls, etc.).
    pub finish_reason: Option<String>,
    /// JSON-serialized extended usage info (cached_tokens, reasoning_tokens, etc.).
    pub usage_json: Option<String>,
}

impl LogRecord {
    /// Create a new LogRecord with required fields, all optional fields set to None.
    pub fn new(model: String, resolved_model: String, response: String) -> Self {
        Self {
            model,
            resolved_model,
            prompt: None,
            system: None,
            prompt_json: None,
            options_json: None,
            response,
            response_json: None,
            conversation_id: None,
            conversation_name: None,
            conversation_model: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            token_details: None,
            schema_id: None,
            tool_calls_json: None,
            tool_results_json: None,
            finish_reason: None,
            usage_json: None,
        }
    }
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
    pub id: String,
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
    // Schema support (upstream parity)
    /// ID of the JSON schema used for this response (if any).
    pub schema_id: Option<String>,
    // Rust extension fields for simplified tool tracking
    /// JSON-serialized list of tool calls made by the model.
    pub tool_calls_json: Option<String>,
    /// JSON-serialized list of tool results returned.
    pub tool_results_json: Option<String>,
    /// Why the model stopped generating.
    pub finish_reason: Option<String>,
    /// JSON-serialized extended usage info.
    pub usage_json: Option<String>,
}

impl LogEntry {
    /// Check if this response involved tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls_json
            .as_ref()
            .map_or(false, |s| !s.is_empty() && s != "[]" && s != "null")
    }
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
    pub id_gt: Option<String>,
    pub id_gte: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    /// Filter to only entries with tool calls.
    pub with_tool_calls: Option<bool>,
    /// Filter to only entries with a specific schema_id.
    pub schema_id: Option<String>,
    /// Use FTS for query matching (default: true when query is set).
    pub use_fts: Option<bool>,
    /// Filter to entries that used a specific fragment (placeholder).
    pub fragment_id: Option<String>,
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

/// Check if FTS is available for the database.
fn has_fts_table(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='responses_fts'",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// List log entries matching the provided filters.
pub fn list_logs(options: ListLogsOptions) -> Result<Vec<LogEntry>> {
    let default_path = logs_db_path()?;
    let path = options.database_path.unwrap_or(default_path);
    if !path.exists() {
        bail!("No log database found at {}", path.display());
    }

    let conn = open_database(&path)?;

    // Determine whether to use FTS
    let use_fts = options.use_fts.unwrap_or(true) && options.query.is_some() && has_fts_table(&conn);

    let mut sql = if use_fts && options.query.is_some() {
        // Use FTS join for better search ranking
        // Note: After ULID migration, FTS table has explicit id column, not rowid
        String::from(
            "SELECT responses.id, responses.model, responses.resolved_model, responses.prompt, \
             responses.system, responses.prompt_json, responses.options_json, responses.response, \
             responses.response_json, responses.datetime_utc, responses.conversation_id, \
             conversations.name, conversations.model, responses.duration_ms, \
             responses.input_tokens, responses.output_tokens, responses.token_details, \
             responses.schema_id, \
             responses.tool_calls_json, responses.tool_results_json, responses.finish_reason, \
             responses.usage_json \
             FROM responses \
             INNER JOIN responses_fts ON responses.id = responses_fts.id \
             LEFT JOIN conversations ON conversations.id = responses.conversation_id",
        )
    } else {
        String::from(
            "SELECT responses.id, responses.model, responses.resolved_model, responses.prompt, \
             responses.system, responses.prompt_json, responses.options_json, responses.response, \
             responses.response_json, responses.datetime_utc, responses.conversation_id, \
             conversations.name, conversations.model, responses.duration_ms, \
             responses.input_tokens, responses.output_tokens, responses.token_details, \
             responses.schema_id, \
             responses.tool_calls_json, responses.tool_results_json, responses.finish_reason, \
             responses.usage_json \
             FROM responses \
             LEFT JOIN conversations ON conversations.id = responses.conversation_id",
        )
    };
    
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
    // String ID comparison (lexicographic for ULIDs)
    if let Some(id_gt) = options.id_gt {
        conditions.push("responses.id > ?".to_string());
        params.push(Value::from(id_gt));
    }
    if let Some(id_gte) = options.id_gte {
        conditions.push("responses.id >= ?".to_string());
        params.push(Value::from(id_gte));
    }
    if let Some(query) = &options.query {
        if use_fts {
            // Use FTS5 MATCH syntax for better search
            conditions.push("responses_fts MATCH ?".to_string());
            params.push(Value::from(query.clone()));
        } else {
            // Fallback to LIKE for databases without FTS
            conditions.push("(responses.prompt LIKE ? OR responses.response LIKE ?)".to_string());
            let pattern = format!("%{}%", query);
            params.push(Value::from(pattern.clone()));
            params.push(Value::from(pattern));
        }
    }
    if let Some(since) = options.since {
        conditions.push("responses.datetime_utc >= ?".to_string());
        params.push(Value::from(since.to_rfc3339()));
    }
    if let Some(before) = options.before {
        conditions.push("responses.datetime_utc < ?".to_string());
        params.push(Value::from(before.to_rfc3339()));
    }
    if let Some(with_tool_calls) = options.with_tool_calls {
        if with_tool_calls {
            conditions.push("responses.tool_calls_json IS NOT NULL AND responses.tool_calls_json != '[]' AND responses.tool_calls_json != 'null'".to_string());
        } else {
            conditions.push("(responses.tool_calls_json IS NULL OR responses.tool_calls_json = '[]' OR responses.tool_calls_json = 'null')".to_string());
        }
    }
    if let Some(schema_id) = options.schema_id {
        conditions.push("responses.schema_id = ?".to_string());
        params.push(Value::from(schema_id));
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    // For FTS queries, order by rank (relevance) first, then by id
    if use_fts && options.query.is_some() {
        sql.push_str(" ORDER BY rank, responses.id ");
        sql.push_str(if options.newest_first { "DESC" } else { "ASC" });
    } else {
        sql.push_str(" ORDER BY responses.id ");
        sql.push_str(if options.newest_first { "DESC" } else { "ASC" });
    }

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
                schema_id: row.get(17)?,
                tool_calls_json: row.get(18)?,
                tool_results_json: row.get(19)?,
                finish_reason: row.get(20)?,
                usage_json: row.get(21)?,
            })
        },
    )?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }

    Ok(entries)
}

/// Persist a log record if logging is enabled or forced, with optional database path override.
pub(crate) fn record_log_entry(
    record: LogRecord, 
    force_logging: bool,
    database_path: Option<&Path>,
) -> Result<()> {
    if !force_logging && !logs_enabled()? {
        return Ok(());
    }
    let path = database_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| logs_db_path().expect("logs_db_path should be available"));
    let conn = open_database(&path)?;
    let now = Utc::now().to_rfc3339();
    
    // Generate a new ULID for this response
    let response_id = generate_response_ulid();

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
        schema_id,
        tool_calls_json,
        tool_results_json,
        finish_reason,
        usage_json,
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
         (id, model, resolved_model, prompt, system, prompt_json, options_json, \
          response, response_json, conversation_id, duration_ms, datetime_utc, \
          input_tokens, output_tokens, token_details, schema_id, \
          tool_calls_json, tool_results_json, finish_reason, usage_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            response_id,
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
            schema_id,
            tool_calls_json,
            tool_results_json,
            finish_reason,
            usage_json,
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
    
    // Run migrations to ensure schema is up to date
    run_migrations(path)?;
    
    let conn =
        Connection::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    
    // Set pragmas
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .context("Failed to set journal mode")?;
    
    Ok(conn)
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


/// Load conversation messages for continuation, reconstructing prompt/response history.
///
/// Returns messages in chronological order (oldest first), with system prompts at the
/// beginning if present. User prompts become User messages, responses become Assistant
/// messages.
pub fn load_conversation_messages(conversation_id: &str) -> Result<Vec<PromptMessage>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_database(&path)?;
    
    // Query all responses for this conversation, ordered by id (chronological)
    let mut stmt = conn.prepare(
        "SELECT prompt, system, response, prompt_json \
         FROM responses \
         WHERE conversation_id = ? \
         ORDER BY id ASC"
    )?;

    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?, // prompt
            row.get::<_, Option<String>>(1)?, // system
            row.get::<_, Option<String>>(2)?, // response
            row.get::<_, Option<String>>(3)?, // prompt_json
        ))
    })?;

    let mut messages: Vec<PromptMessage> = Vec::new();
    let mut seen_system: Option<String> = None;

    for row_result in rows {
        let (prompt, system, response, prompt_json) = row_result?;

        // Handle system prompt - only include first unique system prompt
        if let Some(sys) = system {
            let trimmed = sys.trim();
            if !trimmed.is_empty() {
                let should_add = match &seen_system {
                    None => true,
                    Some(prev) => prev != trimmed,
                };
                if should_add {
                    // If there's a system prompt change mid-conversation, add it
                    if seen_system.is_some() {
                        messages.push(PromptMessage::system(trimmed));
                    }
                    seen_system = Some(trimmed.to_string());
                }
            }
        }

        // Try to reconstruct messages from prompt_json first (preserves multi-turn structure)
        if let Some(json_str) = &prompt_json {
            if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                for msg_value in parsed {
                    if let (Some(role_str), Some(content)) = (
                        msg_value.get("role").and_then(|v| v.as_str()),
                        msg_value.get("content").and_then(|v| v.as_str()),
                    ) {
                        let role = match role_str {
                            "user" => MessageRole::User,
                            "assistant" => MessageRole::Assistant,
                            "system" => MessageRole::System,
                            "tool" => MessageRole::Tool,
                            "function" => MessageRole::Function,
                            _ => continue,
                        };
                        // Skip system messages from JSON - we handle them separately above
                        if matches!(role, MessageRole::System) {
                            continue;
                        }
                        messages.push(PromptMessage::new(role, content));
                    }
                }
                // Add the response as assistant message if not already in prompt_json
                if let Some(resp) = &response {
                    let trimmed = resp.trim();
                    if !trimmed.is_empty() {
                        // Check if the last message is already this response
                        let should_add = messages
                            .last()
                            .map(|m| !matches!(m.role, MessageRole::Assistant) || m.content != trimmed)
                            .unwrap_or(true);
                        if should_add {
                            messages.push(PromptMessage::assistant(trimmed));
                        }
                    }
                }
                continue;
            }
        }

        // Fallback: use simple prompt/response columns
        if let Some(p) = &prompt {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                messages.push(PromptMessage::user(trimmed));
            }
        }
        if let Some(r) = &response {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                messages.push(PromptMessage::assistant(trimmed));
            }
        }
    }

    // If we collected a system prompt, prepend it to the messages
    if let Some(sys) = seen_system {
        if !messages.iter().any(|m| matches!(m.role, MessageRole::System) && m.content == sys) {
            messages.insert(0, PromptMessage::system(sys));
        }
    }

    Ok(messages)
}

/// Get the most recent conversation ID from the logs database.
///
/// Returns the conversation_id of the most recently logged response that has a
/// conversation_id set, or None if no conversations exist.
pub fn get_latest_conversation_id() -> Result<Option<String>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let conn = open_database(&path)?;
    
    let result: Option<String> = conn
        .query_row(
            "SELECT conversation_id FROM responses \
             WHERE conversation_id IS NOT NULL \
             ORDER BY id DESC \
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to query latest conversation")?;

    Ok(result)
}

// ============================================================================
// Schema Support
// ============================================================================

/// A stored JSON schema from the schemas table.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaEntry {
    /// Unique identifier for the schema (typically a hash or name).
    pub id: String,
    /// The JSON schema content.
    pub content: Option<String>,
    /// Number of times this schema was used in responses.
    pub usage_count: i64,
}

/// List all schemas stored in the database.
///
/// Returns schemas sorted by usage count (most used first).
pub fn list_schemas() -> Result<Vec<SchemaEntry>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_database(&path)?;
    
    // Check if schemas table exists
    let has_schemas_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schemas'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    
    if !has_schemas_table {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT s.id, s.content, COUNT(r.id) as usage_count \
         FROM schemas s \
         LEFT JOIN responses r ON r.schema_id = s.id \
         GROUP BY s.id, s.content \
         ORDER BY usage_count DESC, s.id ASC"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(SchemaEntry {
            id: row.get(0)?,
            content: row.get(1)?,
            usage_count: row.get(2)?,
        })
    })?;

    let mut schemas = Vec::new();
    for row in rows {
        schemas.push(row?);
    }

    Ok(schemas)
}

/// Get a specific schema by ID/name.
pub fn get_schema(id: &str) -> Result<Option<SchemaEntry>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let conn = open_database(&path)?;
    
    // Check if schemas table exists
    let has_schemas_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schemas'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    
    if !has_schemas_table {
        return Ok(None);
    }

    let result = conn
        .query_row(
            "SELECT s.id, s.content, COUNT(r.id) as usage_count \
             FROM schemas s \
             LEFT JOIN responses r ON r.schema_id = s.id \
             WHERE s.id = ? \
             GROUP BY s.id, s.content",
            params![id],
            |row| {
                Ok(SchemaEntry {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    usage_count: row.get(2)?,
                })
            },
        )
        .optional()
        .context("Failed to query schema")?;

    Ok(result)
}

// ============================================================================
// Tool Support
// ============================================================================

/// A stored tool definition from the tools table.
#[derive(Debug, Clone, Serialize)]
pub struct ToolEntry {
    /// Internal ID of the tool.
    pub id: i64,
    /// Hash for deduplication.
    pub hash: String,
    /// Tool name.
    pub name: Option<String>,
    /// Tool description.
    pub description: Option<String>,
    /// JSON schema defining the tool's input parameters.
    pub input_schema: Option<String>,
    /// Plugin that provides this tool.
    pub plugin: Option<String>,
    /// Number of times this tool was used in responses.
    pub usage_count: i64,
}

/// Options for listing tools.
#[derive(Debug, Default, Clone)]
pub struct ListToolsOptions {
    /// Filter to tools that have function definitions (input_schema is not null).
    pub functions_only: bool,
    /// Filter by plugin name.
    pub plugin: Option<String>,
}

/// List all tools stored in the database.
///
/// Returns tools sorted by usage count (most used first).
pub fn list_tools(options: ListToolsOptions) -> Result<Vec<ToolEntry>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let conn = open_database(&path)?;
    
    // Check if tools table exists
    let has_tools_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='tools'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    
    if !has_tools_table {
        return Ok(Vec::new());
    }

    let mut sql = String::from(
        "SELECT t.id, t.hash, t.name, t.description, t.input_schema, t.plugin, \
         COUNT(tr.response_id) as usage_count \
         FROM tools t \
         LEFT JOIN tool_responses tr ON tr.tool_id = t.id"
    );

    let mut conditions: Vec<String> = Vec::new();
    let mut params_vec: Vec<Value> = Vec::new();

    if options.functions_only {
        conditions.push("t.input_schema IS NOT NULL".to_string());
    }

    if let Some(plugin) = options.plugin {
        conditions.push("t.plugin = ?".to_string());
        params_vec.push(Value::from(plugin));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    sql.push_str(" GROUP BY t.id, t.hash, t.name, t.description, t.input_schema, t.plugin");
    sql.push_str(" ORDER BY usage_count DESC, t.name ASC");

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(
        rusqlite::params_from_iter(params_vec.iter()),
        |row| {
            Ok(ToolEntry {
                id: row.get(0)?,
                hash: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                input_schema: row.get(4)?,
                plugin: row.get(5)?,
                usage_count: row.get(6)?,
            })
        },
    )?;

    let mut tools = Vec::new();
    for row in rows {
        tools.push(row?);
    }

    Ok(tools)
}

/// Get a specific tool by name.
pub fn get_tool(name: &str) -> Result<Option<ToolEntry>> {
    let path = logs_db_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let conn = open_database(&path)?;
    
    // Check if tools table exists
    let has_tools_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='tools'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    
    if !has_tools_table {
        return Ok(None);
    }

    let result = conn
        .query_row(
            "SELECT t.id, t.hash, t.name, t.description, t.input_schema, t.plugin, \
             COUNT(tr.response_id) as usage_count \
             FROM tools t \
             LEFT JOIN tool_responses tr ON tr.tool_id = t.id \
             WHERE t.name = ? \
             GROUP BY t.id, t.hash, t.name, t.description, t.input_schema, t.plugin",
            params![name],
            |row| {
                Ok(ToolEntry {
                    id: row.get(0)?,
                    hash: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    input_schema: row.get(4)?,
                    plugin: row.get(5)?,
                    usage_count: row.get(6)?,
                })
            },
        )
        .optional()
        .context("Failed to query tool")?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_entry_has_tool_calls() {
        let mut entry = LogEntry {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            model: "test".to_string(),
            resolved_model: None,
            prompt: None,
            system: None,
            prompt_json: None,
            options_json: None,
            response: None,
            response_json: None,
            datetime_utc: None,
            conversation_id: None,
            conversation_name: None,
            conversation_model: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            token_details: None,
            schema_id: None,
            tool_calls_json: None,
            tool_results_json: None,
            finish_reason: None,
            usage_json: None,
        };

        // No tool calls
        assert!(!entry.has_tool_calls());

        // Empty array
        entry.tool_calls_json = Some("[]".to_string());
        assert!(!entry.has_tool_calls());

        // Null
        entry.tool_calls_json = Some("null".to_string());
        assert!(!entry.has_tool_calls());

        // Actual tool calls
        entry.tool_calls_json = Some(r#"[{"id":"call_1","type":"function","function":{"name":"test","arguments":"{}"}}]"#.to_string());
        assert!(entry.has_tool_calls());
    }

    #[test]
    fn log_record_new() {
        let record = LogRecord::new(
            "gpt-4".to_string(),
            "gpt-4-0125-preview".to_string(),
            "Hello, world!".to_string(),
        );
        assert_eq!(record.model, "gpt-4");
        assert_eq!(record.resolved_model, "gpt-4-0125-preview");
        assert_eq!(record.response, "Hello, world!");
        assert!(record.tool_calls_json.is_none());
        assert!(record.finish_reason.is_none());
        assert!(record.schema_id.is_none());
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::*;
    use std::env;

    fn temp_user_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn with_env_lock<F: FnOnce()>(f: F) {
        let guard = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f();
        drop(guard);
    }

    #[test]
    fn load_conversation_messages_empty_db() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // No database exists yet
            let messages = load_conversation_messages("conv-1").expect("load");
            assert!(messages.is_empty());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn load_conversation_messages_with_history() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Record some log entries
            let record1 = LogRecord {
                model: "gpt-4".to_string(),
                resolved_model: "gpt-4".to_string(),
                prompt: Some("Hello".to_string()),
                system: Some("You are a helpful assistant.".to_string()),
                prompt_json: None,
                options_json: None,
                response: "Hi there!".to_string(),
                response_json: None,
                conversation_id: Some("conv-test".to_string()),
                conversation_name: Some("Test Conversation".to_string()),
                conversation_model: Some("gpt-4".to_string()),
                duration_ms: Some(100),
                input_tokens: None,
                output_tokens: None,
                token_details: None,
                schema_id: None,
                tool_calls_json: None,
                tool_results_json: None,
                finish_reason: None,
                usage_json: None,
            };
            record_log_entry(record1, true, None).expect("record1");

            let record2 = LogRecord {
                model: "gpt-4".to_string(),
                resolved_model: "gpt-4".to_string(),
                prompt: Some("How are you?".to_string()),
                system: Some("You are a helpful assistant.".to_string()),
                prompt_json: None,
                options_json: None,
                response: "I'm doing well, thanks!".to_string(),
                response_json: None,
                conversation_id: Some("conv-test".to_string()),
                conversation_name: None,
                conversation_model: None,
                duration_ms: Some(150),
                input_tokens: None,
                output_tokens: None,
                token_details: None,
                schema_id: None,
                tool_calls_json: None,
                tool_results_json: None,
                finish_reason: None,
                usage_json: None,
            };
            record_log_entry(record2, true, None).expect("record2");

            // Load the conversation messages
            let messages = load_conversation_messages("conv-test").expect("load");
            
            // Should have: system, user, assistant, user, assistant
            assert_eq!(messages.len(), 5);
            assert!(matches!(messages[0].role, MessageRole::System));
            assert_eq!(messages[0].content, "You are a helpful assistant.");
            assert!(matches!(messages[1].role, MessageRole::User));
            assert_eq!(messages[1].content, "Hello");
            assert!(matches!(messages[2].role, MessageRole::Assistant));
            assert_eq!(messages[2].content, "Hi there!");
            assert!(matches!(messages[3].role, MessageRole::User));
            assert_eq!(messages[3].content, "How are you?");
            assert!(matches!(messages[4].role, MessageRole::Assistant));
            assert_eq!(messages[4].content, "I'm doing well, thanks!");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn get_latest_conversation_id_empty() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let id = get_latest_conversation_id().expect("get");
            assert!(id.is_none());

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn get_latest_conversation_id_with_entries() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Record first conversation
            let record1 = LogRecord {
                model: "gpt-4".to_string(),
                resolved_model: "gpt-4".to_string(),
                prompt: Some("Hello".to_string()),
                system: None,
                prompt_json: None,
                options_json: None,
                response: "Hi!".to_string(),
                response_json: None,
                conversation_id: Some("conv-first".to_string()),
                conversation_name: None,
                conversation_model: None,
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
                token_details: None,
                schema_id: None,
                tool_calls_json: None,
                tool_results_json: None,
                finish_reason: None,
                usage_json: None,
            };
            record_log_entry(record1, true, None).expect("record1");

            // Small delay to ensure different ULID
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Record second conversation
            let record2 = LogRecord {
                model: "gpt-4".to_string(),
                resolved_model: "gpt-4".to_string(),
                prompt: Some("Goodbye".to_string()),
                system: None,
                prompt_json: None,
                options_json: None,
                response: "Bye!".to_string(),
                response_json: None,
                conversation_id: Some("conv-second".to_string()),
                conversation_name: None,
                conversation_model: None,
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
                token_details: None,
                schema_id: None,
                tool_calls_json: None,
                tool_results_json: None,
                finish_reason: None,
                usage_json: None,
            };
            record_log_entry(record2, true, None).expect("record2");

            // Should return the most recent conversation
            let id = get_latest_conversation_id().expect("get");
            assert_eq!(id, Some("conv-second".to_string()));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn load_conversation_nonexistent() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Create a conversation
            let record = LogRecord {
                model: "gpt-4".to_string(),
                resolved_model: "gpt-4".to_string(),
                prompt: Some("Hello".to_string()),
                system: None,
                prompt_json: None,
                options_json: None,
                response: "Hi!".to_string(),
                response_json: None,
                conversation_id: Some("conv-exists".to_string()),
                conversation_name: None,
                conversation_model: None,
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
                token_details: None,
                schema_id: None,
                tool_calls_json: None,
                tool_results_json: None,
                finish_reason: None,
                usage_json: None,
            };
            record_log_entry(record, true, None).expect("record");

            // Try to load non-existent conversation
            let messages = load_conversation_messages("conv-nonexistent").expect("load");
            assert!(messages.is_empty());

            env::remove_var("LLM_USER_PATH");
        });
    }
}
