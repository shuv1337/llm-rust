//! Provider implementations for prompt execution.

use crate::Attachment;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod anthropic;
pub mod openai;

// ==================== Tool & Function Definitions ====================

/// JSON Schema wrapper for structured output or tool parameter definitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonSchema {
    /// The schema type (typically "object").
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<String>,

    /// Properties for object schemas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,

    /// Required property names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Additional schema properties (additionalProperties, enum, etc.).
    #[serde(flatten)]
    pub additional: Option<serde_json::Value>,
}

impl JsonSchema {
    /// Create a new empty JSON schema.
    pub fn new() -> Self {
        Self {
            schema_type: None,
            properties: None,
            required: None,
            additional: None,
        }
    }

    /// Create an object schema with properties.
    pub fn object(properties: serde_json::Value, required: Vec<String>) -> Self {
        Self {
            schema_type: Some("object".to_string()),
            properties: Some(properties),
            required: Some(required),
            additional: None,
        }
    }
}

impl Default for JsonSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Definition of a function that can be called by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDefinition {
    /// The name of the function.
    pub name: String,

    /// A description of what the function does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The parameters the function accepts (JSON Schema).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<JsonSchema>,

    /// Whether strict mode is enabled (OpenAI-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

impl FunctionDefinition {
    /// Create a new function definition with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters: None,
            strict: None,
        }
    }

    /// Add a description to the function.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add parameters schema to the function.
    pub fn with_parameters(mut self, parameters: JsonSchema) -> Self {
        self.parameters = Some(parameters);
        self
    }
}

/// Definition of a tool that wraps a function.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    /// The type of tool (currently always "function").
    #[serde(rename = "type")]
    pub tool_type: String,

    /// The function definition.
    pub function: FunctionDefinition,
}

impl ToolDefinition {
    /// Create a new tool definition wrapping a function.
    pub fn function(function: FunctionDefinition) -> Self {
        Self {
            tool_type: "function".to_string(),
            function,
        }
    }
}

// ==================== Response Format ====================

/// Specifies the format for model responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Default text response.
    Text,

    /// JSON object response (model will output valid JSON).
    JsonObject,

    /// JSON response conforming to a specific schema.
    JsonSchema {
        /// Name for the schema.
        name: String,

        /// The JSON schema to conform to.
        schema: JsonSchema,

        /// Whether to enforce strict schema adherence.
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
}

impl Default for ResponseFormat {
    fn default() -> Self {
        ResponseFormat::Text
    }
}

// ==================== Usage & Completion Metadata ====================

/// Token usage information from the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageInfo {
    /// Number of tokens in the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,

    /// Number of tokens in the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,

    /// Total tokens used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,

    /// Cached tokens (prompt tokens read from cache).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,

    /// Reasoning tokens (for models with chain-of-thought).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

impl UsageInfo {
    /// Create usage info from basic counts.
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens: Some(prompt_tokens),
            completion_tokens: Some(completion_tokens),
            total_tokens: Some(prompt_tokens + completion_tokens),
            cached_tokens: None,
            reasoning_tokens: None,
        }
    }
}

/// A function call made by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    /// The name of the function to call.
    pub name: String,

    /// The arguments to pass (JSON string).
    pub arguments: String,
}

/// A tool call made by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,

    /// The type of tool (currently always "function").
    #[serde(rename = "type")]
    pub tool_type: String,

    /// The function call details.
    pub function: FunctionCall,
}

impl ToolCall {
    /// Create a new function tool call.
    pub fn function_call(id: impl Into<String>, name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }
}

/// The result of executing a tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    /// The ID of the tool call this is a result for.
    pub tool_call_id: String,

    /// The result content (typically JSON string).
    pub content: String,

    /// Whether the tool execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResult {
    /// Create a successful tool result.
    pub fn success(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
            is_error: None,
        }
    }

    /// Create an error tool result.
    pub fn error(tool_call_id: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            content: error_message.into(),
            is_error: Some(true),
        }
    }
}

