use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

pub const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OPENAI_ISSUER: &str = "https://auth.openai.com";
pub const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const OPENAI_DEVICE_URL: &str = "https://auth.openai.com/codex/device";
pub const OPENAI_DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
pub const OPENAI_OAUTH_PORT: u16 = 1455;
pub const OPENAI_SCOPE: &str = "openid profile email offline_access";
pub const OPENAI_BROWSER_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const REFRESH_SAFETY_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthPreference {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "api-key", alias = "api_key")]
    ApiKey,
    #[serde(rename = "chatgpt", alias = "chatgpt-oauth")]
    ChatGpt,
}

impl AuthPreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ApiKey => "api-key",
            Self::ChatGpt => "chatgpt",
        }
    }
}

impl std::str::FromStr for AuthPreference {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self> {
        match raw {
            "auto" => Ok(Self::Auto),
            "api-key" | "api_key" => Ok(Self::ApiKey),
            "chatgpt" | "chatgpt-oauth" => Ok(Self::ChatGpt),
            _ => bail!("auth preference must be one of: auto, api-key, chatgpt"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StoredAuthInfo {
    #[serde(rename = "oauth")]
    OAuth(OpenAIOAuthCredentials),
    #[serde(rename = "api")]
    Api { key: String },
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

#[derive(Debug, Clone)]
pub struct PkceFlow {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
    pub authorize_url: String,
}

#[derive(Debug, Deserialize)]
pub struct DeviceAuthStart {
    pub device_auth_id: String,
    pub user_code: String,
    #[serde(default)]
    pub interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceAuthToken {
    pub authorization_code: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

pub fn auth_path() -> Result<PathBuf> {
    Ok(crate::user_dir()?.join("auth.json"))
}

pub fn auth_preferences_path() -> Result<PathBuf> {
    Ok(crate::user_dir()?.join("auth_preferences.json"))
}

pub fn load_auth_store() -> Result<Map<String, Value>> {
    let path = auth_path()?;
    load_json_map(&path)
}

pub fn get_oauth_credentials(provider: &str) -> Result<Option<OpenAIOAuthCredentials>> {
    let store = load_auth_store()?;
    let Some(value) = store.get(provider) else {
        return Ok(None);
    };
    match serde_json::from_value::<StoredAuthInfo>(value.clone()) {
        Ok(StoredAuthInfo::OAuth(creds)) => Ok(Some(creds)),
        Ok(StoredAuthInfo::Api { .. }) => Ok(None),
        Err(err) => Err(err).context("failed to parse stored auth record"),
    }
}

pub fn save_oauth_credentials(provider: &str, creds: &OpenAIOAuthCredentials) -> Result<()> {
    let mut store = load_auth_store()?;
    store.insert(
        provider.to_string(),
        serde_json::to_value(StoredAuthInfo::OAuth(creds.clone()))?,
    );
    write_json_map_secure(&auth_path()?, &store)
}

pub fn remove_auth(provider: &str) -> Result<bool> {
    let mut store = load_auth_store()?;
    let removed = store.remove(provider).is_some();
    write_json_map_secure(&auth_path()?, &store)?;
    if get_auth_preference(provider)? == AuthPreference::ChatGpt {
        set_auth_preference(provider, AuthPreference::Auto)?;
    }
    Ok(removed)
}

pub fn get_auth_preference(provider: &str) -> Result<AuthPreference> {
    if let Ok(value) = std::env::var("LLM_OPENAI_AUTH") {
        return value.parse();
    }
    let prefs = load_json_map(&auth_preferences_path()?)?;
    let Some(value) = prefs.get(provider) else {
        return Ok(AuthPreference::Auto);
    };
    serde_json::from_value(value.clone()).context("failed to parse auth preference")
}

pub fn set_auth_preference(provider: &str, mode: AuthPreference) -> Result<()> {
    let mut prefs = load_json_map(&auth_preferences_path()?)?;
    prefs.insert(provider.to_string(), serde_json::to_value(mode)?);
    write_json_map_secure(&auth_preferences_path()?, &prefs)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn credentials_need_refresh(creds: &OpenAIOAuthCredentials) -> bool {
    creds.expires_at_ms <= now_ms().saturating_add(REFRESH_SAFETY_MS)
}

pub fn generate_pkce_flow(redirect_uri: &str) -> Result<PkceFlow> {
    let verifier = random_string(96);
    let state = random_string(48);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    let mut url = Url::parse(OPENAI_AUTHORIZE_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", OPENAI_CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", OPENAI_SCOPE)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "llm-rust");
    Ok(PkceFlow {
        verifier,
        challenge,
        state,
        authorize_url: url.to_string(),
    })
}

pub fn exchange_openai_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OpenAIOAuthCredentials> {
    exchange_openai_code_inner(code, Some(verifier), redirect_uri)
}

fn exchange_openai_code_inner(
    code: &str,
    verifier: Option<&str>,
    redirect_uri: &str,
) -> Result<OpenAIOAuthCredentials> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CLIENT_ID),
        ("code", code),
        ("redirect_uri", redirect_uri),
    ];
    if let Some(verifier) = verifier {
        form.push(("code_verifier", verifier));
    }
    let response = client
        .post(OPENAI_TOKEN_URL)
        .form(&form)
        .send()
        .context("failed to exchange OpenAI OAuth code")?;
    parse_token_response(response, None)
}

pub fn refresh_openai_credentials(
    creds: &OpenAIOAuthCredentials,
) -> Result<OpenAIOAuthCredentials> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let response = client
        .post(OPENAI_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", OPENAI_CLIENT_ID),
            ("refresh_token", creds.refresh.as_str()),
        ])
        .send()
        .context("failed to refresh OpenAI OAuth token")?;
    parse_token_response(response, Some(creds))
}

pub fn start_device_auth() -> Result<DeviceAuthStart> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let response = client
        .post(format!("{OPENAI_ISSUER}/api/accounts/deviceauth/usercode"))
        .header("User-Agent", "llm-rust")
        .json(&serde_json::json!({ "client_id": OPENAI_CLIENT_ID }))
        .send()
        .context("failed to start OpenAI device authorization")?;
    if !response.status().is_success() {
        bail!("OpenAI device authorization failed ({})", response.status());
    }
    response
        .json()
        .context("failed to parse OpenAI device authorization response")
}

pub fn poll_device_auth(device_auth_id: &str, user_code: &str) -> Result<Option<DeviceAuthToken>> {
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let response = client
        .post(format!("{OPENAI_ISSUER}/api/accounts/deviceauth/token"))
        .header("User-Agent", "llm-rust")
        .json(&serde_json::json!({
            "client_id": OPENAI_CLIENT_ID,
            "device_auth_id": device_auth_id,
            "user_code": user_code,
        }))
        .send()
        .context("failed to poll OpenAI device authorization")?;
    if response.status().as_u16() == 403 || response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!(
            "OpenAI device authorization polling failed ({})",
            response.status()
        );
    }
    Ok(Some(
        response
            .json()
            .context("failed to parse device token response")?,
    ))
}

