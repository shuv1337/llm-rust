use super::{
    FinishReason, FunctionCall, MessageRole, PromptCompletion, PromptProvider, PromptRequest,
    ResponseFormat, StreamSink, ToolCall, ToolChoice, ToolDefinition, UsageInfo,
};
use crate::auth::{
    credentials_need_refresh, refresh_openai_credentials, save_oauth_credentials,
    OpenAIOAuthCredentials,
};
use crate::{normalize_model_name, Attachment};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIApiKind {
    Responses,
    ChatCompletions,
}

/// Blocking provider for OpenAI and OpenAI-compatible APIs.
pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    auth: OpenAIAuth,
    retries: usize,
    retry_backoff: Duration,
    api_kind: OpenAIApiKind,
    provider_id: &'static str,
}

#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub base_url: String,
    pub auth: OpenAIAuth,
    pub retries: usize,
    pub retry_backoff: Duration,
    pub api_kind: OpenAIApiKind,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone)]
pub enum OpenAIAuth {
    ApiKey(String),
    ChatGptOAuth(OpenAIChatGptAuth),
}

#[derive(Debug)]
pub struct OpenAIChatGptAuth {
    credentials: Mutex<OpenAIOAuthCredentials>,
}

impl Clone for OpenAIChatGptAuth {
    fn clone(&self) -> Self {
        let credentials = self
            .credentials
            .lock()
            .expect("oauth mutex poisoned")
            .clone();
        Self {
            credentials: Mutex::new(credentials),
        }
    }
}

impl OpenAIChatGptAuth {
    pub fn new(credentials: OpenAIOAuthCredentials) -> Self {
        Self {
            credentials: Mutex::new(credentials),
        }
    }

    fn fresh_credentials(&self) -> Result<OpenAIOAuthCredentials> {
        let mut guard = self.credentials.lock().expect("oauth mutex poisoned");
        if !credentials_need_refresh(&guard) {
            return Ok(guard.clone());
        }
        let refreshed = refresh_openai_credentials(&guard)
            .context("OpenAI ChatGPT OAuth refresh failed; run `llm auth login openai` again")?;
        save_oauth_credentials("openai", &refreshed)?;
        *guard = refreshed.clone();
        Ok(refreshed)
    }
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Result<Self> {
        let base_url = config.base_url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("failed to build OpenAI HTTP client")?;
        Ok(Self {
            client,
            auth: config.auth,
            base_url,
            retries: config.retries,
            retry_backoff: config.retry_backoff,
            api_kind: config.api_kind,
            provider_id: config.provider_id,
        })
    }

    fn endpoint(&self) -> String {
        if matches!(self.auth, OpenAIAuth::ChatGptOAuth(_)) {
            return "https://chatgpt.com/backend-api/codex/responses".to_string();
        }
        match self.api_kind {
            OpenAIApiKind::Responses => format!("{}/responses", self.base_url),
            OpenAIApiKind::ChatCompletions => format!("{}/chat/completions", self.base_url),
        }
    }

    fn post_json<T: Serialize + ?Sized>(
        &self,
        url: &str,
        request: &T,
        stream: bool,
    ) -> Result<reqwest::blocking::Response> {
        let mut attempt = 0usize;
        loop {
            let start = Instant::now();
            let mut builder = self.client.post(url).json(request);
            match &self.auth {
                OpenAIAuth::ApiKey(key) => {
                    builder = builder.bearer_auth(key);
                }
                OpenAIAuth::ChatGptOAuth(auth) => {
                    if self.api_kind != OpenAIApiKind::Responses {
                        bail!("ChatGPT OAuth is only supported for OpenAI Responses requests");
                    }
                    let creds = auth.fresh_credentials()?;
                    builder = builder
                        .bearer_auth(&creds.access)
                        .header("User-Agent", "llm-rust")
                        .header("Originator", "llm-rust");
                    if let Some(account_id) = creds.account_id {
                        builder = builder.header("ChatGPT-Account-Id", account_id);
                    }
                }
            }
            let result = builder.send();

            match result {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::debug!(
                            target: "llm::providers::openai",
                            url = %url,
                            stream,
                            attempt,
                            "request_success"
                        );
                        return Ok(response);
                    }

                    let status = response.status();
                    if attempt >= self.retries || !should_retry_status(status) {
                        let body = response
                            .text()
                            .unwrap_or_else(|_| "<unreadable>".to_string());
                        tracing::error!(
                            target: "llm::providers::openai",
                            url = %url,
                            stream,
                            attempt,
                            status = %status,
                            body = %body,
                            "request_error"
                        );
                        bail!("OpenAI API request failed ({status}): {body}");
                    }

                    tracing::warn!(
                        target: "llm::providers::openai",
                        url = %url,
                        stream,
                        attempt,
                        status = %status,
                        "request_retry_status"
                    );
                }
                Err(err) => {
                    if attempt >= self.retries {
                        return Err(err)
                            .with_context(|| format!("failed to send request to OpenAI at {url}"));
                    }
                    tracing::warn!(
                        target: "llm::providers::openai",
                        url = %url,
                        stream,
                        attempt,
                        error = %err,
                        "request_retry_error"
                    );
                }
            }