/// Reason why the model stopped generating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of generation.
    Stop,

    /// Hit the max_tokens limit.
    Length,

    /// Model wants to call tools.
    ToolCalls,

    /// Content was filtered.
    ContentFilter,

    /// Function call (deprecated, use ToolCalls).
    FunctionCall,

    /// Other/unknown reason.
    #[serde(other)]
    Other,
}

// ==================== Messages ====================

/// Represents a chat-style message sent to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: MessageRole,
    pub content: String,

    /// Tool calls in this message (for assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// Tool call ID this message is responding to (for tool messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Function call in this message (deprecated, use tool_calls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,

    /// Name of the function this message is responding to (deprecated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl PromptMessage {
    /// Create a simple text message with the given role.
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            function_call: None,
            name: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }

    /// Create a tool response message.
    pub fn tool_response(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            function_call: None,
            name: None,
        }
    }

    /// Create an assistant message with tool calls.
    pub fn assistant_with_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            function_call: None,
            name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
    Function,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
            MessageRole::Function => "function",
        }
    }
}

// ==================== Request & Response ====================

/// Common prompt request shared across providers.
#[derive(Debug, Clone)]
pub struct PromptRequest {
    pub model: String,
    pub messages: Vec<PromptMessage>,
    pub attachments: Vec<Attachment>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,

    // Tool/function calling support
    /// Tools available for the model to call.
    pub tools: Option<Vec<ToolDefinition>>,

    /// Functions available for the model to call (deprecated, use tools).
    pub functions: Option<Vec<FunctionDefinition>>,

    /// How the model should select which tool/function to call.
    pub tool_choice: Option<ToolChoice>,

    // Structured output support
    /// Desired response format.
    pub response_format: Option<ResponseFormat>,

    /// JSON schema for structured output (alternative to response_format).
    pub schema: Option<JsonSchema>,
}

/// How the model should select tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolChoice {
    /// String mode: "none", "auto", "required".
    Mode(ToolChoiceMode),

    /// Specific tool to call.
    Specific {
        #[serde(rename = "type")]
        tool_type: String,
        function: ToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceMode {
    None,
    Auto,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolChoiceFunction {
    pub name: String,
}

impl ToolChoice {
    pub fn none() -> Self {
        ToolChoice::Mode(ToolChoiceMode::None)
    }

    pub fn auto() -> Self {
        ToolChoice::Mode(ToolChoiceMode::Auto)
    }

    pub fn required() -> Self {
        ToolChoice::Mode(ToolChoiceMode::Required)
    }

    pub fn specific(name: impl Into<String>) -> Self {
        ToolChoice::Specific {
            tool_type: "function".to_string(),
            function: ToolChoiceFunction { name: name.into() },
        }
    }
}

impl PromptRequest {
    pub fn user_only(model: String, user_content: String) -> Self {
        Self {
            model,
            messages: vec![PromptMessage::user(user_content)],
            attachments: Vec::new(),
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        }
    }

    /// Create a new prompt request with explicit messages.
    pub fn new(model: impl Into<String>, messages: Vec<PromptMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            attachments: Vec::new(),
            temperature: None,
            max_tokens: None,
            tools: None,
            functions: None,
            tool_choice: None,
            response_format: None,
            schema: None,
        }
    }

    /// Add tools to the request.
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the response format.
    pub fn with_response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set a JSON schema for structured output.
    pub fn with_schema(mut self, schema: JsonSchema) -> Self {
        self.schema = Some(schema);
        self
    }
}

#[derive(Debug, Clone)]
pub struct PromptCompletion {
    pub text: String,

    #[allow(dead_code)]
    pub raw_response: Option<String>,

    /// Token usage information.
    pub usage: Option<UsageInfo>,

    /// Tool calls made by the model.
    pub tool_calls: Option<Vec<ToolCall>>,

    /// Why the model stopped generating.
    pub finish_reason: Option<FinishReason>,

    /// Function call made by the model (deprecated, use tool_calls).
    pub function_call: Option<FunctionCall>,
}

impl PromptCompletion {
    /// Create a simple text completion.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            raw_response: None,
            usage: None,
            tool_calls: None,
            finish_reason: None,
            function_call: None,
        }
    }

    /// Create a completion with tool calls.
    pub fn with_tool_calls(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            text: text.into(),
            raw_response: None,
            usage: None,
            tool_calls: Some(tool_calls),
            finish_reason: Some(FinishReason::ToolCalls),
            function_call: None,
        }
    }

    /// Check if the model wants to call tools.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty())
    }
}

