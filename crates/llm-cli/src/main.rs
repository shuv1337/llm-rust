use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use clap::{Args, CommandFactory, Parser, Subcommand};
use llm_core::{
    aliases_path, available_models, backup_logs, core_version, detect_mime_from_content,
    detect_mime_from_path, detect_remote_mime, embeddings_db_path, execute_prompt_with_messages,
    get_default_model, get_latest_conversation_id, get_model_options, get_schema, get_tool,
    keys_path, list_aliases, list_key_names, list_logs, list_model_options, list_schemas,
    list_template_loaders, list_templates, list_tools, load_conversation_messages, load_keys,
    load_template, logs_db_path, logs_status, migrations::generate_response_ulid,
    prompt_debug_info, query_models, remove_alias, remove_model_options, resolve_key, save_key,
    save_template, set_alias, set_default_model, set_logging_enabled, set_model_options,
    stream_prompt_with_messages, templates_path, Attachment, KeyQuery, ListLogsOptions,
    ListToolsOptions, LogEntry, MessageRole, ModelInfo, ModelOptions, PromptConfig, PromptMessage,
    StreamSink,
};
use llm_embeddings::{
    delete_collection, list_collections, list_embedding_models, resolve_embedding_model,
    Collection, EmbedItem, EmbeddingProvider, Entry, OpenAIEmbeddingProvider,
};
use llm_plugin_host::load_plugins;
use rpassword::prompt_password;
use rustyline::{error::ReadlineError, DefaultEditor};
use shell_words::split as shell_split;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tracing::info;

#[derive(Args, Clone, Default)]
struct PromptOptions {
    /// Override the model (defaults to env or gpt-4o-mini)
    #[arg(long)]
    model: Option<String>,
    /// Model query terms to find a matching model (selects shortest match)
    #[arg(long = "query")]
    query: Option<String>,
    /// Sampling temperature for the provider
    #[arg(long)]
    temperature: Option<f32>,
    /// Maximum number of tokens to request from the provider
    #[arg(long = "max-tokens")]
    max_tokens: Option<u32>,
    /// Disable streaming tokens (enabled by default)
    #[arg(long = "no-stream")]
    no_stream: bool,
    /// Number of retries for provider requests
    #[arg(long)]
    retries: Option<u32>,
    /// Retry backoff in milliseconds for provider requests
    #[arg(long = "retry-backoff-ms")]
    retry_backoff_ms: Option<u64>,
    /// Force logging for this invocation even if disabled globally
    #[arg(long, conflicts_with = "no_log")]
    log: bool,
    /// Disable logging for this invocation
    #[arg(long = "no-log", conflicts_with = "log")]
    no_log: bool,
    /// Print token usage after response
    #[arg(short = 'u', long = "usage")]
    usage: bool,
    /// Override the logs database path for this invocation
    #[arg(long = "database")]
    database: Option<String>,
}

impl PromptOptions {
    fn log_override(&self) -> Option<bool> {
        if self.log {
            Some(true)
        } else if self.no_log {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Args, Clone, Default)]
struct PromptInputArgs {
    #[command(flatten)]
    options: PromptOptions,
    /// Custom system prompt override
    #[arg(short, long)]
    system: Option<String>,
    /// API key or alias override for this invocation
    #[arg(long)]
    key: Option<String>,
    /// Attachment path, URL or '-' to read from stdin
    #[arg(short = 'a', long = "attachment", value_name = "PATH|URL|-")]
    attachments: Vec<String>,
    /// Attachment with explicit mimetype (--attachment-type PATH TYPE)
    #[arg(
        long = "attachment-type",
        alias = "at",
        value_names = ["PATH|URL|-", "MIMETYPE"],
        num_args = 2
    )]
    attachment_types: Vec<String>,
    /// Continue the most recent conversation
    #[arg(short = 'c', long = "continue")]
    continue_conversation: bool,
    /// Conversation identifier to continue or associate with this prompt
    #[arg(long = "cid", visible_alias = "conversation")]
    cid: Option<String>,
    /// Optional display name for the conversation
    #[arg(long = "conversation-name")]
    conversation_name: Option<String>,
    /// Override the model recorded for the conversation metadata
    #[arg(long = "conversation-model")]
    conversation_model: Option<String>,
    /// The prompt text to execute
    #[arg()]
    prompt: Vec<String>,
}

fn parse_datetime(raw: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let naive = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("Invalid date '{raw}'"))?;
        return Ok(Utc.from_utc_datetime(&naive));
    }
    Err(format!(
        "Unable to parse datetime '{raw}'. Use RFC3339 or YYYY-MM-DD formats."
    ))
}

const CMD_SYSTEM_PROMPT: &str = r#"Return only the command to be executed as a raw string, no string delimiters
wrapping it, no yapping, no markdown, no fenced code blocks, what you return
will be passed to subprocess.check_output() directly.
For example, if the user asks: undo last git commit
You return only: git reset --soft HEAD~1"#;

#[derive(Parser)]
#[command(
    name = "llm",
    version,
    about = "LLM CLI for prompts, models, plugins, logs, and embeddings",
    disable_version_flag = false
)]
struct Cli {
    #[command(flatten)]
    logging: LoggingOptions,

    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    prompt_input: PromptInputArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Execute a prompt
    Prompt(PromptArgs),
    /// List plugins detected by the host
    Plugins(PluginsArgs),
    /// Inspect or configure models
    Models(ModelsArgs),
    /// Manage stored API keys
    Keys(KeysArgs),
    /// Manage model aliases
    Aliases(AliasesArgs),
    /// Manage prompt logs
    Logs(LogsArgs),
    /// Manage prompt templates
    Templates(TemplatesArgs),
    /// Manage JSON schemas used for structured output
    Schemas(SchemasArgs),
    /// Manage tools/functions for model interactions
    Tools(ToolsArgs),
    /// Embed content and store vectors
    Embed(EmbedArgs),
    /// Manage embedding models
    EmbedModels(EmbedModelsArgs),
    /// Bulk embed multiple files or database content
    EmbedMulti(EmbedMultiArgs),
    /// Find similar embeddings
    Similar(SimilarArgs),
    /// Manage embedding collections
    Collections(CollectionsArgs),
    /// Start an interactive chat session
    Chat(ChatArgs),
    /// Generate and execute commands in your shell
    Cmd(CmdArgs),
    /// Display internal version information
    Version(VersionArgs),
}

#[derive(Args)]
struct PromptArgs {
    #[command(flatten)]
    input: PromptInputArgs,
}

#[derive(Args)]
struct ChatArgs {
    #[command(flatten)]
    options: PromptOptions,
    /// Custom system prompt override
    #[arg(short, long)]
    system: Option<String>,
    /// API key or alias override for this invocation
    #[arg(long)]
    key: Option<String>,
    /// Continue the most recent conversation
    #[arg(short = 'c', long = "continue")]
    continue_conversation: bool,
    /// Conversation identifier to continue or associate with this chat
    #[arg(long = "cid", visible_alias = "conversation")]
    cid: Option<String>,
    /// Optional display name for the conversation
    #[arg(long = "conversation-name")]
    conversation_name: Option<String>,
    /// Override the model recorded for the conversation metadata
    #[arg(long = "conversation-model")]
    conversation_model: Option<String>,
}

#[derive(Args)]
struct CmdArgs {
    #[command(flatten)]
    options: PromptOptions,
    /// Custom system prompt override
    #[arg(short, long)]
    system: Option<String>,
    /// API key or alias override for this invocation
    #[arg(long)]
    key: Option<String>,
    /// The natural language description of the desired command
    #[arg()]
    prompt: Vec<String>,
    /// Continue the most recent conversation
    #[arg(short = 'c', long = "continue")]
    continue_conversation: bool,
    /// Conversation identifier to continue or associate with this prompt
    #[arg(long = "cid", visible_alias = "conversation")]
    cid: Option<String>,
    /// Optional display name for the conversation
    #[arg(long = "conversation-name")]
    conversation_name: Option<String>,
    /// Override the model recorded for the conversation metadata
    #[arg(long = "conversation-model")]
    conversation_model: Option<String>,
}

#[derive(Args, Clone, Default)]
struct LoggingOptions {
    /// Enable info-level logging
    #[arg(long, global = true)]
    info: bool,
    /// Enable debug-level logging
    #[arg(long, global = true)]
    debug: bool,
}

#[derive(Args)]
struct VersionArgs {
    /// Show extra details
    #[arg(long)]
    verbose: bool,
}

#[derive(Args)]
struct PluginsArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ModelsArgs {
    #[command(subcommand)]
    command: Option<ModelsSubcommand>,
}

#[derive(Subcommand)]
enum ModelsSubcommand {
    /// List models currently available to the CLI
    List(ModelsListArgs),
    /// Get or set the default model
    Default(ModelsDefaultArgs),
    /// Manage model default options
    Options(ModelsOptionsArgs),
}

#[derive(Args)]
struct ModelsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Only show models that have stored default options
    #[arg(long = "options")]
    with_options: bool,
    /// Only show models that support async execution
    #[arg(long = "async")]
    with_async: bool,
    /// Only show models that support structured output schemas
    #[arg(long = "schemas")]
    with_schemas: bool,
    /// Only show models that support tool/function calling
    #[arg(long = "tools")]
    with_tools: bool,
    /// Filter models by query terms (matches name, description, aliases)
    #[arg(short = 'q', long = "query")]
    query: Option<String>,
    /// Filter to a specific model by name
    #[arg(short = 'm', long = "model")]
    model: Option<String>,
}

#[derive(Args)]
struct ModelsDefaultArgs {
    /// Optional name of the model to set as default
    model: Option<String>,
}

#[derive(Args)]
struct ModelsOptionsArgs {
    #[command(subcommand)]
    command: Option<ModelsOptionsSubcommand>,
}

#[derive(Subcommand)]
enum ModelsOptionsSubcommand {
    /// List all models with stored default options
    List(ModelsOptionsListArgs),
    /// Show stored options for a specific model
    Show(ModelsOptionsShowArgs),
    /// Set a default option for a model
    Set(ModelsOptionsSetArgs),
    /// Clear all stored options for a model
    Clear(ModelsOptionsClearArgs),
}

