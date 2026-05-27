//! Cursor SDK provider — delegates to the local `@cursor/sdk` runtime via
//! `scripts/cursor-bridge/worker.mjs` (same pool as IDE/SDK runs).
//!
//! Requires Node.js on `PATH`, `npm install` in `scripts/cursor-bridge/`, and
//! `CURSOR_API_KEY` in the environment of the spawned worker.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::debug;

use crate::error::{LlmError, LlmResult};
use crate::provider::LlmProvider;
use crate::types::{ChatMessage, ChatRequest, ChatResponse};

/// Settings for the Cursor bridge worker.
#[derive(Debug, Clone)]
pub struct CursorProviderConfig {
    /// `CURSOR_API_KEY` forwarded to the Node worker.
    pub api_key: SecretString,
    /// Model id (e.g. `composer-2.5`).
    pub model: String,
    /// Local agent working directory (`CURSOR_AGENT_CWD`).
    pub cwd: PathBuf,
    /// Wall-clock timeout for one bridge invocation.
    pub timeout_ms: u64,
    /// Composer `fast` model param.
    pub model_fast: bool,
    /// Path to `scripts/cursor-bridge/worker.mjs`.
    pub bridge_script: PathBuf,
    /// Node executable (`node` or `AI_MEMORY_NODE`).
    pub node_bin: String,
}

/// Cursor Composer via local SDK bridge (Node subprocess).
pub struct CursorSdkProvider {
    cfg: CursorProviderConfig,
}

impl CursorSdkProvider {
    /// Build a provider from config + resolved bridge paths.
    ///
    /// # Errors
    /// Returns [`LlmError::NotConfigured`] when the bridge script is missing.
    pub fn new(cfg: CursorProviderConfig) -> LlmResult<Self> {
        if !cfg.bridge_script.is_file() {
            return Err(LlmError::NotConfigured(format!(
                "cursor bridge script not found at {} — run: cd scripts/cursor-bridge && npm install",
                cfg.bridge_script.display()
            )));
        }
        if !bridge_sdk_installed(&cfg.bridge_script) {
            let bridge_dir = cfg
                .bridge_script
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "scripts/cursor-bridge".into());
            return Err(LlmError::NotConfigured(format!(
                "cursor bridge dependencies missing (@cursor/sdk) — run: cd {bridge_dir} && npm install \
                 (or .\\scripts\\windows\\install-cursor-bridge.ps1)"
            )));
        }
        Ok(Self { cfg })
    }

    /// Resolve `worker.mjs` and the Node executable.
    pub fn resolve_paths() -> LlmResult<(PathBuf, String)> {
        let bridge = resolve_bridge_script()?;
        let node = resolve_node_bin()?;
        Ok((bridge, node))
    }
}

#[derive(Debug, Serialize)]
struct BridgeRequest<'a> {
    system: Option<&'a str>,
    messages: &'a [ChatMessage],
    model: &'a str,
    #[serde(rename = "modelFast")]
    model_fast: bool,
    cwd: String,
    #[serde(rename = "timeoutMs")]
    timeout_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    ok: bool,
    text: Option<String>,
    json: Option<serde_json::Value>,
    model: Option<String>,
    error: Option<String>,
}

#[async_trait]
impl LlmProvider for CursorSdkProvider {
    fn name(&self) -> &'static str {
        "cursor"
    }

    fn model(&self) -> &str {
        &self.cfg.model
    }

    async fn complete(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        let resp = self.invoke_bridge(&request, None).await?;
        Ok(ChatResponse {
            text: resp.text.unwrap_or_default(),
            usage: None,
            model: resp.model.unwrap_or_else(|| self.cfg.model.clone()),
        })
    }

    async fn complete_structured_raw(
        &self,
        request: ChatRequest,
        schema: serde_json::Value,
    ) -> LlmResult<serde_json::Value> {
        let resp = self.invoke_bridge(&request, Some(schema)).await?;
        resp.json.ok_or_else(|| {
            LlmError::UnexpectedShape("cursor bridge returned ok without json field".into())
        })
    }
}

