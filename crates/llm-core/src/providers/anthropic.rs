use super::{
    FinishReason, FunctionCall, MessageRole, PromptCompletion, PromptMessage, PromptProvider,
    PromptRequest, StreamSink, ToolCall, ToolChoice, ToolChoiceMode, ToolDefinition, UsageInfo,
};
use crate::Attachment;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::{Duration, Instant};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 1024;

/// Blocking provider for Anthropic Messages API.
pub struct AnthropicProvider {
    client: Client,
    base_url: String,
    api_key: String,
    retries: usize,
    retry_backoff: Duration,
    default_max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub base_url: String,
    pub api_key: String,
    pub retries: usize,
    pub retry_backoff: Duration,
    pub default_max_tokens: Option<u32>,
    pub timeout: Duration,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Result<Self> {
        let base_url = config.base_url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build Anthropic HTTP client")?;
        Ok(Self {
            client,
            base_url,
            api_key: config.api_key,
            retries: config.retries,
            retry_backoff: config.retry_backoff,
            default_max_tokens: config.default_max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        })
    }

    fn request_url(&self) -> String {
        format!("{}/messages", self.base_url)
    }

    fn request(
        &self,
        mut request: AnthropicRequest,
        stream: bool,
    ) -> Result<reqwest::blocking::Response> {
        if request.max_tokens.is_none() {
            request.max_tokens = Some(self.default_max_tokens);
        }
        if stream {
            request.stream = Some(true);
        }

        let url = self.request_url();
        let mut attempt = 0usize;

        loop {
            let start = Instant::now();
            let mut builder = self
                .client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json");

            if stream {
                builder = builder.header("accept", "text/event-stream");
            } else {
                builder = builder.header("accept", "application/json");
            }

            let result = builder.json(&request).send();

            match result {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::debug!(
                            target: "llm::providers::anthropic",
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
                            target: "llm::providers::anthropic",
                            url = %url,
                            stream,
                            attempt,
                            status = %status,
                            body = %body,
                            "request_error"
                        );
                        bail!("Anthropic API request failed ({status}): {body}");
                    }

                    tracing::warn!(
                        target: "llm::providers::anthropic",
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
                            .context(format!("failed to send request to Anthropic at {url}"));
                    }
                    tracing::warn!(
                        target: "llm::providers::anthropic",
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

    fn request_completion(&self, request: AnthropicRequest) -> Result<AnthropicResponse> {
        let response = self.request(request, false)?;
        let status = response.status();
        let body = response
            .text()
            .context("failed to read Anthropic response body")?;

        if !status.is_success() {
            bail!("Anthropic API request failed ({status}): {body}");
        }

        let mut parsed: AnthropicResponse =
            serde_json::from_str(&body).context("failed to parse Anthropic response")?;
        parsed.raw_body = Some(body);
        Ok(parsed)
    }
}

impl PromptProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion> {
        let response = self.request_completion(AnthropicRequest::from_prompt(request)?)?;

        // Extract tool calls from response
        let tool_calls = response.extract_tool_calls();
        let has_tool_calls = !tool_calls.is_empty();

        // Extract usage from response
        let usage = response.usage.as_ref().map(|u| UsageInfo {
            prompt_tokens: Some(u.input_tokens),
            completion_tokens: Some(u.output_tokens),
            total_tokens: Some(u.input_tokens + u.output_tokens),
            cached_tokens: u.cache_read_input_tokens,
            reasoning_tokens: None,
        });

        // Determine finish reason
        let finish_reason = response.stop_reason.as_deref().map(|r| match r {
            "end_turn" | "stop" => {
                if has_tool_calls {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                }
            }
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            _ => FinishReason::Other,
        });

        Ok(PromptCompletion {
            text: response.primary_text().unwrap_or("").to_string(),
            raw_response: response.raw_body,
            usage,
            tool_calls: if has_tool_calls {
                Some(tool_calls)
            } else {
                None
            },
            finish_reason,
            function_call: None,
        })
    }

    fn stream(&self, request: PromptRequest, sink: &mut dyn StreamSink) -> Result<()> {
        let response = self.request(AnthropicRequest::from_prompt(request)?, true)?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| format!("status {status} with unreadable body"));
            bail!("Anthropic streaming request failed ({status}): {body}");
        }

        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let mut current_event: Option<String> = None;

        // State for accumulating tool calls during streaming
        let mut stream_state = StreamState::default();

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read Anthropic stream")?;
            if bytes == 0 {
                // Emit any accumulated tool calls before finishing
                stream_state.emit_completed_tool_calls(sink)?;
                sink.handle_done()?;
                break;
            }

            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                current_event = None;
                continue;
            }

