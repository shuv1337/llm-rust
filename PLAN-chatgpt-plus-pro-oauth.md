# PLAN - ChatGPT Plus/Pro OAuth for OpenAI Provider

> Created 2026-05-05 after reviewing the current Rust OpenAI provider plus
> OAuth implementations in `~/repos/shuvgeist` and `~/repos/opencode`.
> Revised 2026-05-05 after checking the plan against the current `master`
> codebase.

## Goal

Add ChatGPT Plus/Pro OAuth login support for the OpenAI provider while keeping
direct OpenAI API key support as the stable default path.

## Implementation Update - 2026-05-05

Implemented the local/testable OAuth surface:

- Added separate `auth.json` and `auth_preferences.json` storage with atomic
  `0600` writes, unknown-provider preservation, opencode-shaped OAuth records,
  and API keys kept in `keys.json`.
- Added `llm auth path/login/status/use/refresh/logout openai`, including
  browser PKCE login, headless device-code login, printed browser fallback URL,
  refresh, and status JSON without token disclosure.
- Added OpenAI auth strategy resolution with `auto`, `api-key`, and `chatgpt`
  precedence; `--key` still forces API-key mode.
- Added ChatGPT OAuth provider mode with token refresh, pinned Codex Responses
  endpoint routing, `ChatGPT-Account-Id` header support, and rejection of custom
  `OPENAI_BASE_URL`/`LLM_OPENAI_BASE_URL` in OAuth mode.
- Added auth-mode log/debug metadata and OpenAI model `auth_modes` metadata.
- Verified with `cargo fmt`, `cargo test`, `cargo run -- auth path`, and
  `cargo run -- auth status openai --json`.

Live account checks are still manual: `auth login`, `auth refresh`, and real
`LLM_OPENAI_AUTH=chatgpt` prompt calls require an interactive ChatGPT account.

This is not a replacement for OpenAI API keys. The reference implementations
authenticate against `https://auth.openai.com`, then send model requests through
the ChatGPT/Codex backend (`https://chatgpt.com/backend-api/codex/responses`)
with a ChatGPT access token and `ChatGPT-Account-Id`. That means this feature
should be explicit, isolated, and easy to disable if the consumer-account flow
changes upstream.

## Review Summary

Overall status: **ready for an M0/M1 spike, not ready to implement as a single
feature branch without the revisions below**. The plan matches the current repo
shape, but the first draft under-specified storage compatibility, provider
refresh mutability, exact OAuth parameters, and how to log the resolved auth
mode after `auto` precedence is applied.

Important codebase alignment points:

- `crates/llm-core/src/lib.rs`
  - `OpenAIProviderFactory` currently resolves a string API key before building
    `OpenAIConfig`; this is the right place to call `resolve_openai_auth()`.
  - `options_metadata_json()` only receives `PromptConfig`, so it cannot safely
    log the resolved auth mode when preference is `auto` unless the provider
    returns that metadata.
  - `ModelInfo` currently has only capability booleans and no auth-mode
    metadata.
- `crates/llm-core/src/providers/openai.rs`
  - `OpenAIProvider` methods take `&self`; OAuth refresh therefore needs
    interior mutability, such as a small `Mutex<OpenAIChatGptAuthState>`.
  - The built-in OpenAI provider uses the Responses API, while
    `openai-compatible` uses Chat Completions. OAuth should be accepted only for
    `provider_id == "openai"` and `api_kind == Responses`.
- `Cargo.toml`
  - Existing dependencies include `reqwest`, `base64`, and `sha2`; the OAuth
    work still needs explicit choices for randomness, URL/form encoding, local
    callback handling, and browser opening.

## References Reviewed

Current repo:

- `crates/llm-core/src/lib.rs`
  - `PromptConfig` already has request options and `api_key` override.
  - `OpenAIProviderFactory` resolves an API key with `resolve_api_key()`.
  - `resolve_provider_key()` maps `openai` to `keys.json` alias `openai` and
    env vars `OPENAI_API_KEY`, `LLM_OPENAI_API_KEY`.
