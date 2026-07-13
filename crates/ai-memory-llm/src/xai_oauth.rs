//! xAI (SuperGrok subscription) OAuth — PKCE loopback + token file.
//!
//! Ported from oh-my-pi `packages/ai/src/registry/oauth/xai-oauth.ts` /
//! devin-proxy `src/providers/xai-auth.ts` (MIT). Completions go through
//! [`crate::openai_responses::OpenAiResponsesProvider`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

use crate::auth_file::{load_entry, now_ms, save_entry};
use crate::error::{LlmError, LlmResult};
use crate::openai_responses::{OpenAiResponsesProvider, ResponsesAuth};
use crate::provider::LlmProvider;
use crate::response::{provider_error_body, response_json_limited};
use crate::types::{ChatRequest, ChatResponse};

/// OIDC issuer.
pub const XAI_OAUTH_ISSUER: &str = "https://auth.x.ai";

/// Discovery document.
pub const XAI_OAUTH_DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";

/// Public OAuth client id used by SuperGrok CLI-style apps.
pub const XAI_OAUTH_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";

/// OAuth scopes.
pub const XAI_OAUTH_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";

/// Loopback redirect.
pub const XAI_OAUTH_REDIRECT_PORT: u16 = 56121;
/// Redirect path.
pub const XAI_OAUTH_REDIRECT_PATH: &str = "/callback";

const REFRESH_MARGIN_MS: u64 = 5 * 60 * 1000;

/// Stored SuperGrok OAuth token.
#[derive(Clone)]
pub struct XaiOAuthToken {
    /// Access token.
    pub access: SecretString,
    /// Refresh token.
    pub refresh: SecretString,
    /// Expiry in ms since Unix epoch.
    pub expires_at_ms: u64,
}

