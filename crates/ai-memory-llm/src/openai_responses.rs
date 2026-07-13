//! OpenAI Responses API client (`POST /v1/responses`).
//!
//! Shared by Platform OpenAI (API key), xAI, and Azure OpenAI deployments.
//! Structured output uses `text.format = json_schema` (same shape as Codex
//! OAuth). Prefer non-streaming JSON; fall back to SSE parse when the
//! upstream only returns `text/event-stream`.

use std::time::Duration;

use async_trait::async_trait;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::{LlmError, LlmResult};
use crate::openai::{STRUCTURED_OUTPUT_SCHEMA_NAME, enforce_strict_object_schemas};
use crate::provider::LlmProvider;
use crate::response::{provider_error_body, response_json_limited, response_text_limited};
use crate::text::truncate_with_ellipsis;
use crate::types::{ChatRequest, ChatResponse, Role, Usage};

/// Default OpenAI Platform Responses base (includes `/v1`).
pub const OPENAI_RESPONSES_BASE_URL: &str = "https://api.openai.com/v1";

/// Default xAI Responses base.
pub const XAI_RESPONSES_BASE_URL: &str = "https://api.x.ai/v1";

const SSE_TERMINAL_ERROR_STATUS: u16 = 502;
const SSE_ERROR_BODY_TRIM: usize = 1024;

/// How the HTTP client authenticates.
#[derive(Debug, Clone)]
pub enum ResponsesAuth {
    /// `Authorization: Bearer <key>`.
    Bearer(SecretString),
    /// Azure-style `api-key: <key>` (also sends Bearer for dual-compatible gateways).
    AzureApiKey(SecretString),
}

/// OpenAI-compatible Responses provider.
pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    model: String,
    base_url: String,
    auth: ResponsesAuth,
    name_tag: &'static str,
}

impl OpenAiResponsesProvider {
    /// Platform OpenAI with API key.
    ///
    /// # Errors
    /// Returns a client-build error.
    pub fn openai(api_key: SecretString, model: impl Into<String>) -> LlmResult<Self> {
        Self::new(
            OPENAI_RESPONSES_BASE_URL,
            ResponsesAuth::Bearer(api_key),
            model,
            "openai-responses",
        )
    }

    /// xAI Grok Responses endpoint.
    ///
    /// # Errors
    /// Returns a client-build error.
    pub fn xai(api_key: SecretString, model: impl Into<String>) -> LlmResult<Self> {
        Self::new(
            XAI_RESPONSES_BASE_URL,
            ResponsesAuth::Bearer(api_key),
            model,
            "xai",
        )
    }

    /// Azure OpenAI Responses. `base_url` should be the resource root that
    /// already includes the path prefix used by the deployment (typically
    /// ends with `/openai/v1` or similar).
    ///
    /// # Errors
    /// Returns a client-build error.
    pub fn azure(
        base_url: impl Into<String>,
        api_key: SecretString,
        model: impl Into<String>,
    ) -> LlmResult<Self> {
        Self::new(
            base_url,
            ResponsesAuth::AzureApiKey(api_key),
            model,
            "azure-openai",
        )
    }

    /// Fully custom Responses endpoint.
    ///
    /// # Errors
    /// Returns a client-build error.
    pub fn new(
        base_url: impl Into<String>,
        auth: ResponsesAuth,
        model: impl Into<String>,
        name_tag: &'static str,
    ) -> LlmResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(LlmError::from)?;
        Ok(Self {
            client,
            model: model.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth,
            name_tag,
        })
    }

    /// Override the reported provider name.
    #[must_use]
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name_tag = name;
        self
    }

    /// Override base URL (e.g. regional Azure endpoint).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url)
    }

    async fn post(&self, body: &ResponsesRequest<'_>) -> LlmResult<ResponsesResponse> {
        let url = self.responses_url();
        debug!(url = %url, provider = self.name_tag, "POST responses");
        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .header(
                "accept",
                if body.stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .json(body);
        request = match &self.auth {
            ResponsesAuth::Bearer(key) => request.bearer_auth(key.expose_secret()),
            ResponsesAuth::AzureApiKey(key) => request
                .header("api-key", key.expose_secret())
                .bearer_auth(key.expose_secret()),
        };
        let resp = request.send().await.map_err(LlmError::from)?;
        let status = resp.status();
        if !status.is_success() {
            let body = provider_error_body(resp).await;
            return Err(LlmError::Provider {
                status: status.as_u16(),
                body,
            });
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if body.stream || content_type.contains("text/event-stream") {
            parse_sse_response(&response_text_limited(resp).await?)
        } else {
            response_json_limited::<ResponsesResponse>(resp).await
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn name(&self) -> &'static str {
        self.name_tag
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        let response = self
            .post(&build_request(&self.model, &request, None, false))
            .await?;
        Ok(into_chat_response(response, &self.model))
    }

    async fn complete_structured_raw(
        &self,
        request: ChatRequest,
        mut schema: serde_json::Value,
    ) -> LlmResult<serde_json::Value> {
        enforce_strict_object_schemas(&mut schema);
        let text = ResponsesText {
            format: ResponsesTextFormat::JsonSchema {
                name: STRUCTURED_OUTPUT_SCHEMA_NAME.into(),
                schema,
                strict: true,
            },
        };
        // Prefer non-streaming for structured; some hosts only stream —
        // retry once with stream=true if the non-stream call is rejected
        // purely on accept/shape (handled by caller visibility of error).
        let response = match self
            .post(&build_request(
                &self.model,
                &request,
                Some(text.clone()),
                false,
            ))
            .await
        {
            Ok(r) => r,
            Err(LlmError::Provider { status, body })
                if status == 400 && (body.contains("stream") || body.contains("event-stream")) =>
            {
                self.post(&build_request(&self.model, &request, Some(text), true))
                    .await?
            }
            Err(e) => return Err(e),
        };
        let out = extract_output_text(&response).unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(&out).map_err(LlmError::from)
    }
}

fn build_request<'a>(
    model: &'a str,
    request: &'a ChatRequest,
    text: Option<ResponsesText>,
    stream: bool,
) -> ResponsesRequest<'a> {
    let input = request
        .messages
        .iter()
        .map(|msg| ResponsesInputItem {
            role: match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            },
            content: &msg.content,
        })
        .collect();
    ResponsesRequest {
        model,
        instructions: request.system.as_deref(),
        input,
        max_output_tokens: Some(request.max_tokens),
        temperature: request.temperature,
        stream,
        text,
    }
}