#[derive(Args)]
struct ModelsOptionsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ModelsOptionsShowArgs {
    /// Model name to show options for
    model: String,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ModelsOptionsSetArgs {
    /// Model name to set options for
    model: String,
    /// Option key (temperature, max_tokens, top_p, frequency_penalty, presence_penalty, system)
    key: String,
    /// Option value
    value: String,
}

#[derive(Args)]
struct ModelsOptionsClearArgs {
    /// Model name to clear options for
    model: String,
}

#[derive(Args)]
struct KeysArgs {
    #[command(subcommand)]
    command: KeysSubcommand,
}

#[derive(Subcommand)]
enum KeysSubcommand {
    /// Output the path to the keys.json file
    Path,
    /// Return the value of a stored key
    Get(KeysGetArgs),
    /// List stored key aliases
    List(KeysListArgs),
    /// Save a key in the keys.json file
    Set(KeysSetArgs),
    /// Resolve a key using alias/env precedence (experimental)
    Resolve(KeysResolveArgs),
}

#[derive(Args)]
struct KeysGetArgs {
    /// Alias/name of the stored key
    name: String,
}

#[derive(Args)]
struct KeysResolveArgs {
    /// Input value that may be a literal key or alias
    #[arg(long)]
    input: Option<String>,
    /// Alias to use when resolving
    #[arg(long)]
    alias: Option<String>,
    /// Environment variable to check as fallback
    #[arg(long)]
    env: Option<String>,
}

#[derive(Args)]
struct KeysListArgs {
    /// Output as JSON array
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct KeysSetArgs {
    /// Alias/name for the key
    name: String,
    /// Key value (if omitted, you will be prompted)
    #[arg(long)]
    value: Option<String>,
}

#[derive(Args)]
struct AliasesArgs {
    #[command(subcommand)]
    command: Option<AliasesSubcommand>,
}

#[derive(Subcommand)]
enum AliasesSubcommand {
    /// Output the path to the aliases.json file
    Path,
    /// List all defined aliases
    List(AliasesListArgs),
    /// Set an alias for a model
    Set(AliasesSetArgs),
    /// Remove an alias
    Remove(AliasesRemoveArgs),
}

#[derive(Args)]
struct AliasesListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct AliasesSetArgs {
    /// Alias name to define
    alias: String,
    /// Model identifier the alias should resolve to
    model: String,
}

#[derive(Args)]
struct AliasesRemoveArgs {
    /// Alias name to remove
    alias: String,
}
#[derive(Args)]
struct LogsArgs {
    #[command(subcommand)]
    command: Option<LogsSubcommand>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum LogsSubcommand {
    /// Output the path to the logs.db file
    Path,
    /// Show current logging status and summary information
    Status,
    /// Backup the logs database to a file
    Backup(LogsBackupArgs),
    /// Enable logging for all prompts
    On,
    /// Disable logging for all prompts
    Off,
    /// List stored log entries
    List(LogsListArgs),
}

#[derive(Args)]
struct LogsBackupArgs {
    /// Destination file for the backup
    path: String,
}

#[derive(Args, Default)]
struct LogsListArgs {
    /// Number of entries to show - defaults to 3, use 0 for all
    #[arg(short = 'n', long = "count")]
    count: Option<usize>,
    /// Output logs as JSON
    #[arg(long = "json")]
    json: bool,
    /// Filter by model identifier or alias
    #[arg(long = "model")]
    model: Option<String>,
    /// Search prompt and response text for this substring
    #[arg(short = 'q', long = "query")]
    query: Option<String>,
    /// Filter by conversation identifier
    #[arg(long = "conversation", alias = "cid")]
    conversation: Option<String>,
    /// Only include entries with an id greater than this value
    #[arg(long = "id-gt", conflicts_with = "id_gte")]
    id_gt: Option<String>,
    /// Only include entries with an id greater than or equal to this value
    #[arg(long = "id-gte", conflicts_with = "id_gt")]
    id_gte: Option<String>,
    /// Only include entries logged on or after this timestamp (RFC3339 or YYYY-MM-DD)
    #[arg(long = "since", value_parser = parse_datetime)]
    since: Option<DateTime<Utc>>,
    /// Only include entries logged before this timestamp
    #[arg(long = "before", alias = "until", value_parser = parse_datetime)]
    before: Option<DateTime<Utc>>,
    /// Alternative path to logs database (hidden flag for compatibility)
    #[arg(long = "path", hide = true)]
    path: Option<String>,
    /// Alternative path to logs database
    #[arg(long = "database")]
    database: Option<String>,
    /// Output only the response text (no prompt or metadata)
    #[arg(short = 'r', long = "response")]
    response_only: bool,
    /// Extract fenced code blocks from the response
    #[arg(short = 'x', long = "extract", conflicts_with = "extract_last")]
    extract: bool,
    /// Extract only the last fenced code block from the response
    #[arg(
        long = "extract-last",
        visible_alias = "xl",
        conflicts_with = "extract"
    )]
    extract_last: bool,
    /// Short output format (prompt and response only, no headers or metadata)
    #[arg(long = "short")]
    short: bool,
    /// Truncate long prompts and responses to a reasonable length
    #[arg(long = "truncate")]
    truncate: bool,
    /// Show only the most recent/latest entry
    #[arg(long = "latest", conflicts_with = "current")]
    latest: bool,
    /// Show entries from the current/most recent conversation
    #[arg(long = "current", conflicts_with = "latest")]
    current: bool,
    /// Filter to entries that used a specific fragment (reserved for compatibility)
    #[arg(long = "fragment", hide = true)]
    fragment: Option<String>,
    /// Filter to entries that made tool calls
    #[arg(long = "tools")]
    with_tools: bool,
    /// Filter to entries that used a specific schema (reserved for compatibility)
    #[arg(long = "schema", hide = true)]
    schema: Option<String>,
}

#[derive(Args)]
struct TemplatesArgs {
    #[command(subcommand)]
    command: Option<TemplatesSubcommand>,
}

#[derive(Subcommand)]
enum TemplatesSubcommand {
    /// Output the path to the templates directory
    Path,
    /// List all available templates
    List(TemplatesListArgs),
    /// Show the content of a template
    Show(TemplatesShowArgs),
    /// Edit a template (creates if it doesn't exist)
    Edit(TemplatesEditArgs),
    /// List available template loaders
    Loaders(TemplatesLoadersArgs),
}

#[derive(Args)]
struct TemplatesListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct TemplatesShowArgs {
    /// Name of the template to show
    name: String,
}

#[derive(Args)]
struct TemplatesEditArgs {
    /// Name of the template to edit
    name: String,
    /// Content to set (if omitted, opens editor)
    #[arg(long)]
    content: Option<String>,
}

#[derive(Args)]
struct TemplatesLoadersArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SchemasArgs {
    #[command(subcommand)]
    command: Option<SchemasSubcommand>,
}

#[derive(Subcommand)]
enum SchemasSubcommand {
    /// List all stored schemas
    List(SchemasListArgs),
    /// Show a specific schema by name/ID
    Show(SchemasShowArgs),
    /// Show schema DSL syntax help
    Dsl,
}

#[derive(Args)]
struct SchemasListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SchemasShowArgs {
    /// Name/ID of the schema to show
    name: String,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ToolsArgs {
    #[command(subcommand)]
    command: Option<ToolsSubcommand>,
}

#[derive(Subcommand)]
enum ToolsSubcommand {
    /// List all stored tools
    List(ToolsListArgs),
    /// Show a specific tool by name
    Show(ToolsShowArgs),
}

#[derive(Args)]
struct ToolsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Only show tools with function definitions (input_schema)
    #[arg(long)]
    functions: bool,
}

#[derive(Args)]
struct ToolsShowArgs {
    /// Name of the tool to show
    name: String,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

// ============================================================================
// Embeddings Command Arguments
// ============================================================================

#[derive(Args)]
struct EmbedArgs {
    /// Text to embed (reads from stdin if not provided)
    #[arg()]
    content: Vec<String>,
    /// Embedding model to use
    #[arg(short, long)]
    model: Option<String>,
    /// Store embedding in a collection
    #[arg(short, long)]
    store: Option<String>,
    /// ID for the stored embedding (required with --store)
    #[arg(long)]
    id: Option<String>,
    /// JSON metadata to attach to the stored embedding
    #[arg(long)]
    metadata: Option<String>,
    /// Output raw embedding vector (default: JSON format)
    #[arg(long)]
    raw: bool,
    /// Override the embeddings database path
    #[arg(long)]
    database: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct EmbedModelsArgs {
    #[command(subcommand)]
    command: Option<EmbedModelsSubcommand>,
}

#[derive(Subcommand)]
enum EmbedModelsSubcommand {
    /// List available embedding models
    List(EmbedModelsListArgs),
    /// Get or set the default embedding model
    Default(EmbedModelsDefaultArgs),
}

#[derive(Args)]
struct EmbedModelsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct EmbedModelsDefaultArgs {
    /// Model to set as default (if omitted, shows current default)
    model: Option<String>,
}

#[derive(Args)]
struct EmbedMultiArgs {
    /// Collection name to store embeddings
    collection: String,
    /// Embedding model to use
    #[arg(short, long)]
    model: Option<String>,
    /// Files to embed (can specify multiple)
    #[arg(short, long = "files", value_name = "PATH")]
    files: Vec<PathBuf>,
    /// SQL query to read content from logs database
    #[arg(long)]
    sql: Option<String>,
    /// Batch size for embedding requests
    #[arg(long, default_value = "100")]
    batch_size: usize,
    /// Store content text along with embeddings
    #[arg(long)]
    store_content: bool,
    /// Override the embeddings database path
    #[arg(long)]
    database: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct SimilarArgs {
    /// Text to find similar embeddings for
    #[arg()]
    query: Vec<String>,
    /// Collection to search in
    #[arg(short, long, required = true)]
    collection: String,
    /// Number of results to return
    #[arg(short, long, default_value = "10")]
    number: usize,
    /// Embedding model to use (must match collection's model)
    #[arg(short, long)]
    model: Option<String>,
    /// Find similar to an existing entry by ID instead of query text
    #[arg(long)]
    id: Option<String>,
    /// Override the embeddings database path
    #[arg(long)]
    database: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct CollectionsArgs {
    #[command(subcommand)]
    command: Option<CollectionsSubcommand>,
}

#[derive(Subcommand)]
enum CollectionsSubcommand {
    /// List all collections
    List(CollectionsListArgs),
    /// Output the path to the embeddings database
    Path,
    /// Delete a collection
    Delete(CollectionsDeleteArgs),
}

#[derive(Args)]
struct CollectionsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Override the embeddings database path
    #[arg(long)]
    database: Option<String>,
}

#[derive(Args)]
struct CollectionsDeleteArgs {
    /// Name of the collection to delete
    name: String,
    /// Override the embeddings database path
    #[arg(long)]
    database: Option<String>,
}

/// Migrate legacy `-c <id>` flag usage to `--cid <id>`.
///
/// The upstream CLI changed the -c flag from taking an optional conversation ID
/// to being a boolean --continue flag. For backwards compatibility, we detect
/// the legacy pattern `-c <id> <prompt>` (where there's additional content after
/// the ID) and rewrite it to `--cid <id> <prompt>`, emitting a deprecation warning.
///
/// We only trigger migration when:
/// 1. `-c=<value>` form is used (unambiguous legacy syntax)
/// 2. `-c <value>` is followed by more non-flag arguments (likely legacy usage)
///
/// If `-c <value>` has no additional arguments, we let clap handle it with new
/// semantics (continue=true, value becomes the prompt).
fn migrate_legacy_continuation_args(args: Vec<String>) -> Vec<String> {
    let mut result = Vec::with_capacity(args.len());
    let args_vec: Vec<String> = args;
    let mut i = 0;

    while i < args_vec.len() {
        let arg = &args_vec[i];

        // Handle `-c=<value>` form (unambiguous legacy syntax)
        if arg.starts_with("-c=") {
            let id = arg.strip_prefix("-c=").unwrap().to_string();
            eprintln!(
                "Warning: `-c=<id>` is deprecated. Use `--cid {}` instead. \
                 The -c flag now means --continue (continue most recent conversation).",
                id
            );
            result.push(format!("--cid={}", id));
            i += 1;
            continue;
        }

        // Check for legacy `-c <value>` pattern
        if arg == "-c" && i + 1 < args_vec.len() {
            let next = &args_vec[i + 1];
            // Only migrate if:
            // 1. Next arg doesn't start with '-' (it's a value, not a flag)
            // 2. There's at least one more argument after that (the prompt)
            if !next.starts_with('-') && !next.is_empty() {
                // Check if there are more non-flag arguments after the potential ID
                let has_more_args = args_vec[i + 2..].iter().any(|a| !a.starts_with('-'));

                if has_more_args {
                    // This looks like legacy `-c <id> <prompt>` usage
                    let id = next.clone();
                    eprintln!(
                        "Warning: `-c <id>` is deprecated. Use `--cid {}` instead. \
                         The -c flag now means --continue (continue most recent conversation).",
                        id
                    );
                    result.push("--cid".to_string());
                    result.push(id);
                    i += 2;
                    continue;
                }
            }
        }

        result.push(arg.clone());
        i += 1;
    }

    result
}

fn main() -> Result<()> {
    let args = migrate_legacy_continuation_args(env::args().collect());
    let cli = Cli::parse_from(args);
    init_tracing(&cli.logging);
    let logging = cli.logging.clone();
    let prompt_input = cli.prompt_input.clone();
    match cli.command {
        Some(Command::Prompt(args)) => {
            run_prompt(args.input, &logging)?;
        }
        Some(Command::Plugins(args)) => {
            list_plugins(args.json)?;
        }
        Some(Command::Models(args)) => {
            handle_models(args)?;
        }
        Some(Command::Keys(args)) => {
            handle_keys(args)?;
        }
        Some(Command::Aliases(args)) => {
            handle_aliases(args)?;
        }
        Some(Command::Logs(args)) => {
            handle_logs(args)?;
        }
        Some(Command::Chat(args)) => {
            run_chat(args, &logging)?;
        }
        Some(Command::Templates(args)) => {
            handle_templates(args)?;
        }
        Some(Command::Schemas(args)) => {
            handle_schemas(args)?;
        }
        Some(Command::Tools(args)) => {
            handle_tools(args)?;
        }
        Some(Command::Embed(args)) => {
            handle_embed(args)?;
        }
        Some(Command::EmbedModels(args)) => {
            handle_embed_models(args)?;
        }
        Some(Command::EmbedMulti(args)) => {
            handle_embed_multi(args)?;
        }
        Some(Command::Similar(args)) => {
            handle_similar(args)?;
        }
        Some(Command::Collections(args)) => {
            handle_collections(args)?;
        }
        Some(Command::Cmd(args)) => {
            run_cmd(args, &logging)?;
        }
        Some(Command::Version(args)) => {
            print_version(args.verbose);
        }
        None => {
            if prompt_input.prompt.is_empty() && io::stdin().is_terminal() {
                Cli::command().print_help()?;
                println!();
            } else {
                run_prompt(prompt_input, &logging)?;
            }
        }
    }
    Ok(())
}

fn run_prompt(input: PromptInputArgs, logging: &LoggingOptions) -> Result<()> {
    let PromptInputArgs {
        options,
        system,
        key,
        attachments,
        attachment_types,
        continue_conversation,
        cid,
        conversation_name,
        conversation_model,
        prompt: words,
    } = input;

    // Resolve conversation ID: --continue uses latest, --cid uses explicit ID
    let conversation_id = if continue_conversation {
        // Get the most recent conversation ID from logs
        get_latest_conversation_id()?
    } else {
        cid.clone()
    };

    // Check if stdin has no attachment that uses it
    let stdin_used_for_attachment = attachments.contains(&"-".to_string());

    // Read stdin if piped and not used for attachment
    let stdin_content = if !stdin_used_for_attachment && !io::stdin().is_terminal() {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read from stdin")?;
        // Only use stdin if it actually has content
        if buffer.trim().is_empty() {
            None
        } else {
            Some(buffer)
        }
    } else {
        None
    };

    // Merge stdin (first) with positional prompt (second) per upstream semantics
    let positional_prompt = words.join(" ");
    let prompt = match (&stdin_content, positional_prompt.is_empty()) {
        (Some(stdin), true) => stdin.trim().to_string(),
        (Some(stdin), false) => format!("{}\n{}", stdin.trim(), positional_prompt),
        (None, _) => positional_prompt,
    };

    // Resolve model: explicit --model takes precedence, then --query discovery
    let resolved_model: Option<String> = if options.model.is_some() {
        options.model.clone()
    } else if let Some(ref query) = options.query {
        let matches = query_models(query)?;
        matches.first().map(|m| m.name.clone())
    } else {
        None
    };

    info!(%prompt, "Executing prompt via llm-core");
    let config = PromptConfig {
        database_path: options.database.as_deref(),
        model: resolved_model.as_deref(),
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        retries: options.retries.map(|v| v as usize),
        retry_backoff_ms: options.retry_backoff_ms,
        api_key: key.as_deref(),
        log_override: options.log_override(),
        conversation_id: conversation_id.as_deref(),
        conversation_name: conversation_name.as_deref(),
        conversation_model: conversation_model.as_deref().or(resolved_model.as_deref()),
    };
    let streaming = !options.no_stream;
    log_prompt_debug(logging, streaming, &config)?;
    let resolved_attachments = resolve_prompt_attachments(&attachments, &attachment_types)?;
    // Load conversation history if continuing
    let mut messages = if let Some(ref conv_id) = conversation_id {
        load_conversation_messages(conv_id)?
    } else {
        Vec::new()
    };

    // Build new messages from current prompt
    let new_messages = build_messages(system.as_deref(), &prompt);

    // Append new messages to history (or use new messages if no history)
    if messages.is_empty() {
        messages = new_messages;
    } else {
        // Skip system prompt from new messages if history already has one
        for msg in new_messages {
            if matches!(msg.role, MessageRole::System)
                && messages
                    .iter()
                    .any(|m| matches!(m.role, MessageRole::System))
            {
                continue;
            }
            messages.push(msg);
        }
    }

    if streaming {
        let mut sink = StdoutStreamSink::default();
        stream_prompt_with_messages(messages, resolved_attachments, config, &mut sink)?;
    } else {
        let response = execute_prompt_with_messages(messages, resolved_attachments, config)?;
        println!("{response}");
    }

    // Note: --usage flag is implemented but requires UsageInfo from providers.
    // Currently providers return String; full usage printing needs provider changes
    // to return UsageInfo in the response.
    if options.usage {
        eprintln!("Token usage: (usage statistics require provider support)");
    }

    Ok(())
}

fn run_chat(args: ChatArgs, logging: &LoggingOptions) -> Result<()> {
    let ChatArgs {
        options,
        system,
        key,
        continue_conversation,
        cid,
        conversation_name,
        conversation_model,
    } = args;

    // Track if we are continuing an existing conversation
    let is_continuation = continue_conversation || cid.is_some();

    // Resolve or create conversation ID
    let conversation_id = if continue_conversation {
        get_latest_conversation_id()?.unwrap_or_else(generate_response_ulid)
    } else if let Some(id) = cid {
        id
    } else {
        generate_response_ulid()
    };

    // Resolve model
    let resolved_model: Option<String> = if options.model.is_some() {
        options.model.clone()
    } else if let Some(ref query) = options.query {
        let matches = query_models(query)?;
        matches.first().map(|m| m.name.clone())
    } else {
        None
    };

    // Load conversation history if continuing

    let mut messages = if is_continuation {
        load_conversation_messages(&conversation_id).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Add system prompt if provided and not already in history
    if let Some(ref sys) = system {
        let has_system = messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::System));
        if !has_system && !sys.trim().is_empty() {
            messages.insert(0, PromptMessage::system(sys.trim()));
        }
    }