/// Trait implemented by all prompt providers.
#[allow(dead_code)]
pub trait PromptProvider {
    fn id(&self) -> &'static str;
    fn complete(&self, request: PromptRequest) -> Result<PromptCompletion>;

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_tools(&self) -> bool {
        false
    }

    fn supports_structured_output(&self) -> bool {
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
    fn handle_tool_call(&mut self, _tool_call: &ToolCall) -> Result<()> {
        Ok(())
    }
    fn handle_done(&mut self) -> Result<()>;
}

/// Basic sink implementation that buffers text chunks for later consumption.
#[derive(Default)]
#[allow(dead_code)]
pub struct VecStreamSink {
    chunks: Vec<String>,
    tool_calls: Vec<ToolCall>,
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

    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }
}

impl StreamSink for VecStreamSink {
    fn handle_text_delta(&mut self, delta: &str) -> Result<()> {
        self.chunks.push(delta.to_string());
        Ok(())
    }

    fn handle_tool_call(&mut self, tool_call: &ToolCall) -> Result<()> {
        self.tool_calls.push(tool_call.clone());
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
                usage: None,
                tool_calls: None,
                finish_reason: None,
                function_call: None,
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

    // ==================== Serialization Tests ====================

    #[test]
    fn json_schema_serialization() {
        let schema = JsonSchema::object(
            serde_json::json!({
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }),
            vec!["name".to_string()],
        );

        let json = serde_json::to_string(&schema).expect("serialize");
        assert!(json.contains("\"type\":\"object\""));
        assert!(json.contains("\"required\":[\"name\"]"));

        let parsed: JsonSchema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.schema_type, Some("object".to_string()));
    }

