use super::{
    FinishReason, FunctionCall, MessageRole, PromptCompletion, PromptProvider, PromptRequest,
    ResponseFormat, StreamSink, ToolCall, ToolChoice, ToolDefinition, UsageInfo,
};
use crate::{normalize_model_name, Attachment};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::{Duration, Instant};

/// Simple blocking provider for OpenAI chat completions.
pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: String,
    retries: usize,
    retry_backoff: Duration,
}

#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub base_url: String,
    pub api_key: String,
    pub retries: usize,
    pub retry_backoff: Duration,
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
            api_key: config.api_key,
            base_url,
            retries: config.retries,
            retry_backoff: config.retry_backoff,
        })
    }

    fn request(
        &self,
        mut request: OpenAIRequest,
        stream: bool,
    ) -> Result<reqwest::blocking::Response> {
        let url = format!("{}/chat/completions", self.base_url);
        if stream {
            request.stream = Some(true);
        }
        let mut attempt = 0usize;
        loop {
            let start = Instant::now();
            let result = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&request)
                .send();

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
                            .context(format!("failed to send request to OpenAI at {url}"));
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

    pub fn request_completion(&self, request: OpenAIRequest) -> Result<OpenAIResponse> {
        let http_response = self.request(request, false)?;

        let status = http_response.status();
        let body = http_response
            .text()
            .context("failed to read OpenAI response body")?;

        if !status.is_success() {
            bail!("OpenAI API request failed ({status}): {body}");
        }

        let mut parsed: OpenAIResponse =
            serde_json::from_str(&body).context("failed to parse OpenAI response")?;
        parsed.raw_body = Some(body);
        Ok(parsed)
    }
}

impl PromptProvider for OpenAIProvider {
    fn id(&self) -> &'static str {
        "openai"
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
        let response = self.request_completion(OpenAIRequest::from_prompt(request)?)?;

        // Extract the primary choice
        let choice = response.choices.first();
        let message = choice.and_then(|c| c.message.as_ref());

        // Extract text content
        let text = message
            .and_then(|m| m.content.as_deref())
            .unwrap_or("")
            .to_string();

        // Extract tool calls
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

        // Extract function call (deprecated)
        let function_call = message.and_then(|m| {
            m.function_call.as_ref().map(|fc| FunctionCall {
                name: fc.name.clone(),
                arguments: fc.arguments.clone(),
            })
        });

        // Extract finish reason
        let finish_reason =
            choice
                .and_then(|c| c.finish_reason.as_ref())
                .map(|r| match r.as_str() {
                    "stop" => FinishReason::Stop,
                    "length" => FinishReason::Length,
                    "tool_calls" => FinishReason::ToolCalls,
                    "function_call" => FinishReason::FunctionCall,
                    "content_filter" => FinishReason::ContentFilter,
                    _ => FinishReason::Other,
                });

        // Extract usage
        let usage = response.usage.map(|u| UsageInfo {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            cached_tokens: u.prompt_tokens_details.and_then(|d| d.cached_tokens),
            reasoning_tokens: u.completion_tokens_details.and_then(|d| d.reasoning_tokens),
        });