            if let Some(payload) = trimmed.strip_prefix("event:") {
                current_event = Some(payload.trim().to_string());
                continue;
            }
            if let Some(payload) = trimmed.strip_prefix("data:") {
                let data = payload.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Some(event) = current_event.as_deref() {
                    if handle_stream_event(event, data, sink, &mut stream_state)? {
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

/// State for accumulating streaming tool calls.
#[derive(Default)]
struct StreamState {
    /// Currently accumulating tool calls by content block index.
    tool_calls: std::collections::HashMap<usize, StreamingToolCall>,
}

/// A tool call being accumulated during streaming.
struct StreamingToolCall {
    id: String,
    name: String,
    input_json: String,
}

impl StreamState {
    /// Start a new tool call from content_block_start event.
    fn start_tool_call(&mut self, index: usize, id: String, name: String) {
        self.tool_calls.insert(
            index,
            StreamingToolCall {
                id,
                name,
                input_json: String::new(),
            },
        );
    }

    /// Append JSON delta to an in-progress tool call.
    fn append_input_json(&mut self, index: usize, partial_json: &str) {
        if let Some(tc) = self.tool_calls.get_mut(&index) {
            tc.input_json.push_str(partial_json);
        }
    }

    /// Complete a tool call at the given index and emit it.
    fn complete_tool_call(&mut self, index: usize, sink: &mut dyn StreamSink) -> Result<()> {
        if let Some(tc) = self.tool_calls.remove(&index) {
            let tool_call = ToolCall {
                id: tc.id,
                tool_type: "function".to_string(),
                function: FunctionCall {
                    name: tc.name,
                    arguments: tc.input_json,
                },
            };
            sink.handle_tool_call(&tool_call)?;
        }
        Ok(())
    }

    /// Emit any remaining tool calls (for cleanup at stream end).
    fn emit_completed_tool_calls(&mut self, sink: &mut dyn StreamSink) -> Result<()> {
        let indices: Vec<usize> = self.tool_calls.keys().copied().collect();
        for index in indices {
            self.complete_tool_call(index, sink)?;
        }
        Ok(())
    }
}

fn handle_stream_event(
    event: &str,
    data: &str,
    sink: &mut dyn StreamSink,
    state: &mut StreamState,
) -> Result<bool> {
    match event {
        "content_block_start" => {
            // Parse the content block start to detect tool_use blocks
            let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
            if let Some(content_block) = parsed.get("content_block") {
                if content_block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let index = parsed.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    let id = content_block
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = content_block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    state.start_tool_call(index, id, name);
                }
            }
            Ok(false)
        }
        "content_block_delta" => {
            let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
            let index = parsed.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

            if let Some(delta) = parsed.get("delta") {
                let delta_type = delta.get("type").and_then(|t| t.as_str());

                match delta_type {
                    Some("text_delta") => {
                        // Standard text delta
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                sink.handle_text_delta(text)?;
                            }
                        }
                    }
                    Some("input_json_delta") => {
                        // Tool use input JSON delta
                        if let Some(partial_json) =
                            delta.get("partial_json").and_then(|p| p.as_str())
                        {
                            state.append_input_json(index, partial_json);
                        }
                    }
                    _ => {
                        // Legacy fallback for text field at top level
                        let chunk: Result<StreamContentBlockDelta, _> = serde_json::from_str(data);
                        if let Ok(chunk) = chunk {
                            if let Some(text) = chunk.delta.text {
                                if !text.is_empty() {
                                    sink.handle_text_delta(&text)?;
                                }
                            }
                        }
                    }
                }
            }
            Ok(false)
        }
        "content_block_stop" => {
            // Complete the tool call at this index if there is one
            let parsed: Value = serde_json::from_str(data).unwrap_or(Value::Null);
            let index = parsed.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            state.complete_tool_call(index, sink)?;
            Ok(false)
        }
        "message_delta" | "ping" | "message_start" => Ok(false),
        "message_stop" => {
            // Emit any remaining tool calls
            state.emit_completed_tool_calls(sink)?;
            sink.handle_done()?;
            Ok(true)
        }
        "error" => {
            let err: StreamError = serde_json::from_str(data).or_else(|_| {
                serde_json::from_str::<Value>(data)
                    .map_err(|_| anyhow!("failed to parse Anthropic stream error: {data}"))
                    .and_then(|v| serde_json::from_value(v).context("invalid error structure"))
            })?;
            bail!("Anthropic stream error: {}", err.error.message);
        }
        _ => Ok(false),
    }
}

// ==================== Request Types ====================

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<AnthropicToolChoice>,
}

