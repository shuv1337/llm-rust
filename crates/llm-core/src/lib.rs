//! Core library for the Rust rewrite of LLM.
//!
//! Currently only exposes a handful of utility functions that mirror the
//! behaviour of the existing Python implementation while we port features.

use anyhow::{anyhow, bail, Context, Result};
use directories::ProjectDirs;
use providers::anthropic::{AnthropicConfig, AnthropicProvider};
use providers::openai::{OpenAIConfig, OpenAIProvider};
use providers::StreamSink as ProviderStreamSink;
use providers::{PromptProvider, PromptRequest, VecStreamSink};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod aliases;
mod attachments;
mod logs;
pub mod migrations;
mod model_options;
pub mod providers;
mod templates;
pub use aliases::{
    aliases_path, get_alias, list_aliases, load_aliases, remove_alias, resolve_user_alias,
    save_aliases, set_alias, Aliases,
};
pub use attachments::{
    detect_mime_from_content, detect_mime_from_path, detect_remote_mime, Attachment,
};
pub use logs::{
    backup_logs, get_latest_conversation_id, get_schema, get_tool, list_logs, list_schemas,
    list_tools, load_conversation_messages, logs_enabled, logs_status, set_logging_enabled,
    ListLogsOptions, ListToolsOptions, LogEntry, LogsStatus, SchemaEntry, ToolEntry,
};
pub use model_options::{
    get_model_options, list_model_options, load_model_options, model_options_path,
    remove_model_options, resolve_model_options, save_model_options, set_model_options,
    ModelOptions, StoredModelOptions,
};
pub use providers::{MessageRole, PromptMessage, StreamSink, UsageInfo};
pub use templates::{
    delete_template, get_template, list_template_loaders, list_templates, load_template,
    save_template, templates_path, Template, TemplateLoader,
};

struct BuiltinModel {
    canonical: &'static str,
    provider: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
}

/// Determine if a provider supports tool calling.
fn provider_supports_tools(provider: &str) -> bool {
    matches!(provider, "openai" | "anthropic")
}

/// Determine if a provider supports structured output schemas.
fn provider_supports_schemas(provider: &str) -> bool {
    matches!(provider, "openai" | "anthropic")
}

/// Determine if a provider supports async execution.
fn provider_supports_async(provider: &str) -> bool {
    // Currently none of the built-in providers support async execution mode
    matches!(provider, "openai")
}