- `crates/llm-core/src/providers/openai.rs`
  - `OpenAIConfig` currently stores `api_key: String`.
  - `OpenAIProvider::post_json()` always applies `.bearer_auth(&self.api_key)`.
  - `OpenAIApiKind::Responses` maps `{base_url}/responses`.
- `crates/llm-cli/src/main.rs`
  - Existing `keys` commands are API-key focused.
  - No auth/login command group exists yet.

Reference implementations:

- `/Users/shuv/repos/shuvgeist/src/oauth/browser-oauth.ts`
  - Browser-tab OAuth helper, PKCE, state, token POST.
- `/Users/shuv/repos/shuvgeist/src/oauth/openai-codex.ts`
  - OpenAI Codex OAuth constants:
    - client id `app_EMoamEEZ73f0CkXaXp7hrann`
    - authorize URL `https://auth.openai.com/oauth/authorize`
    - token URL `https://auth.openai.com/oauth/token`
    - redirect URI `http://localhost:1455/auth/callback`
    - scope `openid profile email offline_access`
    - account id extracted from JWT claim `https://api.openai.com/auth`.
- `/Users/shuv/repos/opencode/packages/opencode/src/plugin/codex.ts`
  - Browser PKCE flow and headless device flow.
  - Refresh-token flow.
  - Request rewriting from `/v1/responses` or `/chat/completions` to
    `https://chatgpt.com/backend-api/codex/responses`.
  - Adds `Authorization: Bearer <access_token>` and `ChatGPT-Account-Id`.
  - Filters OAuth-capable models and sets token costs to zero.
- `/Users/shuv/repos/opencode/packages/opencode/src/auth/index.ts`
  - Stores provider auth in `auth.json`, permission `0o600`.
  - Auth variants include `{ type: "api", key }` and
    `{ type: "oauth", access, refresh, expires, accountId }`.
- `/Users/shuv/repos/opencode/packages/opencode/src/provider/auth.ts`
  - Provider auth methods expose API-key and OAuth login options separately.

## Product Decisions

- [ ] Keep `llm keys` and `keys.json` for direct API keys.
- [ ] Add a separate auth store for OAuth credentials. Do not put refresh
      tokens in `keys.json`, because that file is semantically API-key shaped.
- [ ] Keep API key auth as the default when an OpenAI API key is configured.
- [ ] Require explicit ChatGPT auth selection for prompt execution, at least in
      the first release. Proposed knobs:
  - `llm auth use openai chatgpt`
  - `LLM_OPENAI_AUTH=chatgpt`
  - optional one-shot CLI flag later, if useful.
- [ ] Default preference should be `auto`, but with API keys taking precedence.
      OAuth is only selected by default after the user has explicitly logged in
      and no API key is configured.
- [ ] `LLM_OPENAI_AUTH` accepts `auto`, `api-key`, and `chatgpt`; environment
      wins over the persisted preference for one invocation.
- [ ] Keep `openai-compatible/*` API-key only. Do not route compatible
      providers through ChatGPT OAuth.
- [ ] Hard-fail ChatGPT OAuth mode when `OPENAI_BASE_URL` or
      `LLM_OPENAI_BASE_URL` is set. Do not silently ignore a custom base URL
      because the user may believe they are testing a custom endpoint.
- [ ] Log auth mode as metadata (`api_key` or `chatgpt_oauth`) but never log
      access tokens, refresh tokens, authorization codes, or raw auth responses.

## Proposed CLI

Add a top-level `auth` command group:

- [ ] `llm auth path`
      Print the OAuth auth store path.
- [ ] `llm auth login openai`
      Run browser PKCE login against ChatGPT Plus/Pro.
- [ ] `llm auth login openai --headless`
      Run the device-code flow from opencode for SSH/headless sessions.
- [ ] `llm auth login openai --use`
      Optional convenience flag that stores OAuth credentials and immediately
      persists preference `chatgpt`. Without `--use`, login stores credentials
      but leaves the active preference unchanged and prints the next command.
- [ ] `llm auth login openai --callback-port <port>`
      Escape hatch if `1455` is occupied. M0 must confirm whether the OpenAI
      authorize endpoint accepts arbitrary localhost callback ports; if it does
      not, this flag should be deferred and the command should fail clearly when
      the default port is unavailable.