/// Anthropic tool definition format.
#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
}

impl AnthropicTool {
    /// Convert from internal ToolDefinition to Anthropic format.
    fn from_tool_definition(tool: &ToolDefinition) -> Self {
        let input_schema = if let Some(params) = &tool.function.parameters {
            serde_json::json!({
                "type": params.schema_type.as_deref().unwrap_or("object"),
                "properties": params.properties.clone().unwrap_or(serde_json::json!({})),
                "required": params.required.clone().unwrap_or_default()
            })
        } else {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            })
        };

        AnthropicTool {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            input_schema,
        }
    }
}

/// Anthropic tool_choice format.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicToolChoice {
    /// Simple mode string.
    Mode(AnthropicToolChoiceMode),
    /// Specific tool selection.
    Specific(AnthropicToolChoiceSpecific),
}

#[derive(Debug, Serialize)]
struct AnthropicToolChoiceMode {
    #[serde(rename = "type")]
    choice_type: String,
}

#[derive(Debug, Serialize)]
struct AnthropicToolChoiceSpecific {
    #[serde(rename = "type")]
    choice_type: String,
    name: String,
}

impl AnthropicToolChoice {
    fn from_tool_choice(choice: &ToolChoice) -> Self {
        match choice {
            ToolChoice::Mode(mode) => {
                let type_str = match mode {
                    ToolChoiceMode::None => "none",
                    ToolChoiceMode::Auto => "auto",
                    ToolChoiceMode::Required => "any", // Anthropic uses "any" for required
                };
                AnthropicToolChoice::Mode(AnthropicToolChoiceMode {
                    choice_type: type_str.to_string(),
                })
            }
            ToolChoice::Specific { function, .. } => {
                AnthropicToolChoice::Specific(AnthropicToolChoiceSpecific {
                    choice_type: "tool".to_string(),
                    name: function.name.clone(),
                })
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlockRequest>),
}

impl MessageContent {
    fn from_text(text: String) -> Self {
        if text.contains('\n') {
            MessageContent::Blocks(vec![AnthropicContentBlockRequest::text(text)])
        } else {
            MessageContent::Text(text)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlockRequest {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

impl AnthropicContentBlockRequest {
    fn text(text: String) -> Self {
        AnthropicContentBlockRequest::Text { text }
    }

    fn tool_use(id: String, name: String, input: Value) -> Self {
        AnthropicContentBlockRequest::ToolUse { id, name, input }
    }

    fn tool_result(tool_use_id: String, content: String, is_error: Option<bool>) -> Self {
        AnthropicContentBlockRequest::ToolResult {
            tool_use_id,
            content,
            is_error,
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: String,
    data: String,
}

// ==================== Response Types ====================

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(skip)]
    raw_body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default, rename = "cache_creation_input_tokens")]
    _cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

impl AnthropicResponse {
    fn primary_text(&self) -> Option<&str> {
        self.content.iter().find_map(|block| match block {
            AnthropicContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
    }

    /// Extract all tool_use blocks as ToolCall instances.
    fn extract_tool_calls(&self) -> Vec<ToolCall> {
        self.content
            .iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id.clone(),
                    tool_type: "function".to_string(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: input.to_string(),
                    },
                }),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct StreamContentBlockDelta {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamError {
    error: StreamErrorBody,
}

#[derive(Debug, Deserialize)]
struct StreamErrorBody {
    message: String,
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

impl AnthropicRequest {
    fn from_prompt(request: PromptRequest) -> Result<Self> {
        let mut system = None;
        let mut messages = Vec::with_capacity(request.messages.len());
        let mut attachment_blocks = request
            .attachments
            .into_iter()
            .map(anthropic_block_from_attachment)
            .collect::<Result<Vec<_>>>()?;
        let last_user_index = request
            .messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, msg)| matches!(msg.role, MessageRole::User))
            .map(|(idx, _)| idx);

        for (idx, message) in request.messages.into_iter().enumerate() {
            match message.role {
                MessageRole::System => {
                    let content = message.content.trim();
                    if !content.is_empty() {
                        system = Some(content.to_string());
                    }
                }
                MessageRole::Tool => {
                    // Convert tool response to Anthropic tool_result format
                    if let Some(tool_call_id) = message.tool_call_id {
                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: MessageContent::Blocks(vec![
                                AnthropicContentBlockRequest::tool_result(
                                    tool_call_id,
                                    message.content,
                                    None,
                                ),
                            ]),
                        });
                    }
                }
                MessageRole::Assistant => {
                    // Check if this assistant message has tool calls
                    if let Some(tool_calls) = message.tool_calls {
                        let mut blocks = Vec::new();
                        if !message.content.is_empty() {
                            blocks.push(AnthropicContentBlockRequest::text(message.content));
                        }
                        for tc in tool_calls {
                            let input: Value =
                                serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                            blocks.push(AnthropicContentBlockRequest::tool_use(
                                tc.id,
                                tc.function.name,
                                input,
                            ));
                        }
                        messages.push(AnthropicMessage {
                            role: "assistant".to_string(),
                            content: MessageContent::Blocks(blocks),
                        });
                    } else {
                        messages.push(AnthropicMessage::from_message(message, None));
                    }
                }
                _ => {
                    let attachments =
                        if Some(idx) == last_user_index && !attachment_blocks.is_empty() {
                            Some(std::mem::take(&mut attachment_blocks))
                        } else {
                            None
                        };
                    messages.push(AnthropicMessage::from_message(message, attachments));
                }
            }
        }

        if !attachment_blocks.is_empty() {
            messages.push(AnthropicMessage::from_message(
                PromptMessage::user(""),
                Some(attachment_blocks),
            ));
        }

        // Convert tools to Anthropic format
        let tools = request.tools.as_ref().map(|tools| {
            tools
                .iter()
                .map(AnthropicTool::from_tool_definition)
                .collect()
        });

        // Convert tool_choice to Anthropic format
        let tool_choice = request
            .tool_choice
            .as_ref()
            .map(AnthropicToolChoice::from_tool_choice);

        Ok(AnthropicRequest {
            model: canonical_model_name(&request.model),
            system,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: None,
            tools,
            tool_choice,
        })
    }
}

impl AnthropicMessage {
    fn from_message(
        message: PromptMessage,
        attachments: Option<Vec<AnthropicContentBlockRequest>>,
    ) -> Self {
        let role = message.role;
        let content = message.content;
        let content_value = if let Some(mut attachments) = attachments {
            let mut blocks = Vec::new();
            if !content.is_empty() {
                blocks.push(AnthropicContentBlockRequest::text(content));
            }
            blocks.append(&mut attachments);
            MessageContent::Blocks(blocks)
        } else {
            MessageContent::from_text(content)
        };

        AnthropicMessage {
            role: role.as_str().to_string(),
            content: content_value,
        }
    }
}

fn anthropic_block_from_attachment(attachment: Attachment) -> Result<AnthropicContentBlockRequest> {
    let mime = attachment.resolve_type()?;
    if !mime.starts_with("image/") {
        bail!("Anthropic provider only supports image attachments (got {mime})");
    }
    Ok(AnthropicContentBlockRequest::Image {
        source: AnthropicImageSource {
            kind: "base64",
            media_type: mime,
            data: attachment.base64_content()?,
        },
    })
}

fn canonical_model_name(input: &str) -> String {
    let normalized = crate::normalize_model_name(input);
    normalized
        .split_once('/')
        .map(|(_, model)| model.to_string())
        .unwrap_or(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{FunctionDefinition, JsonSchema, VecStreamSink};

    #[test]
    fn anthropic_request_includes_image_attachment() {
        let request = PromptRequest {
            model: "anthropic/claude-3-haiku".to_string(),
            messages: vec![PromptMessage::user("describe")],
            attachments: vec![Attachment::from_content(
                TINY_PNG.to_vec(),
                Some("image/png".to_string()),
            )],
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };
        let req = AnthropicRequest::from_prompt(request).expect("request");
        assert_eq!(req.messages.len(), 1);
        match &req.messages[0].content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(
                    blocks[0],
                    AnthropicContentBlockRequest::Text { .. }
                ));
                match &blocks[1] {
                    AnthropicContentBlockRequest::Image { source } => {
                        assert_eq!(source.media_type, "image/png");
                        assert!(source.data.starts_with("iVBORw0KGgo"));
                    }
                    other => panic!("unexpected block {:?}", other),
                }
            }
            other => panic!("unexpected content {:?}", other),
        }
    }

    #[test]
    fn anthropic_request_rejects_non_image_attachment() {
        let request = PromptRequest {
            model: "anthropic/claude-3-haiku".to_string(),
            messages: vec![PromptMessage::user("describe audio")],
            attachments: vec![Attachment::from_content(
                vec![0u8; 4],
                Some("audio/wav".to_string()),
            )],
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };
        let result = AnthropicRequest::from_prompt(request);
        assert!(result.is_err());
    }

    // ==================== Tool Support Tests ====================

    #[test]
    fn anthropic_request_includes_tools() {
        let tools = vec![ToolDefinition::function(
            FunctionDefinition::new("get_weather")
                .with_description("Get the current weather")
                .with_parameters(JsonSchema::object(
                    serde_json::json!({
                        "location": {"type": "string", "description": "City name"}
                    }),
                    vec!["location".to_string()],
                )),
        )];

        let request = PromptRequest {
            model: "anthropic/claude-3-sonnet".to_string(),
            messages: vec![PromptMessage::user("What's the weather in Seattle?")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: Some(tools),
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };

        let req = AnthropicRequest::from_prompt(request).expect("request");
        assert!(req.tools.is_some());
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
        assert_eq!(
            tools[0].description,
            Some("Get the current weather".to_string())
        );

        // Verify input_schema structure
        let schema = &tools[0].input_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["location"].is_object());
    }

    #[test]
    fn anthropic_request_includes_tool_choice_auto() {
        let tools = vec![ToolDefinition::function(FunctionDefinition::new("search"))];

        let request = PromptRequest {
            model: "anthropic/claude-3-sonnet".to_string(),
            messages: vec![PromptMessage::user("Search for something")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: Some(tools),
            functions: None,
            tool_choice: Some(ToolChoice::auto()),
            response_format: None,
            schema: None,
        };

        let req = AnthropicRequest::from_prompt(request).expect("request");
        assert!(req.tool_choice.is_some());

        let json = serde_json::to_string(&req.tool_choice).unwrap();
        assert!(json.contains("\"type\":\"auto\""));
    }

    #[test]
    fn anthropic_request_includes_tool_choice_specific() {
        let tools = vec![ToolDefinition::function(FunctionDefinition::new(
            "get_weather",
        ))];

        let request = PromptRequest {
            model: "anthropic/claude-3-sonnet".to_string(),
            messages: vec![PromptMessage::user("Get weather")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: Some(tools),
            functions: None,
            tool_choice: Some(ToolChoice::specific("get_weather")),
            response_format: None,
            schema: None,
        };

        let req = AnthropicRequest::from_prompt(request).expect("request");
        assert!(req.tool_choice.is_some());

        let json = serde_json::to_string(&req.tool_choice).unwrap();
        assert!(json.contains("\"type\":\"tool\""));
        assert!(json.contains("\"name\":\"get_weather\""));
    }

    #[test]
    fn anthropic_response_extracts_tool_calls() {
        let response_json = r#"{
            "content": [
                {"type": "text", "text": "I'll check the weather for you."},
                {
                    "type": "tool_use",
                    "id": "toolu_01ABC123",
                    "name": "get_weather",
                    "input": {"location": "Seattle", "unit": "fahrenheit"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        }"#;

        let response: AnthropicResponse =
            serde_json::from_str(response_json).expect("parse response");

        assert_eq!(
            response.primary_text(),
            Some("I'll check the weather for you.")
        );

        let tool_calls = response.extract_tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "toolu_01ABC123");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert!(tool_calls[0].function.arguments.contains("Seattle"));

        assert!(response.usage.is_some());
        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn anthropic_response_extracts_multiple_tool_calls() {
        let response_json = r#"{
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_01",
                    "name": "get_weather",
                    "input": {"location": "Seattle"}
                },
                {
                    "type": "tool_use",
                    "id": "toolu_02",
                    "name": "get_time",
                    "input": {"timezone": "PST"}
                }
            ],
            "stop_reason": "tool_use"
        }"#;

        let response: AnthropicResponse =
            serde_json::from_str(response_json).expect("parse response");

        let tool_calls = response.extract_tool_calls();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(tool_calls[1].function.name, "get_time");
    }

    #[test]
    fn anthropic_request_includes_tool_result_message() {
        let messages = vec![
            PromptMessage::user("What's the weather?"),
            PromptMessage::assistant_with_tool_calls(
                "",
                vec![ToolCall::function_call(
                    "toolu_01",
                    "get_weather",
                    r#"{"location":"Seattle"}"#,
                )],
            ),
            PromptMessage::tool_response("toolu_01", r#"{"temp": 72, "condition": "sunny"}"#),
        ];

        let request = PromptRequest {
            model: "anthropic/claude-3-sonnet".to_string(),
            messages,
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        };

        let req = AnthropicRequest::from_prompt(request).expect("request");

        // Should have 3 messages: user, assistant with tool_use, user with tool_result
        assert_eq!(req.messages.len(), 3);

        // Check assistant message has tool_use block
        match &req.messages[1].content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AnthropicContentBlockRequest::ToolUse { id, name, input } => {
                        assert_eq!(id, "toolu_01");
                        assert_eq!(name, "get_weather");
                        assert!(input.is_object());
                    }
                    other => panic!("expected ToolUse, got {:?}", other),
                }
            }
            other => panic!("expected Blocks, got {:?}", other),
        }

        // Check tool result message (sent as user message in Anthropic format)
        assert_eq!(req.messages[2].role, "user");
        match &req.messages[2].content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AnthropicContentBlockRequest::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        assert_eq!(tool_use_id, "toolu_01");
                        assert!(content.contains("temp"));
                    }
                    other => panic!("expected ToolResult, got {:?}", other),
                }
            }
            other => panic!("expected Blocks, got {:?}", other),
        }
    }

    #[test]
    fn streaming_tool_call_accumulation() {
        let mut state = StreamState::default();
        let mut sink = VecStreamSink::new();

        // Simulate content_block_start for tool_use
        state.start_tool_call(0, "toolu_123".to_string(), "get_weather".to_string());

        // Simulate input_json_delta events
        state.append_input_json(0, r#"{"loc"#);
        state.append_input_json(0, r#"ation": "Seattle"}"#);

        // Simulate content_block_stop
        state.complete_tool_call(0, &mut sink).unwrap();

        let tool_calls = sink.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "toolu_123");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            tool_calls[0].function.arguments,
            r#"{"location": "Seattle"}"#
        );
    }

    #[test]
    fn streaming_handles_text_and_tool_use() {
        let mut state = StreamState::default();
        let mut sink = VecStreamSink::new();

        // Text delta
        let text_event = r#"{"index": 0, "delta": {"type": "text_delta", "text": "Hello "}}"#;
        handle_stream_event("content_block_delta", text_event, &mut sink, &mut state).unwrap();

        let text_event2 = r#"{"index": 0, "delta": {"type": "text_delta", "text": "world!"}}"#;
        handle_stream_event("content_block_delta", text_event2, &mut sink, &mut state).unwrap();

        // Tool use start
        let tool_start =
            r#"{"index": 1, "content_block": {"type": "tool_use", "id": "t1", "name": "search"}}"#;
        handle_stream_event("content_block_start", tool_start, &mut sink, &mut state).unwrap();

        // Tool use delta
        let tool_delta = r#"{"index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"q\":\"test\"}"}}"#;
        handle_stream_event("content_block_delta", tool_delta, &mut sink, &mut state).unwrap();

        // Tool use stop
        let tool_stop = r#"{"index": 1}"#;
        handle_stream_event("content_block_stop", tool_stop, &mut sink, &mut state).unwrap();

        // Verify text accumulated
        assert_eq!(sink.into_string(), "Hello world!");
    }

    #[test]
    fn usage_extraction_with_cache_tokens() {
        let response_json = r#"{
            "content": [{"type": "text", "text": "Hello"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 10,
                "cache_read_input_tokens": 20
            }
        }"#;

        let response: AnthropicResponse =
            serde_json::from_str(response_json).expect("parse response");

        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_input_tokens, Some(20));
    }

    #[test]
    fn finish_reason_mapping() {
        // Test stop_reason to FinishReason mapping
        let test_cases = vec![
            ("end_turn", false, FinishReason::Stop),
            ("end_turn", true, FinishReason::ToolCalls),
            ("tool_use", false, FinishReason::ToolCalls),
            ("max_tokens", false, FinishReason::Length),
        ];

        for (stop_reason, has_tools, expected) in test_cases {
            let response_json = format!(
                r#"{{
                    "content": [{}],
                    "stop_reason": "{}"
                }}"#,
                if has_tools {
                    r#"{"type": "tool_use", "id": "t1", "name": "test", "input": {}}"#
                } else {
                    r#"{"type": "text", "text": "hello"}"#
                },
                stop_reason
            );

            let response: AnthropicResponse =
                serde_json::from_str(&response_json).expect("parse response");
            let tool_calls = response.extract_tool_calls();
            let has_tool_calls = !tool_calls.is_empty();

            let finish_reason = response.stop_reason.as_deref().map(|r| match r {
                "end_turn" | "stop" => {
                    if has_tool_calls {
                        FinishReason::ToolCalls
                    } else {
                        FinishReason::Stop
                    }
                }
                "max_tokens" => FinishReason::Length,
                "tool_use" => FinishReason::ToolCalls,
                _ => FinishReason::Other,
            });

            assert_eq!(
                finish_reason,
                Some(expected),
                "Failed for stop_reason={}, has_tools={}",
                stop_reason,
                has_tools
            );
        }
    }

    #[test]
    fn anthropic_tool_serialization() {
        let tool = AnthropicTool {
            name: "get_weather".to_string(),
            description: Some("Get the weather".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        };

        let json = serde_json::to_string(&tool).expect("serialize");
        assert!(json.contains("\"name\":\"get_weather\""));
        assert!(json.contains("\"input_schema\""));
        assert!(json.contains("\"properties\""));
    }

    const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\xa6\x00\x00\x01\x1a\x02\x03\x00\x00\x00\xe6\x99\xc4^\x00\x00\x00\tPLTE\xff\xff\xff\x00\xff\x00\xfe\x01\x00\x12t\x01J\x00\x00\x00GIDATx\xda\xed\xd81\x11\x000\x08\xc0\xc0.]\xea\xaf&Q\x89\x04V\xe0>\xf3+\xc8\x91Z\xf4\xa2\x08EQ\x14EQ\x14EQ\x14EQ\xd4B\x91$I3\xbb\xbf\x08EQ\x14EQ\x14EQ\x14E\xd1\xa5\xd4\x17\x91\xc6\x95\x05\x15\x0f\x9f\xc5\t\x9f\xa4\x00\x00\x00\x00IEND\xaeB`\x82";
}