    #[test]
    fn function_definition_serialization() {
        let func = FunctionDefinition::new("get_weather")
            .with_description("Get current weather for a location")
            .with_parameters(JsonSchema::object(
                serde_json::json!({
                    "location": {"type": "string", "description": "City name"}
                }),
                vec!["location".to_string()],
            ));

        let json = serde_json::to_string(&func).expect("serialize");
        assert!(json.contains("\"name\":\"get_weather\""));
        assert!(json.contains("\"description\":\"Get current weather"));

        let parsed: FunctionDefinition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, "get_weather");
    }

    #[test]
    fn tool_definition_serialization() {
        let tool = ToolDefinition::function(FunctionDefinition::new("search"));

        let json = serde_json::to_string(&tool).expect("serialize");
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"search\""));

        let parsed: ToolDefinition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.tool_type, "function");
        assert_eq!(parsed.function.name, "search");
    }

    #[test]
    fn response_format_serialization() {
        // Text format
        let text = ResponseFormat::Text;
        let json = serde_json::to_string(&text).expect("serialize");
        assert_eq!(json, r#"{"type":"text"}"#);

        // JSON object format
        let json_obj = ResponseFormat::JsonObject;
        let json = serde_json::to_string(&json_obj).expect("serialize");
        assert_eq!(json, r#"{"type":"json_object"}"#);

        // JSON schema format
        let json_schema = ResponseFormat::JsonSchema {
            name: "person".to_string(),
            schema: JsonSchema::new(),
            strict: Some(true),
        };
        let json = serde_json::to_string(&json_schema).expect("serialize");
        assert!(json.contains("\"type\":\"json_schema\""));
        assert!(json.contains("\"name\":\"person\""));
        assert!(json.contains("\"strict\":true"));
    }

    #[test]
    fn usage_info_serialization() {
        let usage = UsageInfo::new(100, 50);

        let json = serde_json::to_string(&usage).expect("serialize");
        assert!(json.contains("\"prompt_tokens\":100"));
        assert!(json.contains("\"completion_tokens\":50"));
        assert!(json.contains("\"total_tokens\":150"));

        let parsed: UsageInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.prompt_tokens, Some(100));
        assert_eq!(parsed.completion_tokens, Some(50));
    }

    #[test]
    fn tool_call_serialization() {
        let tool_call = ToolCall::function_call(
            "call_123",
            "get_weather",
            r#"{"location": "Seattle"}"#,
        );

        let json = serde_json::to_string(&tool_call).expect("serialize");
        assert!(json.contains("\"id\":\"call_123\""));
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"get_weather\""));
        assert!(json.contains("\"arguments\":"));

        let parsed: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, "call_123");
        assert_eq!(parsed.function.name, "get_weather");
    }

    #[test]
    fn tool_result_serialization() {
        let success = ToolResult::success("call_123", r#"{"temp": 72}"#);
        let json = serde_json::to_string(&success).expect("serialize");
        assert!(json.contains("\"tool_call_id\":\"call_123\""));
        assert!(!json.contains("is_error")); // should be skipped when None

        let error = ToolResult::error("call_456", "API rate limit exceeded");
        let json = serde_json::to_string(&error).expect("serialize");
        assert!(json.contains("\"is_error\":true"));
    }

    #[test]
    fn finish_reason_serialization() {
        assert_eq!(
            serde_json::to_string(&FinishReason::Stop).unwrap(),
            r#""stop""#
        );
        assert_eq!(
            serde_json::to_string(&FinishReason::ToolCalls).unwrap(),
            r#""tool_calls""#
        );

        let parsed: FinishReason = serde_json::from_str(r#""stop""#).unwrap();
        assert_eq!(parsed, FinishReason::Stop);

        // Unknown values should deserialize to Other
        let parsed: FinishReason = serde_json::from_str(r#""unknown_reason""#).unwrap();
        assert_eq!(parsed, FinishReason::Other);
    }

    #[test]
    fn message_role_serialization() {
        assert_eq!(
            serde_json::to_string(&MessageRole::User).unwrap(),
            r#""user""#
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::Tool).unwrap(),
            r#""tool""#
        );

        let parsed: MessageRole = serde_json::from_str(r#""assistant""#).unwrap();
        assert_eq!(parsed, MessageRole::Assistant);
    }

    #[test]
    fn tool_choice_serialization() {
        // Mode variants
        let auto = ToolChoice::auto();
        let json = serde_json::to_string(&auto).expect("serialize");
        assert_eq!(json, r#""auto""#);

        // Specific tool
        let specific = ToolChoice::specific("get_weather");
        let json = serde_json::to_string(&specific).expect("serialize");
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"get_weather\""));
    }

    #[test]
    fn prompt_message_with_tool_calls() {
        let msg = PromptMessage::assistant_with_tool_calls(
            "",
            vec![ToolCall::function_call("id1", "search", "{}")],
        );
        assert!(msg.tool_calls.is_some());
        assert_eq!(msg.tool_calls.unwrap().len(), 1);
    }

    #[test]
    fn prompt_completion_with_tool_calls() {
        let completion = PromptCompletion::with_tool_calls(
            "",
            vec![ToolCall::function_call("id1", "search", "{}")],
        );
        assert!(completion.has_tool_calls());
        assert_eq!(completion.finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn prompt_request_builder_methods() {
        let request = PromptRequest::user_only("gpt-4".to_string(), "hello".to_string())
            .with_tools(vec![ToolDefinition::function(FunctionDefinition::new("test"))])
            .with_response_format(ResponseFormat::JsonObject);

        assert!(request.tools.is_some());
        assert_eq!(request.tools.as_ref().unwrap().len(), 1);
        assert!(matches!(request.response_format, Some(ResponseFormat::JsonObject)));
    }
}