const BUILTIN_MODELS: &[BuiltinModel] = &[
    BuiltinModel {
        canonical: "openai/gpt-4o-mini",
        provider: "openai",
        description: "GPT-4o mini",
        aliases: &["gpt-4o-mini", "4o-mini"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o",
        provider: "openai",
        description: "GPT-4o general-purpose",
        aliases: &["gpt-4o", "4o"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4.1",
        provider: "openai",
        description: "GPT-4.1 flagship",
        aliases: &["gpt-4.1", "4.1"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4.1-mini",
        provider: "openai",
        description: "GPT-4.1 mini",
        aliases: &["gpt-4.1-mini", "4.1-mini"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4.1-nano",
        provider: "openai",
        description: "GPT-4.1 nano",
        aliases: &["gpt-4.1-nano", "4.1-nano"],
    },
    BuiltinModel {
        canonical: "openai/gpt-3.5-turbo",
        provider: "openai",
        description: "GPT-3.5 Turbo",
        aliases: &["gpt-3.5-turbo", "3.5", "chatgpt"],
    },
    BuiltinModel {
        canonical: "openai/gpt-3.5-turbo-16k",
        provider: "openai",
        description: "GPT-3.5 Turbo 16k",
        aliases: &["gpt-3.5-turbo-16k", "chatgpt-16k", "3.5-16k"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4",
        provider: "openai",
        description: "GPT-4",
        aliases: &["gpt-4", "4", "gpt4"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4-1106-preview",
        provider: "openai",
        description: "GPT-4 1106 preview",
        aliases: &["gpt-4-1106-preview"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4-0125-preview",
        provider: "openai",
        description: "GPT-4 0125 preview",
        aliases: &["gpt-4-0125-preview"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4-turbo-2024-04-09",
        provider: "openai",
        description: "GPT-4 Turbo (2024-04-09)",
        aliases: &["gpt-4-turbo-2024-04-09"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4-turbo",
        provider: "openai",
        description: "GPT-4 Turbo",
        aliases: &["gpt-4-turbo", "gpt-4-turbo-preview", "4-turbo", "4t"],
    },
    BuiltinModel {
        canonical: "openai/o1",
        provider: "openai",
        description: "o1 reasoning",
        aliases: &["o1"],
    },
    BuiltinModel {
        canonical: "openai/o1-2024-12-17",
        provider: "openai",
        description: "o1 (2024-12-17)",
        aliases: &["o1-2024-12-17"],
    },
    BuiltinModel {
        canonical: "openai/o3",
        provider: "openai",
        description: "o3 reasoning",
        aliases: &["o3"],
    },
    BuiltinModel {
        canonical: "openai/o3-mini",
        provider: "openai",
        description: "o3 mini",
        aliases: &["o3-mini"],
    },
    BuiltinModel {
        canonical: "openai/o4-mini",
        provider: "openai",
        description: "o4 mini reasoning",
        aliases: &["o4-mini"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5",
        provider: "openai",
        description: "GPT-5",
        aliases: &["gpt-5"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5-mini",
        provider: "openai",
        description: "GPT-5 mini",
        aliases: &["gpt-5-mini"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5-nano",
        provider: "openai",
        description: "GPT-5 nano",
        aliases: &["gpt-5-nano"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5-2025-08-07",
        provider: "openai",
        description: "GPT-5 (2025-08-07)",
        aliases: &["gpt-5-2025-08-07"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5-mini-2025-08-07",
        provider: "openai",
        description: "GPT-5 mini (2025-08-07)",
        aliases: &["gpt-5-mini-2025-08-07"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5-nano-2025-08-07",
        provider: "openai",
        description: "GPT-5 nano (2025-08-07)",
        aliases: &["gpt-5-nano-2025-08-07"],
    },
    BuiltinModel {
        canonical: "openai/gpt-5.2-2025-12-11",
        provider: "openai",
        description: "GPT-5.2 (2025-12-11)",
        aliases: &["gpt-5.2-2025-12-11", "gpt-5.2"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-haiku-20240307",
        provider: "anthropic",
        description: "Claude 3 Haiku (2024-03-07)",
        aliases: &["claude-3-haiku-20240307"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-opus-4-0",
        provider: "anthropic",
        description: "Claude 4 Opus",
        aliases: &["claude-4-opus"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-sonnet-4-0",
        provider: "anthropic",
        description: "Claude 4 Sonnet",
        aliases: &["claude-4-sonnet"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-opus-4-1-20250805",
        provider: "anthropic",
        description: "Claude 4.1 Opus (2025-08-05)",
        aliases: &["claude-opus-4.1"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-sonnet-4-5",
        provider: "anthropic",
        description: "Claude 4.5 Sonnet",
        aliases: &["claude-sonnet-4.5"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-sonnet-4-6",
        provider: "anthropic",
        description: "Claude 4.6 Sonnet",
        aliases: &["claude-sonnet-4.6"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-opus-4-6",
        provider: "anthropic",
        description: "Claude 4.6 Opus",
        aliases: &["claude-opus-4.6"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-haiku-4-5-20251001",
        provider: "anthropic",
        description: "Claude 4.5 Haiku (2025-10-01)",
        aliases: &["claude-haiku-4.5"],
    },
];

const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";
const DEFAULT_RETRIES: usize = 2;
const DEFAULT_BACKOFF_MS: u64 = 250;
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Metadata describing a model known to the Rust CLI.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub provider: String,
    pub description: String,
    pub is_default: bool,
    pub aliases: Vec<String>,
    /// Whether the model supports tool/function calling.
    pub supports_tools: bool,
    /// Whether the model supports structured output schemas.
    pub supports_schemas: bool,
    /// Whether the model supports async execution.
    pub supports_async: bool,
    /// Whether the model has stored default options.
    pub has_options: bool,
}

/// Returns a static string identifying the core crate version.
pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Configuration options for prompt execution.
#[derive(Default)]
pub struct PromptConfig<'a> {
    /// Override database path for logging.
    pub database_path: Option<&'a str>,
    /// Override model identifier (falls back to env/Default).
    pub model: Option<&'a str>,
    /// Sampling temperature override.
    pub temperature: Option<f32>,
    /// Maximum tokens override.
    pub max_tokens: Option<u32>,
    /// Retry override (number of retries).
    pub retries: Option<usize>,
    /// Retry backoff override in milliseconds.
    pub retry_backoff_ms: Option<u64>,
    /// Optional API key override for this request.
    pub api_key: Option<&'a str>,
    /// Force logging on/off for this invocation.
    pub log_override: Option<bool>,
    /// Optional conversation identifier to associate with the response.
    pub conversation_id: Option<&'a str>,
    /// Optional conversation name to persist when the ID is provided.
    pub conversation_name: Option<&'a str>,
    /// Optional conversation model metadata for the conversation row.
    pub conversation_model: Option<&'a str>,
}

/// Debug metadata describing how a prompt will execute.
#[derive(Debug, Clone)]
pub struct PromptDebugInfo {
    pub model: String,
    pub provider: String,
    pub retries: usize,
    pub retry_backoff_ms: u64,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Execute a prompt using the OpenAI chat completions API.
///
/// By default this targets `https://api.openai.com/v1/chat/completions`
/// with the model specified via `LLM_OPENAI_MODEL` (default:
/// `gpt-4o-mini`). Set the environment variable `LLM_PROMPT_STUB`
/// to `1`/`true` for test environments that should avoid making a
/// network request and instead return the historical stub output.
pub fn execute_prompt(prompt: &str, config: PromptConfig<'_>) -> Result<String> {
    stream_prompt_internal(prompt, config, None)
}

/// Execute a prompt while streaming tokens to the provided sink.
pub fn stream_prompt(
    prompt: &str,
    config: PromptConfig<'_>,
    sink: &mut dyn ProviderStreamSink,
) -> Result<String> {
    stream_prompt_internal(prompt, config, Some(sink))
}

/// Execute a prompt built from arbitrary message history.
pub fn execute_prompt_with_messages(
    messages: Vec<PromptMessage>,
    attachments: Vec<Attachment>,
    config: PromptConfig<'_>,
) -> Result<String> {
    if prompt_stub_enabled() {
        let stub = stub_response_text(last_user_message_content(&messages));
        let mut request = PromptRequest {
            model: resolve_model_name(&config)?,
            messages,
            attachments,
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };
        apply_prompt_overrides(&mut request, &config);
        log_prompt_result(&request, &stub, Duration::from_millis(0), &config)?;
        return Ok(stub);
    }
    let request = build_prompt_request_from_messages(messages, attachments, &config)?;
    execute_request(request, &config, None)
}

/// Stream a prompt built from arbitrary message history.
pub fn stream_prompt_with_messages(
    messages: Vec<PromptMessage>,
    attachments: Vec<Attachment>,
    config: PromptConfig<'_>,
    sink: &mut dyn ProviderStreamSink,
) -> Result<String> {
    if prompt_stub_enabled() {
        let stub = stub_response_text(last_user_message_content(&messages));
        sink.handle_text_delta(&stub)?;
        sink.handle_done()?;
        let mut request = PromptRequest {
            model: resolve_model_name(&config)?,
            messages,
            attachments,
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };
        apply_prompt_overrides(&mut request, &config);
        log_prompt_result(&request, &stub, Duration::from_millis(0), &config)?;
        return Ok(stub);
    }
    let request = build_prompt_request_from_messages(messages, attachments, &config)?;
    execute_request(request, &config, Some(sink))
}

fn prompt_stub_enabled() -> bool {
    match env::var("LLM_PROMPT_STUB") {
        Ok(value) => matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// Resolve details that are useful for logging/diagnostics without executing a prompt.
pub fn prompt_debug_info(config: &PromptConfig<'_>) -> Result<PromptDebugInfo> {
    let model = resolve_model_name(config)?;
    let provider = provider_from_model(&model).to_string();
    let retries = resolve_retries(provider.as_str(), config);
    let retry_backoff_ms = resolve_retry_backoff_ms(provider.as_str(), config);
    Ok(PromptDebugInfo {
        model,
        provider,
        retries,
        retry_backoff_ms,
        temperature: config.temperature,
        max_tokens: config.max_tokens,
    })
}

fn last_user_message_content(messages: &[PromptMessage]) -> &str {
    messages
        .iter()
        .rev()
        .find(|msg| matches!(msg.role, MessageRole::User))
        .map(|msg| msg.content.as_str())
        .unwrap_or_default()
}

fn stub_response_text(prompt: &str) -> String {
    format!("llm-core stub response to: {}", prompt)
}

fn stream_prompt_internal(
    prompt: &str,
    config: PromptConfig<'_>,
    mut external_sink: Option<&mut dyn ProviderStreamSink>,
) -> Result<String> {
    if prompt_stub_enabled() {
        let model = resolve_model_name(&config)?;
        let mut request = PromptRequest::user_only(model, prompt.to_string());
        apply_prompt_overrides(&mut request, &config);
        let text = stub_response_text(prompt);
        if let Some(sink) = external_sink.as_deref_mut() {
            sink.handle_text_delta(&text)?;
            sink.handle_done()?;
        }
        log_prompt_result(&request, &text, Duration::from_millis(0), &config)?;
        return Ok(text);
    }

    let model = resolve_model_name(&config)?;
    let mut request = PromptRequest::user_only(model, prompt.to_string());
    apply_prompt_overrides(&mut request, &config);

    execute_request(request, &config, external_sink)
}

fn build_prompt_request_from_messages(
    messages: Vec<PromptMessage>,
    attachments: Vec<Attachment>,
    config: &PromptConfig<'_>,
) -> Result<PromptRequest> {
    let model = resolve_model_name(config)?;
    let mut request = PromptRequest {
        model,
        messages,
        attachments,
        temperature: None,
        max_tokens: None,
        tools: None,
        functions: None,
        tool_choice: None,
        response_format: None,
        schema: None,
    };
    apply_prompt_overrides(&mut request, config);
    Ok(request)
}

struct TeeStreamSink<'a, 'b> {
    collector: &'a mut dyn ProviderStreamSink,
    forward: &'b mut dyn ProviderStreamSink,
}

impl<'a, 'b> TeeStreamSink<'a, 'b> {
    fn new(
        collector: &'a mut dyn ProviderStreamSink,
        forward: &'b mut dyn ProviderStreamSink,
    ) -> Self {
        Self { collector, forward }
    }
}

impl<'a, 'b> ProviderStreamSink for TeeStreamSink<'a, 'b> {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()> {
        self.collector.handle_text_delta(delta)?;
        self.forward.handle_text_delta(delta)
    }

    fn handle_done(&mut self) -> Result<()> {
        self.collector.handle_done()?;
        self.forward.handle_done()
    }
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name).ok().and_then(|value| value.parse().ok())
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name).ok().and_then(|value| value.parse().ok())
}

fn env_u32(name: &str) -> Option<u32> {
    env::var(name).ok().and_then(|value| value.parse().ok())
}

fn resolve_model_name(config: &PromptConfig<'_>) -> Result<String> {
    let saved_default = get_default_model()?;
    Ok(config
        .model
        .map(normalize_model_name)
        .or_else(|| saved_default.clone())
        .or_else(|| {
            env::var("LLM_DEFAULT_MODEL")
                .ok()
                .map(|s| normalize_model_name(&s))
        })
        .or_else(|| {
            env::var("LLM_OPENAI_MODEL")
                .ok()
                .map(|s| normalize_model_name(&s))
        })
        .unwrap_or_else(|| normalize_model_name(DEFAULT_MODEL)))
}

fn provider_from_model(model: &str) -> &str {
    model
        .split_once('/')
        .map(|(provider, _)| provider)
        .unwrap_or("openai")
}

fn apply_prompt_overrides(request: &mut PromptRequest, config: &PromptConfig<'_>) {
    if let Some(temp) = config.temperature {
        request.temperature = Some(temp);
    }
    if let Some(max_tokens) = config.max_tokens {
        request.max_tokens = Some(max_tokens);
    }
}

fn build_provider(
    provider_id: &str,
    request: &PromptRequest,
    config: &PromptConfig<'_>,
) -> Result<Box<dyn PromptProvider>> {
    let retries = resolve_retries(provider_id, config);
    let retry_backoff_ms = resolve_retry_backoff_ms(provider_id, config);
    match provider_id {
        "openai" => {
            let key = resolve_api_key(provider_id, config)?;
            let base_url = env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            Ok(Box::new(OpenAIProvider::new(OpenAIConfig {
                base_url,
                api_key: key,
                retries,
                retry_backoff: Duration::from_millis(retry_backoff_ms),
            })?))
        }
        "openai-compatible" => {
            let key = resolve_api_key(provider_id, config)?;
            let base_url = resolve_openai_compatible_base_url();
            Ok(Box::new(OpenAIProvider::new(OpenAIConfig {
                base_url,
                api_key: key,
                retries,
                retry_backoff: Duration::from_millis(retry_backoff_ms),
            })?))
        }
        "anthropic" => {
            let key = resolve_api_key(provider_id, config)?;
            let base_url = resolve_anthropic_base_url();
            let default_max_tokens = resolve_anthropic_default_max_tokens();
            Ok(Box::new(AnthropicProvider::new(AnthropicConfig {
                base_url,
                api_key: key,
                retries,
                retry_backoff: Duration::from_millis(retry_backoff_ms),
                default_max_tokens,
                timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            })?))
        }
        other => bail!(
            "Unsupported provider '{other}' for model '{}'",
            request.model
        ),
    }
}

fn execute_request(
    request: PromptRequest,
    config: &PromptConfig<'_>,
    external_sink: Option<&mut dyn ProviderStreamSink>,
) -> Result<String> {
    let provider_id = provider_from_model(&request.model);
    let provider = build_provider(provider_id, &request, config)?;
    let mut external_sink = external_sink;
    let request_for_logging = request.clone();
    let mut accumulator = VecStreamSink::new();
    let start = Instant::now();

    if provider.supports_streaming() {
        if let Some(ref mut sink) = external_sink {
            let mut tee = TeeStreamSink::new(&mut accumulator, *sink);
            provider.stream(request, &mut tee)?;
        } else {
            provider.stream(request, &mut accumulator)?;
        }
    } else {
        let completion = provider.complete(request)?;
        accumulator.handle_text_delta(&completion.text)?;
        accumulator.handle_done()?;
        if let Some(ref mut sink) = external_sink {
            (*sink).handle_text_delta(&completion.text)?;
            (*sink).handle_done()?;
        }
    }

    let duration = start.elapsed();
    let text = accumulator.into_string();
    log_prompt_result(&request_for_logging, &text, duration, config)?;
    Ok(text)
}

fn log_prompt_result(
    request: &PromptRequest,
    response: &str,
    duration: Duration,
    config: &PromptConfig<'_>,
) -> Result<()> {
    if matches!(config.log_override, Some(false)) {
        return Ok(());
    }
    let force_logging = matches!(config.log_override, Some(true));
    let (prompt, system) = extract_prompt_and_system(request);
    let prompt_json = serialize_prompt_messages(&request.messages)?;
    let options_json = options_metadata_json(config)?;
    let record = logs::LogRecord {
        model: config
            .model
            .map(|model| model.to_string())
            .unwrap_or_else(|| request.model.clone()),
        resolved_model: request.model.clone(),
        prompt,
        system,
        prompt_json,
        options_json,
        response: response.to_string(),
        response_json: None,
        conversation_id: config.conversation_id.map(|s| s.to_string()),
        conversation_name: config.conversation_name.map(|s| s.to_string()),
        conversation_model: config.conversation_model.map(|s| s.to_string()),
        duration_ms: Some(duration.as_millis()),
        input_tokens: None,
        output_tokens: None,
        token_details: None,
        tool_calls_json: None,
        tool_results_json: None,
        finish_reason: None,
        usage_json: None,
        schema_id: None,
    };
    let db_path = config.database_path.map(Path::new);
    logs::record_log_entry(record, force_logging, db_path)?;
    Ok(())
}

fn extract_prompt_and_system(request: &PromptRequest) -> (Option<String>, Option<String>) {
    let mut last_user: Option<String> = None;
    let mut first_system: Option<String> = None;
    for message in &request.messages {
        match message.role {
            MessageRole::User => {
                last_user = Some(message.content.clone());
            }
            MessageRole::System => {
                if first_system.is_none() {
                    first_system = Some(message.content.clone());
                }
            }
            MessageRole::Assistant | MessageRole::Tool | MessageRole::Function => {}
        }
    }
    (last_user, first_system)
}

fn serialize_prompt_messages(messages: &[PromptMessage]) -> Result<Option<String>> {
    if messages.is_empty() {
        return Ok(None);
    }
    let payload: Vec<JsonValue> = messages
        .iter()
        .map(|message| {
            json!({
                "role": message.role.as_str(),
                "content": message.content,
            })
        })
        .collect();
    let serialized = serde_json::to_string(&payload)?;
    Ok(Some(serialized))
}

fn options_metadata_json(config: &PromptConfig<'_>) -> Result<Option<String>> {
    let mut map = JsonMap::new();

    if let Some(model) = config.model {
        map.insert("model".to_string(), json!(model));
    }
    if let Some(temp) = config.temperature {
        map.insert("temperature".to_string(), json!(temp));
    }
    if let Some(max) = config.max_tokens {
        map.insert("max_tokens".to_string(), json!(max));
    }
    if let Some(retries) = config.retries {
        map.insert("retries".to_string(), json!(retries));
    }
    if let Some(backoff) = config.retry_backoff_ms {
        map.insert("retry_backoff_ms".to_string(), json!(backoff));
    }
    if let Some(log_override) = config.log_override {
        map.insert("log_override".to_string(), json!(log_override));
    }

    if map.is_empty() {
        Ok(None)
    } else {
        Ok(Some(JsonValue::Object(map).to_string()))
    }
}

fn resolve_api_key(provider_id: &str, config: &PromptConfig<'_>) -> Result<String> {
    if let Some(override_key) = config.api_key {
        resolve_key_override(override_key)
    } else {
        resolve_provider_key(provider_id)
    }
}

fn resolve_key_override(value: &str) -> Result<String> {
    resolve_key(KeyQuery {
        input: Some(value),
        alias: None,
        env: None,
    })?
    .map(|value| sanitize_secret(&value))
    .ok_or_else(|| anyhow!("Key override did not resolve to a value"))
}

fn resolve_provider_key(provider_id: &str) -> Result<String> {
    struct ProviderKey<'a> {
        alias: &'a str,
        env_vars: &'a [&'a str],
        display: &'a str,
    }
    let info = match provider_id {
        "openai" => ProviderKey {
            alias: "openai",
            env_vars: &["OPENAI_API_KEY", "LLM_OPENAI_API_KEY"],
            display: "OpenAI",
        },
        "openai-compatible" => ProviderKey {
            alias: "openai-compatible",
            env_vars: &["OPENAI_COMPATIBLE_API_KEY", "LLM_OPENAI_COMPATIBLE_API_KEY"],
            display: "OpenAI-compatible",
        },
        "anthropic" => ProviderKey {
            alias: "anthropic",
            env_vars: &["ANTHROPIC_API_KEY", "LLM_ANTHROPIC_API_KEY"],
            display: "Anthropic",
        },
        other => {
            bail!("No key resolution configured for provider '{other}'");
        }
    };

    if let Some(value) = resolve_key(KeyQuery {
        input: None,
        alias: Some(info.alias),
        env: info.env_vars.first().copied(),
    })? {
        return Ok(sanitize_secret(&value));
    }

    for env_var in info.env_vars.iter().skip(1) {
        if let Ok(value) = env::var(env_var) {
            if !value.is_empty() {
                return Ok(sanitize_secret(&value));
            }
        }
    }

    bail!(
        "{} API key not configured. Run `llm keys set {} --value <key>` or set one of: {}",
        info.display,
        info.alias,
        info.env_vars.join(", ")
    )
}

fn sanitize_secret(value: &str) -> String {
    let trimmed = value.trim();
    let cleaned: String = trimmed.chars().filter(|c| !c.is_control()).collect();
    if cleaned.is_empty() {
        trimmed.to_string()
    } else {
        cleaned
    }
}

fn resolve_retries(provider_id: &str, config: &PromptConfig<'_>) -> usize {
    config
        .retries
        .or_else(|| match provider_id {
            "openai" => env_usize("LLM_OPENAI_RETRIES"),
            "openai-compatible" => env_usize("LLM_OPENAI_COMPATIBLE_RETRIES")
                .or_else(|| env_usize("LLM_OPENAI_RETRIES")),
            "anthropic" => env_usize("LLM_ANTHROPIC_RETRIES"),
            _ => None,
        })
        .unwrap_or(DEFAULT_RETRIES)
}

fn resolve_retry_backoff_ms(provider_id: &str, config: &PromptConfig<'_>) -> u64 {
    config
        .retry_backoff_ms
        .or_else(|| match provider_id {
            "openai" => env_u64("LLM_OPENAI_RETRY_BACKOFF_MS"),
            "openai-compatible" => env_u64("LLM_OPENAI_COMPATIBLE_RETRY_BACKOFF_MS")
                .or_else(|| env_u64("LLM_OPENAI_RETRY_BACKOFF_MS")),
            "anthropic" => env_u64("LLM_ANTHROPIC_RETRY_BACKOFF_MS"),
            _ => None,
        })
        .unwrap_or(DEFAULT_BACKOFF_MS)
}

fn resolve_openai_compatible_base_url() -> String {
    env::var("OPENAI_COMPATIBLE_BASE_URL")
        .or_else(|_| env::var("LLM_OPENAI_COMPATIBLE_BASE_URL"))
        .or_else(|_| env::var("OPENAI_BASE_URL"))
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
}

fn resolve_anthropic_base_url() -> String {
    env::var("ANTHROPIC_BASE_URL")
        .or_else(|_| env::var("LLM_ANTHROPIC_BASE_URL"))
        .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string())
}

fn resolve_anthropic_default_max_tokens() -> Option<u32> {
    env_u32("LLM_ANTHROPIC_MAX_TOKENS")
        .or_else(|| env_u32("LLM_ANTHROPIC_DEFAULT_MAX_TOKENS"))
        .or_else(|| env_u32("ANTHROPIC_MAX_TOKENS"))
}

/// Return the list of built-in models annotated with the current default.
pub fn available_models() -> Result<Vec<ModelInfo>> {
    let default = get_default_model()?;
    let default = default.or_else(|| {
        env::var("LLM_DEFAULT_MODEL")
            .ok()
            .map(|s| normalize_model_name(&s))
    });
    // Load stored model options to check which models have options configured
    let stored_options = model_options::load_model_options().unwrap_or_default();

    let mut models: Vec<ModelInfo> = BUILTIN_MODELS
        .iter()
        .map(|model| ModelInfo {
            name: model.canonical.to_string(),
            provider: model.provider.to_string(),
            description: model.description.to_string(),
            is_default: default.as_deref() == Some(model.canonical),
            aliases: model
                .aliases
                .iter()
                .map(|alias| alias.to_string())
                .collect(),
            supports_tools: provider_supports_tools(model.provider),
            supports_schemas: provider_supports_schemas(model.provider),
            supports_async: provider_supports_async(model.provider),
            has_options: stored_options.contains_key(model.canonical),
        })
        .collect();
    if let Some(ref default_name) = default {
        if !models.iter().any(|m| m.is_default) {
            let provider = provider_from_model(default_name).to_string();
            models.push(ModelInfo {
                name: default_name.clone(),
                provider: provider.clone(),
                description: format!("Custom model for provider '{provider}'"),
                is_default: true,
                aliases: Vec::new(),
                supports_tools: provider_supports_tools(&provider),
                supports_schemas: provider_supports_schemas(&provider),
                supports_async: provider_supports_async(&provider),
                has_options: stored_options.contains_key(default_name),
            });
        } else {
            for model in &mut models {
                model.is_default = model.name == *default_name;
            }
        }
    } else if let Some(first) = models.first_mut() {
        first.is_default = true;
    }
    Ok(models)
}

/// Query models by search terms, returning matches sorted by name length (shortest first).
///
/// This implements upstream `--query` behavior: when the user provides query terms
/// instead of a specific model name, we find all models whose name, description,
/// or aliases contain any of the query terms (case-insensitive), then return
/// them sorted by name length so the shortest/simplest match is first.
pub fn query_models(query: &str) -> Result<Vec<ModelInfo>> {
    let models = available_models()?;
    let query_lower = query.to_ascii_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    if terms.is_empty() {
        return Ok(models);
    }

    let mut matches: Vec<ModelInfo> = models
        .into_iter()
        .filter(|model| {
            let name_lower = model.name.to_ascii_lowercase();
            let desc_lower = model.description.to_ascii_lowercase();
            let aliases_lower: Vec<String> = model
                .aliases
                .iter()
                .map(|a| a.to_ascii_lowercase())
                .collect();

            terms.iter().any(|term| {
                name_lower.contains(term)
                    || desc_lower.contains(term)
                    || aliases_lower.iter().any(|a| a.contains(term))
            })
        })
        .collect();

    // Sort by name length (shortest first) for upstream parity
    matches.sort_by_key(|m| m.name.len());
    Ok(matches)
}

/// Persist the default model selection to disk.
pub fn set_default_model(name: &str) -> Result<()> {
    let normalized = normalize_model_name(name);
    if !model_is_allowed(&normalized) {
        bail!("Unknown model '{name}'. Use `llm-cli models list` to see available models.");
    }
    let path = default_model_path()?;
    fs::write(path, normalized).context("failed to write default model")?;
    Ok(())
}

/// Load the current default model if one has been configured.
///
/// Uses `default_model.txt` with fallback to legacy `default-model.txt`.
pub fn get_default_model() -> Result<Option<String>> {
    let path = default_model_path()?;
    let legacy_path = legacy_default_model_path()?;

    // Try new path first, then fallback to legacy
    let read_path = if path.exists() {
        Some(path)
    } else if legacy_path.exists() {
        Some(legacy_path)
    } else {
        None
    };

    let Some(file_path) = read_path else {
        return Ok(None);
    };

    let contents = fs::read_to_string(&file_path).context("failed to read default model")?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalize_model_name(trimmed)))
    }
}

/// Return the path to `default_model.txt` within the user directory.
pub fn default_model_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("default_model.txt");
    Ok(path)
}

/// Return the legacy path to `default-model.txt` for fallback reading.
fn legacy_default_model_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("default-model.txt");
    Ok(path)
}

/// Normalize a model name by resolving built-in aliases, user-defined aliases,
/// and provider prefixes.
///
/// Resolution order:
/// 1. Check for built-in canonical match
/// 2. Check for built-in alias match
/// 3. Check for user-defined alias in aliases.json
/// 4. Handle provider/model format
/// 5. Return as-is if no match
pub fn normalize_model_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 1. Check built-in aliases first
    if let Some(canonical) = canonical_for(trimmed) {
        return canonical.to_string();
    }

    let replaced = trimmed.replace(':', "/");
    if let Some(canonical) = canonical_for(&replaced) {
        return canonical.to_string();
    }

    // 2. Check user-defined aliases
    if let Ok(Some(target)) = aliases::resolve_user_alias(trimmed) {
        // Recursively normalize the target (in case it's also an alias)
        return normalize_model_name(&target);
    }
    if let Ok(Some(target)) = aliases::resolve_user_alias(&replaced) {
        return normalize_model_name(&target);
    }

    // 3. Handle provider/model format
    if let Some((provider, model)) = replaced.split_once('/') {
        let provider = provider.trim();
        let model = model.trim();
        let candidate = format!("{}/{}", provider, model);
        if let Some(canonical) = canonical_for(&candidate) {
            return canonical.to_string();
        }
        if let Some(canonical) = canonical_for(model) {
            return canonical.to_string();
        }
        return candidate;
    }

    // 4. Try with openai prefix
    if let Some(canonical) = canonical_for(&format!("openai/{}", replaced)) {
        canonical.to_string()
    } else {
        replaced
    }
}

fn canonical_for(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace(':', "/");
    let normalized_lower = normalized.to_ascii_lowercase();

    for model in BUILTIN_MODELS {
        if normalized_lower == model.canonical.to_ascii_lowercase() {
            return Some(model.canonical);
        }
        for alias in model.aliases {
            if normalized_lower == alias.to_ascii_lowercase() {
                return Some(model.canonical);
            }
            let provider_alias = format!("{}/{}", model.provider, alias);
            if normalized_lower == provider_alias.to_ascii_lowercase() {
                return Some(model.canonical);
            }
        }
    }

    None
}

fn model_is_allowed(name: &str) -> bool {
    if canonical_for(name).is_some() {
        return true;
    }
    if let Some((provider, model)) = name.split_once('/') {
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() {
            return false;
        }
        matches!(provider, "openai" | "openai-compatible" | "anthropic")
    } else {
        false
    }
}

/// Resolve the user directory where configuration, logs and databases reside.
///
/// Mirrors the Python implementation:
/// - `LLM_USER_PATH` environment variable takes precedence.
/// - Otherwise use the platform-appropriate application data directory.
pub fn user_dir() -> Result<PathBuf> {
    if let Ok(env_path) = env::var("LLM_USER_PATH") {
        let path = PathBuf::from(env_path);
        ensure_directory(&path)?;
        return Ok(path);
    }

    let project = ProjectDirs::from("io", "datasette", "llm")
        .ok_or_else(|| anyhow!("Unable to determine application data directory"))?;
    let path = project.data_dir().to_path_buf();
    ensure_directory(&path)?;
    Ok(path)
}

fn ensure_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).with_context(|| format!("Failed to create {}", path.display()))?;
    }
    Ok(())
}

/// Return the path to `keys.json` within the user directory.
pub fn keys_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("keys.json");
    Ok(path)
}

/// Return the path to `logs.db` within the user directory.
pub fn logs_db_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("logs.db");
    Ok(path)
}

/// Return the path to `embeddings.db` within the user directory.
pub fn embeddings_db_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("embeddings.db");
    Ok(path)
}

/// Load keys from `keys.json`, returning an empty map if the file is missing.
pub fn load_keys() -> Result<HashMap<String, String>> {
    let path = keys_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let parsed: HashMap<String, String> = serde_json::from_str(&contents)
        .with_context(|| format!("Invalid JSON in {}", path.display()))?;
    Ok(parsed)
}

/// Return sorted key aliases, excluding reserved metadata entries.
pub fn list_key_names() -> Result<Vec<String>> {
    let mut names: Vec<String> = load_keys()?
        .into_keys()
        .filter(|k| k != "// Note")
        .collect();
    names.sort();
    Ok(names)
}

/// Persist a key value to `keys.json`, inserting the warning note if needed.
pub fn save_key(name: &str, value: &str) -> Result<()> {
    let mut keys = load_keys()?;
    keys.insert(
        "// Note".to_string(),
        "This file stores secret API credentials. Do not share!".to_string(),
    );
    keys.insert(name.to_string(), value.to_string());
    let path = keys_path()?;
    let json = serde_json::to_string_pretty(&keys)? + "\n";
    fs::write(path, json).context("Failed to write keys.json")?;
    Ok(())
}

/// Represents a request for resolving an API key.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct KeyQuery<'a> {
    /// User-provided input (either literal key or alias).
    pub input: Option<&'a str>,
    /// Alias to look up in `keys.json`.
    pub alias: Option<&'a str>,
    /// Environment variable name to use as final fallback.
    pub env: Option<&'a str>,
}

/// Resolve an API key following the precedence rules of the Python version.
pub fn resolve_key(query: KeyQuery<'_>) -> Result<Option<String>> {
    let keys = load_keys()?;

    if let Some(explicit) = query.input.filter(|s| !s.is_empty()) {
        if let Some(value) = keys.get(explicit) {
            return Ok(Some(value.clone()));
        }
        return Ok(Some(explicit.to_string()));
    }

    if let Some(alias) = query.alias.filter(|s| !s.is_empty()) {
        if let Some(value) = keys.get(alias) {
            return Ok(Some(value.clone()));
        }
    }

    if let Some(env_var) = query.env.filter(|s| !s.is_empty()) {
        if let Ok(value) = env::var(env_var) {
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
    fn user_dir_respects_env_override() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            let path = user_dir().expect("user dir");
            assert_eq!(path, tmp.path());
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn load_keys_missing_file() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            let keys = load_keys().expect("keys");
            assert!(keys.is_empty());
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn available_models_uses_saved_default() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // default should fall back to first entry
            let models = available_models().expect("models");
            assert!(models.first().unwrap().is_default);

            set_default_model("openai:gpt-4.1-mini").expect("set default");
            let models = available_models().expect("models");
            let default_count = models.iter().filter(|m| m.is_default).count();
            assert_eq!(default_count, 1);
            let default = models.iter().find(|m| m.is_default).unwrap();
            assert_eq!(default.name, "openai/gpt-4.1-mini");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn available_models_include_recent_openai_and_anthropic_releases() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let models = available_models().expect("models");
            assert!(models.iter().any(|m| m.name == "openai/gpt-5.2-2025-12-11"));
            assert!(models.iter().any(|m| m.name == "openai/gpt-5"));
            assert!(models
                .iter()
                .any(|m| m.name == "anthropic/claude-sonnet-4-6"));
            assert!(models.iter().any(|m| m.name == "anthropic/claude-opus-4-6"));

            let sonnet_46 = models
                .iter()
                .find(|m| m.name == "anthropic/claude-sonnet-4-6")
                .expect("sonnet 4.6 present");
            assert!(sonnet_46.aliases.iter().any(|a| a == "claude-sonnet-4.6"));

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_default_model_accepts_new_release_aliases() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_default_model("gpt-5.2").expect("set default gpt-5.2");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "openai/gpt-5.2-2025-12-11");

            set_default_model("claude-opus-4.6").expect("set default claude-opus-4.6");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "anthropic/claude-opus-4-6");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_default_model_rejects_unknown() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            let err = set_default_model("unknown:model").unwrap_err();
            assert!(err.to_string().contains("Unknown model"));
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_default_model_accepts_openai_compatible() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            set_default_model("openai-compatible:custom-model").expect("set default");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "openai-compatible/custom-model");
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_default_model_accepts_anthropic() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            set_default_model("anthropic:claude-4-opus").expect("set default");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "anthropic/claude-opus-4-0");
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn normalize_model_name_accepts_aliases() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());
            set_default_model("4o").expect("alias resolves");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "openai/gpt-4o");
            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn resolve_key_precedence() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            let mut path = tmp.path().to_path_buf();
            path.push("keys.json");
            fs::write(&path, r#"{"openai": "stored-secret"}"#).unwrap();

            // explicit input that matches alias in file
            let key = resolve_key(KeyQuery {
                input: Some("openai"),
                alias: None,
                env: None,
            })
            .unwrap();
            assert_eq!(key.as_deref(), Some("stored-secret"));

            // explicit literal
            let key = resolve_key(KeyQuery {
                input: Some("literal-key"),
                alias: None,
                env: None,
            })
            .unwrap();
            assert_eq!(key.as_deref(), Some("literal-key"));

            // alias fallback
            let key = resolve_key(KeyQuery {
                input: None,
                alias: Some("openai"),
                env: None,
            })
            .unwrap();
            assert_eq!(key.as_deref(), Some("stored-secret"));

            // env fallback
            env::set_var("OPENAI_API_KEY", "env-secret");
            let key = resolve_key(KeyQuery {
                input: None,
                alias: Some("missing"),
                env: Some("OPENAI_API_KEY"),
            })
            .unwrap();
            assert_eq!(key.as_deref(), Some("env-secret"));

            env::remove_var("LLM_USER_PATH");
            env::remove_var("OPENAI_API_KEY");
        });
    }

    #[test]
    fn save_key_creates_file_with_note() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            save_key("openai", "secret").unwrap();
            let contents = fs::read_to_string(tmp.path().join("keys.json")).unwrap();
            let map: HashMap<String, String> = serde_json::from_str(&contents).unwrap();
            assert_eq!(map.get("openai"), Some(&"secret".to_string()));
            assert!(map.contains_key("// Note"));

            env::remove_var("LLM_USER_PATH");
        });
    }
    #[test]
    fn normalize_model_name_resolves_user_alias() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Set up a user-defined alias
            crate::aliases::set_alias("myfast", "openai/gpt-4o-mini").expect("set alias");

            // The alias should resolve through normalize_model_name
            let resolved = normalize_model_name("myfast");
            assert_eq!(resolved, "openai/gpt-4o-mini");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn normalize_model_name_user_alias_to_builtin() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // User alias that points to a built-in alias
            crate::aliases::set_alias("smart", "4o").expect("set alias");

            // Should recursively resolve to canonical
            let resolved = normalize_model_name("smart");
            assert_eq!(resolved, "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn normalize_model_name_builtin_takes_precedence() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Try to override a built-in alias (shouldn't work - built-ins checked first)
            crate::aliases::set_alias("4o", "anthropic/claude-3-opus").expect("set alias");

            // Built-in should win
            let resolved = normalize_model_name("4o");
            assert_eq!(resolved, "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn default_model_fallback_to_legacy() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Write to legacy path only
            let legacy_path = tmp.path().join("default-model.txt");
            fs::write(&legacy_path, "openai/gpt-4").expect("write legacy");

            // Should read from legacy
            let default = get_default_model().expect("get default").unwrap();
            assert_eq!(default, "openai/gpt-4");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn default_model_new_path_takes_precedence() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Write to both paths
            let new_path = tmp.path().join("default_model.txt");
            let legacy_path = tmp.path().join("default-model.txt");
            fs::write(&new_path, "openai/gpt-4o").expect("write new");
            fs::write(&legacy_path, "openai/gpt-3.5-turbo").expect("write legacy");

            // New path should win
            let default = get_default_model().expect("get default").unwrap();
            assert_eq!(default, "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn set_default_model_writes_to_new_path() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            set_default_model("openai/gpt-4o").expect("set default");

            // Should be written to new path
            let new_path = tmp.path().join("default_model.txt");
            assert!(new_path.exists());

            let contents = fs::read_to_string(&new_path).expect("read");
            assert_eq!(contents.trim(), "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn user_alias_with_colon_syntax() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Alias using colon syntax target
            crate::aliases::set_alias("custom", "anthropic:claude-opus-4.6").expect("set alias");

            // Should resolve and normalize
            let resolved = normalize_model_name("custom");
            assert_eq!(resolved, "anthropic/claude-opus-4-6");

            env::remove_var("LLM_USER_PATH");
        });
    }

    #[test]
    fn chained_user_aliases() {
        with_env_lock(|| {
            let tmp = temp_user_dir();
            env::set_var("LLM_USER_PATH", tmp.path());

            // Chain: a -> b -> gpt-4o
            crate::aliases::set_alias("b", "4o").expect("set alias b");
            crate::aliases::set_alias("a", "b").expect("set alias a");

            let resolved = normalize_model_name("a");
            assert_eq!(resolved, "openai/gpt-4o");

            env::remove_var("LLM_USER_PATH");
        });
    }
}