    println!(
        "Chatting with {}. Type '!help' for commands.",
        resolved_model.as_deref().unwrap_or("default model")
    );
    println!("Conversation ID: {}", conversation_id);
    println!();

    let mut editor = DefaultEditor::new().context("failed to initialize readline editor")?;
    let mut multi_line_mode = false;
    let mut multi_line_buffer = String::new();

    // Set up Ctrl+C handler for response cancellation
    let interrupted = Arc::new(AtomicBool::new(false));

    loop {
        let prompt = if multi_line_mode { "... " } else { "> " };

        // Reset interrupt flag at start of each prompt
        interrupted.store(false, Ordering::SeqCst);

        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                if multi_line_mode {
                    println!("Multi-line input cancelled.");
                    multi_line_mode = false;
                    multi_line_buffer.clear();
                    continue;
                }
                println!("\nUse '!exit' or Ctrl+D to exit.");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("\nGoodbye!");
                break;
            }
            Err(ReadlineError::WindowResized) => continue,
            Err(err) => return Err(err.into()),
        };

        // Handle multi-line mode
        if multi_line_mode {
            if line.trim() == "!end" || line.is_empty() {
                multi_line_mode = false;
                let input = std::mem::take(&mut multi_line_buffer);
                if !input.trim().is_empty() {
                    process_chat_input(
                        &input,
                        &mut messages,
                        &conversation_id,
                        conversation_name.as_deref(),
                        conversation_model.as_deref().or(resolved_model.as_deref()),
                        &options,
                        key.as_deref(),
                        logging,
                        &interrupted,
                    )?;
                }
                continue;
            }
            multi_line_buffer.push_str(&line);
            multi_line_buffer.push('\n');
            continue;
        }

        let trimmed = line.trim();

        // Handle special commands
        if trimmed.starts_with('!') {
            match trimmed {
                "!exit" | "!quit" | "!q" => {
                    println!("Goodbye!");
                    break;
                }
                "!help" | "!h" | "!?" => {
                    print_chat_help();
                    continue;
                }
                "!multi" | "!m" => {
                    println!("Entering multi-line mode. Type '!end' or empty line to send.");
                    multi_line_mode = true;
                    multi_line_buffer.clear();
                    continue;
                }
                "!edit" | "!e" => {
                    match edit_chat_input()? {
                        Some(input) if !input.trim().is_empty() => {
                            process_chat_input(
                                &input,
                                &mut messages,
                                &conversation_id,
                                conversation_name.as_deref(),
                                conversation_model.as_deref().or(resolved_model.as_deref()),
                                &options,
                                key.as_deref(),
                                logging,
                                &interrupted,
                            )?;
                        }
                        _ => println!("Edit cancelled or empty."),
                    }
                    continue;
                }
                "!clear" => {
                    // Clear conversation history but keep system prompt
                    let system_msg = messages
                        .iter()
                        .find(|m| matches!(m.role, MessageRole::System))
                        .cloned();
                    messages.clear();
                    if let Some(sys) = system_msg {
                        messages.push(sys);
                    }
                    println!("Conversation history cleared.");
                    continue;
                }
                cmd if cmd.starts_with("!fragment ") || cmd.starts_with("!f ") => {
                    let name = cmd.split_whitespace().nth(1).unwrap_or("");
                    println!("Fragment support is not yet implemented. Fragment '{}' would be inserted here.", name);
                    continue;
                }
                _ => {
                    println!(
                        "Unknown command: {}. Type '!help' for available commands.",
                        trimmed
                    );
                    continue;
                }
            }
        }

