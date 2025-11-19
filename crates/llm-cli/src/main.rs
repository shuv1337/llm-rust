use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use clap::{Args, CommandFactory, Parser, Subcommand};
use llm_core::{
    available_models, backup_logs, core_version, detect_mime_from_content, detect_mime_from_path,
    detect_remote_mime, execute_prompt_with_messages, get_default_model, keys_path, list_key_names,
    list_logs, load_keys, logs_db_path, logs_status, prompt_debug_info, resolve_key, save_key,
    set_default_model, set_logging_enabled, stream_prompt_with_messages, Attachment, KeyQuery,
    ListLogsOptions, LogEntry, MessageRole, ModelInfo, PromptConfig, PromptMessage, StreamSink,
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
use tempfile::NamedTempFile;
use tracing::info;

#[derive(Args, Clone, Default)]
struct PromptOptions {
    /// Override the model (defaults to env or gpt-4o-mini)
    #[arg(long)]
    model: Option<String>,
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
    /// Conversation identifier to associate with this prompt
    #[arg(short = 'c', long = "conversation")]
    conversation: Option<String>,
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
    about = "Experimental Rust port of the LLM CLI (keys, plugins, prompts, models)",
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
    /// Execute a prompt (placeholder implementation)
    Prompt(PromptArgs),
    /// List plugins detected by the host
    Plugins(PluginsArgs),
    /// Inspect or configure models
    Models(ModelsArgs),
    /// Manage stored API keys
    Keys(KeysArgs),
    /// Manage prompt logs (placeholder)
    Logs(LogsArgs),
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
    /// Conversation identifier to associate with this prompt
    #[arg(short = 'c', long = "conversation")]
    conversation: Option<String>,
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
}

#[derive(Args)]
struct ModelsListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ModelsDefaultArgs {
    /// Optional name of the model to set as default
    model: Option<String>,
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
struct LogsArgs {
    #[command(subcommand)]
    command: Option<LogsSubcommand>,
}

#[derive(Subcommand)]
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
    id_gt: Option<i64>,
    /// Only include entries with an id greater than or equal to this value
    #[arg(long = "id-gte", conflicts_with = "id_gt")]
    id_gte: Option<i64>,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
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
        Some(Command::Logs(args)) => {
            handle_logs(args)?;
        }
        Some(Command::Cmd(args)) => {
            run_cmd(args, &logging)?;
        }
        Some(Command::Version(args)) => {
            print_version(args.verbose);
        }
        None => {
            if prompt_input.prompt.is_empty() {
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
        conversation,
        conversation_name,
        conversation_model,
        prompt: words,
    } = input;
    let prompt = words.join(" ");
    info!(%prompt, "Executing prompt via llm-core");
    let config = PromptConfig {
        model: options.model.as_deref(),
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        retries: options.retries.map(|v| v as usize),
        retry_backoff_ms: options.retry_backoff_ms,
        api_key: key.as_deref(),
        log_override: options.log_override(),
        conversation_id: conversation.as_deref(),
        conversation_name: conversation_name.as_deref(),
        conversation_model: conversation_model
            .as_deref()
            .or_else(|| options.model.as_deref()),
    };
    let streaming = !options.no_stream;
    log_prompt_debug(logging, streaming, &config)?;
    let resolved_attachments = resolve_prompt_attachments(&attachments, &attachment_types)?;
    let messages = build_messages(system.as_deref(), &prompt);
    if streaming {
        let mut sink = StdoutStreamSink::default();
        stream_prompt_with_messages(messages, resolved_attachments, config, &mut sink)?;
    } else {
        let response = execute_prompt_with_messages(messages, resolved_attachments, config)?;
        println!("{response}");
    }
    Ok(())
}

fn run_cmd(args: CmdArgs, logging: &LoggingOptions) -> Result<()> {
    if args.prompt.is_empty() {
        bail!("Describe the command you would like to run.");
    }
    let prompt = args.prompt.join(" ");
    let system_prompt = args.system.as_deref().unwrap_or(CMD_SYSTEM_PROMPT);
    info!(%prompt, "Generating shell command via llm-core");

    let config = PromptConfig {
        model: args.options.model.as_deref(),
        temperature: args.options.temperature,
        max_tokens: args.options.max_tokens,
        retries: args.options.retries.map(|v| v as usize),
        retry_backoff_ms: args.options.retry_backoff_ms,
        api_key: args.key.as_deref(),
        log_override: args.options.log_override(),
        conversation_id: args.conversation.as_deref(),
        conversation_name: args.conversation_name.as_deref(),
        conversation_model: args
            .conversation_model
            .as_deref()
            .or_else(|| args.options.model.as_deref()),
    };
    log_prompt_debug(logging, false, &config)?;

    let messages = build_cmd_messages(system_prompt, &prompt);
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
            messages.push(PromptMessage {
                role: MessageRole::System,
                content: trimmed_system.to_string(),
            });
        }
    }
    messages.push(PromptMessage {
        role: MessageRole::User,
        content: user_prompt.to_string(),
    });
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
    println!("llm-cli {}", env!("CARGO_PKG_VERSION"));
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
        println!("No plugins loaded (stub)");
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
        None => list_models(ModelsListArgs { json: false }),
    }
}

fn list_models(args: ModelsListArgs) -> Result<()> {
    let mut models: Vec<ModelInfo> = available_models()?;
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
                "No default model configured. Use `llm-cli models default <model>` to set one."
            ),
        }
    }
    Ok(())
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
    } = args;

    let mut options = ListLogsOptions::default();
    match count.unwrap_or(3) {
        0 => options.limit = None,
        value => options.limit = Some(value),
    }
    options.newest_first = true;
    options.model = model;
    options.query = query;
    options.conversation_id = conversation;
    options.id_gt = id_gt;
    options.id_gte = id_gte;
    options.since = since;
    options.before = before;
    options.database_path = database.or(path).map(PathBuf::from);

    let entries = list_logs(options)?;
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
        display_log_entry(entry);
    }
    Ok(())
}

fn display_log_entry(entry: &LogEntry) {
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
    println!(
        "\nPrompt:\n{}\n",
        entry
            .prompt
            .as_deref()
            .filter(|p| !p.is_empty())
            .unwrap_or("-- none --")
    );
    println!(
        "Response:\n{}\n",
        entry
            .response
            .as_deref()
            .filter(|r| !r.is_empty())
            .unwrap_or("-- none --")
    );
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
        if attachment_types.len() % 2 != 0 {
            bail!("each --attachment-type must include a path/URL and a mimetype");
        }
        for pair in attachment_types.chunks(2) {
            let source = pair.get(0).expect("path present");
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
