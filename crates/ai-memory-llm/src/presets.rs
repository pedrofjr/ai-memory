//! Named OpenAI-compatible provider presets.
//!
//! Most multi-provider catalogs (oh-my-pi, OpenRouter, local engines) speak
//! Chat Completions with a fixed base URL and API key env var. Rather than
//! one Rust client per brand name, operators set
//! `AI_MEMORY_LLM_PROVIDER=<preset>` and we fill `base_url` + auth for the
//! existing [`crate::OpenAiCompatProvider`].
//!
//! Base URLs and env names are adapted from the oh-my-pi model catalog
//! (`packages/catalog/src/models.json`, MIT — see root `NOTICE`) and common
//! vendor docs. Only **openai-completions** hosts land here;
//! Anthropic-messages-only gateways (e.g. `zai` anthropic path) stay out
//! until a native client exists.

/// One OpenAI-compatible hosted or local endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatPreset {
    /// Canonical name used in `AI_MEMORY_LLM_PROVIDER`.
    pub name: &'static str,
    /// Extra accepted spellings (underscores, short aliases).
    pub aliases: &'static [&'static str],
    /// Default OpenAI-compatible base URL (no trailing slash required).
    pub base_url: &'static str,
    /// Primary API-key environment variable.
    pub env_var: &'static str,
    /// Model used when `AI_MEMORY_LLM_MODEL` is unset.
    pub default_model: &'static str,
    /// When true, construction fails without a key; local engines use false.
    pub require_api_key: bool,
}

