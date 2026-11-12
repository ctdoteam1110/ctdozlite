//! Unified LLM provider: OpenAI-compatible chat API + model discovery.
//!
//! Resolution order (first hit wins):
//! 1. Explicit overrides (GUI settings / `--model` triple).
//! 2. opencode config (`~/.config/opencode/opencode.json`): `model: "provider/id"`
//!    resolved against `provider.<name>.options.baseURL`.
//! 3. Ollama local default (`http://127.0.0.1:11434/v1`, model `llama3.1`).
//!
//! Auth note: opencode's own `auth.json` (OAuth/device flow of the Bun binary)
//! is intentionally NOT read — copy a plain API key via env or `zlite --auth set`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Resolved endpoint a provider call needs. No secrets stored here by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmEndpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Where this came from: `settings` | `opencode:<ref>` | `ollama-default` | `manual`.
    #[serde(default)]
    pub source: String,
}

impl LlmEndpoint {
    pub fn is_stub(&self) -> bool {
        self.base_url.is_empty() || self.model.is_empty()
    }

    pub fn with_source(mut self, s: &str) -> Self {
        self.source = s.to_string();
        self
    }
}

/// Parse `model: "provider/id"` from opencode.json. Returns (provider, id).
pub fn parse_opencode_model(s: &str) -> Option<(String, String)> {
    let (p, m) = s.split_once('/')?;
    if p.trim().is_empty() || m.trim().is_empty() {
        return None;
    }
    Some((p.trim().to_string(), m.trim().to_string()))
}

fn opencode_config_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| {
        PathBuf::from(h)
            .join(".config")
            .join("opencode")
            .join("opencode.json")
    })
}

/// opencode config subset we read.
#[derive(Debug, Default, Deserialize)]
struct OpencodeConfig {
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider: std::collections::HashMap<String, OpencodeProvider>,
}

#[derive(Debug, Default, Deserialize)]
struct OpencodeProvider {
    #[serde(default)]
    options: OpencodeOptions,
    /// omp format: `models: { "model-id": { "name": ... } }`.
    #[serde(default)]
    models: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct OpencodeOptions {
    #[serde(default, rename = "baseURL")]
    base_url: String,
    // apiKey placeholder (e.g. ___NAME_API_KEY___) intentionally ignored:
    // real keys come from env/keychain/settings, never from opencode.json.
}

/// Resolve the endpoint. `overrides`: (base_url, api_key, model) — empty = unset.
pub fn resolve_endpoint(overrides: (&str, &str, &str)) -> LlmEndpoint {
    let (o_url, o_key, o_model) = overrides;
    if !o_url.trim().is_empty() && !o_model.trim().is_empty() {
        return LlmEndpoint {
            base_url: o_url.trim().trim_end_matches('/').to_string(),
            api_key: o_key.trim().to_string(),
            model: o_model.trim().to_string(),
            source: "settings".into(),
        };
    }
    if let Some((mref, url)) = opencode_model_info() {
        if let Some((_prov, id)) = parse_opencode_model(&mref) {
            return LlmEndpoint {
                base_url: url,
                api_key: o_key.trim().to_string(),
                model: if !o_model.trim().is_empty() {
                    o_model.trim().to_string()
                } else {
                    id
                },
                source: format!("opencode:{mref}"),
            };
        }
    }
    LlmEndpoint {
        base_url: "http://127.0.0.1:11434/v1".into(),
        api_key: "ollama".into(),
        model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1".into()),
        source: "ollama-default".into(),
    }
}

/// One OpenAI-compatible `/chat/completions` call (async core).
pub async fn chat_completions(
    ep: &LlmEndpoint,
    system: &str,
    user: &str,
) -> anyhow::Result<String> {
    if ep.is_stub() {
        anyhow::bail!("chưa cấu hình LLM");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let mut req = client
        .post(format!("{}/chat/completions", ep.base_url))
        .json(&serde_json::json!({
            "model": ep.model,
            "stream": false,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }));
    if !ep.api_key.is_empty() {
        req = req.bearer_auth(&ep.api_key);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let short: String = body.chars().take(200).collect();
        anyhow::bail!("LLM HTTP {code}: {short}");
    }
    let v: serde_json::Value = resp.json().await?;
    v.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("LLM: missing choices[0].message.content"))
}

/// Blocking wrapper for the sync `LlmProvider` trait. Must be called
/// outside an async runtime (GUI/TUI worker threads).
pub fn chat_completions_blocking(ep: &LlmEndpoint, system: &str, user: &str) -> anyhow::Result<String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        anyhow::bail!("chat_completions_blocking gọi trong async runtime — dùng chat_completions().await");
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(chat_completions(ep, system, user))
}