impl CursorSdkProvider {
    async fn invoke_bridge(
        &self,
        request: &ChatRequest,
        schema: Option<serde_json::Value>,
    ) -> LlmResult<BridgeResponse> {
        let body = BridgeRequest {
            system: request.system.as_deref(),
            messages: &request.messages,
            model: &self.cfg.model,
            model_fast: self.cfg.model_fast,
            cwd: self.cfg.cwd.display().to_string(),
            timeout_ms: self.cfg.timeout_ms,
            schema,
        };
        let stdin = serde_json::to_vec(&body).map_err(LlmError::from)?;

        debug!(
            bridge = %self.cfg.bridge_script.display(),
            model = %self.cfg.model,
            cwd = %self.cfg.cwd.display(),
            "spawning cursor bridge"
        );

        let mut child = Command::new(&self.cfg.node_bin)
            .arg(&self.cfg.bridge_script)
            .env("CURSOR_API_KEY", self.cfg.api_key.expose_secret())
            .env("AI_MEMORY_CURSOR_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                LlmError::NotConfigured(format!(
                    "failed to spawn node for cursor bridge ({e}); is Node.js installed?"
                ))
            })?;

        let mut stdin_pipe = child.stdin.take().ok_or_else(|| {
            LlmError::UnexpectedShape("cursor bridge child stdin unavailable".into())
        })?;
        stdin_pipe.write_all(&stdin).await.map_err(|e| {
            LlmError::UnexpectedShape(format!("cursor bridge stdin write failed: {e}"))
        })?;
        drop(stdin_pipe);

        let wait = child.wait_with_output();
        let budget_ms = self.cfg.timeout_ms.saturating_add(10_000);
        let output = tokio::time::timeout(Duration::from_millis(budget_ms), wait)
            .await
            .map_err(|_| {
                LlmError::UnexpectedShape(format!(
                    "cursor bridge process timed out after {budget_ms}ms"
                ))
            })?
            .map_err(|e| LlmError::UnexpectedShape(format!("cursor bridge wait failed: {e}")))?;

        let parsed: BridgeResponse = match serde_json::from_slice(&output.stdout) {
            Ok(p) => p,
            Err(e) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !output.status.success() {
                    return Err(LlmError::Provider {
                        status: output.status.code().unwrap_or(-1) as u16,
                        body: bridge_failure_message(&stdout, &stderr, &output.status),
                    });
                }
                return Err(LlmError::UnexpectedShape(format!(
                    "cursor bridge stdout was not valid JSON ({e}); snippet: {}",
                    truncate_snippet(&stdout, 400)
                )));
            }
        };

        if !output.status.success() || !parsed.ok {
            let msg = parsed
                .error
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bridge_failure_message(&stdout, &stderr, &output.status)
                });
            return Err(LlmError::Provider {
                status: output.status.code().unwrap_or(-1) as u16,
                body: msg,
            });
        }
        Ok(parsed)
    }
}

/// The Node worker writes `{ ok: false, error: "..." }` to stdout and exits 1.
/// Without parsing stdout, callers only see an opaque `ExitStatus(1)`.
fn bridge_failure_message(
    stdout: &str,
    stderr: &str,
    status: &std::process::ExitStatus,
) -> String {
    if let Ok(resp) = serde_json::from_str::<BridgeResponse>(stdout)
        && let Some(err) = resp.error.filter(|s| !s.is_empty())
    {
        return err;
    }
    if !stderr.trim().is_empty() {
        return stderr.trim().to_string();
    }
    if !stdout.trim().is_empty() {
        return truncate_snippet(stdout.trim(), 400);
    }
    format!("cursor bridge exited with {status:?}")
}

fn truncate_snippet(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..max])
}

/// Locate `scripts/cursor-bridge/worker.mjs`.
pub fn resolve_bridge_script() -> LlmResult<PathBuf> {
    if let Ok(p) = std::env::var("AI_MEMORY_CURSOR_BRIDGE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(LlmError::NotConfigured(format!(
            "AI_MEMORY_CURSOR_BRIDGE={} is not a file",
            path.display()
        )));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
    {
        let candidate = root
            .join("scripts")
            .join("cursor-bridge")
            .join("worker.mjs");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("scripts").join("cursor-bridge").join("worker.mjs");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(LlmError::NotConfigured(
        "could not find scripts/cursor-bridge/worker.mjs — set AI_MEMORY_CURSOR_BRIDGE or run from repo root"
            .into(),
    ))
}

fn resolve_node_bin() -> LlmResult<String> {
    if let Ok(n) = std::env::var("AI_MEMORY_NODE") {
        let t = n.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    Ok("node".to_string())
}

/// Whether `npm install` has been run in the bridge directory.
fn bridge_sdk_installed(bridge_script: &std::path::Path) -> bool {
    bridge_script
        .parent()
        .is_some_and(|dir| dir.join("node_modules/@cursor/sdk/package.json").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_sdk_installed_checks_package_json() {
        let script = PathBuf::from("scripts/cursor-bridge/worker.mjs");
        // Passes when devs have run npm install; documents the expected layout.
        if script.is_file() {
            assert!(
                bridge_sdk_installed(&script),
                "run: cd scripts/cursor-bridge && npm install"
            );
        }
    }

    #[test]
    fn bridge_failure_message_reads_stdout_json() {
        let stdout = r#"{"ok":false,"error":"CURSOR_API_KEY is not set"}"#;
        let msg = bridge_failure_message(stdout, "", &std::process::ExitStatus::default());
        assert_eq!(msg, "CURSOR_API_KEY is not set");
    }
}