pub fn exchange_openai_device_code(code: &str) -> Result<OpenAIOAuthCredentials> {
    exchange_openai_code_inner(code, None, OPENAI_DEVICE_REDIRECT_URI)
}

pub fn extract_chatgpt_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("chatgpt_account_id")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .get("organizations")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn parse_token_response(
    response: reqwest::blocking::Response,
    previous: Option<&OpenAIOAuthCredentials>,
) -> Result<OpenAIOAuthCredentials> {
    let status = response.status();
    if !status.is_success() {
        bail!("OpenAI OAuth token request failed ({status}); response body redacted");
    }
    let token: TokenResponse = response
        .json()
        .context("failed to parse OpenAI OAuth token response")?;
    let refresh = token
        .refresh_token
        .or_else(|| previous.map(|creds| creds.refresh.clone()))
        .ok_or_else(|| anyhow!("OpenAI OAuth token response did not include a refresh token"))?;
    let account_id = token
        .id_token
        .as_deref()
        .and_then(extract_chatgpt_account_id)
        .or_else(|| extract_chatgpt_account_id(&token.access_token))
        .or_else(|| previous.and_then(|creds| creds.account_id.clone()));
    Ok(OpenAIOAuthCredentials {
        access: token.access_token,
        refresh,
        expires_at_ms: now_ms() + token.expires_in.unwrap_or(3600) * 1000,
        account_id,
    })
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn load_json_map(path: &Path) -> Result<Map<String, Value>> {
    match fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(Map::new()),
        Ok(raw) => serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn write_json_map_secure(path: &Path, map: &Map<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_vec_pretty(map)?;
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    file.write_all(&data)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