        Ok(PromptCompletion {
            text,
            raw_response: response.raw_body,
            usage,
            tool_calls,
            finish_reason,
            function_call,
        })
    }

    fn stream(&self, request: PromptRequest, sink: &mut dyn StreamSink) -> Result<()> {
        let response = self.request(OpenAIRequest::from_prompt(request)?, true)?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| format!("status {status} with unreadable body"));
            bail!("OpenAI streaming request failed ({status}): {body}");
        }

        let mut reader = BufReader::new(response);
        let mut line = String::new();

        // Track tool calls being accumulated across chunks
        // Key: tool call index, Value: (id, name, arguments_buffer)
        let mut tool_call_buffers: HashMap<u32, ToolCallBuffer> = HashMap::new();

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read OpenAI stream")?;
            if bytes == 0 {
                // Emit any accumulated tool calls before done
                emit_accumulated_tool_calls(&mut tool_call_buffers, sink)?;
                sink.handle_done()?;
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
                // Emit any accumulated tool calls before done
                emit_accumulated_tool_calls(&mut tool_call_buffers, sink)?;
                sink.handle_done()?;
                break;
            }
            if data.is_empty() {
                continue;
            }
            let chunk: OpenAIStreamChunk = serde_json::from_str(data).or_else(|_| {
                // fallback to Value for better errors
                serde_json::from_str::<Value>(data)
                    .map_err(|_| anyhow!("failed to parse OpenAI stream chunk: {data}"))
                    .and_then(|v| serde_json::from_value(v).context("invalid chunk structure"))
            })?;

            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    // Handle text content
                    if let Some(content) = delta.content {
                        if !content.is_empty() {
                            sink.handle_text_delta(&content)?;
                        }
                    }

                    // Handle tool calls
                    if let Some(tool_calls) = delta.tool_calls {
                        for tc_delta in tool_calls {
                            let index = tc_delta.index;
                            let buffer =
                                tool_call_buffers
                                    .entry(index)
                                    .or_insert_with(|| ToolCallBuffer {
                                        id: String::new(),
                                        name: String::new(),
                                        arguments: String::new(),
                                    });

                            // Update buffer with delta values
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

                    // Handle deprecated function_call
                    if let Some(fc) = delta.function_call {
                        // For function_call, we use index 0 in the buffer with a special ID
                        let buffer = tool_call_buffers
                            .entry(0)
                            .or_insert_with(|| ToolCallBuffer {
                                id: "function_call".to_string(),
                                name: String::new(),
                                arguments: String::new(),
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
                    if reason == "stop" {
                        emit_accumulated_tool_calls(&mut tool_call_buffers, sink)?;
                        sink.handle_done()?;
                        return Ok(());
                    }
                    if reason == "tool_calls" || reason == "function_call" {
                        emit_accumulated_tool_calls(&mut tool_call_buffers, sink)?;
                        sink.handle_done()?;
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }
}

/// Buffer for accumulating tool call data across streaming chunks.
#[derive(Debug, Default)]
struct ToolCallBuffer {
    id: String,
    name: String,
    arguments: String,
}

/// Emit all accumulated tool calls to the sink.
fn emit_accumulated_tool_calls(
    buffers: &mut HashMap<u32, ToolCallBuffer>,
    sink: &mut dyn StreamSink,
) -> Result<()> {
    // Sort by index to maintain order
    let mut indices: Vec<u32> = buffers.keys().copied().collect();
    indices.sort();

    for index in indices {
        if let Some(buffer) = buffers.remove(&index) {
            if !buffer.id.is_empty() && !buffer.name.is_empty() {
                let tool_call =
                    ToolCall::function_call(&buffer.id, &buffer.name, &buffer.arguments);
                sink.handle_tool_call(&tool_call)?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Serialize)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<OpenAIResponseFormat>,
}

/// OpenAI tool definition for serialization.
#[derive(Debug, Serialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

/// OpenAI function definition for serialization.
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

/// OpenAI response_format for serialization.
#[derive(Debug, Serialize)]
pub struct OpenAIResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<OpenAIJsonSchema>,
}

/// OpenAI JSON schema wrapper for structured output.
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
pub struct OpenAIResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<OpenAIUsage>,
    #[serde(skip)]
    pub raw_body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PromptTokensDetails {
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
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

/// Tool call in the response.
#[derive(Debug, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunctionCall,
}

/// Function call in the response.
#[derive(Debug, Deserialize, Clone)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

impl OpenAIResponse {
    pub fn primary_text(&self) -> Result<&str> {
        self.choices
            .iter()
            .find_map(|choice| choice.message.as_ref()?.content.as_deref())
            .ok_or_else(|| anyhow!("OpenAI response did not include any message content"))
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

/// Tool call delta in streaming response.
#[derive(Debug, Deserialize)]
struct OpenAIToolCallDelta {
    index: u32,
    id: Option<String>,
    function: Option<OpenAIFunctionCallDelta>,
}

/// Function call delta in streaming response.
#[derive(Debug, Deserialize)]
struct OpenAIFunctionCallDelta {
    name: Option<String>,
    arguments: Option<String>,
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

impl OpenAIRequest {
    fn from_prompt(request: PromptRequest) -> Result<Self> {
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
            let role = message.role;
            let content = message.content;
            let role_str = role.as_str().to_string();
            if Some(idx) == last_user_index && !attachment_parts.is_empty() {
                let mut parts = Vec::new();
                if !content.is_empty() {
                    parts.push(ChatMessagePart::Text { text: content });
                }
                parts.append(&mut attachment_parts);
                messages.push(ChatMessage {
                    role: role_str,
                    content: ChatMessageContent::Parts(parts),
                });
            } else {
                messages.push(ChatMessage {
                    role: role_str,
                    content: ChatMessageContent::Text(content),
                });
            }
        }

        if last_user_index.is_none() && !attachment_parts.is_empty() {
            messages.push(ChatMessage {
                role: MessageRole::User.as_str().to_string(),
                content: ChatMessageContent::Parts(attachment_parts),
            });
        }

        // Convert tools
        let tools = request
            .tools
            .map(|tools| tools.into_iter().map(openai_tool_from_definition).collect());

        // Convert tool_choice
        let tool_choice = request.tool_choice.map(tool_choice_to_value);

        // Convert response_format
        let response_format = request.response_format.map(openai_response_format);

        Ok(OpenAIRequest {
            model: canonical_model_name(&request.model),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: None,
            tools,
            tool_choice,
            response_format,
        })
    }
}

/// Convert a ToolDefinition to OpenAI's tool format.
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

/// Convert ToolChoice to a JSON value for the API.
fn tool_choice_to_value(choice: ToolChoice) -> Value {
    serde_json::to_value(choice).unwrap_or(Value::String("auto".to_string()))
}

/// Convert ResponseFormat to OpenAI's format.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{FunctionDefinition, JsonSchema, VecStreamSink};
    use crate::PromptMessage;

    #[test]
    fn request_includes_image_attachment_parts() {
        let request = PromptRequest {
            model: "openai/gpt-4o-mini".to_string(),
            messages: vec![PromptMessage::user("Describe")],
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
        let req = OpenAIRequest::from_prompt(request).expect("request");
        assert_eq!(req.messages.len(), 1);
        match &req.messages[0].content {
            ChatMessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                matches!(
                    parts[0],
                    ChatMessagePart::Text {
                        ref text
                    } if text == "Describe"
                );
                match &parts[1] {
                    ChatMessagePart::Image { image_url } => {
                        assert!(image_url.url.starts_with("data:image/png;base64,"));
                    }
                    other => panic!("unexpected part {:?}", other),
                }
            }
            other => panic!("unexpected content {:?}", other),
        }
    }

    #[test]
    fn request_includes_tools_when_specified() {
        let tools = vec![ToolDefinition::function(
            FunctionDefinition::new("get_weather")
                .with_description("Get current weather")
                .with_parameters(JsonSchema::object(
                    serde_json::json!({
                        "location": {"type": "string"}
                    }),
                    vec!["location".to_string()],
                )),
        )];

        let request = PromptRequest {
            model: "gpt-4".to_string(),
            messages: vec![PromptMessage::user("What's the weather?")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: Some(tools),
            functions: None,
            tool_choice: Some(ToolChoice::auto()),
            response_format: None,
            schema: None,
        };

        let req = OpenAIRequest::from_prompt(request).expect("request");
        assert!(req.tools.is_some());
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "function");
        assert_eq!(tools[0].function.name, "get_weather");
        assert!(tools[0].function.description.is_some());
        assert!(req.tool_choice.is_some());
    }

    #[test]
    fn request_includes_response_format_json_object() {
        let request = PromptRequest {
            model: "gpt-4".to_string(),
            messages: vec![PromptMessage::user("Return JSON")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: Some(ResponseFormat::JsonObject),
            schema: None,
        };

        let req = OpenAIRequest::from_prompt(request).expect("request");
        assert!(req.response_format.is_some());
        let format = req.response_format.unwrap();
        assert_eq!(format.format_type, "json_object");
        assert!(format.json_schema.is_none());
    }

    #[test]
    fn request_includes_response_format_json_schema() {
        let schema = JsonSchema::object(
            serde_json::json!({
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }),
            vec!["name".to_string()],
        );

        let request = PromptRequest {
            model: "gpt-4".to_string(),
            messages: vec![PromptMessage::user("Extract person info")],
            attachments: vec![],
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: Some(ResponseFormat::JsonSchema {
                name: "person".to_string(),
                schema,
                strict: Some(true),
            }),
            schema: None,
        };

        let req = OpenAIRequest::from_prompt(request).expect("request");
        assert!(req.response_format.is_some());
        let format = req.response_format.unwrap();
        assert_eq!(format.format_type, "json_schema");
        assert!(format.json_schema.is_some());
        let json_schema = format.json_schema.unwrap();
        assert_eq!(json_schema.name, "person");
        assert_eq!(json_schema.strict, Some(true));
    }

    #[test]
    fn parse_response_with_tool_calls() {
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"Seattle\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 20,
                "total_tokens": 70
            }
        }"#;

        let response: OpenAIResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(response.choices.len(), 1);

        let message = response.choices[0].message.as_ref().unwrap();
        assert!(message.tool_calls.is_some());
        let tool_calls = message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc123");
        assert_eq!(tool_calls[0].function.name, "get_weather");

        assert!(response.usage.is_some());
        let usage = response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, Some(50));
        assert_eq!(usage.completion_tokens, Some(20));
    }

    #[test]
    fn parse_response_with_usage_details() {
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "prompt_tokens_details": {
                    "cached_tokens": 25
                },
                "completion_tokens_details": {
                    "reasoning_tokens": 10
                }
            }
        }"#;

        let response: OpenAIResponse = serde_json::from_str(json).expect("parse");
        let usage = response.usage.expect("usage");
        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, Some(50));

        let prompt_details = usage.prompt_tokens_details.expect("prompt_details");
        assert_eq!(prompt_details.cached_tokens, Some(25));

        let completion_details = usage.completion_tokens_details.expect("completion_details");
        assert_eq!(completion_details.reasoning_tokens, Some(10));
    }

    #[test]
    fn parse_streaming_tool_call_chunks() {
        // Simulate chunks that would arrive during streaming
        let chunks = vec![
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"lo"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"cation\": \"Seattle\"}"}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ];

        let mut buffers: HashMap<u32, ToolCallBuffer> = HashMap::new();

        for chunk_str in chunks {
            let chunk: OpenAIStreamChunk = serde_json::from_str(chunk_str).expect("parse chunk");
            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(tool_calls) = delta.tool_calls {
                        for tc_delta in tool_calls {
                            let index = tc_delta.index;
                            let buffer = buffers.entry(index).or_insert_with(|| ToolCallBuffer {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
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
                }
            }
        }

        // Verify accumulated tool call
        assert_eq!(buffers.len(), 1);
        let buffer = buffers.get(&0).expect("tool call at index 0");
        assert_eq!(buffer.id, "call_abc");
        assert_eq!(buffer.name, "get_weather");
        assert_eq!(buffer.arguments, r#"{"location": "Seattle"}"#);
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
        emit_accumulated_tool_calls(&mut buffers, &mut sink).expect("emit");

        let tool_calls = sink.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].function.name, "search");
        assert_eq!(tool_calls[0].function.arguments, r#"{"query": "test"}"#);
    }

    #[test]
    fn tool_choice_serialization_modes() {
        let auto = tool_choice_to_value(ToolChoice::auto());
        assert_eq!(auto, serde_json::json!("auto"));

        let none = tool_choice_to_value(ToolChoice::none());
        assert_eq!(none, serde_json::json!("none"));

        let required = tool_choice_to_value(ToolChoice::required());
        assert_eq!(required, serde_json::json!("required"));

        let specific = tool_choice_to_value(ToolChoice::specific("get_weather"));
        assert!(specific.is_object());
        assert_eq!(specific["type"], "function");
        assert_eq!(specific["function"]["name"], "get_weather");
    }

    #[test]
    fn parse_deprecated_function_call() {
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "function_call": {
                        "name": "get_weather",
                        "arguments": "{\"location\": \"NYC\"}"
                    }
                },
                "finish_reason": "function_call"
            }],
            "usage": {
                "prompt_tokens": 30,
                "completion_tokens": 15,
                "total_tokens": 45
            }
        }"#;

        let response: OpenAIResponse = serde_json::from_str(json).expect("parse");
        let message = response.choices[0].message.as_ref().unwrap();
        assert!(message.function_call.is_some());
        let fc = message.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "get_weather");
        assert_eq!(fc.arguments, r#"{"location": "NYC"}"#);
    }

    const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\xa6\x00\x00\x01\x1a\x02\x03\x00\x00\x00\xe6\x99\xc4^\x00\x00\x00\tPLTE\xff\xff\xff\x00\xff\x00\xfe\x01\x00\x12t\x01J\x00\x00\x00GIDATx\xda\xed\xd81\x11\x000\x08\xc0\xc0.]\xea\xaf&Q\x89\x04V\xe0>\xf3+\xc8\x91Z\xf4\xa2\x08EQ\x14EQ\x14EQ\x14EQ\xd4B\x91$I3\xbb\xbf\x08EQ\x14EQ\x14EQ\x14E\xd1\xa5\xd4\x17\x91\xc6\x95\x05\x15\x0f\x9f\xc5\t\x9f\xa4\x00\x00\x00\x00IEND\xaeB`\x82";
}