/// Discover models live from a server: OpenAI `GET /models` first,
/// Ollama-native `/api/tags` fallback (same host, strip trailing `/v1`).
pub async fn fetch_models(base_url: &str, api_key: &str) -> Vec<String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Vec::new();
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let get = |url: String| {
        let mut r = client.get(url);
        if !api_key.trim().is_empty() {
            r = r.bearer_auth(api_key.trim());
        }
        r
    };
    if let Ok(resp) = get(format!("{base}/models")).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let mut ids: Vec<String> = v
                    .get("data")
                    .and_then(|d| d.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|m| {
                                m.get("id").and_then(|i| i.as_str()).map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ids.sort();
                ids.dedup();
                if !ids.is_empty() {
                    return ids;
                }
            }
        }
    }
    let host = base.strip_suffix("/v1").unwrap_or(base);
    if let Ok(resp) = get(format!("{host}/api/tags")).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let mut ids: Vec<String> = v
                    .get("models")
                    .and_then(|d| d.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|m| {
                                m.get("name").and_then(|i| i.as_str()).map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ids.sort();
                ids.dedup();
                return ids;
            }
        }
    }
    Vec::new()
}

/// Known OpenAI-compatible vendors (baseURL + env key + starter models).
/// omp users paste these into opencode.json; zlite reads them from there.
/// Registry here is only a shortcut for `/model add` and GUI hints.
pub struct VendorDef {
    pub name: &'static str,
    pub base_url: &'static str,
    pub env_key: &'static str,
    pub models: &'static [&'static str],
}

pub const VENDOR_REGISTRY: &[VendorDef] = &[
    VendorDef {
        name: "opencode-go",
        base_url: "http://127.0.0.1:8045/v1",
        env_key: "OPENCODE_GO_API_KEY",
        models: &["claude-opus-4-6", "claude-sonnet-4-6"],
    },
    VendorDef {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        env_key: "DEEPSEEK_API_KEY",
        models: &["deepseek-chat", "deepseek-reasoner"],
    },
    VendorDef {
        name: "minimax",
        base_url: "https://api.minimax.io/v1",
        env_key: "MINIMAX_API_KEY",
        models: &["MiniMax-M2.1", "MiniMax-M2"],
    },
    VendorDef {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        env_key: "OPENAI_API_KEY",
        models: &["gpt-5.2", "gpt-4.1"],
    },
    VendorDef {
        name: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        env_key: "ANTHROPIC_API_KEY",
        models: &["claude-opus-4-6", "claude-sonnet-4-5"],
    },
    VendorDef {
        name: "google",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        env_key: "GOOGLE_API_KEY",
        models: &["gemini-2.5-flash", "gemini-2.5-pro"],
    },
    VendorDef {
        name: "qwen",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        env_key: "QWEN_API_KEY",
        models: &["qwen-max", "qwen-plus"],
    },
    VendorDef {
        name: "moonshot",
        base_url: "https://api.moonshot.ai/v1",
        env_key: "MOONSHOT_API_KEY",
        models: &["kimi-k2", "kimi-k2-thinking"],
    },
    VendorDef {
        name: "zhipu",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        env_key: "ZHIPU_API_KEY",
        models: &["glm-4.6", "glm-4.5"],
    },
    VendorDef {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        env_key: "GROQ_API_KEY",
        models: &["llama-3.3-70b-versatile", "moonshotai/kimi-k2-instruct"],
    },
    VendorDef {
        name: "together",
        base_url: "https://api.together.xyz/v1",
        env_key: "TOGETHER_API_KEY",
        models: &["meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8", "Qwen/Qwen3-235B-A22B-fp8"],
    },
    VendorDef {
        name: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        env_key: "FIREWORKS_API_KEY",
        models: &["accounts/fireworks/models/kimi-k2-instruct", "accounts/fireworks/models/glm-4-6"],
    },
    VendorDef {
        name: "deepinfra",
        base_url: "https://api.deepinfra.com/v1/openai",
        env_key: "DEEPINFRA_API_TOKEN",
        models: &["Qwen/Qwen3-235B-A22B", "moonshotai/Kimi-K2-Instruct"],
    },
    VendorDef {
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        env_key: "OPENROUTER_API_KEY",
        models: &["qwen/qwen3-max", "moonshotai/kimi-k2"],
    },
    VendorDef {
        name: "cerebras",
        base_url: "https://api.cerebras.ai/v1",
        env_key: "CEREBRAS_API_KEY",
        models: &["llama-3.3-70b", "qwen-3-32b"],
    },
    VendorDef {
        name: "novita",
        base_url: "https://api.novita.ai/openai/v1",
        env_key: "NOVITA_API_KEY",
        models: &["qwen/qwen3-max", "moonshotai/kimi-k2-instruct"],
    },
    VendorDef {
        name: "siliconflow",
        base_url: "https://api.siliconflow.com/v1",
        env_key: "SILICONFLOW_API_KEY",
        models: &["Qwen/Qwen3-235B-A22B", "moonshotai/Kimi-K2-Instruct"],
    },
    VendorDef {
        name: "xai",
        base_url: "https://api.x.ai/v1",
        env_key: "XAI_API_KEY",
        models: &["grok-4", "grok-3-mini"],
    },
    VendorDef {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        env_key: "MISTRAL_API_KEY",
        models: &["mistral-large-latest", "mistral-medium-latest"],
    },
    VendorDef {
        name: "volcengine",
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        env_key: "VOLCENGINE_API_KEY",
        models: &["doubao-seed-1-6", "doubao-seed-1-6-thinking"],
    },
    VendorDef {
        name: "stepfun",
        base_url: "https://api.stepfun.com/v1",
        env_key: "STEPFUN_API_KEY",
        models: &["step-3", "step-2-mini"],
    },
    VendorDef {
        name: "baidu",
        base_url: "https://qianfan.baidubce.com/v2",
        env_key: "BAIDU_API_KEY",
        models: &["ernie-4.5-turbo", "ernie-x1-turbo"],
    },
    VendorDef {
        name: "tencent",
        base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        env_key: "TENCENT_API_KEY",
        models: &["hunyuan-turbos-latest", "hunyuan-t1-latest"],
    },
    VendorDef {
        name: "iflytek",
        base_url: "https://spark-api.open.xf-yun.com/v1",
        env_key: "IFLYTEK_API_KEY",
        models: &["spark-4.0-ultra", "spark-x1"],
    },
    VendorDef {
        name: "internlm",
        base_url: "https://internlm-chat.intern-ai.org.cn/api/v1/chat/completions",
        env_key: "INTERNLM_API_KEY",
        models: &["internlm3-latest"],
    },
];

