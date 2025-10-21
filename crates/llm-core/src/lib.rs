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
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod logs;
mod providers;
pub use providers::{MessageRole, PromptMessage, StreamSink};
pub use logs::{
    backup_logs,
    list_logs,
    logs_enabled,
    logs_status,
    set_logging_enabled,
    ListLogsOptions,
    LogEntry,
    LogsStatus,
};

struct BuiltinModel {
    canonical: &'static str,
    provider: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
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
        canonical: "openai/chatgpt-4o-latest",
        provider: "openai",
        description: "ChatGPT 4o latest",
        aliases: &["chatgpt-4o-latest", "chatgpt-4o"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o-audio-preview",
        provider: "openai",
        description: "GPT-4o audio preview",
        aliases: &["gpt-4o-audio-preview"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o-audio-preview-2024-12-17",
        provider: "openai",
        description: "GPT-4o audio preview (2024-12-17)",
        aliases: &["gpt-4o-audio-preview-2024-12-17"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o-audio-preview-2024-10-01",
        provider: "openai",
        description: "GPT-4o audio preview (2024-10-01)",
        aliases: &["gpt-4o-audio-preview-2024-10-01"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o-mini-audio-preview",
        provider: "openai",
        description: "GPT-4o mini audio preview",
        aliases: &["gpt-4o-mini-audio-preview"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4o-mini-audio-preview-2024-12-17",
        provider: "openai",
        description: "GPT-4o mini audio preview (2024-12-17)",
        aliases: &["gpt-4o-mini-audio-preview-2024-12-17"],
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
        canonical: "openai/gpt-4-32k",
        provider: "openai",
        description: "GPT-4 32k",
        aliases: &["gpt-4-32k", "4-32k"],
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
        canonical: "openai/gpt-4.5-preview-2025-02-27",
        provider: "openai",
        description: "GPT-4.5 preview (2025-02-27)",
        aliases: &["gpt-4.5-preview-2025-02-27"],
    },
    BuiltinModel {
        canonical: "openai/gpt-4.5-preview",
        provider: "openai",
        description: "GPT-4.5 preview",
        aliases: &["gpt-4.5-preview", "gpt-4.5"],
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
        canonical: "openai/o1-preview",
        provider: "openai",
        description: "o1 preview",
        aliases: &["o1-preview"],
    },
    BuiltinModel {
        canonical: "openai/o1-mini",
        provider: "openai",
        description: "o1 mini",
        aliases: &["o1-mini"],
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
        canonical: "openai/gpt-3.5-turbo-instruct",
        provider: "openai",
        description: "GPT-3.5 Turbo instruct",
        aliases: &["gpt-3.5-turbo-instruct", "3.5-instruct", "chatgpt-instruct"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-opus-20240229",
        provider: "anthropic",
        description: "Claude 3 Opus (2024-02-29)",
        aliases: &[],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-opus-latest",
        provider: "anthropic",
        description: "Claude 3 Opus (latest)",
        aliases: &["claude-3-opus"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-sonnet-latest",
        provider: "anthropic",
        description: "Claude 3 Sonnet (latest)",
        aliases: &["claude-3-sonnet"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-sonnet-20240229",
        provider: "anthropic",
        description: "Claude 3 Sonnet (2024-02-29)",
        aliases: &["claude-3-sonnet-20240229"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-haiku-latest",
        provider: "anthropic",
        description: "Claude 3 Haiku (latest)",
        aliases: &["claude-3-haiku"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-haiku-20240307",
        provider: "anthropic",
        description: "Claude 3 Haiku (2024-03-07)",
        aliases: &["claude-3-haiku-20240307"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-5-sonnet-latest",
        provider: "anthropic",
        description: "Claude 3.5 Sonnet (latest)",
        aliases: &["claude-3.5-sonnet", "claude-3.5-sonnet-latest"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-5-sonnet-20240620",
        provider: "anthropic",
        description: "Claude 3.5 Sonnet (2024-06-20)",
        aliases: &["claude-3-5-sonnet-20240620"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-5-sonnet-20241022",
        provider: "anthropic",
        description: "Claude 3.5 Sonnet (2024-10-22)",
        aliases: &["claude-3-5-sonnet-20241022"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-5-haiku-latest",
        provider: "anthropic",
        description: "Claude 3.5 Haiku (latest)",
        aliases: &["claude-3.5-haiku"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-7-sonnet-latest",
        provider: "anthropic",
        description: "Claude 3.7 Sonnet (latest)",
        aliases: &["claude-3.7-sonnet", "claude-3.7-sonnet-latest"],
    },
    BuiltinModel {
        canonical: "anthropic/claude-3-7-sonnet-20250219",
        provider: "anthropic",
        description: "Claude 3.7 Sonnet (2025-02-19)",
        aliases: &["claude-3-7-sonnet-20250219"],
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
}

/// Returns a static string identifying the core crate version.
pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Configuration options for prompt execution.
pub struct PromptConfig<'a> {
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
}

impl Default for PromptConfig<'_> {
    fn default() -> Self {
        PromptConfig {
            model: None,
            temperature: None,
            max_tokens: None,
            retries: None,
            retry_backoff_ms: None,
            api_key: None,
        }
    }
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
    config: PromptConfig<'_>,
) -> Result<String> {
    if prompt_stub_enabled() {
        let stub = stub_response_text(last_user_message_content(&messages));
        let mut request = PromptRequest {
            model: resolve_model_name(&config)?,
            messages,
            temperature: None,
            max_tokens: None,
        };
        apply_prompt_overrides(&mut request, &config);
        log_prompt_result(&request, &stub, Duration::from_millis(0))?;
        return Ok(stub);
    }
    let request = build_prompt_request_from_messages(messages, &config)?;
    execute_request(request, &config, None)
}

/// Stream a prompt built from arbitrary message history.
pub fn stream_prompt_with_messages(
    messages: Vec<PromptMessage>,
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
            temperature: None,
            max_tokens: None,
        };
        apply_prompt_overrides(&mut request, &config);
        log_prompt_result(&request, &stub, Duration::from_millis(0))?;
        return Ok(stub);
    }
    let request = build_prompt_request_from_messages(messages, &config)?;
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
        log_prompt_result(&request, &text, Duration::from_millis(0))?;
        return Ok(text);
    }

    let model = resolve_model_name(&config)?;
    let mut request = PromptRequest::user_only(model, prompt.to_string());
    apply_prompt_overrides(&mut request, &config);

    execute_request(request, &config, external_sink)
}

fn build_prompt_request_from_messages(
    messages: Vec<PromptMessage>,
    config: &PromptConfig<'_>,
) -> Result<PromptRequest> {
    let model = resolve_model_name(config)?;
    let mut request = PromptRequest {
        model,
        messages,
        temperature: None,
        max_tokens: None,
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
        if let Some(sink) = external_sink.as_deref_mut() {
            let mut tee = TeeStreamSink::new(&mut accumulator, sink);
            provider.stream(request, &mut tee)?;
        } else {
            provider.stream(request, &mut accumulator)?;
        }
    } else {
        let completion = provider.complete(request)?;
        accumulator.handle_text_delta(&completion.text)?;
        accumulator.handle_done()?;
        if let Some(sink) = external_sink.as_deref_mut() {
            sink.handle_text_delta(&completion.text)?;
            sink.handle_done()?;
        }
    }

    let duration = start.elapsed();
    let text = accumulator.into_string();
    log_prompt_result(&request_for_logging, &text, duration)?;
    Ok(text)
}

fn log_prompt_result(request: &PromptRequest, response: &str, duration: Duration) -> Result<()> {
    let (prompt, system) = extract_prompt_and_system(request);
    let record = logs::LogRecord {
        model: request.model.clone(),
        resolved_model: request.model.clone(),
        prompt,
        system,
        prompt_json: None,
        options_json: None,
        response: response.to_string(),
        response_json: None,
        conversation_id: None,
        duration_ms: Some(duration.as_millis()),
        input_tokens: None,
        output_tokens: None,
        token_details: None,
    };
    logs::record_log_entry(record)?;
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
            MessageRole::Assistant => {}
        }
    }
    (last_user, first_system)
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
        })
        .collect();
    if let Some(default_name) = default {
        if !models.iter().any(|m| m.is_default) {
            let provider = provider_from_model(&default_name).to_string();
            models.push(ModelInfo {
                name: default_name,
                provider: provider.clone(),
                description: format!("Custom model for provider '{provider}'"),
                is_default: true,
                aliases: Vec::new(),
            });
        } else {
            for model in &mut models {
                model.is_default = model.name == default_name;
            }
        }
    } else if let Some(first) = models.first_mut() {
        first.is_default = true;
    }
    Ok(models)
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
pub fn get_default_model() -> Result<Option<String>> {
    let path = default_model_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path).context("failed to read default model")?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalize_model_name(trimmed)))
    }
}

fn default_model_path() -> Result<PathBuf> {
    let mut path = user_dir()?;
    path.push("default-model.txt");
    Ok(path)
}

pub(crate) fn normalize_model_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(canonical) = canonical_for(trimmed) {
        return canonical.to_string();
    }

    let replaced = trimmed.replace(':', "/");
    if let Some(canonical) = canonical_for(&replaced) {
        return canonical.to_string();
    }

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
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn temp_user_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn with_env_lock<F: FnOnce()>(f: F) {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
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
            set_default_model("anthropic:claude-3-opus").expect("set default");
            let stored = get_default_model().expect("stored").unwrap();
            assert_eq!(stored, "anthropic/claude-3-opus-latest");
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
}
