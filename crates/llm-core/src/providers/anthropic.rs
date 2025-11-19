use super::{
    MessageRole, PromptCompletion, PromptMessage, PromptProvider, PromptRequest, StreamSink,
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

    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion> {
        let response = self.request_completion(AnthropicRequest::from_prompt(request)?)?;
        Ok(PromptCompletion {
            text: response.primary_text()?.to_string(),
            raw_response: response.raw_body,
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

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read Anthropic stream")?;
            if bytes == 0 {
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
                    if handle_stream_event(event, data, sink)? {
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

fn handle_stream_event(event: &str, data: &str, sink: &mut dyn StreamSink) -> Result<bool> {
    match event {
        "content_block_delta" => {
            let chunk: StreamContentBlockDelta = serde_json::from_str(data).or_else(|_| {
                serde_json::from_str::<Value>(data)
                    .map_err(|_| anyhow!("failed to parse Anthropic stream chunk: {data}"))
                    .and_then(|v| serde_json::from_value(v).context("invalid chunk structure"))
            })?;
            if let Some(text) = chunk.delta.text {
                if !text.is_empty() {
                    sink.handle_text_delta(&text)?;
                }
            }
            Ok(false)
        }
        "message_delta" | "content_block_start" | "content_block_stop" | "ping" => Ok(false),
        "message_stop" => {
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
}

impl AnthropicContentBlockRequest {
    fn text(text: String) -> Self {
        AnthropicContentBlockRequest::Text { text }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(skip)]
    raw_body: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

impl AnthropicResponse {
    fn primary_text(&self) -> Result<&str> {
        self.content
            .iter()
            .find_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text.as_str()),
                AnthropicContentBlock::Other => None,
            })
            .ok_or_else(|| anyhow!("Anthropic response did not include any text content"))
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
                PromptMessage {
                    role: MessageRole::User,
                    content: String::new(),
                },
                Some(attachment_blocks),
            ));
        }

        Ok(AnthropicRequest {
            model: canonical_model_name(&request.model),
            system,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: None,
        })
    }
}

impl AnthropicMessage {
    fn from_message(
        message: PromptMessage,
        attachments: Option<Vec<AnthropicContentBlockRequest>>,
    ) -> Self {
        let PromptMessage { role, content } = message;
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
    use crate::PromptMessage;

    #[test]
    fn anthropic_request_includes_image_attachment() {
        let request = PromptRequest {
            model: "anthropic/claude-3-haiku".to_string(),
            messages: vec![PromptMessage {
                role: MessageRole::User,
                content: "describe".to_string(),
            }],
            attachments: vec![Attachment::from_content(
                TINY_PNG.to_vec(),
                Some("image/png".to_string()),
            )],
            temperature: None,
            max_tokens: None,
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
            messages: vec![PromptMessage {
                role: MessageRole::User,
                content: "describe audio".to_string(),
            }],
            attachments: vec![Attachment::from_content(
                vec![0u8; 4],
                Some("audio/wav".to_string()),
            )],
            temperature: None,
            max_tokens: None,
        };
        let result = AnthropicRequest::from_prompt(request);
        assert!(result.is_err());
    }

    const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\xa6\x00\x00\x01\x1a\x02\x03\x00\x00\x00\xe6\x99\xc4^\x00\x00\x00\tPLTE\xff\xff\xff\x00\xff\x00\xfe\x01\x00\x12t\x01J\x00\x00\x00GIDATx\xda\xed\xd81\x11\x000\x08\xc0\xc0.]\xea\xaf&Q\x89\x04V\xe0>\xf3+\xc8\x91Z\xf4\xa2\x08EQ\x14EQ\x14EQ\x14EQ\xd4B\x91$I3\xbb\xbf\x08EQ\x14EQ\x14EQ\x14E\xd1\xa5\xd4\x17\x91\xc6\x95\x05\x15\x0f\x9f\xc5\t\x9f\xa4\x00\x00\x00\x00IEND\xaeB`\x82";
}