        // Skip empty input
        if trimmed.is_empty() {
            continue;
        }

        // Add to history
        let _ = editor.add_history_entry(&line);

        // Process the input
        process_chat_input(
            trimmed,
            &mut messages,
            &conversation_id,
            conversation_name.as_deref(),
            conversation_model.as_deref().or(resolved_model.as_deref()),
            &options,
            key.as_deref(),
            logging,
            &interrupted,
        )?;
    }

    Ok(())
}

fn print_chat_help() {
    println!("Chat commands:");
    println!("  !help, !h, !?     - Show this help");
    println!("  !exit, !quit, !q  - Exit chat");
    println!("  !multi, !m        - Enter multi-line input mode (end with !end or empty line)");
    println!("  !edit, !e         - Open $EDITOR for input");
    println!("  !fragment <name>  - Insert a named fragment (not yet implemented)");
    println!("  !clear            - Clear conversation history");
    println!();
    println!("Press Ctrl+C during a response to cancel it.");
    println!("Press Ctrl+D at the prompt to exit.");
}

fn edit_chat_input() -> Result<Option<String>> {
    let mut file = NamedTempFile::new().context("failed to create temporary file")?;
    file.flush()?;
    let path = file.into_temp_path();

    let mut command = build_editor_command()?;
    let path_ref: &Path = path.as_ref();
    command.arg(path_ref);
    let status = command.status().context("failed to launch editor")?;
    if !status.success() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).context("failed to read edited input")?;
    let edited = contents.trim().to_string();
    if edited.is_empty() {
        Ok(None)
    } else {
        Ok(Some(edited))
    }
}

#[allow(clippy::too_many_arguments)]
fn process_chat_input(
    input: &str,
    messages: &mut Vec<PromptMessage>,
    conversation_id: &str,
    conversation_name: Option<&str>,
    conversation_model: Option<&str>,
    options: &PromptOptions,
    key: Option<&str>,
    logging: &LoggingOptions,
    interrupted: &Arc<AtomicBool>,
) -> Result<()> {
    // Add user message to history
    messages.push(PromptMessage::user(input));

    let config = PromptConfig {
        database_path: options.database.as_deref(),
        model: options.model.as_deref(),
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        retries: options.retries.map(|v| v as usize),
        retry_backoff_ms: options.retry_backoff_ms,
        api_key: key,
        log_override: options.log_override(),
        conversation_id: Some(conversation_id),
        conversation_name,
        conversation_model,
    };

    log_prompt_debug(logging, !options.no_stream, &config)?;

    // Stream the response
    let mut sink = ChatStreamSink::new(interrupted.clone());

    match stream_prompt_with_messages(messages.clone(), Vec::new(), config, &mut sink) {
        Ok(response) => {
            // Add assistant response to history
            messages.push(PromptMessage::assistant(&response));

            if options.usage {
                eprintln!("Token usage: (usage statistics require provider support)");
            }
        }
        Err(e) => {
            // Remove the user message if the request failed
            messages.pop();

            if sink.was_interrupted() {
                println!("\n[Response cancelled]");
            } else {
                eprintln!("Error: {}", e);
            }
        }
    }

    println!();
    Ok(())
}

/// Stream sink for chat that supports interruption via Ctrl+C
struct ChatStreamSink {
    started: bool,
    interrupted: Arc<AtomicBool>,
}

impl ChatStreamSink {
    fn new(interrupted: Arc<AtomicBool>) -> Self {
        Self {
            started: false,
            interrupted,
        }
    }

    fn was_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

impl StreamSink for ChatStreamSink {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()> {
        // Check for interruption
        if self.interrupted.load(Ordering::SeqCst) {
            bail!("Response cancelled by user");
        }

        print!("{}", delta);
        io::stdout().flush().context("failed to flush stdout")?;
        self.started = true;
        Ok(())
    }

    fn handle_done(&mut self) -> Result<()> {
        if self.started {
            println!();
        }
        Ok(())
    }
}

fn run_cmd(args: CmdArgs, logging: &LoggingOptions) -> Result<()> {
    if args.prompt.is_empty() {
        bail!("Describe the command you would like to run.");
    }
    let prompt = args.prompt.join(" ");
    let system_prompt = args.system.as_deref().unwrap_or(CMD_SYSTEM_PROMPT);
    info!(%prompt, "Generating shell command via llm-core");

    // Resolve conversation ID: --continue uses latest, --cid uses explicit ID
    let conversation_id = if args.continue_conversation {
        get_latest_conversation_id()?
    } else {
        args.cid.clone()
    };

    let config = PromptConfig {
        database_path: args.options.database.as_deref(),
        model: args.options.model.as_deref(),
        temperature: args.options.temperature,
        max_tokens: args.options.max_tokens,
        retries: args.options.retries.map(|v| v as usize),
        retry_backoff_ms: args.options.retry_backoff_ms,
        api_key: args.key.as_deref(),
        log_override: args.options.log_override(),
        conversation_id: conversation_id.as_deref(),
        conversation_name: args.conversation_name.as_deref(),
        conversation_model: args
            .conversation_model
            .as_deref()
            .or(args.options.model.as_deref()),
    };
    log_prompt_debug(logging, false, &config)?;

    // Load conversation history if continuing
    let mut messages = if let Some(ref conv_id) = conversation_id {
        load_conversation_messages(conv_id)?
    } else {
        Vec::new()
    };

    // Build new messages for this command
    let new_messages = build_cmd_messages(system_prompt, &prompt);

    // Append new messages to history (or use new messages if no history)
    if messages.is_empty() {
        messages = new_messages;
    } else {
        // Skip system prompt from new messages if history already has one
        for msg in new_messages {
            if matches!(msg.role, MessageRole::System)
                && messages
                    .iter()
                    .any(|m| matches!(m.role, MessageRole::System))
            {
                continue;
            }
            messages.push(msg);
        }
    }

    let suggestion = execute_prompt_with_messages(messages, Vec::new(), config)?;
    let suggestion = suggestion.trim().to_string();
    if suggestion.is_empty() {
        bail!("Model returned an empty command suggestion.");
    }

    let maybe_command = edit_command(&suggestion)?;
    match maybe_command {
        Some(command) => {
            if command.trim().is_empty() {
                println!("Aborted: generated command is empty after edits.");
                return Ok(());
            }
            execute_shell_command(&command)?;
        }
        None => {
            println!("Cancelled command execution.");
        }
    }
    Ok(())
}

fn build_messages(system_prompt: Option<&str>, user_prompt: &str) -> Vec<PromptMessage> {
    let mut messages = Vec::new();
    if let Some(system_prompt) = system_prompt {
        let trimmed_system = system_prompt.trim();
        if !trimmed_system.is_empty() {
            messages.push(PromptMessage::system(trimmed_system));
        }
    }
    messages.push(PromptMessage::user(user_prompt));
    messages
}

fn build_cmd_messages(system_prompt: &str, user_prompt: &str) -> Vec<PromptMessage> {
    build_messages(Some(system_prompt), user_prompt)
}

fn edit_command(initial: &str) -> Result<Option<String>> {
    if auto_accept_commands() {
        println!("Auto-accepting generated command.");
        return Ok(Some(initial.to_string()));
    }
    if initial.contains('\n') {
        println!("Generated a multi-line command. Opening your editor for review.");
        edit_with_editor(initial)
    } else {
        edit_with_line_editor(initial)
    }
}

fn edit_with_line_editor(initial: &str) -> Result<Option<String>> {
    println!("Review generated command. Press Enter to accept, edit before executing, or Ctrl+C to cancel.");
    let mut editor = DefaultEditor::new().context("failed to initialize line editor")?;
    loop {
        match editor.readline_with_initial("> ", (initial, "")) {
            Ok(line) => return Ok(Some(line)),
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => return Ok(None),
            Err(ReadlineError::WindowResized) => continue,
            Err(err) => return Err(err.into()),
        }
    }
}

fn edit_with_editor(initial: &str) -> Result<Option<String>> {
    let mut file = NamedTempFile::new().context("failed to create temporary file")?;
    write!(file, "{}", initial).context("failed to write to temporary file")?;
    file.flush().context("failed to flush temporary file")?;
    let path = file.into_temp_path();

    let mut command = build_editor_command()?;
    let path_ref: &Path = path.as_ref();
    command.arg(path_ref);
    let status = command.status().context("failed to launch editor")?;
    if !status.success() {
        bail!("Editor exited with status {status}");
    }

    let contents = fs::read_to_string(&path).context("failed to read edited command")?;
    let edited = contents.trim_end_matches(&['\r', '\n'][..]).to_string();
    Ok(Some(edited))
}

fn build_editor_command() -> Result<ProcessCommand> {
    let spec = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".to_string()
            } else {
                "nano".to_string()
            }
        });

    let mut parts = shell_split(&spec).unwrap_or_else(|_| vec![spec.clone()]);
    if parts.is_empty() {
        parts.push(spec);
    }
    let program = parts.remove(0);
    let mut command = ProcessCommand::new(program);
    if !parts.is_empty() {
        command.args(parts);
    }
    Ok(command)
}

fn execute_shell_command(command: &str) -> Result<()> {
    println!("Executing: {command}");
    let mut process = if cfg!(windows) {
        let mut cmd = ProcessCommand::new("cmd");
        cmd.args(["/C", command]);
        cmd
    } else {
        let mut cmd = ProcessCommand::new("sh");
        cmd.args(["-c", command]);
        cmd
    };
    let output = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to execute generated command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        if !stdout.trim().is_empty() {
            print!("{stdout}");
        }
        if !stderr.trim().is_empty() {
            eprint!("{stderr}");
        }
    } else {
        let code = output.status.code().unwrap_or(-1);
        let combined = if stderr.trim().is_empty() {
            stdout.to_string()
        } else {
            format!("{stdout}{stderr}")
        };
        println!("Command failed with error (exit status {code}): {combined}");
    }
    Ok(())
}

fn auto_accept_commands() -> bool {
    truthy_env("LLM_CMD_AUTO_ACCEPT") || !io::stdin().is_terminal()
}

