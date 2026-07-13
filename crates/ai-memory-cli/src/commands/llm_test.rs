//! `ai-memory llm-test` — smoke test an LLM provider end-to-end.

use ai_memory_llm::{ChatRequest, build_provider};
use anyhow::{Context, Result};
use tracing::info;

use crate::cli::LlmTestArgs;
use crate::config::Config;

/// Run the `llm-test` subcommand.
///
/// # Errors
/// Returns an error if the provider cannot be constructed, the env
/// lacks the required keys, or the HTTP call fails.
pub async fn run(config: &Config, args: LlmTestArgs) -> Result<()> {
    let api_key_override = args
        .api_key
        .filter(|s| !s.is_empty())
        .map(secrecy::SecretString::from);
    let provider_config = config
        .llm_test_provider_config(&args.provider, args.model, args.base_url, api_key_override)
        .context("resolving LLM provider")?;
    let client = build_provider(provider_config).context("building LLM provider")?;
    info!(
        provider = client.name(),
        model = client.model(),
        "sending prompt",
    );
    let resp = client
        .complete(ChatRequest::user_prompt(args.prompt))
        .await
        .context("calling provider")?;

    println!("--- model: {} ---", resp.model);
    if let Some(u) = resp.usage {
        println!(
            "--- usage: in={} out={} ---",
            u.input_tokens, u.output_tokens
        );
    }
    println!("{}", resp.text);
    Ok(())
}