fn parse_sse_response(body: &str) -> LlmResult<ResponsesResponse> {
    let mut current_event: Option<String> = None;
    let mut data_lines: Vec<&str> = Vec::new();
    let mut output_text = String::new();
    let mut completed: Option<ResponsesResponse> = None;

    let mut flush_event = |event: Option<&str>, data_lines: &mut Vec<&str>| -> LlmResult<()> {
        if data_lines.is_empty() {
            return Ok(());
        }
        let data = data_lines.join("\n");
        data_lines.clear();
        let trimmed = data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return Ok(());
        }
        let value = serde_json::from_str::<serde_json::Value>(trimmed)?;
        if value.get("error").is_some() {
            return Err(LlmError::Provider {
                status: SSE_TERMINAL_ERROR_STATUS,
                body: truncate_with_ellipsis(trimmed, SSE_ERROR_BODY_TRIM),
            });
        }
        let kind = value
            .get("type")
            .and_then(|v| v.as_str())
            .or(event)
            .unwrap_or_default();
        match kind {
            "response.output_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(|v| v.as_str()) {
                    output_text.push_str(delta);
                }
            }
            "response.completed" => {
                let response = value.get("response").cloned().unwrap_or(value);
                completed = Some(serde_json::from_value(response)?);
            }
            "response.failed" | "response.incomplete" | "response.cancelled" | "error" => {
                return Err(LlmError::Provider {
                    status: SSE_TERMINAL_ERROR_STATUS,
                    body: truncate_with_ellipsis(trimmed, SSE_ERROR_BODY_TRIM),
                });
            }
            _ => {}
        }
        Ok(())
    };

    for line in body.lines() {
        if line.is_empty() {
            flush_event(current_event.as_deref(), &mut data_lines)?;
            current_event = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            current_event = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
    }
    flush_event(current_event.as_deref(), &mut data_lines)?;

    let mut response = completed.ok_or_else(|| {
        LlmError::UnexpectedShape("responses stream closed before response.completed".into())
    })?;
    if response
        .output_text
        .as_deref()
        .unwrap_or_default()
        .is_empty()
        && !output_text.is_empty()
    {
        response.output_text = Some(output_text);
    }
    Ok(response)
}

#[derive(Debug, Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'a str>,
    input: Vec<ResponsesInputItem<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<ResponsesText>,
}

#[derive(Debug, Serialize)]
struct ResponsesInputItem<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesText {
    format: ResponsesTextFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesTextFormat {
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        strict: bool,
    },
}

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(default)]
    content: Vec<ResponsesOutputContent>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputContent {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

fn extract_output_text(response: &ResponsesResponse) -> Option<String> {
    if let Some(t) = response.output_text.as_ref().filter(|s| !s.is_empty()) {
        return Some(t.clone());
    }
    let mut buf = String::new();
    for item in &response.output {
        for part in &item.content {
            if let Some(t) = &part.text {
                buf.push_str(t);
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}

fn into_chat_response(response: ResponsesResponse, fallback_model: &str) -> ChatResponse {
    let text = extract_output_text(&response).unwrap_or_default();
    ChatResponse {
        text,
        model: response.model.unwrap_or_else(|| fallback_model.to_string()),
        usage: response.usage.map(|u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_maps_system_to_instructions() {
        let req = ChatRequest::user_prompt("hi")
            .with_system("sys")
            .with_max_tokens(100);
        let built = build_request("grok-3-mini", &req, None, false);
        assert_eq!(built.instructions, Some("sys"));
        assert_eq!(built.input.len(), 1);
        assert_eq!(built.input[0].content, "hi");
        assert!(!built.stream);
    }
}