fn truthy_env(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn log_prompt_debug(
    logging: &LoggingOptions,
    streaming: bool,
    config: &PromptConfig<'_>,
) -> Result<()> {
    if logging.debug {
        let info = prompt_debug_info(config)?;
        tracing::debug!(
            model = %info.model,
            provider = %info.provider,
            temperature = ?info.temperature,
            max_tokens = ?info.max_tokens,
            retries = info.retries,
            retry_backoff_ms = info.retry_backoff_ms,
            streaming,
            "prompt_debug_info"
        );
    }
    Ok(())
}

fn print_version(verbose: bool) {
    println!("llm {}", env!("CARGO_PKG_VERSION"));
    if verbose {
        println!("llm-core {}", core_version());
    }
}

fn init_tracing(logging: &LoggingOptions) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = if logging.debug {
            "debug"
        } else if logging.info {
            "info"
        } else {
            "warn"
        };
        level.into()
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn list_plugins(json: bool) -> Result<()> {
    let plugins = load_plugins().context("failed to load plugins")?;
    if json {
        let names: Vec<_> = plugins.iter().map(|p| &p.name).collect();
        let json = serde_json::to_string_pretty(&names)?;
        println!("{json}");
    } else if plugins.is_empty() {
        println!("No plugins loaded.");
    } else {
        for plugin in plugins {
            println!("{}", plugin.name);
        }
    }
    Ok(())
}

fn handle_models(args: ModelsArgs) -> Result<()> {
    match args.command {
        Some(ModelsSubcommand::List(list_args)) => list_models(list_args),
        Some(ModelsSubcommand::Default(def_args)) => default_model(def_args),
        Some(ModelsSubcommand::Options(opts_args)) => handle_models_options(opts_args),
        None => list_models(ModelsListArgs {
            json: false,
            with_options: false,
            with_async: false,
            with_schemas: false,
            with_tools: false,
            query: None,
            model: None,
        }),
    }
}

fn list_models(args: ModelsListArgs) -> Result<()> {
    let mut models: Vec<ModelInfo> = available_models()?;

    // Apply filters
    if args.with_options {
        models.retain(|m| m.has_options);
    }
    if args.with_async {
        models.retain(|m| m.supports_async);
    }
    if args.with_schemas {
        models.retain(|m| m.supports_schemas);
    }
    if args.with_tools {
        models.retain(|m| m.supports_tools);
    }
    if let Some(ref model_name) = args.model {
        let model_lower = model_name.to_ascii_lowercase();
        models.retain(|m| m.name.to_ascii_lowercase() == model_lower);
    }
    if let Some(ref query) = args.query {
        let query_lower = query.to_ascii_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        models.retain(|m| {
            let name_lower = m.name.to_ascii_lowercase();
            let desc_lower = m.description.to_ascii_lowercase();
            let aliases_lower: Vec<String> =
                m.aliases.iter().map(|a| a.to_ascii_lowercase()).collect();
            terms.iter().all(|term| {
                name_lower.contains(term)
                    || desc_lower.contains(term)
                    || aliases_lower.iter().any(|a| a.contains(term))
            })
        });
    }

    models.sort_by(|a, b| a.name.cmp(&b.name));
    models.sort_by_key(|m| (!m.is_default, m.name.clone()));

    if args.json {
        let json = serde_json::to_string_pretty(&models)?;
        println!("{json}");
        return Ok(());
    }

    for model in &models {
        let marker = if model.is_default { "*" } else { " " };
        let alias_text = if model.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", model.aliases.join(", "))
        };
        println!(
            "{marker} {} ({}) - {}{}",
            model.name, model.provider, model.description, alias_text
        );
    }
    if !models.is_empty() {
        println!("\n* indicates the default model");
    }
    Ok(())
}

fn default_model(args: ModelsDefaultArgs) -> Result<()> {
    if let Some(name) = args.model {
        set_default_model(&name)?;
        let current = get_default_model()?.unwrap_or(name);
        println!("Default model set to {current}.");
    } else {
        match get_default_model()? {
            Some(current) => println!("Current default model: {current}"),
            None => println!(
                "No default model configured. Use `llm models default <model>` to set one."
            ),
        }
    }
    Ok(())
}

fn handle_models_options(args: ModelsOptionsArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(ModelsOptionsSubcommand::List(ModelsOptionsListArgs {
            json: false,
        }));
    match command {
        ModelsOptionsSubcommand::List(list_args) => models_options_list(list_args),
        ModelsOptionsSubcommand::Show(show_args) => models_options_show(show_args),
        ModelsOptionsSubcommand::Set(set_args) => models_options_set(set_args),
        ModelsOptionsSubcommand::Clear(clear_args) => models_options_clear(clear_args),
    }
}

fn models_options_list(args: ModelsOptionsListArgs) -> Result<()> {
    let options = list_model_options()?;
    if args.json {
        let map: std::collections::HashMap<String, ModelOptions> = options.into_iter().collect();
        let json = serde_json::to_string_pretty(&map)?;
        println!("{json}");
        return Ok(());
    }
    if options.is_empty() {
        println!("No model options configured.");
        return Ok(());
    }
    for (model, opts) in options {
        println!("{model}:");
        print_model_options(&opts, "  ");
    }
    Ok(())
}

fn models_options_show(args: ModelsOptionsShowArgs) -> Result<()> {
    let options = get_model_options(&args.model)?;
    match options {
        Some(opts) => {
            if args.json {
                let json = serde_json::to_string_pretty(&opts)?;
                println!("{json}");
            } else {
                println!("Options for {}:", args.model);
                print_model_options(&opts, "  ");
            }
        }
        None => {
            if args.json {
                println!("null");
            } else {
                println!("No options configured for '{}'.", args.model);
            }
        }
    }
    Ok(())
}

fn models_options_set(args: ModelsOptionsSetArgs) -> Result<()> {
    let mut opts = get_model_options(&args.model)?.unwrap_or_default();

    match args.key.as_str() {
        "temperature" => {
            let value: f32 = args
                .value
                .parse()
                .with_context(|| format!("Invalid temperature value: {}", args.value))?;
            opts.temperature = Some(value);
        }
        "max_tokens" | "max-tokens" => {
            let value: u32 = args
                .value
                .parse()
                .with_context(|| format!("Invalid max_tokens value: {}", args.value))?;
            opts.max_tokens = Some(value);
        }
        "top_p" | "top-p" => {
            let value: f32 = args
                .value
                .parse()
                .with_context(|| format!("Invalid top_p value: {}", args.value))?;
            opts.top_p = Some(value);
        }
        "frequency_penalty" | "frequency-penalty" => {
            let value: f32 = args
                .value
                .parse()
                .with_context(|| format!("Invalid frequency_penalty value: {}", args.value))?;
            opts.frequency_penalty = Some(value);
        }
        "presence_penalty" | "presence-penalty" => {
            let value: f32 = args
                .value
                .parse()
                .with_context(|| format!("Invalid presence_penalty value: {}", args.value))?;
            opts.presence_penalty = Some(value);
        }
        "system" => {
            opts.system = Some(args.value.clone());
        }
        "stop" => {
            // Parse as comma-separated list or JSON array
            let stops = if args.value.starts_with('[') {
                serde_json::from_str(&args.value)
                    .with_context(|| format!("Invalid stop sequences JSON: {}", args.value))?
            } else {
                args.value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            };
            opts.stop = Some(stops);
        }
        other => {
            bail!("Unknown option key '{}'. Valid keys: temperature, max_tokens, top_p, frequency_penalty, presence_penalty, system, stop", other);
        }
    }

    set_model_options(&args.model, &opts)?;
    println!(
        "Set {} = {} for model '{}'.",
        args.key, args.value, args.model
    );
    Ok(())
}

fn models_options_clear(args: ModelsOptionsClearArgs) -> Result<()> {
    let removed = remove_model_options(&args.model)?;
    if removed {
        println!("Cleared all options for '{}'.", args.model);
    } else {
        println!("No options were configured for '{}'.", args.model);
    }
    Ok(())
}

fn print_model_options(opts: &ModelOptions, indent: &str) {
    if let Some(temp) = opts.temperature {
        println!("{indent}temperature: {temp}");
    }
    if let Some(max) = opts.max_tokens {
        println!("{indent}max_tokens: {max}");
    }
    if let Some(top_p) = opts.top_p {
        println!("{indent}top_p: {top_p}");
    }
    if let Some(freq) = opts.frequency_penalty {
        println!("{indent}frequency_penalty: {freq}");
    }
    if let Some(pres) = opts.presence_penalty {
        println!("{indent}presence_penalty: {pres}");
    }
    if let Some(ref system) = opts.system {
        let display = if system.len() > 50 {
            format!("{}...", &system[..50])
        } else {
            system.clone()
        };
        println!("{indent}system: \"{display}\"");
    }
    if let Some(ref stops) = opts.stop {
        println!("{indent}stop: {:?}", stops);
    }
}

fn handle_keys(args: KeysArgs) -> Result<()> {
    match args.command {
        KeysSubcommand::Path => {
            let path = keys_path()?;
            println!("{}", path.display());
        }
        KeysSubcommand::Get(get_args) => {
            let keys = load_keys()?;
            let value = keys.get(&get_args.name).ok_or_else(|| {
                anyhow!(
                    "No key found with name '{}'. Use the Python CLI's 'llm keys set' for now.",
                    get_args.name
                )
            })?;
            println!("{value}");
        }
        KeysSubcommand::List(list_args) => {
            list_keys(list_args.json)?;
        }
        KeysSubcommand::Set(set_args) => {
            let value = match set_args.value {
                Some(v) => v,
                None => prompt_password("Enter key: ")?,
            };
            save_key(&set_args.name, value.trim())?;
            println!("Saved key '{}'.", set_args.name);
        }
        KeysSubcommand::Resolve(resolve_args) => {
            let key = resolve_key(KeyQuery {
                input: resolve_args.input.as_deref(),
                alias: resolve_args.alias.as_deref(),
                env: resolve_args.env.as_deref(),
            })?;
            if let Some(value) = key {
                println!("{value}");
            } else {
                bail!("No key could be resolved");
            }
        }
    }
    Ok(())
}

fn handle_aliases(args: AliasesArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(AliasesSubcommand::List(AliasesListArgs { json: false }));
    match command {
        AliasesSubcommand::Path => {
            let path = aliases_path()?;
            println!("{}", path.display());
        }
        AliasesSubcommand::List(list_args) => {
            list_aliases_command(list_args.json)?;
        }
        AliasesSubcommand::Set(set_args) => {
            set_alias(&set_args.alias, &set_args.model)?;
            println!(
                "Alias '{}' now points to '{}'.",
                set_args.alias, set_args.model
            );
        }
        AliasesSubcommand::Remove(remove_args) => {
            let removed = remove_alias(&remove_args.alias)?;
            if removed {
                println!("Alias '{}' removed.", remove_args.alias);
            } else {
                bail!("No alias '{}' found.", remove_args.alias);
            }
        }
    }
    Ok(())
}

fn list_aliases_command(as_json: bool) -> Result<()> {
    let aliases = list_aliases()?;
    if aliases.is_empty() {
        if !as_json {
            println!("No aliases defined.");
        } else {
            println!("{{}}");
        }
        return Ok(());
    }
    if as_json {
        let map: std::collections::HashMap<String, String> = aliases.into_iter().collect();
        let json = serde_json::to_string_pretty(&map)?;
        println!("{json}");
    } else {
        for (alias, model) in aliases {
            println!("{alias}: {model}");
        }
    }
    Ok(())
}
fn handle_logs(args: LogsArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or_else(|| LogsSubcommand::List(LogsListArgs::default()));
    match command {
        LogsSubcommand::Path => {
            let path = logs_db_path()?;
            println!("{}", path.display());
        }
        LogsSubcommand::Status => {
            print_logs_status()?;
        }
        LogsSubcommand::Backup(backup_args) => {
            backup_logs_command(backup_args)?;
        }
        LogsSubcommand::On => {
            set_logging_enabled(true)?;
            println!("Logging enabled for all prompts.");
        }
        LogsSubcommand::Off => {
            set_logging_enabled(false)?;
            println!("Logging disabled. Prompts will not be recorded.");
        }
        LogsSubcommand::List(list_args) => {
            list_logs_command(list_args)?;
        }
    }
    Ok(())
}

