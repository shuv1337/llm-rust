use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::blocking::{get as blocking_get, Client};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Represents binary or remote data that can be sent alongside a prompt.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub content_type: Option<String>,
    pub path: Option<PathBuf>,
    pub url: Option<String>,
    pub content: Option<Vec<u8>>,
}

impl Attachment {
    pub fn from_path(path: PathBuf, content_type: Option<String>) -> Self {
        Attachment {
            content_type,
            path: Some(path),
            url: None,
            content: None,
        }
    }

    pub fn from_url(url: String, content_type: Option<String>) -> Self {
        Attachment {
            content_type,
            path: None,
            url: Some(url),
            content: None,
        }
    }

    pub fn from_content(content: Vec<u8>, content_type: Option<String>) -> Self {
        Attachment {
            content_type,
            path: None,
            url: None,
            content: Some(content),
        }
    }

    /// Returns the fully qualified type, deriving it from the underlying data when needed.
    pub fn resolve_type(&self) -> Result<String> {
        if let Some(content_type) = &self.content_type {
            return Ok(content_type.clone());
        }
        if let Some(path) = &self.path {
            if let Some(mime) = detect_mime_from_path(path) {
                return Ok(mime);
            }
        }
        if let Some(url) = &self.url {
            if let Some(mime) = detect_remote_mime(url)? {
                return Ok(mime);
            }
        }
        if let Some(bytes) = &self.content {
            if let Some(mime) = detect_mime_from_content(bytes) {
                return Ok(mime);
            }
        }
        Err(anyhow!("Unable to determine content type for attachment"))
    }

    /// Returns the attachment bytes, fetching or reading them when necessary.
    pub fn content_bytes(&self) -> Result<Vec<u8>> {
        if let Some(content) = &self.content {
            return Ok(content.clone());
        }
        if let Some(path) = &self.path {
            return fs::read(path)
                .with_context(|| format!("failed to read attachment at {}", path.display()));
        }
        if let Some(url) = &self.url {
            let response = blocking_get(url)
                .with_context(|| format!("failed to download attachment from {url}"))?;
            return response
                .bytes()
                .map(|bytes| bytes.to_vec())
                .context("failed to read attachment body");
        }
        Err(anyhow!("Attachment is missing content, path, or url"))
    }

    /// Base64 encode the attachment bytes.
    pub fn base64_content(&self) -> Result<String> {
        let bytes = self.content_bytes()?;
        Ok(BASE64_STANDARD.encode(bytes))
    }

    /// Compute a stable identifier for the attachment.
    pub fn id(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        if let Some(content) = &self.content {
            hasher.update(content);
        } else if let Some(path) = &self.path {
            let bytes = fs::read(path)
                .with_context(|| format!("failed to read attachment at {}", path.display()))?;
            hasher.update(bytes);
        } else if let Some(url) = &self.url {
            let payload = json!({ "url": url });
            hasher.update(payload.to_string().as_bytes());
        } else {
            return Err(anyhow!("Cannot compute attachment id without any data"));
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

/// Attempt to detect the mimetype using the attachment bytes.
pub fn detect_mime_from_content(bytes: &[u8]) -> Option<String> {
    infer::get(bytes).map(|kind| kind.mime_type().to_string())
}

/// Attempt to detect the mimetype from a file path.
pub fn detect_mime_from_path(path: &Path) -> Option<String> {
    infer::get_from_path(path)
        .ok()
        .flatten()
        .map(|kind| kind.mime_type().to_string())
}

/// Perform a HEAD request to retrieve the mimetype for a remote resource.
pub fn detect_remote_mime(url: &str) -> Result<Option<String>> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client for attachment detection")?;
    let response = client
        .head(url)
        .send()
        .with_context(|| format!("failed to fetch headers from {url}"))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "remote attachment {url} returned HTTP status {}",
            response.status()
        ));
    }
    Ok(response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string()))
}