            attempt += 1;
            let multiplier = (attempt as u32).max(1);
            let backoff = self
                .retry_backoff
                .checked_mul(multiplier)
                .unwrap_or(self.retry_backoff);
            let elapsed = start.elapsed();
            if backoff > elapsed {
                thread::sleep(backoff - elapsed);
            }
        }
    }

    fn auth_metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "openai_auth_mode": match self.auth {
                OpenAIAuth::ApiKey(_) => "api_key",
                OpenAIAuth::ChatGptOAuth(_) => "chatgpt_oauth",
            }
        })
    }

    fn request_chat(
        &self,
        mut request: OpenAIChatRequest,
        stream: bool,
    ) -> Result<reqwest::blocking::Response> {
        if stream {
            request.stream = Some(true);
        }
        self.post_json(&self.endpoint(), &request, stream)
    }

    fn request_responses(
        &self,
        mut request: OpenAIResponsesRequest,
        stream: bool,
    ) -> Result<reqwest::blocking::Response> {
        request.stream = Some(stream);
        if matches!(self.auth, OpenAIAuth::ChatGptOAuth(_)) && request.instructions.is_none() {
            request.instructions = Some("Follow the user's instructions.".to_string());
        }
        self.post_json(&self.endpoint(), &request, stream)
    }

    pub fn request_chat_completion(
        &self,
        request: OpenAIChatRequest,
    ) -> Result<OpenAIChatResponse> {
        let http_response = self.request_chat(request, false)?;
        let body = http_response
            .text()
            .context("failed to read OpenAI response body")?;
        let mut parsed: OpenAIChatResponse =
            serde_json::from_str(&body).context("failed to parse OpenAI response")?;
        parsed.raw_body = Some(body);
        Ok(parsed)
    }

    pub fn request_responses_completion(
        &self,
        request: OpenAIResponsesRequest,
    ) -> Result<OpenAIResponsesResponse> {
        let http_response = self.request_responses(request, false)?;
        let body = http_response
            .text()
            .context("failed to read OpenAI response body")?;
        let mut parsed: OpenAIResponsesResponse =
            serde_json::from_str(&body).context("failed to parse OpenAI Responses response")?;
        parsed.raw_body = Some(body);
        Ok(parsed)
    }
}

impl PromptProvider for OpenAIProvider {
    fn id(&self) -> &'static str {
        self.provider_id
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_structured_output(&self) -> bool {
        true
    }

    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion> {
        match self.api_kind {
            OpenAIApiKind::Responses => {
                let response = self
                    .request_responses_completion(OpenAIResponsesRequest::from_prompt(request)?)?;
                let mut completion = response.into_completion();
                completion.metadata = Some(self.auth_metadata());
                Ok(completion)
            }
            OpenAIApiKind::ChatCompletions => {
                let response =
                    self.request_chat_completion(OpenAIChatRequest::from_prompt(request)?)?;
                let mut completion = response.into_completion();
                completion.metadata = Some(self.auth_metadata());
                Ok(completion)
            }
        }
    }

