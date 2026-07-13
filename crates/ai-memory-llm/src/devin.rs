//! Devin (Codeium Cascade) provider via Connect-RPC + protobuf.
//!
//! Ported from oh-my-pi `packages/ai/src/providers/devin.ts` and
//! devin-proxy `src/services/devin-client.ts` / `devin-auth.ts` (MIT).
//!
//! Full generated protos are huge (multi-MB with buf/validate deps). We
//! hand-encode the **minimal** subset of fields required for
//! GetUserJwt + GetChatMessage text completion, and hand-decode
//! `delta_text` from stream frames. Structured output asks for JSON in
//! the system prompt and uses the tolerant JSON extractor.

use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::auth_file::{load_entry, now_ms, save_entry};
use crate::error::{LlmError, LlmResult};
use crate::openai_compat::strip_reasoning_blocks;
use crate::provider::LlmProvider;
use crate::response::{provider_error_body, response_json_limited};
use crate::types::{ChatRequest, ChatResponse, Role};

/// Cascade API origin.
pub const DEVIN_API_URL: &str = "https://server.codeium.com";
/// Session token prefix expected by GetUserJwt.
pub const DEVIN_SESSION_TOKEN_PREFIX: &str = "devin-session-token$";
/// Web app for OAuth.
pub const DEVIN_WEBAPP_URL: &str = "https://app.devin.ai";
/// Token exchange API.
pub const DEVIN_TOKEN_API_URL: &str = "https://api.devin.ai";
/// OAuth loopback port (same as omp / devin-proxy).
pub const DEVIN_OAUTH_CALLBACK_PORT: u16 = 59653;
/// OAuth callback path.
pub const DEVIN_OAUTH_CALLBACK_PATH: &str = "/callback";

const CHAT_MESSAGE_PATH: &str = "/exa.api_server_pb.ApiServerService/GetChatMessage";
const AUTH_PATH: &str = "/exa.auth_pb.AuthService/GetUserJwt";
const IDE_VERSION: &str = "3.2.23";
const EXT_VERSION: &str = "1.48.2";
const CONNECT_COMPRESSED_FLAG: u8 = 0x01;
const CONNECT_END_STREAM_FLAG: u8 = 0x02;
const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Default model when unset.
pub const DEVIN_DEFAULT_MODEL: &str = "swe-1-6";

/// Stored Devin session token (from OAuth or paste).
#[derive(Clone)]
pub struct DevinToken {
    /// Session token (with or without prefix).
    pub token: SecretString,
    /// Optional expiry ms.
    pub expires_at_ms: Option<u64>,
}