/// Look up a vendor by name (case-insensitive).
pub fn vendor_def(name: &str) -> Option<&'static VendorDef> {
    VENDOR_REGISTRY
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(name.trim()))
}

/// All selectable models: opencode.json first, then vendor starters.
/// Returns (display `provider/id`, base_url, id). No hardcoded-only list:
/// opencode.json always wins on conflict.
pub fn list_models() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Some(path) = opencode_config_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<OpencodeConfig>(&text) {
                let mut provs: Vec<(&String, &OpencodeProvider)> =
                    cfg.provider.iter().collect();
                provs.sort_by_key(|(n, _)| n.as_str());
                for (prov, p) in provs {
                    let url = p.options.base_url.trim().trim_end_matches('/').to_string();
                    if url.is_empty() {
                        continue;
                    }
                    let mut ids: Vec<&String> = p.models.keys().collect();
                    ids.sort();
                    for id in ids {
                        out.push((format!("{prov}/{id}"), url.clone(), id.clone()));
                    }
                }
            }
        }
    }
    for v in VENDOR_REGISTRY {
        for m in v.models {
            let disp = format!("{}/{}", v.name, m);
            if !out.iter().any(|(d, _, _)| d == &disp) {
                out.push((disp, v.base_url.to_string(), m.to_string()));
            }
        }
    }
    out
}

/// Env key convention for a provider.
pub fn env_key_for_provider(prov: &str) -> Option<String> {
    let p = prov.trim();
    if p.is_empty() || p == "ollama" {
        return None;
    }
    if let Some(v) = vendor_def(p) {
        return Some(v.env_key.to_string());
    }
    Some(format!("{}_API_KEY", p.to_uppercase().replace('-', "_")))
}

/// Sync model info from opencode config: returns (provider/model, base_url).
pub fn opencode_model_info() -> Option<(String, String)> {
    let path = opencode_config_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let cfg: OpencodeConfig = serde_json::from_str(&text).ok()?;
    let (prov, id) = parse_opencode_model(&cfg.model)?;
    let url = cfg.provider.get(&prov)?.options.base_url.clone();
    if url.is_empty() {
        return None;
    }
    Some((format!("{prov}/{id}"), url.trim_end_matches('/').to_string()))
}