fn handle_templates(args: TemplatesArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(TemplatesSubcommand::List(TemplatesListArgs { json: false }));
    match command {
        TemplatesSubcommand::Path => {
            let path = templates_path()?;
            println!("{}", path.display());
        }
        TemplatesSubcommand::List(list_args) => {
            list_templates_command(list_args)?;
        }
        TemplatesSubcommand::Show(show_args) => {
            show_template_command(show_args)?;
        }
        TemplatesSubcommand::Edit(edit_args) => {
            edit_template_command(edit_args)?;
        }
        TemplatesSubcommand::Loaders(loaders_args) => {
            list_loaders_command(loaders_args)?;
        }
    }
    Ok(())
}

fn list_templates_command(args: TemplatesListArgs) -> Result<()> {
    let templates = list_templates()?;
    if args.json {
        let json = serde_json::to_string_pretty(&templates)?;
        println!("{json}");
        return Ok(());
    }
    if templates.is_empty() {
        println!("No templates found.");
        return Ok(());
    }
    for name in templates {
        println!("{name}");
    }
    Ok(())
}

fn show_template_command(args: TemplatesShowArgs) -> Result<()> {
    let template = load_template(&args.name)?;
    match template {
        Some(t) => println!("{}", t.content),
        None => bail!("Template '{}' not found", args.name),
    }
    Ok(())
}

fn edit_template_command(args: TemplatesEditArgs) -> Result<()> {
    let content = if let Some(content) = args.content {
        content
    } else {
        // Load existing content or start empty
        let existing = load_template(&args.name)?
            .map(|t| t.content)
            .unwrap_or_default();
        edit_template_with_editor(&existing)?
    };
    save_template(&args.name, &content)?;
    println!("Template '{}' saved.", args.name);
    Ok(())
}

fn edit_template_with_editor(initial: &str) -> Result<String> {
    let mut file = NamedTempFile::new().context("failed to create temporary file")?;
    write!(file, "{}", initial).context("failed to write to temporary file")?;
    file.flush().context("failed to flush temporary file")?;
    let path = file.into_temp_path();

    let mut command = build_editor_command()?;
    let path_ref: &Path = path.as_ref();
    command.arg(path_ref);
    let status = command.status().context("failed to launch editor")?;
    if !status.success() {
        bail!("Editor exited with status {status}");
    }

    let contents = fs::read_to_string(&path).context("failed to read edited template")?;
    Ok(contents)
}

fn list_loaders_command(args: TemplatesLoadersArgs) -> Result<()> {
    let loaders = list_template_loaders();
    if args.json {
        let json = serde_json::to_string_pretty(&loaders)?;
        println!("{json}");
        return Ok(());
    }
    if loaders.is_empty() {
        println!("No template loaders available.");
        return Ok(());
    }
    for loader in loaders {
        println!("{}: {}", loader.name, loader.description);
    }
    Ok(())
}

// ============================================================================
// Schemas Command Handlers
// ============================================================================

fn handle_schemas(args: SchemasArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(SchemasSubcommand::List(SchemasListArgs { json: false }));
    match command {
        SchemasSubcommand::List(list_args) => {
            list_schemas_command(list_args)?;
        }
        SchemasSubcommand::Show(show_args) => {
            show_schema_command(show_args)?;
        }
        SchemasSubcommand::Dsl => {
            show_schemas_dsl_help()?;
        }
    }
    Ok(())
}

fn list_schemas_command(args: SchemasListArgs) -> Result<()> {
    let schemas = list_schemas()?;
    if args.json {
        let json = serde_json::to_string_pretty(&schemas)?;
        println!("{json}");
        return Ok(());
    }
    if schemas.is_empty() {
        println!("No schemas stored in the database.");
        println!("Schemas are created when using structured output with --schema.");
        return Ok(());
    }
    println!("Stored schemas:");
    for schema in &schemas {
        let usage = if schema.usage_count == 1 {
            "1 use".to_string()
        } else {
            format!("{} uses", schema.usage_count)
        };
        println!("  {} ({})", schema.id, usage);
    }
    Ok(())
}

fn show_schema_command(args: SchemasShowArgs) -> Result<()> {
    let schema = get_schema(&args.name)?;
    match schema {
        Some(s) => {
            if args.json {
                let json = serde_json::to_string_pretty(&s)?;
                println!("{json}");
            } else {
                println!("Schema: {}", s.id);
                println!("Usage count: {}", s.usage_count);
                if let Some(content) = &s.content {
                    println!("\nContent:");
                    // Try to pretty-print if it's valid JSON
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                        let pretty = serde_json::to_string_pretty(&parsed)?;
                        println!("{pretty}");
                    } else {
                        println!("{content}");
                    }
                } else {
                    println!("\n(No content stored)");
                }
            }
        }
        None => bail!("Schema '{}' not found", args.name),
    }
    Ok(())
}

fn show_schemas_dsl_help() -> Result<()> {
    println!("Schema DSL Help");
    println!("===============");
    println!();
    println!("Schemas define the structure for structured output from models.");
    println!();
    println!("Usage examples:");
    println!("  llm --schema 'name: str, age: int' \"Extract person info\"");
    println!("  llm --schema @schema.json \"Process with JSON schema\"");
    println!();
    println!("DSL syntax (simplified):");
    println!("  field_name: type");
    println!("  field_name: type = default");
    println!();
    println!("Supported types:");
    println!("  str, string    - Text value");
    println!("  int, integer   - Integer number");
    println!("  float, number  - Floating point number");
    println!("  bool, boolean  - True/false value");
    println!("  list[type]     - Array of values");
    println!("  dict, object   - Nested object");
    println!();
    println!("Note: Full DSL support is not yet implemented in the Rust CLI.");
    println!("Use JSON schema files (@schema.json) for complex schemas.");
    Ok(())
}

// ============================================================================
// Tools Command Handlers
// ============================================================================

fn handle_tools(args: ToolsArgs) -> Result<()> {
    let command = args.command.unwrap_or(ToolsSubcommand::List(ToolsListArgs {
        json: false,
        functions: false,
    }));
    match command {
        ToolsSubcommand::List(list_args) => {
            list_tools_command(list_args)?;
        }
        ToolsSubcommand::Show(show_args) => {
            show_tool_command(show_args)?;
        }
    }
    Ok(())
}

fn list_tools_command(args: ToolsListArgs) -> Result<()> {
    let options = ListToolsOptions {
        functions_only: args.functions,
        ..Default::default()
    };
    let tools = list_tools(options)?;
    if args.json {
        let json = serde_json::to_string_pretty(&tools)?;
        println!("{json}");
        return Ok(());
    }
    if tools.is_empty() {
        println!("No tools stored in the database.");
        println!("Tools are recorded when models make function/tool calls.");
        return Ok(());
    }
    println!("Stored tools:");
    for tool in &tools {
        let name = tool.name.as_deref().unwrap_or("<unnamed>");
        let usage = if tool.usage_count == 1 {
            "1 use".to_string()
        } else {
            format!("{} uses", tool.usage_count)
        };
        let plugin_info = tool
            .plugin
            .as_ref()
            .map(|p| format!(" [{}]", p))
            .unwrap_or_default();
        let desc = tool
            .description
            .as_ref()
            .map(|d| {
                let truncated: String = d.chars().take(50).collect();
                if d.len() > 50 {
                    format!(" - {}...", truncated)
                } else {
                    format!(" - {}", truncated)
                }
            })
            .unwrap_or_default();
        println!("  {}{} ({}){}", name, plugin_info, usage, desc);
    }
    Ok(())
}

fn show_tool_command(args: ToolsShowArgs) -> Result<()> {
    let tool = get_tool(&args.name)?;
    match tool {
        Some(t) => {
            if args.json {
                let json = serde_json::to_string_pretty(&t)?;
                println!("{json}");
            } else {
                let name = t.name.as_deref().unwrap_or("<unnamed>");
                println!("Tool: {}", name);
                println!("Hash: {}", t.hash);
                if let Some(plugin) = &t.plugin {
                    println!("Plugin: {}", plugin);
                }
                println!("Usage count: {}", t.usage_count);
                if let Some(desc) = &t.description {
                    println!("\nDescription:");
                    println!("  {}", desc);
                }
                if let Some(schema) = &t.input_schema {
                    println!("\nInput Schema:");
                    // Try to pretty-print if it's valid JSON
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(schema) {
                        let pretty = serde_json::to_string_pretty(&parsed)?;
                        for line in pretty.lines() {
                            println!("  {}", line);
                        }
                    } else {
                        println!("  {}", schema);
                    }
                }
            }
        }
        None => bail!("Tool '{}' not found", args.name),
    }
    Ok(())
}

// ============================================================================
// Embeddings Command Handlers
// ============================================================================

