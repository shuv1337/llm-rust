use super::{PromptCompletion, PromptProvider, PromptRequest, StreamSink};
use crate::normalize_model_name;
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
        let response = self.request_completion(OpenAIRequest::from_prompt(request))?;

        Ok(PromptCompletion {
            text: response.primary_text()?.to_string(),
            raw_response: response.raw_body,
        })
    }

    fn stream(&self, request: PromptRequest, sink: &mut dyn StreamSink) -> Result<()> {
        let response = self.request(OpenAIRequest::from_prompt(request), true)?;
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
    pub content: String,
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
    fn from_prompt(request: PromptRequest) -> Self {
        OpenAIRequest {
            model: canonical_model_name(&request.model),
            messages: request
                .messages
                .into_iter()
                .map(|msg| ChatMessage {
                    role: msg.role.as_str().to_string(),
                    content: msg.content,
                })
                .collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: None,
        }
    }
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