impl std::fmt::Debug for DevinToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevinToken")
            .field("token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

impl DevinToken {
    /// Load from auth file (`devin` entry).
    ///
    /// # Errors
    /// Parse errors.
    pub fn load(path: &Path) -> LlmResult<Option<Self>> {
        let Some(entry) = load_entry::<TokenEntry>(path, "devin")? else {
            return Ok(None);
        };
        Ok(Some(Self {
            token: SecretString::from(entry.token),
            expires_at_ms: entry.expires,
        }))
    }

    /// Save to auth file.
    ///
    /// # Errors
    /// Write errors.
    pub fn save(&self, path: &Path) -> LlmResult<()> {
        save_entry(
            path,
            "devin",
            Some(TokenEntry {
                kind: "oauth".into(),
                token: self.token.expose_secret().to_string(),
                expires: self.expires_at_ms,
            }),
        )
    }

    /// Remove entry.
    ///
    /// # Errors
    /// Write errors.
    pub fn remove(path: &Path) -> LlmResult<()> {
        save_entry::<TokenEntry>(path, "devin", None)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenEntry {
    #[serde(rename = "type")]
    kind: String,
    token: String,
    #[serde(default)]
    expires: Option<u64>,
}

/// Exchange OAuth authorization code for a session token.
///
/// # Errors
/// Network / empty token.
pub async fn exchange_devin_token(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
) -> LlmResult<DevinToken> {
    let resp = client
        .post(format!("{DEVIN_TOKEN_API_URL}/auth/cli/token"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "code": code,
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(LlmError::from)?;
    let status = resp.status();
    if !status.is_success() {
        let body = provider_error_body(resp).await;
        return Err(LlmError::Auth(format!(
            "Devin token exchange failed ({status}): {body}"
        )));
    }
    let value = response_json_limited::<serde_json::Value>(resp).await?;
    let token = value
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LlmError::Auth("Devin token exchange returned empty token".into()))?;
    Ok(DevinToken {
        token: SecretString::from(token.to_string()),
        expires_at_ms: Some(now_ms().saturating_add(365 * 24 * 60 * 60 * 1000)),
    })
}

/// Build the browser OAuth URL.
#[must_use]
pub fn build_devin_authorize_url(redirect_uri: &str, state: &str, code_challenge: &str) -> String {
    format!(
        "{DEVIN_WEBAPP_URL}/auth/cli/continue?{}",
        [
            ("redirect_uri", redirect_uri),
            ("state", state),
            ("prompt", "select_account"),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
        ]
        .into_iter()
        .map(|(k, v)| format!("{k}={}", urlencoding_minimal(v)))
        .collect::<Vec<_>>()
        .join("&")
    )
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Devin Cascade LLM provider.
pub struct DevinProvider {
    client: reqwest::Client,
    model: String,
    session: SecretString,
    base_url: String,
}

impl DevinProvider {
    /// From explicit session token (env or login).
    ///
    /// # Errors
    /// Client build errors.
    pub fn new(session: SecretString, model: impl Into<String>) -> LlmResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(LlmError::from)?;
        Ok(Self {
            client,
            model: model.into(),
            session,
            base_url: DEVIN_API_URL.to_string(),
        })
    }

    /// From shared auth file.
    ///
    /// # Errors
    /// Missing token.
    pub fn from_token_file(path: PathBuf, model: impl Into<String>) -> LlmResult<Self> {
        let tok = DevinToken::load(&path)?.ok_or_else(|| {
            LlmError::NotConfigured(format!(
                "no devin token at {}; run `ai-memory auth login devin` or set DEVIN_API_KEY",
                path.display()
            ))
        })?;
        Self::new(tok.token, model)
    }

    async fn get_user_jwt(&self) -> LlmResult<(String, String)> {
        let session = normalize_session(self.session.expose_secret());
        let meta = encode_metadata(&session, None);
        let req = {
            let mut b = Vec::new();
            write_bytes_field(&mut b, 1, &meta); // metadata
            b
        };
        let url = format!("{}{AUTH_PATH}", self.base_url);
        debug!(%url, "POST devin GetUserJwt");
        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/proto")
            .header("connect-protocol-version", "1")
            .header("accept", "*/*")
            .body(req)
            .send()
            .await
            .map_err(LlmError::from)?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(LlmError::from)?;
        if !status.is_success() {
            return Err(LlmError::Provider {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let decoded = decode_user_jwt(&bytes)?;
        if decoded.0.is_empty() {
            return Err(LlmError::Auth(
                "Devin GetUserJwt returned empty user JWT".into(),
            ));
        }
        Ok(decoded)
    }

    async fn complete_text(&self, request: &ChatRequest) -> LlmResult<String> {
        let (user_jwt, custom_base) = self.get_user_jwt().await?;
        let chat_base = if custom_base.is_empty() {
            self.base_url.clone()
        } else {
            custom_base.trim_end_matches('/').to_string()
        };
        let session = normalize_session(self.session.expose_secret());
        let body = build_get_chat_message(&session, &user_jwt, &self.model, request);
        let gz = gzip(&body)?;
        let frame = connect_frame(&gz, CONNECT_COMPRESSED_FLAG);
        let url = format!("{chat_base}{CHAT_MESSAGE_PATH}");
        debug!(%url, "POST devin GetChatMessage");
        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/connect+proto")
            .header("connect-protocol-version", "1")
            .header("connect-content-encoding", "gzip")
            .header("accept-encoding", "identity")
            .header("connect-accept-encoding", "gzip")
            .header("user-agent", "connect-rs/ai-memory")
            .body(frame)
            .send()
            .await
            .map_err(LlmError::from)?;
        let status = resp.status();
        if !status.is_success() {
            let body = provider_error_body(resp).await;
            return Err(LlmError::Provider {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await.map_err(LlmError::from)?;
        parse_connect_text_stream(&bytes)
    }
}

#[async_trait]
impl LlmProvider for DevinProvider {
    fn name(&self) -> &'static str {
        "devin"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        let text = self.complete_text(&request).await?;
        Ok(ChatResponse {
            text,
            model: self.model.clone(),
            usage: None,
        })
    }

    async fn complete_structured_raw(
        &self,
        mut request: ChatRequest,
        schema: serde_json::Value,
    ) -> LlmResult<serde_json::Value> {
        let schema_hint = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into());
        let inject = format!(
            "Respond with a single JSON object only (no markdown fences) matching this schema:\n{schema_hint}"
        );
        request.system = Some(match request.system.take() {
            Some(s) => format!("{s}\n\n{inject}"),
            None => inject,
        });
        let text = self.complete_text(&request).await?;
        let cleaned = strip_reasoning_blocks(&text);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cleaned) {
            return Ok(v);
        }
        let Some(obj) = first_json_object(&cleaned) else {
            return Err(LlmError::UnexpectedShape(format!(
                "devin structured response did not contain a JSON object: {}",
                truncate_preview(&cleaned)
            )));
        };
        serde_json::from_str(&obj).map_err(LlmError::from)
    }
}

fn truncate_preview(s: &str) -> String {
    let t = s.trim();
    if t.len() <= 200 {
        t.to_string()
    } else {
        format!("{}…", &t[..200])
    }
}

fn normalize_session(api_key: &str) -> String {
    if api_key.is_empty() {
        return String::new();
    }
    if api_key.starts_with(DEVIN_SESSION_TOKEN_PREFIX) {
        api_key.to_string()
    } else {
        format!("{DEVIN_SESSION_TOKEN_PREFIX}{api_key}")
    }
}

// --- minimal protobuf wire helpers ---

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn write_tag(out: &mut Vec<u8>, field: u32, wire: u8) {
    write_varint(out, u64::from(field << 3 | u32::from(wire)));
}

fn write_bytes_field(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    write_tag(out, field, 2);
    write_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn write_string_field(out: &mut Vec<u8>, field: u32, s: &str) {
    write_bytes_field(out, field, s.as_bytes());
}

fn write_varint_field(out: &mut Vec<u8>, field: u32, v: u64) {
    write_tag(out, field, 0);
    write_varint(out, v);
}

fn write_double_field(out: &mut Vec<u8>, field: u32, v: f64) {
    write_tag(out, field, 1); // 64-bit
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_bool_field(out: &mut Vec<u8>, field: u32, v: bool) {
    write_varint_field(out, field, u64::from(v));
}

fn encode_metadata(api_key: &str, user_jwt: Option<&str>) -> Vec<u8> {
    let mut m = Vec::new();
    write_string_field(&mut m, 1, "windsurf"); // ide_name
    write_string_field(&mut m, 2, EXT_VERSION); // extension_version
    write_string_field(&mut m, 3, api_key); // api_key
    write_string_field(&mut m, 4, "en"); // locale
    write_string_field(&mut m, 7, IDE_VERSION); // ide_version
    write_string_field(&mut m, 12, "windsurf"); // extension_name
    if let Some(jwt) = user_jwt {
        write_string_field(&mut m, 21, jwt); // user_jwt
    }
    m
}

fn encode_chat_prompt(message_id: &str, source: u32, prompt: &str) -> Vec<u8> {
    let mut m = Vec::new();
    write_string_field(&mut m, 1, message_id);
    write_varint_field(&mut m, 2, u64::from(source));
    write_string_field(&mut m, 3, prompt);
    m
}

fn encode_configuration(max_tokens: u32, temperature: f32) -> Vec<u8> {
    let mut m = Vec::new();
    write_varint_field(&mut m, 1, 1); // num_completions
    write_varint_field(&mut m, 2, u64::from(max_tokens.max(1)));
    write_varint_field(&mut m, 3, 200); // max_newlines
    write_double_field(&mut m, 5, f64::from(temperature));
    write_double_field(&mut m, 6, f64::from(temperature));
    write_varint_field(&mut m, 7, 50); // top_k
    write_double_field(&mut m, 8, 1.0); // top_p
    for pat in [
        "\u{200b}",
        "<|bot|>",
        "<|context_request|>",
        "<|end_of_turn|>",
    ] {
        write_string_field(&mut m, 9, pat);
    }
    write_double_field(&mut m, 11, 1.0); // fim_eot_prob_threshold
    m
}

fn encode_tool_choice_auto() -> Vec<u8> {
    let mut m = Vec::new();
    write_string_field(&mut m, 1, "auto"); // option_name
    m
}

fn encode_prompt_cache_ephemeral() -> Vec<u8> {
    let mut m = Vec::new();
    write_varint_field(&mut m, 1, 1); // CACHE_CONTROL_TYPE_EPHEMERAL
    m
}

fn build_get_chat_message(
    session: &str,
    user_jwt: &str,
    model: &str,
    request: &ChatRequest,
) -> Vec<u8> {
    let cascade_id = uuid::Uuid::new_v4().to_string();
    let execution_id = uuid::Uuid::new_v4().to_string();
    let system = request.system.as_deref().unwrap_or("");
    let meta = encode_metadata(session, Some(user_jwt));
    let mut body = Vec::new();
    write_bytes_field(&mut body, 1, &meta);
    write_string_field(&mut body, 2, system);
    for (i, msg) in request.messages.iter().enumerate() {
        let (source, mid) = match msg.role {
            Role::User => (1u32, format!("{cascade_id}\0{i}\0user")),
            Role::Assistant => (2u32, format!("bot-{cascade_id}\0{i}\0assistant")),
        };
        let p = encode_chat_prompt(&mid, source, &msg.content);
        write_bytes_field(&mut body, 3, &p);
    }
    write_varint_field(&mut body, 7, 5); // CASCADE
    let cfg = encode_configuration(request.max_tokens, request.temperature.unwrap_or(0.4));
    write_bytes_field(&mut body, 8, &cfg);
    write_bool_field(&mut body, 11, true); // disable_parallel_tool_calls
    let choice = encode_tool_choice_auto();
    write_bytes_field(&mut body, 12, &choice);
    let cache = encode_prompt_cache_ephemeral();
    write_bytes_field(&mut body, 13, &cache);
    write_string_field(&mut body, 16, &cascade_id);
    write_varint_field(&mut body, 20, 1); // planner DEFAULT
    write_string_field(&mut body, 21, model); // chat_model_uid
    write_string_field(&mut body, 22, &execution_id);
    body
}

fn gzip(data: &[u8]) -> LlmResult<Vec<u8>> {
    use std::io::Write as _;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)
        .map_err(|e| LlmError::UnexpectedShape(format!("gzip: {e}")))?;
    enc.finish()
        .map_err(|e| LlmError::UnexpectedShape(format!("gzip finish: {e}")))
}

fn gunzip(data: &[u8]) -> LlmResult<Vec<u8>> {
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| LlmError::UnexpectedShape(format!("gunzip: {e}")))?;
    Ok(out)
}

fn connect_frame(payload: &[u8], flags: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(flags);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn decode_user_jwt(payload: &[u8]) -> LlmResult<(String, String)> {
    if let Ok(r) = parse_user_jwt_message(payload) {
        return Ok(r);
    }
    let inflated = gunzip(payload)?;
    parse_user_jwt_message(&inflated)
}

fn parse_user_jwt_message(data: &[u8]) -> LlmResult<(String, String)> {
    let mut jwt = String::new();
    let mut custom = String::new();
    let mut cur = Cursor::new(data);
    while (cur.position() as usize) < data.len() {
        let key = read_varint(&mut cur)?;
        let field = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        match (field, wire) {
            (1, 2) => {
                jwt = String::from_utf8_lossy(&read_len_bytes(&mut cur)?).into_owned();
            }
            (2, 2) => {
                custom = String::from_utf8_lossy(&read_len_bytes(&mut cur)?).into_owned();
            }
            (_, 0) => {
                let _ = read_varint(&mut cur)?;
            }
            (_, 1) => {
                let mut buf = [0u8; 8];
                cur.read_exact(&mut buf)
                    .map_err(|e| LlmError::UnexpectedShape(format!("proto skip: {e}")))?;
            }
            (_, 2) => {
                let _ = read_len_bytes(&mut cur)?;
            }
            (_, 5) => {
                let mut buf = [0u8; 4];
                cur.read_exact(&mut buf)
                    .map_err(|e| LlmError::UnexpectedShape(format!("proto skip: {e}")))?;
            }
            _ => {
                return Err(LlmError::UnexpectedShape(format!(
                    "unsupported protobuf wire type {wire}"
                )));
            }
        }
    }
    Ok((jwt, custom))
}

fn read_varint(cur: &mut Cursor<&[u8]>) -> LlmResult<u64> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let mut b = [0u8; 1];
        cur.read_exact(&mut b)
            .map_err(|e| LlmError::UnexpectedShape(format!("varint: {e}")))?;
        result |= u64::from(b[0] & 0x7f) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift > 63 {
            return Err(LlmError::UnexpectedShape("varint overflow".into()));
        }
    }
}

fn read_len_bytes(cur: &mut Cursor<&[u8]>) -> LlmResult<Vec<u8>> {
    let len = read_varint(cur)? as usize;
    let mut buf = vec![0u8; len];
    cur.read_exact(&mut buf)
        .map_err(|e| LlmError::UnexpectedShape(format!("bytes: {e}")))?;
    Ok(buf)
}

fn parse_connect_text_stream(payload: &[u8]) -> LlmResult<String> {
    let mut text = String::new();
    let mut offset = 0;
    while offset + 5 <= payload.len() {
        let flag = payload[offset];
        let len = u32::from_be_bytes([
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
            payload[offset + 4],
        ]) as usize;
        offset += 5;
        if len > MAX_FRAME || offset + len > payload.len() {
            return Err(LlmError::UnexpectedShape(format!(
                "devin connect frame length {len} invalid"
            )));
        }
        let mut chunk = payload[offset..offset + len].to_vec();
        offset += len;
        if flag & CONNECT_COMPRESSED_FLAG != 0 {
            chunk = gunzip(&chunk)?;
        }
        if flag & CONNECT_END_STREAM_FLAG != 0 {
            // Trailer JSON may carry errors.
            if let Ok(s) = std::str::from_utf8(&chunk)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
                && let Some(err) = v.get("error")
            {
                return Err(LlmError::Provider {
                    status: 502,
                    body: err.to_string(),
                });
            }
            break;
        }
        // GetChatMessageResponse.delta_text = field 3
        if let Some(delta) = extract_string_field(&chunk, 3) {
            text.push_str(&delta);
        }
    }
    if text.is_empty() {
        return Err(LlmError::UnexpectedShape(
            "devin stream produced no delta_text".into(),
        ));
    }
    Ok(text)
}

fn extract_string_field(data: &[u8], want: u32) -> Option<String> {
    let mut cur = Cursor::new(data);
    while (cur.position() as usize) < data.len() {
        let key = read_varint(&mut cur).ok()?;
        let field = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        match wire {
            0 => {
                let _ = read_varint(&mut cur).ok()?;
            }
            1 => {
                let mut buf = [0u8; 8];
                cur.read_exact(&mut buf).ok()?;
            }
            2 => {
                let bytes = read_len_bytes(&mut cur).ok()?;
                if field == want {
                    return Some(String::from_utf8_lossy(&bytes).into_owned());
                }
            }
            5 => {
                let mut buf = [0u8; 4];
                cur.read_exact(&mut buf).ok()?;
            }
            _ => return None,
        }
    }
    None
}

fn first_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_prefix() {
        assert_eq!(
            normalize_session("abc"),
            format!("{DEVIN_SESSION_TOKEN_PREFIX}abc")
        );
        assert_eq!(
            normalize_session(&format!("{DEVIN_SESSION_TOKEN_PREFIX}abc")),
            format!("{DEVIN_SESSION_TOKEN_PREFIX}abc")
        );
    }

    #[test]
    fn encode_metadata_round_trips_api_key_field() {
        let m = encode_metadata("devin-session-token$x", None);
        assert!(
            extract_string_field(&m, 3)
                .unwrap()
                .contains("devin-session-token")
        );
    }

    #[test]
    fn first_json_extracts_object() {
        assert_eq!(
            first_json_object("noise {\"a\":1} tail").as_deref(),
            Some("{\"a\":1}")
        );
    }
}