fn handle_embed(args: EmbedArgs) -> Result<()> {
    let EmbedArgs {
        content,
        model,
        store,
        id,
        metadata,
        raw,
        database,
        json,
    } = args;

    // Read content from stdin if not provided
    let text = if content.is_empty() {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read from stdin")?;
        buffer.trim().to_string()
    } else {
        content.join(" ")
    };

    if text.is_empty() {
        bail!("No content provided to embed. Pass content as argument or pipe to stdin.");
    }

    // Resolve embedding model
    let model_name = model.unwrap_or_else(|| "text-embedding-3-small".to_string());
    let resolved_model = resolve_embedding_model(&model_name)
        .map(|s| s.to_string())
        .unwrap_or(model_name);

    // Create embedding provider
    let provider = OpenAIEmbeddingProvider::from_env(&resolved_model)
        .context("failed to create embedding provider")?;

    // Generate embedding
    let result = provider
        .embed(&text)
        .context("failed to generate embedding")?;

    // Store if requested
    if let Some(collection_name) = store {
        let embed_id = id.unwrap_or_else(|| {
            // Generate a unique ID if not provided
            format!("emb_{}", chrono::Utc::now().timestamp_millis())
        });

        let db_path = database
            .map(PathBuf::from)
            .or_else(|| embeddings_db_path().ok())
            .ok_or_else(|| anyhow!("failed to determine embeddings database path"))?;

        let collection = Collection::open(&db_path, &collection_name, Some(&resolved_model))
            .context("failed to open collection")?;

        let metadata_value: Option<serde_json::Value> = metadata
            .as_ref()
            .map(|m| serde_json::from_str(m))
            .transpose()
            .context("invalid JSON metadata")?;

        collection
            .store(&embed_id, &result.embedding, Some(&text), metadata_value)
            .context("failed to store embedding")?;

        if json {
            let output = serde_json::json!({
                "id": embed_id,
                "collection": collection_name,
                "model": resolved_model,
                "dimensions": result.embedding.len(),
                "tokens": result.tokens,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "Stored embedding '{}' in collection '{}'",
                embed_id, collection_name
            );
            println!("Model: {}", resolved_model);
            println!("Dimensions: {}", result.embedding.len());
            if let Some(tokens) = result.tokens {
                println!("Tokens: {}", tokens);
            }
        }
    } else {
        // Output the embedding
        if raw {
            // Output as space-separated floats
            let values: Vec<String> = result
                .embedding
                .iter()
                .map(|f: &f32| f.to_string())
                .collect();
            println!("{}", values.join(" "));
        } else if json {
            let output = serde_json::json!({
                "embedding": result.embedding,
                "model": resolved_model,
                "dimensions": result.embedding.len(),
                "tokens": result.tokens,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("Model: {}", resolved_model);
            println!("Dimensions: {}", result.embedding.len());
            if let Some(tokens) = result.tokens {
                println!("Tokens: {}", tokens);
            }
            println!("\nEmbedding (first 10 dimensions):");
            for (i, val) in result.embedding.iter().take(10).enumerate() {
                println!("  [{:4}] {:.6}", i, val);
            }
            if result.embedding.len() > 10 {
                println!("  ... ({} more dimensions)", result.embedding.len() - 10);
            }
        }
    }

    Ok(())
}

fn handle_embed_models(args: EmbedModelsArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(EmbedModelsSubcommand::List(EmbedModelsListArgs {
            json: false,
        }));
    match command {
        EmbedModelsSubcommand::List(list_args) => {
            embed_models_list(list_args)?;
        }
        EmbedModelsSubcommand::Default(default_args) => {
            embed_models_default(default_args)?;
        }
    }
    Ok(())
}

fn embed_models_list(args: EmbedModelsListArgs) -> Result<()> {
    let models = list_embedding_models();

    if args.json {
        let json = serde_json::to_string_pretty(&models)?;
        println!("{json}");
        return Ok(());
    }

    println!("Available embedding models:\n");
    for model in &models {
        let aliases = if model.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", model.aliases.join(", "))
        };
        let dims = model
            .dimensions
            .map(|d| format!("{} dims", d))
            .unwrap_or_else(|| "unknown dims".to_string());
        println!(
            "  {} ({}) - {}{}",
            model.model_id, model.provider, dims, aliases
        );
    }
    Ok(())
}

fn embed_models_default(args: EmbedModelsDefaultArgs) -> Result<()> {
    // TODO: persist default embedding model selection in config storage.
    if let Some(model) = args.model {
        let resolved = resolve_embedding_model(&model)
            .map(|s| s.to_string())
            .unwrap_or(model.clone());
        println!("Default embedding model would be set to: {}", resolved);
        println!("Note: Persistent default embedding model is not yet implemented.");
    } else {
        println!("Current default embedding model: text-embedding-3-small");
        println!("Note: Persistent default embedding model is not yet implemented.");
    }
    Ok(())
}

fn handle_embed_multi(args: EmbedMultiArgs) -> Result<()> {
    let EmbedMultiArgs {
        collection,
        model,
        files,
        sql,
        batch_size,
        store_content,
        database,
        json: json_output,
    } = args;

    if files.is_empty() && sql.is_none() {
        bail!("Specify files with --files or content source with --sql");
    }

    // Resolve embedding model
    let model_name = model.unwrap_or_else(|| "text-embedding-3-small".to_string());
    let resolved_model = resolve_embedding_model(&model_name)
        .map(|s| s.to_string())
        .unwrap_or(model_name);

    // Create embedding provider
    let provider = OpenAIEmbeddingProvider::from_env(&resolved_model)
        .context("failed to create embedding provider")?;

    // Open collection
    let db_path = database
        .map(PathBuf::from)
        .or_else(|| embeddings_db_path().ok())
        .ok_or_else(|| anyhow!("failed to determine embeddings database path"))?;

    let coll = Collection::open(&db_path, &collection, Some(&resolved_model))
        .context("failed to open collection")?;

    let mut total_embedded = 0usize;
    let mut total_skipped = 0usize;

    // Process files
    if !files.is_empty() {
        let mut items: Vec<EmbedItem> = Vec::new();

        for file_path in &files {
            if !file_path.exists() {
                eprintln!("Warning: File not found: {}", file_path.display());
                total_skipped += 1;
                continue;
            }

            let content = fs::read_to_string(file_path)
                .with_context(|| format!("failed to read file: {}", file_path.display()))?;

            if content.trim().is_empty() {
                eprintln!("Warning: Empty file: {}", file_path.display());
                total_skipped += 1;
                continue;
            }

            let id = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let metadata = serde_json::json!({
                "source": "file",
                "path": file_path.display().to_string(),
            });

            items.push(EmbedItem::new(id, content).with_metadata(metadata));
        }

        // Batch embed
        for chunk in items.chunks(batch_size) {
            coll.embed_multi(&provider, chunk, store_content)
                .context("failed to embed batch")?;
            total_embedded += chunk.len();
            if !json_output {
                eprint!("\rEmbedded {} items...", total_embedded);
            }
        }
    }

    // TODO: support SQL source queries against logs DB for embed-multi.
    if let Some(query) = sql {
        if json_output {
            eprintln!("SQL embedding source is not yet fully implemented");
        } else {
            println!("\nSQL query support is not yet fully implemented.");
            println!("Query: {}", query);
        }
    }

    if json_output {
        let output = serde_json::json!({
            "collection": collection,
            "model": resolved_model,
            "embedded": total_embedded,
            "skipped": total_skipped,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("\nDone!");
        println!("Collection: {}", collection);
        println!("Model: {}", resolved_model);
        println!("Embedded: {} items", total_embedded);
        if total_skipped > 0 {
            println!("Skipped: {} items", total_skipped);
        }
    }

    Ok(())
}

fn handle_similar(args: SimilarArgs) -> Result<()> {
    let SimilarArgs {
        query,
        collection,
        number,
        model,
        id,
        database,
        json: json_output,
    } = args;

    // Open collection
    let db_path = database
        .map(PathBuf::from)
        .or_else(|| embeddings_db_path().ok())
        .ok_or_else(|| anyhow!("failed to determine embeddings database path"))?;

    if !db_path.exists() {
        bail!("Embeddings database not found at: {}", db_path.display());
    }

    // Open collection (model is optional - will use stored model)
    let coll = Collection::open(&db_path, &collection, model.as_deref())
        .with_context(|| format!("failed to open collection '{}'", collection))?;

    let results: Vec<Entry> = if let Some(entry_id) = id {
        // Find similar to existing entry
        coll.similar_by_id(&entry_id, number)
            .with_context(|| format!("failed to find similar entries to '{}'", entry_id))?
    } else {
        // Find similar to query text
        let query_text = if query.is_empty() {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .context("failed to read query from stdin")?;
            buffer.trim().to_string()
        } else {
            query.join(" ")
        };

        if query_text.is_empty() {
            bail!("No query provided. Pass query as argument, use --id, or pipe to stdin.");
        }

        // Resolve model (use collection's model if not specified)
        let model_name = model.unwrap_or_else(|| coll.model_id().to_string());
        let resolved_model = resolve_embedding_model(&model_name)
            .map(|s| s.to_string())
            .unwrap_or(model_name);

        let provider = OpenAIEmbeddingProvider::from_env(&resolved_model)
            .context("failed to create embedding provider")?;

        coll.similar(&provider, &query_text, number)
            .context("failed to find similar entries")?
    };

    if json_output {
        let json = serde_json::to_string_pretty(&results)?;
        println!("{json}");
    } else {
        if results.is_empty() {
            println!("No similar entries found in collection '{}'.", collection);
            return Ok(());
        }

        println!("Similar entries in collection '{}':\n", collection);
        for (i, entry) in results.iter().enumerate() {
            let score = entry
                .score
                .map(|s| format!("{:.4}", s))
                .unwrap_or_else(|| "N/A".to_string());
            println!("{}. {} (score: {})", i + 1, entry.id, score);
            if let Some(ref content) = entry.content {
                let preview: String = content.chars().take(100).collect();
                let ellipsis = if content.len() > 100 { "..." } else { "" };
                println!("   {}{}", preview, ellipsis);
            }
            if let Some(ref meta) = entry.metadata {
                println!("   metadata: {}", meta);
            }
            println!();
        }
    }

    Ok(())
}

fn handle_collections(args: CollectionsArgs) -> Result<()> {
    let command = args
        .command
        .unwrap_or(CollectionsSubcommand::List(CollectionsListArgs {
            json: false,
            database: None,
        }));
    match command {
        CollectionsSubcommand::List(list_args) => {
            collections_list(list_args)?;
        }
        CollectionsSubcommand::Path => {
            collections_path()?;
        }
        CollectionsSubcommand::Delete(delete_args) => {
            collections_delete(delete_args)?;
        }
    }
    Ok(())
}

fn collections_list(args: CollectionsListArgs) -> Result<()> {
    let db_path = args
        .database
        .map(PathBuf::from)
        .or_else(|| embeddings_db_path().ok())
        .ok_or_else(|| anyhow!("failed to determine embeddings database path"))?;

    if !db_path.exists() {
        if args.json {
            println!("[]");
        } else {
            println!("No embeddings database found at: {}", db_path.display());
            println!("Create a collection with `llm embed --store <collection> <text>`");
        }
        return Ok(());
    }

    let collections = list_collections(&db_path).context("failed to list collections")?;

    if args.json {
        let items: Vec<serde_json::Value> = collections
            .iter()
            .map(|(name, model)| {
                serde_json::json!({
                    "name": name,
                    "model": model,
                })
            })
            .collect();
        let json = serde_json::to_string_pretty(&items)?;
        println!("{json}");
        return Ok(());
    }

    if collections.is_empty() {
        println!("No collections found.");
        return Ok(());
    }

    println!("Collections in {}:\n", db_path.display());
    for (name, model) in &collections {
        println!("  {} (model: {})", name, model);
    }
    Ok(())
}

fn collections_path() -> Result<()> {
    let path = embeddings_db_path()?;
    println!("{}", path.display());
    Ok(())
}

fn collections_delete(args: CollectionsDeleteArgs) -> Result<()> {
    let db_path = args
        .database
        .map(PathBuf::from)
        .or_else(|| embeddings_db_path().ok())
        .ok_or_else(|| anyhow!("failed to determine embeddings database path"))?;

    if !db_path.exists() {
        bail!("Embeddings database not found at: {}", db_path.display());
    }

    let deleted = delete_collection(&db_path, &args.name).context("failed to delete collection")?;

    if deleted {
        println!("Deleted collection '{}'", args.name);
    } else {
        bail!("Collection '{}' not found", args.name);
    }
    Ok(())
}

fn print_logs_status() -> Result<()> {
    let status = logs_status()?;
    if !status.database_exists {
        let state = if status.logging_enabled { "ON" } else { "OFF" };
        println!("Logging is {state}.");
        println!(
            "No log database found at {}",
            status.database_path.display()
        );
        return Ok(());
    }

    if status.logging_enabled {
        println!("Logging is ON for all prompts");
    } else {
        println!("Logging is OFF");
    }
    println!("Found log database at {}", status.database_path.display());
    println!("Number of conversations logged:\t{}", status.conversations);
    println!("Number of responses logged:\t{}", status.responses);
    if let Some(size) = status.file_size_bytes {
        println!("Database file size:\t\t{}", human_readable_size(size));
    }
    Ok(())
}

fn backup_logs_command(args: LogsBackupArgs) -> Result<()> {
    let destination = PathBuf::from(args.path);
    backup_logs(&destination)?;
    let size = fs::metadata(&destination).ok().map(|meta| meta.len());
    if let Some(bytes) = size {
        println!(
            "Backed up {} to {}",
            human_readable_size(bytes),
            destination.display()
        );
    } else {
        println!("Backed up logs to {}", destination.display());
    }
    Ok(())
}

/// Display options for log entries.
#[derive(Default)]
struct LogDisplayOptions {
    response_only: bool,
    extract: bool,
    extract_last: bool,
    short: bool,
    truncate: bool,
}

/// Extract fenced code blocks from text.
/// Returns all code blocks found, preserving their order.
fn extract_code_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_block = String::new();
    let mut fence_char = ' ';
    let mut fence_len = 0;

    for line in text.lines() {
        let trimmed = line.trim_start();

        if !in_block {
            // Check for opening fence (``` or ~~~)
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence_char = trimmed.chars().next().unwrap();
                fence_len = trimmed.chars().take_while(|&c| c == fence_char).count();
                if fence_len >= 3 {
                    in_block = true;
                    current_block.clear();
                    continue;
                }
            }
        } else {
            // Check for closing fence
            let close_fence: String = std::iter::repeat_n(fence_char, fence_len).collect();
            if trimmed.starts_with(&close_fence)
                && trimmed.trim_start_matches(fence_char).trim().is_empty()
            {
                blocks.push(current_block.trim_end().to_string());
                in_block = false;
                current_block.clear();
            } else {
                if !current_block.is_empty() {
                    current_block.push('\n');
                }
                current_block.push_str(line);
            }
        }
    }

    // If we ended while still in a block, include it anyway
    if in_block && !current_block.is_empty() {
        blocks.push(current_block.trim_end().to_string());
    }

    blocks
}

/// Truncate text to a maximum length, adding ellipsis if truncated.
fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Default truncation length for --truncate option.
const TRUNCATE_LENGTH: usize = 100;

fn list_logs_command(args: LogsListArgs) -> Result<()> {
    let LogsListArgs {
        count,
        json,
        model,
        query,
        conversation,
        id_gt,
        id_gte,
        since,
        before,
        path,
        database,
        response_only,
        extract,
        extract_last,
        short,
        truncate,
        latest,
        current,
        fragment,
        with_tools,
        schema,
    } = args;

    let mut options = ListLogsOptions::default();

    // Handle --latest: override count to 1
    if latest {
        options.limit = Some(1);
    } else {
        match count.unwrap_or(3) {
            0 => options.limit = None,
            value => options.limit = Some(value),
        }
    }

    options.newest_first = true;
    options.model = model;
    options.query = query;
    options.id_gt = id_gt;
    options.id_gte = id_gte;
    options.since = since;
    options.before = before;
    options.database_path = database.or(path).map(PathBuf::from);

    // Handle --tools filter
    if with_tools {
        options.with_tool_calls = Some(true);
    }

    // Handle --schema filter
    if let Some(schema_id) = schema {
        options.schema_id = Some(schema_id);
    }

    // Handle --fragment filter
    if let Some(frag_id) = fragment {
        options.fragment_id = Some(frag_id);
    }

    // Handle --current: get the latest conversation ID and filter by it
    let conversation_id = if current {
        get_latest_conversation_id()?
    } else {
        conversation
    };
    options.conversation_id = conversation_id;

    let entries = list_logs(options)?;

    // Build display options
    let display_opts = LogDisplayOptions {
        response_only,
        extract,
        extract_last,
        short,
        truncate,
    };

    if json {
        let json = serde_json::to_string_pretty(&entries)?;
        println!("{json}");
        return Ok(());
    }

    if entries.is_empty() {
        println!("No logs found.");
        return Ok(());
    }

    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            println!();
        }
        display_log_entry_with_options(entry, &display_opts);
    }
    Ok(())
}