    fn stream(
        &self,
        request: PromptRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<PromptCompletion> {
        match self.api_kind {
            OpenAIApiKind::Responses => self.stream_responses(request, sink),
            OpenAIApiKind::ChatCompletions => self.stream_chat(request, sink),
        }
    }
}

impl OpenAIProvider {
    fn stream_responses(
        &self,
        request: PromptRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<PromptCompletion> {
        let response =
            self.request_responses(OpenAIResponsesRequest::from_prompt(request)?, true)?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| format!("status {status} with unreadable body"));
            bail!("OpenAI streaming request failed ({status}): {body}");
        }

        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let mut event = String::new();
        let mut text = String::new();
        let mut tool_buffers: HashMap<String, ResponseToolCallBuffer> = HashMap::new();
        let mut final_response: Option<OpenAIResponsesResponse> = None;

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read OpenAI stream")?;
            if bytes == 0 {
                break;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("event:") {
                event = value.trim().to_string();
                continue;
            }
            let Some(payload) = trimmed.strip_prefix("data:") else {
                continue;
            };
            let data = payload.trim();
            if data == "[DONE]" || data.is_empty() {
                continue;
            }

            let value: Value = serde_json::from_str(data)
                .map_err(|_| anyhow!("failed to parse OpenAI stream event: {data}"))?;
            let event_type = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or(event.as_str());

            match event_type {
                "response.output_text.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        text.push_str(delta);
                        sink.handle_text_delta(delta)?;
                    }
                }
                "response.function_call_arguments.delta" => {
                    let key = stream_tool_key(&value);
                    let buffer = tool_buffers.entry(key).or_default();
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        buffer.arguments.push_str(delta);
                    }
                }
                "response.function_call_arguments.done" => {
                    let key = stream_tool_key(&value);
                    let buffer = tool_buffers.entry(key).or_default();
                    if let Some(arguments) = value.get("arguments").and_then(Value::as_str) {
                        buffer.arguments = arguments.to_string();
                    }
                }
                "response.output_item.done" => {
                    if let Some(item) = value.get("item") {
                        accumulate_response_function_call(item, &mut tool_buffers);
                    }
                }
                "response.completed" => {
                    if let Some(response_value) = value.get("response") {
                        let mut parsed: OpenAIResponsesResponse =
                            serde_json::from_value(response_value.clone())
                                .context("failed to parse OpenAI completed response")?;
                        parsed.raw_body = Some(response_value.to_string());
                        final_response = Some(parsed);
                    }
                    break;
                }
                "response.failed" | "response.incomplete" => {
                    bail!("OpenAI stream {event_type}: {data}");
                }
                "error" => {
                    let message = value
                        .get("message")
                        .or_else(|| value.pointer("/error/message"))
                        .and_then(Value::as_str)
                        .unwrap_or(data);
                    bail!("OpenAI stream error: {message}");
                }
                _ => {}
            }
        }

        let mut completion = final_response
            .map(OpenAIResponsesResponse::into_completion)
            .unwrap_or_else(|| PromptCompletion::text(&text));

        let streamed_tool_calls = response_tool_buffers_to_tool_calls(tool_buffers);
        if !streamed_tool_calls.is_empty() && completion.tool_calls.is_none() {
            completion.tool_calls = Some(streamed_tool_calls);
            completion.finish_reason = Some(FinishReason::ToolCalls);
        }
        if completion.text.is_empty() {
            completion.text = text;
        }
        if let Some(tool_calls) = completion.tool_calls.clone() {
            for tool_call in tool_calls {
                sink.handle_tool_call(&tool_call)?;
            }
        }
        sink.handle_done()?;
        completion.metadata = Some(self.auth_metadata());
        Ok(completion)
    }

    fn stream_chat(
        &self,
        request: PromptRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<PromptCompletion> {
        let response = self.request_chat(OpenAIChatRequest::from_prompt(request)?, true)?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| format!("status {status} with unreadable body"));
            bail!("OpenAI streaming request failed ({status}): {body}");
        }

        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let mut text = String::new();
        let mut tool_call_buffers: HashMap<u32, ToolCallBuffer> = HashMap::new();
        let mut finish_reason = None;

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read OpenAI stream")?;
            if bytes == 0 {
                break;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            let Some(payload) = trimmed.strip_prefix("data:") else {
                continue;
            };
            let data = payload.trim();
            if data == "[DONE]" {
                break;
            }
            if data.is_empty() {
                continue;
            }
            let chunk: OpenAIStreamChunk = serde_json::from_str(data).or_else(|_| {
                serde_json::from_str::<Value>(data)
                    .map_err(|_| anyhow!("failed to parse OpenAI stream chunk: {data}"))
                    .and_then(|v| serde_json::from_value(v).context("invalid chunk structure"))
            })?;

            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(content) = delta.content {
                        if !content.is_empty() {
                            text.push_str(&content);
                            sink.handle_text_delta(&content)?;
                        }
                    }

                    if let Some(tool_calls) = delta.tool_calls {
                        for tc_delta in tool_calls {
                            let buffer = tool_call_buffers.entry(tc_delta.index).or_default();
                            if let Some(id) = tc_delta.id {
                                buffer.id = id;
                            }
                            if let Some(function) = tc_delta.function {
                                if let Some(name) = function.name {
                                    buffer.name = name;
                                }
                                if let Some(arguments) = function.arguments {
                                    buffer.arguments.push_str(&arguments);
                                }
                            }
                        }
                    }

                    if let Some(fc) = delta.function_call {
                        let buffer = tool_call_buffers
                            .entry(0)
                            .or_insert_with(|| ToolCallBuffer {
                                id: "function_call".to_string(),
                                ..Default::default()
                            });
                        if let Some(name) = fc.name {
                            buffer.name = name;
                        }
                        if let Some(arguments) = fc.arguments {
                            buffer.arguments.push_str(&arguments);
                        }
                    }
                }
                if let Some(reason) = choice.finish_reason {
                    finish_reason = Some(chat_finish_reason(&reason));
                    if reason == "stop" || reason == "tool_calls" || reason == "function_call" {
                        break;
                    }
                }
            }
        }

        let tool_calls = emit_accumulated_tool_calls(&mut tool_call_buffers, sink)?;
        sink.handle_done()?;
        Ok(PromptCompletion {
            text,
            raw_response: None,
            usage_json: None,
            response_id: None,
            status: None,
            incomplete_reason: None,
            usage: None,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            finish_reason,
            function_call: None,
            metadata: Some(self.auth_metadata()),
        })
    }
}

#[derive(Debug, Default)]
struct ToolCallBuffer {
    id: String,
    name: String,
    arguments: String,
}

