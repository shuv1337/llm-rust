//! Provider implementations for prompt execution.

use anyhow::Result;

pub mod anthropic;
pub mod openai;

/// Represents a chat-style message sent to a provider.
#[derive(Debug, Clone)]
pub struct PromptMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        }
    }
}

/// Common prompt request shared across providers.
#[derive(Debug, Clone)]
pub struct PromptRequest {
    pub model: String,
    pub messages: Vec<PromptMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl PromptRequest {
    pub fn user_only(model: String, user_content: String) -> Self {
        Self {
            model,
            messages: vec![PromptMessage {
                role: MessageRole::User,
                content: user_content,
            }],
            temperature: None,
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptCompletion {
    pub text: String,
    #[allow(dead_code)]
    pub raw_response: Option<String>,
}

/// Trait implemented by all prompt providers.
#[allow(dead_code)]
pub trait PromptProvider {
    fn id(&self) -> &'static str;
    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion>;

    fn supports_streaming(&self) -> bool {
        false
    }

    fn stream(&self, request: PromptRequest, sink: &mut dyn StreamSink) -> Result<()> {
        let completion = self.complete(request)?;
        sink.handle_text_delta(&completion.text)?;
        sink.handle_done()
    }
}

/// Sink used for streaming responses (planned extension).
#[allow(dead_code)]
pub trait StreamSink {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()>;
    fn handle_done(&mut self) -> Result<()>;
}

/// Basic sink implementation that buffers text chunks for later consumption.
#[derive(Default)]
#[allow(dead_code)]
pub struct VecStreamSink {
    chunks: Vec<String>,
    finished: bool,
}

#[allow(dead_code)]
impl VecStreamSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_string(self) -> String {
        self.chunks.concat()
    }
}

impl StreamSink for VecStreamSink {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()> {
        self.chunks.push(delta.to_string());
        Ok(())
    }

    fn handle_done(&mut self) -> Result<()> {
        self.finished = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticProvider {
        text: String,
    }

    impl PromptProvider for StaticProvider {
        fn id(&self) -> &'static str {
            "static"
        }

        fn complete(&self, _request: PromptRequest) -> Result<PromptCompletion> {
            Ok(PromptCompletion {
                text: self.text.clone(),
                raw_response: None,
            })
        }
    }

    #[test]
    fn default_stream_delegates_to_complete() {
        let provider = StaticProvider {
            text: "streamed".to_string(),
        };
        let request = PromptRequest::user_only("model".to_string(), "hello".to_string());
        let mut sink = VecStreamSink::new();
        provider.stream(request, &mut sink).expect("stream");
        assert_eq!(sink.into_string(), "streamed");
    }
}