fn display_log_entry_with_options(entry: &LogEntry, opts: &LogDisplayOptions) {
    let response_text = entry.response.as_deref().unwrap_or("");

    // Handle extract modes
    if opts.extract || opts.extract_last {
        let blocks = extract_code_blocks(response_text);
        if opts.extract_last {
            // Output only the last code block
            if let Some(last) = blocks.last() {
                println!("{}", last);
            }
        } else {
            // Output all code blocks
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("{}", block);
            }
        }
        return;
    }

    // Handle response-only mode
    if opts.response_only {
        let text = if opts.truncate {
            truncate_text(response_text, TRUNCATE_LENGTH)
        } else {
            response_text.to_string()
        };
        println!("{}", text);
        return;
    }

    // Handle short mode (prompt and response only, no metadata)
    if opts.short {
        let prompt_text = entry.prompt.as_deref().unwrap_or("-- none --");
        let (prompt_display, response_display) = if opts.truncate {
            (
                truncate_text(prompt_text, TRUNCATE_LENGTH),
                truncate_text(response_text, TRUNCATE_LENGTH),
            )
        } else {
            (prompt_text.to_string(), response_text.to_string())
        };
        println!("Prompt: {}", prompt_display);
        println!("Response: {}", response_display);
        return;
    }

    // Full display mode
    display_log_entry_full(entry, opts.truncate);
}

fn display_log_entry_full(entry: &LogEntry, truncate: bool) {
    let timestamp = entry
        .datetime_utc
        .as_deref()
        .unwrap_or("-- unknown timestamp --");
    let resolved_suffix = entry
        .resolved_model
        .as_deref()
        .filter(|resolved| !resolved.is_empty() && resolved != &entry.model)
        .map(|resolved| format!(" (resolved: {resolved})"))
        .unwrap_or_default();
    println!("# {timestamp}    id: {}", entry.id);
    println!("Model: {}{}", entry.model, resolved_suffix);
    if let Some(conversation) = entry.conversation_id.as_deref() {
        let name_suffix = entry
            .conversation_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(|name| format!(" ({name})"))
            .unwrap_or_default();
        println!("Conversation: {conversation}{name_suffix}");
        if let Some(meta_model) = entry
            .conversation_model
            .as_deref()
            .filter(|model| !model.is_empty())
        {
            println!("Conversation model: {meta_model}");
        }
    }
    if let Some(duration) = entry.duration_ms {
        println!("Duration: {} ms", duration);
    }

    let prompt_text = entry
        .prompt
        .as_deref()
        .filter(|p| !p.is_empty())
        .unwrap_or("-- none --");
    let response_text = entry
        .response
        .as_deref()
        .filter(|r| !r.is_empty())
        .unwrap_or("-- none --");

    let (prompt_display, response_display) = if truncate {
        (
            truncate_text(prompt_text, TRUNCATE_LENGTH),
            truncate_text(response_text, TRUNCATE_LENGTH),
        )
    } else {
        (prompt_text.to_string(), response_text.to_string())
    };

    println!("\nPrompt:\n{}\n", prompt_display);
    println!("Response:\n{}\n", response_display);

    if let Some(input) = entry.input_tokens {
        if let Some(output) = entry.output_tokens {
            println!("Token usage: input {} • output {}", input, output);
        }
    }
}

fn human_readable_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn list_keys(as_json: bool) -> Result<()> {
    let names = list_key_names()?;
    if names.is_empty() {
        println!("No keys found");
        return Ok(());
    }
    if as_json {
        let json = serde_json::to_string_pretty(&names)?;
        println!("{json}");
    } else {
        for name in names {
            println!("{name}");
        }
    }
    Ok(())
}

fn resolve_prompt_attachments(
    attachments: &[String],
    attachment_types: &[String],
) -> Result<Vec<Attachment>> {
    if attachments.is_empty() && attachment_types.is_empty() {
        return Ok(Vec::new());
    }
    let mut resolved = Vec::new();
    let mut stdin_state = StdinAttachmentState::default();
    for value in attachments {
        resolved.push(build_attachment_from_source(value, None, &mut stdin_state)?);
    }
    if !attachment_types.is_empty() {
        if !attachment_types.len().is_multiple_of(2) {
            bail!("each --attachment-type must include a path/URL and a mimetype");
        }
        for pair in attachment_types.chunks(2) {
            let source = pair.first().expect("path present");
            let mimetype = pair.get(1).expect("mimetype present");
            resolved.push(build_attachment_from_source(
                source,
                Some(mimetype),
                &mut stdin_state,
            )?);
        }
    }
    Ok(resolved)
}

fn build_attachment_from_source(
    value: &str,
    explicit_type: Option<&str>,
    stdin_state: &mut StdinAttachmentState,
) -> Result<Attachment> {
    if value == "-" {
        let bytes = stdin_state.read_once()?;
        let mimetype = explicit_type
            .map(|value| value.to_string())
            .or_else(|| detect_mime_from_content(&bytes))
            .ok_or_else(|| {
                anyhow!(
                    "Could not determine mimetype for stdin attachment, supply --attachment-type"
                )
            })?;
        return Ok(Attachment::from_content(bytes, Some(mimetype)));
    }
    if value.contains("://") {
        let mimetype = if let Some(explicit) = explicit_type {
            Some(explicit.to_string())
        } else {
            Some(
                detect_remote_mime(value)?
                    .ok_or_else(|| anyhow!("Unable to detect mimetype for {value}"))?,
            )
        };
        return Ok(Attachment::from_url(value.to_string(), mimetype));
    }

    let path = PathBuf::from(value);
    if !path.exists() {
        bail!("Attachment file '{}' does not exist", value);
    }
    let canonical = fs::canonicalize(&path)
        .with_context(|| format!("failed to resolve attachment path {}", path.display()))?;
    let mimetype = explicit_type
        .map(|value| value.to_string())
        .or_else(|| detect_mime_from_path(&canonical))
        .ok_or_else(|| {
            anyhow!(
                "Could not determine mimetype for attachment {}. Use --attachment-type to specify one.",
                canonical.display()
            )
        })?;
    Ok(Attachment::from_path(canonical, Some(mimetype)))
}

#[derive(Default)]
struct StdinAttachmentState {
    consumed: bool,
}

impl StdinAttachmentState {
    fn read_once(&mut self) -> Result<Vec<u8>> {
        if self.consumed {
            bail!("Standard input can only be used for a single attachment");
        }
        let mut buffer = Vec::new();
        io::stdin()
            .read_to_end(&mut buffer)
            .context("failed to read attachment from stdin")?;
        self.consumed = true;
        Ok(buffer)
    }
}

#[derive(Default)]
struct StdoutStreamSink {
    started: bool,
}

impl StreamSink for StdoutStreamSink {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()> {
        use std::io::{self, Write};
        print!("{}", delta);
        io::stdout()
            .flush()
            .context("failed to flush stdout during stream")?;
        self.started = true;
        Ok(())
    }

    fn handle_done(&mut self) -> Result<()> {
        if self.started {
            println!();
        }
        Ok(())
    }
}