/// Provider glue: builds prompts from [`crate::group_llm::GroupContext`]
/// and answers through the resolved endpoint.
pub struct UnifiedProvider {
    pub ep: LlmEndpoint,
}

impl UnifiedProvider {
    /// Resolve from settings triple (empty strings = unset → fall through).
    /// Key order: env → OS keychain → settings field.
    pub fn from_settings(base_url: &str, api_key: &str, model: &str) -> Self {
        let env_url = std::env::var("CTDOZ_BASE_URL").unwrap_or_default();
        let env_model = std::env::var("CTDOZ_MODEL").unwrap_or_default();
        let url = if !env_url.trim().is_empty() {
            env_url
        } else {
            base_url.trim().to_string()
        };
        let model_v = if !env_model.trim().is_empty() {
            env_model
        } else {
            model.trim().to_string()
        };
        let (key, _) = crate::auth::resolve_key(api_key);
        let ep = resolve_endpoint((&url, &key, &model_v));
        Self { ep }
    }

    /// Direct constructor (session override, key already resolved).
    pub fn from_parts(base_url: &str, api_key: &str, model: &str, source: &str) -> Self {
        Self {
            ep: resolve_endpoint((base_url, api_key, model)).with_source(source),
        }
    }

    pub fn sys_and_user(&self, ctx: &crate::group_llm::GroupContext, q: Option<&str>) -> (String, String) {
        let system = "Bạn là trợ lý tóm tắt và hỏi đáp hội thoại Zalo. Trả lời ngắn gọn, tiếng Việt.".to_string();
        let mut u = format!(
            "Hội thoại: {}\nGhi chú: {}\nThống kê: {}\n",
            ctx.thread_name, ctx.note, ctx.stats_summary
        );
        for m in ctx.recent_messages.iter().rev().take(15) {
            u.push_str(&format!("- {}: {}\n", m.sender, m.text));
        }
        if let Some(q) = q {
            u.push_str(&format!("\nCâu hỏi: {q}"));
        } else {
            u.push_str("\nTóm tắt hội thoại trên trong 5-8 dòng, nêu việc cần theo dõi.");
        }
        (system, u)
    }
}

impl crate::group_llm::LlmProvider for UnifiedProvider {
    fn summarize(&self, ctx: &crate::group_llm::GroupContext) -> anyhow::Result<String> {
        let (s, u) = self.sys_and_user(ctx, None);
        chat_completions_blocking(&self.ep, &s, &u)
    }

    fn ask(&self, ctx: &crate::group_llm::GroupContext, question: &str) -> anyhow::Result<String> {
        let (s, u) = self.sys_and_user(ctx, Some(question));
        chat_completions_blocking(&self.ep, &s, &u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_splits_provider_and_id() {
        assert_eq!(
            parse_opencode_model("ollama/qwen3.5:4b"),
            Some(("ollama".into(), "qwen3.5:4b".into()))
        );
        assert_eq!(
            parse_opencode_model("opencode-go/claude-opus-4-6"),
            Some(("opencode-go".into(), "claude-opus-4-6".into()))
        );
        assert!(parse_opencode_model("nonslash").is_none());
        assert!(parse_opencode_model("/empty").is_none());
    }

    #[test]
    fn explicit_overrides_win() {
        let ep = resolve_endpoint(("https://x.ai/v1", "k", "m"));
        assert_eq!((ep.base_url.as_str(), ep.model.as_str(), ep.source.as_str()), ("https://x.ai/v1", "m", "settings"));
    }

    #[test]
    fn vendor_registry_covers_common_providers() {
        assert!(vendor_def("deepseek").is_some());
        assert!(vendor_def("minimax").is_some());
        assert!(vendor_def("opencode-go").is_some());
        assert_eq!(env_key_for_provider("minimax").as_deref(), Some("MINIMAX_API_KEY"));
        assert_eq!(env_key_for_provider("opencode-go").as_deref(), Some("OPENCODE_GO_API_KEY"));
        let all = list_models();
        assert!(all.iter().any(|(d, _, _)| d.starts_with("ollama/")));
        assert!(all.iter().any(|(d, _, _)| d.starts_with("opencode-go/")));
    }

    #[test]
    fn opencode_info_reads_live_config() {
        let info = opencode_model_info();
        if let Some((m, u)) = info {
            assert!(m.contains('/'), "model ref has provider/");
            assert!(u.starts_with("http"), "base url looks http");
        }
    }
}