/// All built-in OpenAI-compatible presets.
pub const COMPAT_PRESETS: &[CompatPreset] = &[
    CompatPreset {
        name: "openrouter",
        aliases: &[],
        base_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
        default_model: "openai/gpt-4o-mini",
        require_api_key: true,
    },
    // `xai` / `grok` are first-class Responses clients (not Chat Completions).
    CompatPreset {
        name: "groq",
        aliases: &[],
        base_url: "https://api.groq.com/openai/v1",
        env_var: "GROQ_API_KEY",
        default_model: "llama-3.3-70b-versatile",
        require_api_key: true,
    },
    CompatPreset {
        name: "mistral",
        aliases: &[],
        base_url: "https://api.mistral.ai/v1",
        env_var: "MISTRAL_API_KEY",
        default_model: "mistral-small-latest",
        require_api_key: true,
    },
    CompatPreset {
        name: "deepseek",
        aliases: &[],
        base_url: "https://api.deepseek.com",
        env_var: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
        require_api_key: true,
    },
    CompatPreset {
        name: "together",
        aliases: &["together-ai", "together_ai"],
        base_url: "https://api.together.xyz/v1",
        env_var: "TOGETHER_API_KEY",
        default_model: "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
        require_api_key: true,
    },
    CompatPreset {
        name: "fireworks",
        aliases: &[],
        base_url: "https://api.fireworks.ai/inference/v1",
        env_var: "FIREWORKS_API_KEY",
        default_model: "accounts/fireworks/models/llama-v3p1-8b-instruct",
        require_api_key: true,
    },
    CompatPreset {
        name: "cerebras",
        aliases: &[],
        base_url: "https://api.cerebras.ai/v1",
        env_var: "CEREBRAS_API_KEY",
        default_model: "llama3.1-8b",
        require_api_key: true,
    },
    CompatPreset {
        name: "huggingface",
        aliases: &["hf", "hugging-face", "hugging_face"],
        base_url: "https://router.huggingface.co/v1",
        env_var: "HF_TOKEN",
        default_model: "meta-llama/Meta-Llama-3.1-8B-Instruct",
        require_api_key: true,
    },
    CompatPreset {
        name: "nvidia",
        aliases: &["nim", "nvidia-nim", "nvidia_nim"],
        base_url: "https://integrate.api.nvidia.com/v1",
        env_var: "NVIDIA_API_KEY",
        default_model: "meta/llama-3.1-8b-instruct",
        require_api_key: true,
    },
    CompatPreset {
        name: "moonshot",
        aliases: &["kimi"],
        base_url: "https://api.moonshot.ai/v1",
        env_var: "MOONSHOT_API_KEY",
        default_model: "kimi-k2-0711-preview",
        require_api_key: true,
    },
    CompatPreset {
        name: "minimax-code",
        aliases: &["minimax_code", "minimax-coding", "minimax"],
        base_url: "https://api.minimax.io/v1",
        env_var: "MINIMAX_API_KEY",
        default_model: "MiniMax-M2",
        require_api_key: true,
    },
    CompatPreset {
        name: "minimax-code-cn",
        aliases: &["minimax_code_cn", "minimax-cn"],
        base_url: "https://api.minimaxi.com/v1",
        env_var: "MINIMAX_API_KEY",
        default_model: "MiniMax-M2",
        require_api_key: true,
    },
    CompatPreset {
        name: "kimi-code",
        aliases: &["kimi_code"],
        base_url: "https://api.kimi.com/coding/v1",
        env_var: "KIMI_API_KEY",
        default_model: "kimi-for-coding",
        require_api_key: true,
    },
    CompatPreset {
        name: "novita",
        aliases: &[],
        base_url: "https://api.novita.ai/openai/v1",
        env_var: "NOVITA_API_KEY",
        default_model: "meta-llama/llama-3.1-8b-instruct",
        require_api_key: true,
    },
    CompatPreset {
        name: "venice",
        aliases: &[],
        base_url: "https://api.venice.ai/api/v1",
        env_var: "VENICE_API_KEY",
        default_model: "llama-3.3-70b",
        require_api_key: true,
    },
    CompatPreset {
        name: "nanogpt",
        aliases: &["nano-gpt", "nano_gpt"],
        base_url: "https://nano-gpt.com/api/v1",
        env_var: "NANOGPT_API_KEY",
        default_model: "chatgpt-4o-latest",
        require_api_key: true,
    },
    CompatPreset {
        name: "baseten",
        aliases: &[],
        base_url: "https://inference.baseten.co/v1",
        env_var: "BASETEN_API_KEY",
        default_model: "deepseek-ai/DeepSeek-V3",
        require_api_key: true,
    },
    CompatPreset {
        name: "kilo",
        aliases: &[],
        base_url: "https://api.kilo.ai/api/gateway",
        env_var: "KILO_API_KEY",
        default_model: "anthropic/claude-sonnet-4",
        require_api_key: true,
    },
    CompatPreset {
        name: "alibaba-coding-plan",
        aliases: &["alibaba_coding_plan", "dashscope-coding"],
        base_url: "https://coding-intl.dashscope.aliyuncs.com/v1",
        env_var: "DASHSCOPE_API_KEY",
        default_model: "qwen3-coder-plus",
        require_api_key: true,
    },
    CompatPreset {
        name: "zhipu-coding-plan",
        aliases: &["zhipu_coding_plan", "glm-coding", "glm_coding"],
        base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
        env_var: "ZHIPU_API_KEY",
        default_model: "glm-4.5",
        require_api_key: true,
    },
    CompatPreset {
        name: "qianfan",
        aliases: &[],
        base_url: "https://qianfan.baidubce.com/v2",
        env_var: "QIANFAN_API_KEY",
        default_model: "deepseek-v3",
        require_api_key: true,
    },
    CompatPreset {
        name: "qwen-portal",
        aliases: &["qwen_portal", "qwen"],
        base_url: "https://portal.qwen.ai/v1",
        env_var: "QWEN_API_KEY",
        default_model: "coder-model",
        require_api_key: true,
    },
    CompatPreset {
        name: "synthetic",
        aliases: &[],
        base_url: "https://api.synthetic.new/openai/v1",
        env_var: "SYNTHETIC_API_KEY",
        default_model: "hf:meta-llama/Meta-Llama-3.1-8B-Instruct",
        require_api_key: true,
    },
    CompatPreset {
        name: "wafer-serverless",
        aliases: &["wafer_serverless", "wafer"],
        base_url: "https://pass.wafer.ai/v1",
        env_var: "WAFER_API_KEY",
        default_model: "deepseek-v3",
        require_api_key: true,
    },
    CompatPreset {
        name: "xiaomi",
        aliases: &["mimo", "xiaomi-mimo"],
        base_url: "https://api.xiaomimimo.com/v1",
        env_var: "XIAOMI_API_KEY",
        default_model: "mimo-v2-flash",
        require_api_key: true,
    },
    // Local / self-hosted (key optional).
    CompatPreset {
        name: "ollama",
        aliases: &[],
        base_url: "http://127.0.0.1:11434/v1",
        env_var: "OLLAMA_API_KEY",
        default_model: "llama3.1:8b",
        require_api_key: false,
    },
    CompatPreset {
        name: "lm-studio",
        aliases: &["lm_studio", "lmstudio"],
        base_url: "http://127.0.0.1:1234/v1",
        env_var: "LM_STUDIO_API_KEY",
        default_model: "local-model",
        require_api_key: false,
    },
    CompatPreset {
        name: "vllm",
        aliases: &[],
        base_url: "http://127.0.0.1:8000/v1",
        env_var: "VLLM_API_KEY",
        default_model: "local-model",
        require_api_key: false,
    },
    CompatPreset {
        name: "llama-cpp",
        aliases: &["llama_cpp", "llamacpp"],
        base_url: "http://127.0.0.1:8080/v1",
        env_var: "LLAMA_CPP_API_KEY",
        default_model: "local-model",
        require_api_key: false,
    },
];

