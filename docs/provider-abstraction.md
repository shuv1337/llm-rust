# Provider Abstraction Plan

## Requirements
- Support multiple provider types:
  - HTTP REST (OpenAI, Anthropic, Gemini, OpenRouter).
  - SSE/WebSocket streaming (Anthropic, OpenRouter, Grok).
  - Local runners (Ollama, local plugins via subprocess).
- Unified interface for both sync and async models.
- Capability flags: tools, schemas, multimodal attachments, function calling, streaming, async availability.

## Proposed Trait Hierarchy
- `ModelProvider` trait:
  ```rust
  #[async_trait]
  pub trait ModelProvider {
      fn id(&self) -> &str;
      fn display_name(&self) -> &str;
      fn capabilities(&self) -> CapabilitySet;
      async fn complete(&self, request: PromptRequest) -> Result<ModelResponse>;
      async fn stream(
          &self,
          request: PromptRequest,
          sink: &mut dyn StreamSink,
      ) -> Result<()>;
  }
  ```
- `EmbeddingProvider` trait with `embed_batch(&self, inputs: &[EmbeddingInput])`.
- Provider adapters for plugin-supplied implementations (Python or Rust).
- `CapabilitySet` enumerates features (tool use, schema, multimodal, async, json-output).

## Request/Response Models
- `PromptRequest`:
  - `model_id`, `system`, `messages`, `attachments`, `tools`, `options`, `schema`.
- `ModelResponse`:
  - `text`, `usage`, `tool_calls`, `raw` (optional JSON), `metadata` (provider-specific).
- `StreamSink` trait to emit chunks: `on_delta`, `on_tool_call`, `on_error`, `on_complete`.

## Option Handling
- Normalize options via strongly typed map (`HashMap<OptionKey, OptionValue>`).
- Provide schema describing supported options per model (min/max values, validation).
- Ensure compatibility with `llm models options` CLI (persist default options).

## Retrying & Backoff
- Built-in retry policy configurable per provider (status-based, rate-limit).
- Expose hooks allowing plugins to override (e.g., exponential backoff with jitter).
- Log retry attempts via `tracing`.

## Authentication
- Ingest keys from key store, environment variables, or CLI override.
- Support provider-specific tokens (e.g., session tokens, service accounts).
- Provide secure storage abstraction (bridge to OS keychain when available).

## Testing Strategy
- Use `wiremock` to simulate HTTP providers and SSE streams.
- Provide fixture suite replicating known provider responses for regression tests.
- Offer contract tests for plugin providers ensuring they respect trait invariants.

## Migration Notes
- Map existing `Model`, `AsyncModel`, `EmbeddingModel` Python classes to Rust trait objects accessible via Python bindings.
- Ensure conversation state machine (threads, message history) migrates seamlessly.
- Document provider registration process for both Python and Rust plugins.
