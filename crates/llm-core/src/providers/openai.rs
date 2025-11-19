use super::{MessageRole, PromptCompletion, PromptProvider, PromptRequest, StreamSink};
use crate::{normalize_model_name, Attachment, PromptMessage};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion> {
        let response = self.request_completion(OpenAIRequest::from_prompt(request)?)?;

        Ok(PromptCompletion {
            text: response.primary_text()?.to_string(),
            raw_response: response.raw_body,
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

        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .context("failed to read OpenAI stream")?;
            if bytes == 0 {
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
                    if let Some(content) = delta.content {
                        if !content.is_empty() {
                            sink.handle_text_delta(&content)?;
                        }
                    }
                }
                if let Some(reason) = choice.finish_reason {
                    if reason == "stop" {
                        sink.handle_done()?;
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }
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
    #[serde(skip)]
    pub raw_body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: Option<ChoiceMessage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChoiceMessage {
    pub role: Option<String>,
    pub content: Option<String>,
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
            let PromptMessage { role, content } = message;
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

        Ok(OpenAIRequest {
            model: canonical_model_name(&request.model),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: None,
        })
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
    use crate::PromptMessage;

    #[test]
    fn request_includes_image_attachment_parts() {
        let request = PromptRequest {
            model: "openai/gpt-4o-mini".to_string(),
            messages: vec![PromptMessage {
                role: MessageRole::User,
                content: "Describe".to_string(),
            }],
            attachments: vec![Attachment::from_content(
                TINY_PNG.to_vec(),
                Some("image/png".to_string()),
            )],
            temperature: None,
            max_tokens: None,
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

    const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\xa6\x00\x00\x01\x1a\x02\x03\x00\x00\x00\xe6\x99\xc4^\x00\x00\x00\tPLTE\xff\xff\xff\x00\xff\x00\xfe\x01\x00\x12t\x01J\x00\x00\x00GIDATx\xda\xed\xd81\x11\x000\x08\xc0\xc0.]\xea\xaf&Q\x89\x04V\xe0>\xf3+\xc8\x91Z\xf4\xa2\x08EQ\x14EQ\x14EQ\x14EQ\xd4B\x91$I3\xbb\xbf\x08EQ\x14EQ\x14EQ\x14E\xd1\xa5\xd4\x17\x91\xc6\x95\x05\x15\x0f\x9f\xc5\t\x9f\xa4\x00\x00\x00\x00IEND\xaeB`\x82";
}