- [ ] `llm auth status openai [--json]`
      Show whether OAuth credentials exist, expiry, account id, and active auth
      preference. Do not print tokens.
- [ ] `llm auth use openai api-key|chatgpt|auto`
      Persist the preferred auth mode.
- [ ] `llm auth refresh openai`
      Optional but useful diagnostic command. It should exercise the refresh
      path without sending a prompt and should not be required for normal use.
- [ ] `llm auth logout openai`
      Remove OAuth credentials and reset preference if needed.

`keys` remains the API-key UX:

- [ ] `llm keys set openai --value <sk-...>` remains valid.
- [ ] Missing OpenAI credential errors should mention both supported paths:
      `llm keys set openai --value <key>` or `llm auth login openai`.

## Core Auth Model

Add a new core module, likely `crates/llm-core/src/auth.rs`.

- [ ] Keep the OAuth credential store separate from API keys. Do not write API
      keys into `auth.json`; continue to use `keys.json` for direct API keys.
- [ ] Deserialize opencode-style `{ "type": "api", "key": "..." }` records
      only as read-only compatibility if needed for status/debugging. The Rust
      CLI should not write that variant.
- [ ] Define persisted OAuth records:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StoredAuthInfo {
    OAuth(OpenAIOAuthCredentials),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIOAuthCredentials {
    #[serde(rename = "access")]
    pub access: String,
    #[serde(rename = "refresh")]
    pub refresh: String,
    #[serde(rename = "expires")]
    pub expires_at_ms: u64,
    #[serde(rename = "accountId", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthPreference {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "api-key", alias = "api_key")]
    ApiKey,
    #[serde(rename = "chatgpt", alias = "chatgpt-oauth")]
    ChatGpt,
}
```

- [ ] Store auth records in `$LLM_USER_PATH/auth.json`, compatible with the
      opencode shape where practical:

```json
{
  "openai": {
    "type": "oauth",
    "access": "...",
    "refresh": "...",
    "expires": 1778000000000,
    "accountId": "..."
  }
}
```

- [ ] Store preferences in `$LLM_USER_PATH/auth_preferences.json` rather than
      top-level metadata inside `auth.json`. This preserves the opencode-shaped
      provider map and avoids another tool dropping preference metadata when it
      rewrites auth records.
- [ ] Write auth files atomically: create a temporary file in the same
      directory, set `0o600` before writing on Unix, write + sync, then rename.
      Also chmod existing files back to `0o600` before/after updates.
- [ ] Preserve unknown provider records when saving `auth.json`; updating the
      `openai` OAuth record should not delete future auth records for other
      providers.
- [ ] Do not persist `id_token`, authorization code, code verifier, or the raw
      token response body. Extract what is needed, then discard.
- [ ] Add helpers:
  - `auth_path()`
  - `auth_preferences_path()`
  - `load_auth_store()`
  - `get_oauth_credentials(provider)`
  - `save_oauth_credentials(provider, OpenAIOAuthCredentials)`
  - `remove_auth(provider)`
  - `get_auth_preference(provider)`
  - `set_auth_preference(provider, mode)`
- [ ] Keep token exchange/refresh and JWT parsing in core so both CLI login and
      provider refresh share one implementation. Keep browser-opening and local
      callback UX in `llm-cli`.

## Dependencies

Add the smallest dependency set needed for safe OAuth support.

- [ ] Randomness: add `rand` or `getrandom` for PKCE verifier and state bytes.
- [ ] URL/form encoding: add `url` and/or `serde_urlencoded`; do not build
      OAuth URLs or form bodies with ad hoc string concatenation.
- [ ] Local callback server: either use a tiny dependency such as `tiny_http` or
      a narrowly scoped `TcpListener` helper with tests for callback parsing.
- [ ] Browser opening: add `open` or `webbrowser`, and always print the URL as a
      fallback.
- [ ] Tests: prefer local fake HTTP servers over live endpoints for unit tests;
      live ChatGPT verification stays manual and documented in M0/M5.

## OAuth Flow

Port the reference flow from TypeScript to Rust.

- [ ] Constants:
  - `CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"`
  - `ISSUER = "https://auth.openai.com"`
  - `OAUTH_PORT = 1455`
  - `REDIRECT_URI = "http://localhost:1455/auth/callback"`
  - `SCOPE = "openid profile email offline_access"`
- [ ] Browser login:
  - Generate PKCE verifier/challenge using Rust crypto primitives.
  - Generate random state and verify it on callback.
  - Start a temporary local HTTP server on `localhost:1455`.
  - Open the authorization URL with the system browser.
  - Include the reference authorize params:
    - `id_token_add_organizations=true`
    - `codex_cli_simplified_flow=true`
    - `originator=llm-rust`
  - Serve a small success/error page on callback and time out cleanly if the
    browser flow does not complete.
  - Exchange authorization code at `/oauth/token` using
    `application/x-www-form-urlencoded`.
  - Extract `accountId` from `id_token` first, then `access_token` fallback.
    Claim precedence should match opencode:
    - top-level `chatgpt_account_id`
    - nested `https://api.openai.com/auth.chatgpt_account_id`
    - first `organizations[].id`
  - Save `access`, `refresh`, `expires`, and `accountId`.
- [ ] Headless login:
  - Start with `POST https://auth.openai.com/api/accounts/deviceauth/usercode`.
  - Send JSON `{ "client_id": CLIENT_ID }` plus a deterministic `User-Agent`.
  - Print `https://auth.openai.com/codex/device` plus user code.
  - Poll `/api/accounts/deviceauth/token` with `device_auth_id` and `user_code`.
    Treat `403` and `404` as pending states, and fail on other statuses.
  - Respect the returned polling `interval`, with a small safety margin.
  - Exchange returned authorization code through `/oauth/token` with redirect
    URI `https://auth.openai.com/deviceauth/callback`.
- [ ] Refresh:
  - Refresh before expiry with a small safety window, for example 60 seconds.
  - If the refresh response omits `refresh_token`, keep the existing refresh
    token; otherwise replace it.
  - Persist refreshed credentials atomically after a successful refresh.
  - On refresh failure, return a clear re-login error and leave existing creds
    untouched unless the user runs logout.

## Provider Integration

Replace OpenAI's provider config from a raw API key to an auth strategy.

- [ ] Add:

```rust
pub enum OpenAIAuth {
    ApiKey(String),
    ChatGptOAuth(OpenAIChatGptAuth),
}
```

- [ ] Change `OpenAIConfig`:
  - `api_key: String` -> `auth: OpenAIAuth`
  - keep `base_url`, retries, `api_kind`, `provider_id`.
  - OAuth auth should only be constructed when `provider_id == "openai"` and
    `api_kind == OpenAIApiKind::Responses`.
- [ ] Change `OpenAIProvider::post_json()`:
  - API key mode: unchanged `.bearer_auth(key)`.
  - OAuth mode:
    - ensure access token is fresh before each request using interior
      mutability, since provider methods currently take `&self`.
    - set bearer auth to OAuth access token.
    - set `ChatGPT-Account-Id` when present.
    - set originator/user-agent headers similar to Codex/opencode if required.
    - rewrite the URL to exactly
      `https://chatgpt.com/backend-api/codex/responses` for Responses requests.
    - reject Chat Completions/OAuth combinations instead of forwarding tokens.
- [ ] Keep `OPENAI_BASE_URL` meaningful only for API-key mode. In ChatGPT OAuth
      mode, reject custom `OPENAI_BASE_URL`/`LLM_OPENAI_BASE_URL`; do not
      accidentally send ChatGPT tokens to arbitrary hosts.
- [ ] Add `resolve_openai_auth(config)`:
  - request `--key` / `PromptConfig.api_key` always forces API key.
  - auth preference `api-key` forces API key resolution.
  - auth preference `chatgpt` forces OAuth credential resolution.
  - auth preference `auto` uses API key if configured, otherwise OAuth if
    configured.
  - missing credentials should produce an error mentioning both setup paths.
- [ ] Leave `resolve_api_key()` for `openai-compatible` and other API-key
      providers.
- [ ] Add provider debug/status metadata for resolved auth mode without exposing
      secrets. Avoid deriving log metadata directly from user input because
      `auto` may resolve differently at runtime.

## Model Catalog

Do not overload normal API models with hidden behavior unless auth mode is
explicit.

- [ ] Keep `openai/gpt-5.5` as the default model.
- [ ] Add metadata that indicates models are usable via API key and/or ChatGPT
      OAuth. Current `ModelInfo` has no auth fields, so this requires a small
      serialized shape extension such as:

```rust
pub auth_modes: Vec<String>, // "api_key", "chatgpt_oauth"
```

- [ ] Current built-in OpenAI models are only `openai/gpt-5.5` and
      `openai/gpt-5.5-2026-04-23`. Do not advertise OAuth support for snapshot
      models until M0 confirms the ChatGPT backend accepts them.
- [ ] In ChatGPT OAuth mode, initially allow the same family opencode allows:
  - `gpt-5.5`
  - `gpt-5.4`
  - `gpt-5.4-mini`
  - `gpt-5.2`
  - `gpt-5.3-codex`
  - `gpt-5.3-codex-spark`
  - Codex models if/when they are added to this catalog.
- [ ] If models beyond the current built-ins are allowed, add them explicitly to
      `BUILTIN_MODELS` or document that they are accepted only as custom model
      IDs. Keep `models list` honest.
- [ ] If ChatGPT endpoint rejects a model, surface a targeted message:
      "This model is not available through ChatGPT Plus/Pro OAuth; use an API
      key or select an OAuth-supported model."

## Logging and Security

- [ ] Add auth mode to prompt options/log metadata:
      `openai_auth_mode = "api_key" | "chatgpt_oauth"`.
- [ ] Because the resolved auth mode is known only after provider creation,
      prefer adding provider metadata to `PromptCompletion` or a similarly
      internal result path, then merge it into `options_json` in
      `log_prompt_result()`.
- [ ] Redact these fields everywhere:
  - `access`
  - `refresh`
  - `id_token`
  - `access_token`
  - `refresh_token`
  - `authorization_code`
  - `code_verifier`
  - `Authorization` header
  - `ChatGPT-Account-Id` header
- [ ] Ensure raw request/response logging for prompt calls cannot include auth
      token response bodies.
- [ ] OAuth token exchange/refresh helpers should not log response bodies on
      failure. Return status plus a redacted, bounded error summary instead.
- [ ] Add a regression test that greps serialized log output for a sentinel
      token and fails if present.
- [ ] Add a regression test for tracing/error formatting so failed token
      exchange and failed provider requests do not expose bearer tokens.

## Implementation Milestones

### M0 - OAuth Compatibility Spike

- [ ] Port only enough of the OAuth constants and JWT account-id extraction to
      a test module.
- [ ] Confirm required authorize params:
      `id_token_add_organizations`, `codex_cli_simplified_flow`, and
      `originator`.
- [ ] Confirm whether callback ports other than `1455` are accepted.
- [ ] Confirm with a live login that tokens can still be acquired as of
      2026-05-05.
- [ ] Confirm a live request through
      `https://chatgpt.com/backend-api/codex/responses` succeeds with
      `gpt-5.5`.
- [ ] Confirm whether the snapshot model `gpt-5.5-2026-04-23` is accepted.
- [ ] Record exact required headers and request differences from `/v1/responses`.
- [ ] Record which current Responses fields work through ChatGPT OAuth:
      reasoning, verbosity, structured output, tools, images, files, and
      streaming.

### M1 - Auth Store and CLI Skeleton

- [ ] Add `crates/llm-core/src/auth.rs`.
- [ ] Add workspace dependencies selected in the Dependencies section.
- [ ] Add `auth path/status/logout/use` commands.
- [ ] Add tests for auth JSON parsing, malformed records, permissions, logout,
      and preference resolution.
- [ ] Add tests that saving the OpenAI OAuth record preserves unknown provider
      records and never writes API keys to `auth.json`.

### M2 - Browser and Headless Login

- [ ] Implement PKCE generation and local callback server.
- [ ] Implement system browser opening with a printed fallback URL.
- [ ] Implement device-code flow.
- [ ] Add tests with mocked token/device endpoints.
- [ ] Add tests for state mismatch, missing code, timeout, device pending
      statuses, and redacted token endpoint failures.

### M3 - OpenAI Provider Auth Strategy

- [ ] Add `OpenAIAuth` and update `OpenAIConfig`.
- [ ] Implement token refresh and persisted refresh updates.
- [ ] Rewrite Responses requests to the ChatGPT Codex endpoint only in OAuth
      mode.
- [ ] Add tests for API-key mode, OAuth mode, URL rewrite, header injection,
      refresh on expiry, and refresh failure.
- [ ] Add tests that OAuth mode rejects custom `OPENAI_BASE_URL` and never
      routes tokens to `openai-compatible`.
- [ ] Add tests that `PromptConfig.api_key` forces API-key mode even when
      OAuth credentials and `LLM_OPENAI_AUTH=chatgpt` are present.
- [ ] Add a concurrency test or documented invariant for refresh interior
      mutability, so simultaneous prompt calls do not corrupt `auth.json`.

### M4 - UX, Models, and Docs

- [ ] Update credential error messages.
- [ ] Update `models list` output or model metadata for auth compatibility.
- [ ] Update `prompt_debug_info()` and debug logs to show resolved auth mode
      without secrets.
- [ ] Document:
  - API key setup.
  - ChatGPT Plus/Pro login.
  - auth precedence.
  - unsupported/custom base URL behavior.
  - known risk that ChatGPT/Codex OAuth behavior may change upstream.

### M5 - Verification and Release

- [ ] `cargo fmt`
- [ ] `cargo test`
- [ ] `cargo run -- auth path`
- [ ] `cargo run -- auth login openai`
- [ ] `cargo run -- auth status openai --json`
- [ ] `cargo run -- auth refresh openai`
- [ ] `LLM_OPENAI_AUTH=chatgpt cargo run -- --no-stream "ping"`
- [ ] `LLM_OPENAI_AUTH=chatgpt cargo run -- "ping"`
- [ ] `cargo run -- keys set openai --value <test-key>` still selects API-key
      mode when preference is `auto`.
- [ ] `OPENAI_BASE_URL=http://127.0.0.1:9 LLM_OPENAI_AUTH=chatgpt cargo run -- --no-stream "ping"`
      fails before sending a request.
- [ ] `cargo run -- auth logout openai`

## Open Questions

- [ ] Should the persisted preference default to `auto` or `api-key`? Initial
      recommendation: resolved above as `auto`, with API key taking precedence
      over OAuth.
- [ ] Should OAuth mode be exposed as `openai` auth mode only, or also as a
      separate provider prefix such as `chatgpt/gpt-5.5`? Initial
      recommendation: auth mode only, to avoid duplicate model catalogs.
- [ ] Should we support arbitrary callback ports if `1455` is occupied?
      Recommendation: yes only if M0 proves arbitrary localhost redirect URIs
      are accepted. Otherwise fail clearly when the default port is occupied.
- [ ] Does the ChatGPT endpoint accept all Responses API fields we currently
      send for GPT-5.5, including tools, structured output, and images? This
      must be answered in M0 before enabling OAuth broadly.
- [ ] Should refreshed credentials use a file lock across processes? Initial
      recommendation: not in M1, but design atomic writes so adding a lock later
      is straightforward.

## Risks

- ChatGPT Plus/Pro OAuth and the Codex backend are consumer-account flows, not
  ordinary OpenAI API-key flows. They can change without normal API versioning.
- This flow is not the normal OpenAI API contract. Keep docs explicit that it
  depends on ChatGPT/Codex behavior and may stop working independently of API
  key access.
- Tokens are more sensitive than API keys because refresh tokens grant ongoing
  account access. File permissions and redaction are mandatory.
- Sending ChatGPT OAuth bearer tokens to a custom `OPENAI_BASE_URL` would be a
  credential leak. OAuth mode must pin the request host.
- Plus/Pro plan limits and model availability differ from OpenAI API limits.
  Logs and cost estimates should avoid pretending OAuth calls have API billing.
- Refresh races can lose a new refresh token if two processes refresh at once.
  Atomic writes reduce corruption risk but do not fully serialize refreshes.