/// Look up a preset by canonical name or alias (case-sensitive, as with
/// existing `AI_MEMORY_LLM_PROVIDER` values).
#[must_use]
pub fn lookup_compat_preset(raw: &str) -> Option<&'static CompatPreset> {
    let key = raw.trim();
    if key.is_empty() {
        return None;
    }
    COMPAT_PRESETS.iter().find(|p| {
        p.name.eq_ignore_ascii_case(key) || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(key))
    })
}

/// Unique env var names referenced by presets (for one-shot env capture).
#[must_use]
pub fn preset_api_key_env_vars() -> Vec<&'static str> {
    let mut vars: Vec<&'static str> = COMPAT_PRESETS.iter().map(|p| p.env_var).collect();
    vars.sort_unstable();
    vars.dedup();
    vars
}

/// Human-readable list of preset names for error messages.
#[must_use]
pub fn compat_preset_names_csv() -> String {
    COMPAT_PRESETS
        .iter()
        .map(|p| p.name)
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_preset_name_is_unique() {
        let mut seen = HashSet::new();
        for p in COMPAT_PRESETS {
            assert!(seen.insert(p.name), "duplicate preset name {}", p.name);
        }
    }

    #[test]
    fn aliases_do_not_collide_with_other_canonical_names() {
        let names: HashSet<_> = COMPAT_PRESETS.iter().map(|p| p.name).collect();
        for p in COMPAT_PRESETS {
            for alias in p.aliases {
                assert!(
                    !names.contains(alias),
                    "alias {alias} collides with a canonical name"
                );
            }
        }
    }

    #[test]
    fn lookup_accepts_aliases_and_case_folding() {
        assert_eq!(
            lookup_compat_preset("OpenRouter").map(|p| p.name),
            Some("openrouter")
        );
        assert_eq!(
            lookup_compat_preset("lmstudio").map(|p| p.name),
            Some("lm-studio")
        );
        assert!(lookup_compat_preset("not-a-real-provider").is_none());
    }

    #[test]
    fn local_presets_do_not_require_api_keys() {
        for name in ["ollama", "lm-studio", "vllm", "llama-cpp"] {
            let p = lookup_compat_preset(name).expect(name);
            assert!(!p.require_api_key, "{name} should be optional-key");
        }
    }
}
