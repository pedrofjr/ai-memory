//! OpenCode Zen/Go provider.
//!
//! Thin wrapper around [`OpenAiCompatProvider`] that bakes in the OpenCode
//! Zen API base URL (`https://opencode.ai/zen/go/v1`) and names the provider
//! `"opencode"`. Accepts an `sk-...` API key from `OPENCODE_API_KEY`.

use async_trait::async_trait;
use secrecy::SecretString;

use crate::error::LlmResult;
use crate::openai_compat::OpenAiCompatProvider;
use crate::provider::LlmProvider;
use crate::types::{ChatRequest, ChatResponse};

/// Public OpenCode Zen/Go OpenAI-compatible base URL.
pub const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/go/v1";

/// Default model when `AI_MEMORY_LLM_MODEL` is not set.
pub const OPENCODE_DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Client identifier sent as `user-agent`. The gateway asks clients to
/// identify themselves instead of presenting a generic SDK or HTTP
/// library agent.
const OPENCODE_USER_AGENT: &str = concat!("ai-memory/", env!("CARGO_PKG_VERSION"));

/// Optional override for the routing session id.
const OPENCODE_SESSION_ENV: &str = "OPENCODE_SESSION_ID";

/// Stable id sent as `x-opencode-session`.
///
/// The gateway rejects requests without it (`400 MissingSessionID`) and
/// uses it for routing and prompt caching, so it must not change per
/// request. A per-process UUID is used unless `OPENCODE_SESSION_ID`
/// pins a specific value.
fn opencode_session_id() -> String {
    session_id_from(std::env::var(OPENCODE_SESSION_ENV).ok())
}

/// Resolve the session id from an optional override. Split out from
/// [`opencode_session_id`] because `std::env::set_var` is `unsafe` under
/// edition 2024, so the override path is tested through this seam.
fn session_id_from(override_value: Option<String>) -> String {
    override_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

/// OpenCode Zen/Go LLM provider.
///
/// Routes through `https://opencode.ai/zen/go/v1` using the OpenAI chat
/// completions wire format. Authenticate with the `sk-...` key obtained
/// from <https://opencode.ai/auth>.
pub struct OpenCodeProvider {
    inner: OpenAiCompatProvider,
}

impl OpenCodeProvider {
    /// Construct an OpenCode Zen/Go provider.
    ///
    /// # Errors
    /// Returns a `reqwest::Error` if the HTTP client cannot be built.
    pub fn new(api_key: SecretString, model: impl Into<String>) -> LlmResult<Self> {
        let inner = OpenAiCompatProvider::new(OPENCODE_ZEN_BASE_URL, Some(api_key), model.into())?
            .with_header("x-opencode-session", opencode_session_id())
            .with_header("user-agent", OPENCODE_USER_AGENT);
        Ok(Self { inner })
    }

    /// Override the per-request timeout on the wrapped
    /// [`OpenAiCompatProvider`]. The factory calls this with
    /// `ProviderConfig::request_timeout_secs`.
    #[must_use]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.inner = self.inner.with_timeout_secs(secs);
        self
    }
}

#[async_trait]
impl LlmProvider for OpenCodeProvider {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn complete(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        self.inner.complete(request).await
    }

    async fn complete_structured_raw(
        &self,
        request: ChatRequest,
        schema: serde_json::Value,
    ) -> LlmResult<serde_json::Value> {
        self.inner.complete_structured_raw(request, schema).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_reports_opencode_name_and_configured_model() {
        let provider = OpenCodeProvider::new(SecretString::from("sk-test"), "model-x").unwrap();
        assert_eq!(provider.name(), "opencode");
        assert_eq!(provider.model(), "model-x");
    }

    #[test]
    fn public_constants_point_at_zen_base_url() {
        assert_eq!(OPENCODE_ZEN_BASE_URL, "https://opencode.ai/zen/go/v1");
        assert!(!OPENCODE_DEFAULT_MODEL.is_empty());
    }

    #[test]
    fn provider_sends_session_and_user_agent_headers() {
        let provider = OpenCodeProvider::new(SecretString::from("sk-test"), "model-x").unwrap();
        let session = provider
            .inner
            .header_value("x-opencode-session")
            .expect("gateway rejects requests without x-opencode-session");
        assert!(!session.is_empty());
        assert!(provider
            .inner
            .header_value("user-agent")
            .expect("client must identify itself")
            .starts_with("ai-memory/"));
    }

    #[test]
    fn session_id_override_is_trimmed_and_used() {
        assert_eq!(session_id_from(Some("  sess-fixed  ".into())), "sess-fixed");
    }

    #[test]
    fn session_id_falls_back_to_a_uuid() {
        let generated = session_id_from(None);
        assert_eq!(generated.len(), 36);
        assert_ne!(generated, session_id_from(Some("   ".into())));
    }
}