fn emit_accumulated_tool_calls(
    buffers: &mut HashMap<u32, ToolCallBuffer>,
    sink: &mut dyn StreamSink,
) -> Result<Vec<ToolCall>> {
    let mut indices: Vec<u32> = buffers.keys().copied().collect();
    indices.sort();
    let mut calls = Vec::new();

    for index in indices {
        if let Some(buffer) = buffers.remove(&index) {
            if !buffer.id.is_empty() && !buffer.name.is_empty() {
                let tool_call =
                    ToolCall::function_call(&buffer.id, &buffer.name, &buffer.arguments);
                sink.handle_tool_call(&tool_call)?;
                calls.push(tool_call);
            }
        }
    }

    Ok(calls)
}

#[derive(Debug, Default)]
struct ResponseToolCallBuffer {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

fn stream_tool_key(value: &Value) -> String {
    value
        .get("item_id")
        .or_else(|| value.get("call_id"))
        .or_else(|| value.get("output_index"))
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| Some(v.to_string()))
        })
        .unwrap_or_else(|| "0".to_string())
}

fn accumulate_response_function_call(
    item: &Value,
    buffers: &mut HashMap<String, ResponseToolCallBuffer>,
) {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return;
    }
    let key = item
        .get("id")
        .or_else(|| item.get("call_id"))
        .and_then(Value::as_str)
        .unwrap_or("0")
        .to_string();
    let buffer = buffers.entry(key).or_default();
    buffer.id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| buffer.id.clone());
    buffer.name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| buffer.name.clone());
    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
        buffer.arguments = arguments.to_string();
    }
}

fn response_tool_buffers_to_tool_calls(
    buffers: HashMap<String, ResponseToolCallBuffer>,
) -> Vec<ToolCall> {
    let mut entries: Vec<_> = buffers.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
        .into_iter()
        .filter_map(|(key, buffer)| {
            let name = buffer.name?;
            let id = buffer.id.unwrap_or(key);
            Some(ToolCall::function_call(id, name, buffer.arguments))
        })
        .collect()
}

// ==================== Responses API wire format ====================

#[derive(Debug, Serialize)]
pub struct OpenAIResponsesRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub input: Vec<ResponseInputItem>,
    pub store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<ResponseTextOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ResponseReasoning {
    pub effort: String,
}

#[derive(Debug, Serialize)]
pub struct ResponseTextOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ResponseTextFormat>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ResponseTextFormat {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json_object")]
    JsonObject,
    #[serde(rename = "json_schema")]
    JsonSchema {
        name: String,
        schema: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
}

#[derive(Debug, Serialize)]
pub struct ResponseInputItem {
    pub role: String,
    pub content: Vec<ResponseInputPart>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ResponseInputPart {
    #[serde(rename = "input_text")]
    Text { text: String },
    #[serde(rename = "input_image")]
    Image { image_url: String },
    #[serde(rename = "input_file")]
    File { filename: String, file_data: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ResponseTool {
    #[serde(rename = "function")]
    Function {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
}

impl OpenAIResponsesRequest {
    pub fn from_prompt(request: PromptRequest) -> Result<Self> {
        let model = canonical_model_name(&request.model);
        let mut attachment_parts = request
            .attachments
            .into_iter()
            .map(response_part_from_attachment)
            .collect::<Result<Vec<_>>>()?;
        let last_user_index = request
            .messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, msg)| matches!(msg.role, MessageRole::User))
            .map(|(idx, _)| idx);

        let mut input = Vec::with_capacity(request.messages.len().max(1));
        let mut instructions = Vec::new();
        for (idx, message) in request.messages.into_iter().enumerate() {
            if matches!(message.role, MessageRole::System) {
                if !message.content.is_empty() {
                    instructions.push(message.content);
                }
                continue;
            }
            let mut content = Vec::new();
            if !message.content.is_empty() {
                content.push(ResponseInputPart::Text {
                    text: message.content,
                });
            }
            if Some(idx) == last_user_index {
                content.append(&mut attachment_parts);
            }
            if content.is_empty() {
                content.push(ResponseInputPart::Text {
                    text: String::new(),
                });
            }
            input.push(ResponseInputItem {
                role: response_role(message.role).to_string(),
                content,
            });
        }

        if last_user_index.is_none() && !attachment_parts.is_empty() {
            input.push(ResponseInputItem {
                role: "user".to_string(),
                content: attachment_parts,
            });
        }

        let tools = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(response_tool_from_definition)
                .collect()
        });
        let tool_choice = request.tool_choice.map(response_tool_choice_to_value);
        let text = response_text_options(request.response_format, request.verbosity);

        Ok(Self {
            model,
            instructions: (!instructions.is_empty()).then(|| instructions.join("\n\n")),
            input,
            store: false,
            stream: None,
            temperature: request.temperature,
            max_output_tokens: request.max_output_tokens.or(request.max_tokens),
            reasoning: request
                .reasoning_effort
                .map(|effort| ResponseReasoning { effort }),
            text,
            tools,
            tool_choice,
        })
    }
}

fn response_role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool | MessageRole::Function => "user",
    }
}