impl std::fmt::Debug for XaiOAuthToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiOAuthToken")
            .field("access", &"<redacted>")
            .field("refresh", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

impl XaiOAuthToken {
    /// Build from token endpoint response.
    #[must_use]
    pub fn from_token_response(
        access: impl Into<String>,
        refresh: impl Into<String>,
        expires_in_secs: u64,
    ) -> Self {
        Self {
            access: SecretString::from(access.into()),
            refresh: SecretString::from(refresh.into()),
            expires_at_ms: now_ms().saturating_add(expires_in_secs.saturating_mul(1000)),
        }
    }

    /// True when access token needs refresh.
    #[must_use]
    pub fn needs_refresh(&self) -> bool {
        now_ms().saturating_add(REFRESH_MARGIN_MS) >= self.expires_at_ms
    }

    /// Load from shared auth file (`xai` entry).
    ///
    /// # Errors
    /// Auth file I/O / parse errors.
    pub fn load(path: &Path) -> LlmResult<Option<Self>> {
        let Some(entry) = load_entry::<OAuthEntry>(path, "xai")? else {
            return Ok(None);
        };
        if entry.kind != "oauth" {
            return Ok(None);
        }
        Ok(Some(Self {
            access: SecretString::from(entry.access),
            refresh: SecretString::from(entry.refresh),
            expires_at_ms: entry.expires,
        }))
    }

    /// Persist to shared auth file.
    ///
    /// # Errors
    /// Auth file write errors.
    pub fn save(&self, path: &Path) -> LlmResult<()> {
        save_entry(
            path,
            "xai",
            Some(OAuthEntry {
                kind: "oauth".into(),
                access: self.access.expose_secret().to_string(),
                refresh: self.refresh.expose_secret().to_string(),
                expires: self.expires_at_ms,
            }),
        )
    }

    /// Remove the xAI entry.
    ///
    /// # Errors
    /// Auth file write errors.
    pub fn remove(path: &Path) -> LlmResult<()> {
        save_entry::<OAuthEntry>(path, "xai", None)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OAuthEntry {
    #[serde(rename = "type")]
    kind: String,
    access: String,
    refresh: String,
    expires: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
}

/// Validate that a URL is HTTPS on `x.ai` / `*.x.ai`.
///
/// # Errors
/// Invalid URL or host.
pub fn validate_xai_endpoint(url: &str, field: &str) -> LlmResult<String> {
    let lower = url.trim().to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err(LlmError::Auth(format!("invalid xAI {field}: {url}")));
    }
    let rest = &lower["https://".len()..];
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Strip optional port.
    let host = host.split(':').next().unwrap_or(host);
    if host != "x.ai" && !host.ends_with(".x.ai") {
        return Err(LlmError::Auth(format!("invalid xAI {field}: {url}")));
    }
    Ok(url.trim().to_string())
}

/// Fetch OIDC discovery document.
///
/// # Errors
/// Network / JSON / host validation errors.
pub async fn discover_xai(client: &reqwest::Client) -> LlmResult<XaiDiscovery> {
    let resp = client
        .get(XAI_OAUTH_DISCOVERY_URL)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(LlmError::from)?;
    let status = resp.status();
    if !status.is_success() {
        let body = provider_error_body(resp).await;
        return Err(LlmError::Auth(format!(
            "xAI OIDC discovery failed ({status}): {body}"
        )));
    }
    let disc = response_json_limited::<Discovery>(resp).await?;
    Ok(XaiDiscovery {
        authorization_endpoint: validate_xai_endpoint(
            disc.authorization_endpoint.trim(),
            "authorization_endpoint",
        )?,
        token_endpoint: validate_xai_endpoint(disc.token_endpoint.trim(), "token_endpoint")?,
    })
}

/// Discovered OAuth endpoints.
#[derive(Debug, Clone)]
pub struct XaiDiscovery {
    /// Authorization endpoint.
    pub authorization_endpoint: String,
    /// Token endpoint.
    pub token_endpoint: String,
}

/// Build the browser authorize URL.
#[must_use]
pub fn build_authorize_url(
    discovery: &XaiDiscovery,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    nonce: &str,
) -> String {
    let qs = [
        ("response_type", "code"),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", XAI_OAUTH_SCOPE),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("nonce", nonce),
        ("plan", "generic"),
        ("referrer", "ai-memory"),
    ]
    .into_iter()
    .map(|(k, v)| format!("{}={}", k, form_urlencoded_encode(v)))
    .collect::<Vec<_>>()
    .join("&");
    let base = discovery.authorization_endpoint.trim_end_matches('?');
    if base.contains('?') {
        format!("{base}&{qs}")
    } else {
        format!("{base}?{qs}")
    }
}

fn form_urlencoded_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Exchange an authorization code for tokens.
///
/// # Errors
/// Network / auth failures.
pub async fn exchange_code(
    client: &reqwest::Client,
    discovery: &XaiDiscovery,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> LlmResult<XaiOAuthToken> {
    let resp = client
        .post(&discovery.token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", XAI_OAUTH_CLIENT_ID),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(LlmError::from)?;
    let status = resp.status();
    if !status.is_success() {
        let body = provider_error_body(resp).await;
        return Err(LlmError::Auth(format!(
            "xAI token exchange failed ({status}): {body}"
        )));
    }
    let tokens = response_json_limited::<TokenResponse>(resp).await?;
    let refresh = tokens
        .refresh_token
        .ok_or_else(|| LlmError::Auth("xAI token exchange missing refresh_token".into()))?;
    Ok(XaiOAuthToken::from_token_response(
        tokens.access_token,
        refresh,
        tokens.expires_in.unwrap_or(3600),
    ))
}

/// Refresh an access token.
///
/// # Errors
/// Network / auth failures.
pub async fn refresh_xai_token(
    client: &reqwest::Client,
    refresh: &SecretString,
) -> LlmResult<XaiOAuthToken> {
    let discovery = discover_xai(client).await?;
    let resp = client
        .post(&discovery.token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", XAI_OAUTH_CLIENT_ID),
            ("refresh_token", refresh.expose_secret()),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(LlmError::from)?;
    let status = resp.status();
    if !status.is_success() {
        let body = provider_error_body(resp).await;
        return Err(LlmError::Auth(format!(
            "xAI refresh failed ({status}): {body}. Run `ai-memory auth login xai-oauth` again."
        )));
    }
    let tokens = response_json_limited::<TokenResponse>(resp).await?;
    Ok(XaiOAuthToken::from_token_response(
        tokens.access_token,
        tokens
            .refresh_token
            .unwrap_or_else(|| refresh.expose_secret().to_string()),
        tokens.expires_in.unwrap_or(3600),
    ))
}

/// Generate PKCE verifier + S256 challenge (base64url, no padding).
#[must_use]
pub fn generate_pkce() -> (String, String) {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("os rng");
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        h.finalize()
    };
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

/// xAI OAuth-backed Responses provider with auto-refresh.
pub struct XaiOAuthProvider {
    token_path: PathBuf,
    token: Mutex<XaiOAuthToken>,
    client: reqwest::Client,
    model: String,
}

impl XaiOAuthProvider {
    /// Build from token file.
    ///
    /// # Errors
    /// Missing token.
    pub fn new(token_path: PathBuf, model: impl Into<String>) -> LlmResult<Self> {
        let token = XaiOAuthToken::load(&token_path)?.ok_or_else(|| {
            LlmError::NotConfigured(format!(
                "no xai-oauth token at {}; run `ai-memory auth login xai-oauth`",
                token_path.display()
            ))
        })?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(LlmError::from)?;
        Ok(Self {
            token_path,
            token: Mutex::new(token),
            client,
            model: model.into(),
        })
    }

    async fn access_token(&self) -> LlmResult<SecretString> {
        let mut guard = self.token.lock().await;
        if guard.needs_refresh() {
            info!("xai-oauth token near expiry, refreshing");
            let refreshed = refresh_xai_token(&self.client, &guard.refresh).await?;
            refreshed.save(&self.token_path)?;
            *guard = refreshed;
        }
        Ok(guard.access.clone())
    }

    async fn inner(&self) -> LlmResult<OpenAiResponsesProvider> {
        let access = self.access_token().await?;
        OpenAiResponsesProvider::new(
            crate::openai_responses::XAI_RESPONSES_BASE_URL,
            ResponsesAuth::Bearer(access),
            self.model.clone(),
            "xai-oauth",
        )
    }
}

#[async_trait]
impl LlmProvider for XaiOAuthProvider {
    fn name(&self) -> &'static str {
        "xai-oauth"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        self.inner().await?.complete(request).await
    }

    async fn complete_structured_raw(
        &self,
        request: ChatRequest,
        schema: serde_json::Value,
    ) -> LlmResult<serde_json::Value> {
        self.inner()
            .await?
            .complete_structured_raw(request, schema)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_xai_hosts() {
        assert!(validate_xai_endpoint("https://auth.x.ai/oauth/authorize", "a").is_ok());
        assert!(validate_xai_endpoint("http://auth.x.ai/oauth", "a").is_err());
        assert!(validate_xai_endpoint("https://evil.com/oauth", "a").is_err());
    }

    #[test]
    fn pkce_lengths_are_sane() {
        let (v, c) = generate_pkce();
        assert!(v.len() >= 40);
        assert_eq!(c.len(), 43);
    }
}