fn response_tool_from_definition(tool: ToolDefinition) -> ResponseTool {
    let parameters = tool
        .function
        .parameters
        .map(|schema| serde_json::to_value(&schema).unwrap_or(Value::Null));
    ResponseTool::Function {
        name: tool.function.name,
        description: tool.function.description,
        parameters,
        strict: tool.function.strict,
    }
}

fn response_tool_choice_to_value(choice: ToolChoice) -> Value {
    match choice {
        ToolChoice::Specific { function, .. } => {
            json!({"type": "function", "name": function.name})
        }
        other => serde_json::to_value(other).unwrap_or(Value::String("auto".to_string())),
    }
}

fn response_text_options(
    format: Option<ResponseFormat>,
    verbosity: Option<String>,
) -> Option<ResponseTextOptions> {
    let format = format.map(|format| match format {
        ResponseFormat::Text => ResponseTextFormat::Text,
        ResponseFormat::JsonObject => ResponseTextFormat::JsonObject,
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => ResponseTextFormat::JsonSchema {
            name,
            schema: serde_json::to_value(&schema).unwrap_or(Value::Null),
            strict,
        },
    });
    if verbosity.is_none() && format.is_none() {
        None
    } else {
        Some(ResponseTextOptions { verbosity, format })
    }
}

fn response_part_from_attachment(attachment: Attachment) -> Result<ResponseInputPart> {
    let mime = attachment.resolve_type()?;
    if mime.starts_with("audio/") || mime.starts_with("video/") {
        bail!("OpenAI Responses provider does not support {mime} attachments yet");
    }

    let mut url = attachment.url.clone();
    if url.is_none() {
        let base64 = attachment.base64_content()?;
        url = Some(format!("data:{mime};base64,{base64}"));
    }

    if mime.starts_with("image/") {
        return Ok(ResponseInputPart::Image {
            image_url: url.expect("image attachments should resolve to URL"),
        });
    }

    let base64 = attachment.base64_content()?;
    Ok(ResponseInputPart::File {
        filename: attachment.id()?,
        file_data: format!("data:{mime};base64,{base64}"),
    })
}

#[derive(Debug, Deserialize)]
pub struct OpenAIResponsesResponse {
    pub id: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub output: Vec<ResponseOutputItem>,
    #[serde(default)]
    pub usage: Option<ResponseUsage>,
    #[serde(default)]
    pub incomplete_details: Option<ResponseIncompleteDetails>,
    #[serde(skip)]
    pub raw_body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseIncompleteDetails {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseOutputItem {
    #[serde(rename = "message")]
    Message {
        #[serde(default)]
        content: Vec<ResponseOutputContent>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        id: Option<String>,
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseOutputContent {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResponseUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    #[serde(default)]
    pub input_tokens_details: Option<ResponseInputTokenDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<ResponseOutputTokenDetails>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResponseInputTokenDetails {
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResponseOutputTokenDetails {
    pub reasoning_tokens: Option<u32>,
}

impl OpenAIResponsesResponse {
    fn into_completion(self) -> PromptCompletion {
        let mut text = String::new();
        let mut tool_calls = Vec::new();

        for item in &self.output {
            match item {
                ResponseOutputItem::Message { content } => {
                    for part in content {
                        match part {
                            ResponseOutputContent::OutputText { text: part_text }
                            | ResponseOutputContent::Text { text: part_text } => {
                                text.push_str(part_text);
                            }
                            ResponseOutputContent::Other => {}
                        }
                    }
                }
                ResponseOutputItem::FunctionCall {
                    id,
                    call_id,
                    name,
                    arguments,
                } => {
                    tool_calls.push(ToolCall::function_call(
                        call_id.clone().or_else(|| id.clone()).unwrap_or_default(),
                        name,
                        arguments,
                    ));
                }
                ResponseOutputItem::Other => {}
            }
        }

        let finish_reason = if !tool_calls.is_empty() {
            Some(FinishReason::ToolCalls)
        } else if self.status.as_deref() == Some("incomplete") {
            match self
                .incomplete_details
                .as_ref()
                .and_then(|details| details.reason.as_deref())
            {
                Some("max_output_tokens") => Some(FinishReason::Length),
                _ => Some(FinishReason::Other),
            }
        } else {
            Some(FinishReason::Stop)
        };

        let usage = self.usage.clone().map(|u| UsageInfo {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.total_tokens,
            cached_tokens: u.input_tokens_details.and_then(|d| d.cached_tokens),
            reasoning_tokens: u.output_tokens_details.and_then(|d| d.reasoning_tokens),
        });

        PromptCompletion {
            text,
            raw_response: self.raw_body,
            usage_json: self.usage.and_then(|u| serde_json::to_string(&u).ok()),
            response_id: self.id,
            status: self.status,
            incomplete_reason: self.incomplete_details.and_then(|d| d.reason),
            usage,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            finish_reason,
            function_call: None,
            metadata: None,
        }
    }
}

// ==================== Chat Completions wire format ====================

#[derive(Debug, Serialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<OpenAIResponseFormat>,
}

#[derive(Debug, Serialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Serialize)]
pub struct OpenAIFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<OpenAIJsonSchema>,
}

#[derive(Debug, Serialize)]
pub struct OpenAIJsonSchema {
    pub name: String,
    pub schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatMessagePart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ChatMessagePart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    Image { image_url: ImageUrl },
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: InputAudio },
    #[serde(rename = "file")]
    File { file: FileDescriptor },
}

#[derive(Debug, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct InputAudio {
    pub data: String,
    pub format: String,
}

#[derive(Debug, Serialize)]
pub struct FileDescriptor {
    pub filename: String,
    pub file_data: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIChatResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<OpenAIUsage>,
    #[serde(skip)]
    pub raw_body: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAIUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PromptTokensDetails {
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct CompletionTokensDetails {
    pub reasoning_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: Option<ChoiceMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChoiceMessage {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    pub function_call: Option<OpenAIFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

impl OpenAIChatResponse {
    fn into_completion(self) -> PromptCompletion {
        let choice = self.choices.first();
        let message = choice.and_then(|c| c.message.as_ref());
        let text = message
            .and_then(|m| m.content.as_deref())
            .unwrap_or("")
            .to_string();
        let tool_calls = message.and_then(|m| {
            m.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|tc| {
                        ToolCall::function_call(&tc.id, &tc.function.name, &tc.function.arguments)
                    })
                    .collect()
            })
        });
        let function_call = message.and_then(|m| {
            m.function_call.as_ref().map(|fc| FunctionCall {
                name: fc.name.clone(),
                arguments: fc.arguments.clone(),
            })
        });
        let finish_reason = choice
            .and_then(|c| c.finish_reason.as_ref())
            .map(|r| chat_finish_reason(r));
        let usage = self.usage.clone().map(|u| UsageInfo {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            cached_tokens: u.prompt_tokens_details.and_then(|d| d.cached_tokens),
            reasoning_tokens: u.completion_tokens_details.and_then(|d| d.reasoning_tokens),
        });

        PromptCompletion {
            text,
            raw_response: self.raw_body,
            usage_json: self.usage.and_then(|u| serde_json::to_string(&u).ok()),
            response_id: None,
            status: None,
            incomplete_reason: None,
            usage,
            tool_calls,
            finish_reason,
            function_call,
            metadata: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: Option<OpenAIStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCallDelta>>,
    function_call: Option<OpenAIFunctionCallDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCallDelta {
    index: u32,
    id: Option<String>,
    function: Option<OpenAIFunctionCallDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunctionCallDelta {
    name: Option<String>,
    arguments: Option<String>,
}

impl OpenAIChatRequest {
    pub fn from_prompt(request: PromptRequest) -> Result<Self> {
        let mut attachment_parts = request
            .attachments
            .into_iter()
            .map(openai_part_from_attachment)
            .collect::<Result<Vec<_>>>()?;
        let last_user_index = request
            .messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, msg)| matches!(msg.role, MessageRole::User))
            .map(|(idx, _)| idx);

        let mut messages = Vec::with_capacity(request.messages.len());
        for (idx, message) in request.messages.into_iter().enumerate() {
            let role_str = message.role.as_str().to_string();
            if Some(idx) == last_user_index && !attachment_parts.is_empty() {
                let mut parts = Vec::new();
                if !message.content.is_empty() {
                    parts.push(ChatMessagePart::Text {
                        text: message.content,
                    });
                }
                parts.append(&mut attachment_parts);
                messages.push(ChatMessage {
                    role: role_str,
                    content: ChatMessageContent::Parts(parts),
                });
            } else {
                messages.push(ChatMessage {
                    role: role_str,
                    content: ChatMessageContent::Text(message.content),
                });
            }
        }

        if last_user_index.is_none() && !attachment_parts.is_empty() {
            messages.push(ChatMessage {
                role: MessageRole::User.as_str().to_string(),
                content: ChatMessageContent::Parts(attachment_parts),
            });
        }

        let tools = request
            .tools
            .map(|tools| tools.into_iter().map(openai_tool_from_definition).collect());
        let tool_choice = request.tool_choice.map(tool_choice_to_value);
        let response_format = request.response_format.map(openai_response_format);
        let model = canonical_model_name(&request.model);
        let (max_tokens, max_completion_tokens) = if uses_max_completion_tokens(&model) {
            (None, request.max_output_tokens.or(request.max_tokens))
        } else {
            (request.max_tokens.or(request.max_output_tokens), None)
        };

        Ok(OpenAIChatRequest {
            model,
            messages,
            temperature: request.temperature,
            max_tokens,
            max_completion_tokens,
            stream: None,
            tools,
            tool_choice,
            response_format,
        })
    }
}

fn openai_tool_from_definition(tool: ToolDefinition) -> OpenAITool {
    let parameters = tool
        .function
        .parameters
        .map(|schema| serde_json::to_value(&schema).unwrap_or(Value::Null));

    OpenAITool {
        tool_type: tool.tool_type,
        function: OpenAIFunction {
            name: tool.function.name,
            description: tool.function.description,
            parameters,
            strict: tool.function.strict,
        },
    }
}

fn tool_choice_to_value(choice: ToolChoice) -> Value {
    serde_json::to_value(choice).unwrap_or(Value::String("auto".to_string()))
}

fn openai_response_format(format: ResponseFormat) -> OpenAIResponseFormat {
    match format {
        ResponseFormat::Text => OpenAIResponseFormat {
            format_type: "text".to_string(),
            json_schema: None,
        },
        ResponseFormat::JsonObject => OpenAIResponseFormat {
            format_type: "json_object".to_string(),
            json_schema: None,
        },
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => OpenAIResponseFormat {
            format_type: "json_schema".to_string(),
            json_schema: Some(OpenAIJsonSchema {
                name,
                schema: serde_json::to_value(&schema).unwrap_or(Value::Null),
                strict,
            }),
        },
    }
}

fn openai_part_from_attachment(attachment: Attachment) -> Result<ChatMessagePart> {
    let mime = attachment.resolve_type()?;
    let mut url = attachment.url.clone();
    let mut base64_cache: Option<String> = None;

    if url.is_none() || mime.starts_with("audio/") {
        let base64 = attachment.base64_content()?;
        url = Some(format!("data:{mime};base64,{base64}"));
        base64_cache = Some(base64);
    }

    if mime == "application/pdf" {
        let base64 = match base64_cache {
            Some(data) => data,
            None => attachment.base64_content()?,
        };
        return Ok(ChatMessagePart::File {
            file: FileDescriptor {
                filename: format!("{}.pdf", attachment.id()?),
                file_data: format!("data:{mime};base64,{base64}"),
            },
        });
    }

    if mime.starts_with("image/") {
        let final_url = url.expect("image attachments should always resolve to a URL");
        return Ok(ChatMessagePart::Image {
            image_url: ImageUrl { url: final_url },
        });
    }

    let base64 = match base64_cache {
        Some(data) => data,
        None => attachment.base64_content()?,
    };
    let format = if mime == "audio/wav" {
        "wav".to_string()
    } else {
        "mp3".to_string()
    };
    Ok(ChatMessagePart::InputAudio {
        input_audio: InputAudio {
            data: base64,
            format,
        },
    })
}

fn canonical_model_name(input: &str) -> String {
    let normalized = normalize_model_name(input);
    normalized
        .split_once('/')
        .map(|(_, model)| model.to_string())
        .or_else(|| {
            normalized
                .split_once(':')
                .map(|(_, model)| model.to_string())
        })
        .unwrap_or(normalized)
}

fn uses_max_completion_tokens(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}

fn chat_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" | "max_tokens" | "max_output_tokens" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "function_call" => FinishReason::FunctionCall,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{FunctionDefinition, JsonSchema, VecStreamSink};
    use crate::PromptMessage;

    fn request(prompt: &str) -> PromptRequest {
        PromptRequest::user_only("openai/gpt-5.5".to_string(), prompt.to_string())
    }

    #[test]
    fn responses_request_serializes_plain_text_and_store_false() {
        let req = OpenAIResponsesRequest::from_prompt(request("hello")).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["model"], "gpt-5.5");
        assert_eq!(serialized["store"], false);
        assert_eq!(serialized["input"][0]["role"], "user");
        assert_eq!(serialized["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(serialized["input"][0]["content"][0]["text"], "hello");
    }

    #[test]
    fn responses_request_includes_system_history_and_image_attachment() {
        let request = PromptRequest {
            model: "openai/gpt-5.5".to_string(),
            messages: vec![
                PromptMessage::system("Be terse"),
                PromptMessage::user("Describe"),
            ],
            attachments: vec![Attachment::from_content(
                TINY_PNG.to_vec(),
                Some("image/png".to_string()),
            )],
            ..PromptRequest::user_only("openai/gpt-5.5".to_string(), String::new())
        };
        let req = OpenAIResponsesRequest::from_prompt(request).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["instructions"], "Be terse");
        assert_eq!(serialized["input"][0]["role"], "user");
        assert_eq!(serialized["input"][0]["content"][1]["type"], "input_image");
        assert!(serialized["input"][0]["content"][1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn responses_request_includes_new_options() {
        let mut request = request("short");
        request.max_tokens = Some(20);
        request.reasoning_effort = Some("low".to_string());
        request.verbosity = Some("low".to_string());
        let req = OpenAIResponsesRequest::from_prompt(request).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["max_output_tokens"], 20);
        assert_eq!(serialized["reasoning"]["effort"], "low");
        assert_eq!(serialized["text"]["verbosity"], "low");
        assert!(serialized.get("max_tokens").is_none());
    }

    #[test]
    fn responses_request_includes_json_schema_format() {
        let schema = JsonSchema::object(
            serde_json::json!({"name": {"type": "string"}}),
            vec!["name".to_string()],
        );
        let mut request = request("Extract person info");
        request.response_format = Some(ResponseFormat::JsonSchema {
            name: "person".to_string(),
            schema,
            strict: Some(true),
        });
        let req = OpenAIResponsesRequest::from_prompt(request).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["text"]["format"]["type"], "json_schema");
        assert_eq!(serialized["text"]["format"]["name"], "person");
        assert_eq!(serialized["text"]["format"]["strict"], true);
    }

    #[test]
    fn responses_request_includes_function_tools_and_choice() {
        let tools = vec![ToolDefinition::function(
            FunctionDefinition::new("get_weather")
                .with_description("Get current weather")
                .with_parameters(JsonSchema::object(
                    serde_json::json!({"location": {"type": "string"}}),
                    vec!["location".to_string()],
                )),
        )];
        let mut request = request("What's the weather?");
        request.tools = Some(tools);
        request.tool_choice = Some(ToolChoice::specific("get_weather"));

        let req = OpenAIResponsesRequest::from_prompt(request).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["tools"][0]["type"], "function");
        assert_eq!(serialized["tools"][0]["name"], "get_weather");
        assert_eq!(serialized["tool_choice"]["type"], "function");
        assert_eq!(serialized["tool_choice"]["name"], "get_weather");
    }

    #[test]
    fn responses_parse_completed_text_and_usage() {
        let json = r#"{
            "id": "resp_123",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "Hello "},
                    {"type": "output_text", "text": "world"}
                ]
            }],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "total_tokens": 150,
                "input_tokens_details": {"cached_tokens": 25},
                "output_tokens_details": {"reasoning_tokens": 10}
            }
        }"#;
        let response: OpenAIResponsesResponse = serde_json::from_str(json).expect("parse");
        let completion = response.into_completion();
        assert_eq!(completion.text, "Hello world");
        assert_eq!(completion.response_id.as_deref(), Some("resp_123"));
        assert_eq!(completion.usage.as_ref().unwrap().prompt_tokens, Some(100));
        assert_eq!(completion.usage.as_ref().unwrap().cached_tokens, Some(25));
        assert_eq!(
            completion.usage.as_ref().unwrap().reasoning_tokens,
            Some(10)
        );
    }

    #[test]
    fn responses_parse_function_call_and_incomplete() {
        let json = r#"{
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_abc",
                "name": "search",
                "arguments": "{\"q\":\"rust\"}"
            }]
        }"#;
        let response: OpenAIResponsesResponse = serde_json::from_str(json).expect("parse");
        let completion = response.into_completion();
        assert_eq!(completion.finish_reason, Some(FinishReason::ToolCalls));
        assert_eq!(completion.tool_calls.as_ref().unwrap()[0].id, "call_abc");
        assert_eq!(
            completion.incomplete_reason.as_deref(),
            Some("max_output_tokens")
        );
    }

    #[test]
    fn response_streaming_tool_call_accumulates() {
        let mut buffers = HashMap::new();
        let delta_1: Value = serde_json::from_str(
            r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\"q\""}"#,
        )
        .unwrap();
        let delta_2: Value = serde_json::from_str(
            r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":":\"rust\"}"}"#,
        )
        .unwrap();
        for value in [delta_1, delta_2] {
            let key = stream_tool_key(&value);
            let buffer: &mut ResponseToolCallBuffer = buffers.entry(key).or_default();
            buffer.name = Some("search".to_string());
            if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                buffer.arguments.push_str(delta);
            }
        }
        let calls = response_tool_buffers_to_tool_calls(buffers);
        assert_eq!(calls[0].function.arguments, r#"{"q":"rust"}"#);
    }

    #[test]
    fn chat_request_keeps_compatible_max_tokens() {
        let mut request = PromptRequest::user_only(
            "openai-compatible/custom".to_string(),
            "Short answer".to_string(),
        );
        request.max_tokens = Some(20);
        let req = OpenAIChatRequest::from_prompt(request).expect("request");
        let serialized = serde_json::to_value(&req).expect("serialize");
        assert_eq!(serialized["model"], "custom");
        assert_eq!(serialized["max_tokens"], 20);
        assert!(serialized.get("max_completion_tokens").is_none());
    }

    #[test]
    fn streaming_emits_tool_calls_to_sink() {
        let mut buffers: HashMap<u32, ToolCallBuffer> = HashMap::new();
        buffers.insert(
            0,
            ToolCallBuffer {
                id: "call_123".to_string(),
                name: "search".to_string(),
                arguments: r#"{"query": "test"}"#.to_string(),
            },
        );

        let mut sink = VecStreamSink::new();
        let tool_calls = emit_accumulated_tool_calls(&mut buffers, &mut sink).expect("emit");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(sink.tool_calls()[0].id, "call_123");
    }

    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60,
        0x00, 0x02, 0x00, 0x00, 0x05, 0x00, 0x01, 0xe2, 0x26, 0x05, 0x9b, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
}
