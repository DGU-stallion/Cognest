//! Provider 路由器 — 根据用户配置将 Agent 请求路由到对应 LLM Provider

use std::collections::HashMap;

use rig::providers::openai;

use super::AgentError;
use crate::core::settings::{AgentRouting, AppSettings, ProviderConfig, ProviderType, SettingsManager};

/// 支持的 Provider — 全部使用 rig openai::CompletionsClient（DeepSeek/Ollama 兼容 OpenAI Chat Completions API）
#[derive(Debug, Clone)]
pub enum RigProvider {
    DeepSeek { client: openai::CompletionsClient, model: String },
    OpenAI { client: openai::CompletionsClient, model: String },
    Anthropic { client: openai::CompletionsClient, model: String },
    Ollama { client: openai::CompletionsClient, model: String },
}

impl RigProvider {
    /// 获取该 Provider 持有的 CompletionsClient 引用
    pub fn client(&self) -> &openai::CompletionsClient {
        match self {
            Self::DeepSeek { client, .. } => client,
            Self::OpenAI { client, .. } => client,
            Self::Anthropic { client, .. } => client,
            Self::Ollama { client, .. } => client,
        }
    }

    /// 获取该 Provider 配置的模型名
    pub fn model(&self) -> &str {
        match self {
            Self::DeepSeek { model, .. } => model,
            Self::OpenAI { model, .. } => model,
            Self::Anthropic { model, .. } => model,
            Self::Ollama { model, .. } => model,
        }
    }
}

/// Provider 路由器 — 管理所有已构建的 Provider Client 并按路由规则分配给 Agent
pub struct ProviderRouter {
    /// provider_id → RigProvider
    providers: HashMap<String, RigProvider>,
    /// Agent 路由配置
    routing: AgentRouting,
}

impl ProviderRouter {
    /// 从 AppSettings 构建所有 Provider Client
    ///
    /// 跳过不可用的 Provider（disabled 或缺少 API key），不阻塞其他 Provider 的构建。
    pub fn from_config(settings: &AppSettings, settings_mgr: &SettingsManager) -> Result<Self, AgentError> {
        let mut providers = HashMap::new();

        for config in &settings.providers {
            if !config.enabled {
                continue;
            }

            match Self::build_provider(config, settings_mgr) {
                Ok(provider) => {
                    providers.insert(config.id.clone(), provider);
                }
                Err(e) => {
                    log::warn!("跳过 Provider '{}': {}", config.id, e);
                }
            }
        }

        Ok(Self {
            providers,
            routing: settings.routing.clone(),
        })
    }

    /// 按 overrides map 解析 Agent 到 Provider
    ///
    /// 查找顺序：overrides[agent_name] → defaultProvider
    /// 如果解析到的 provider_id 在 providers map 中不存在，返回 NoProvider 错误。
    pub fn resolve(&self, agent_name: &str) -> Result<&RigProvider, AgentError> {
        let provider_id = self.resolve_provider_id(agent_name)?;

        self.providers.get(&provider_id).ok_or(AgentError::NoProvider)
    }

    /// Provider 回退：指定 Provider 不可用时回退到 defaultProvider
    ///
    /// 如果 overrides 指定的 Provider 在 providers map 中不存在（被跳过或 disabled），
    /// 则回退到 defaultProvider。如果 defaultProvider 也不可用，返回 NoProvider。
    pub fn resolve_with_fallback(&self, agent_name: &str) -> Result<&RigProvider, AgentError> {
        // 先尝试 overrides 指定的 provider
        if let Some(override_id) = self.routing.overrides.get(agent_name) {
            if let Some(provider) = self.providers.get(override_id) {
                return Ok(provider);
            }
            // override 指定的 provider 不可用，尝试回退
            log::info!(
                "Provider 回退: agent '{}' 的 override '{}' 不可用，回退到 defaultProvider",
                agent_name,
                override_id
            );
        }

        // 回退到 defaultProvider
        let default_id = self.routing.default_provider.as_ref().ok_or(AgentError::NoProvider)?;
        self.providers.get(default_id).ok_or(AgentError::NoProvider)
    }

    /// 获取已构建的 providers map（用于调试/状态查询）
    pub fn providers(&self) -> &HashMap<String, RigProvider> {
        &self.providers
    }

    // ─── Private ────────────────────────────────────────────────────────────

    /// 解析 agent_name 对应的 provider_id（不检查是否可用）
    fn resolve_provider_id(&self, agent_name: &str) -> Result<String, AgentError> {
        if let Some(id) = self.routing.overrides.get(agent_name) {
            return Ok(id.clone());
        }
        self.routing
            .default_provider
            .clone()
            .ok_or(AgentError::NoProvider)
    }

    /// 根据 ProviderConfig 构建单个 RigProvider
    fn build_provider(
        config: &ProviderConfig,
        settings_mgr: &SettingsManager,
    ) -> Result<RigProvider, AgentError> {
        let api_key = Self::get_api_key_for_provider(config, settings_mgr)?;

        let client = openai::CompletionsClient::builder()
            .api_key(api_key)
            .base_url(&config.endpoint)
            .build()
            .map_err(|e| AgentError::LlmFailure(format!("构建 Client 失败: {}", e)))?;

        let model = config.model.clone();

        let provider = match config.provider_type {
            ProviderType::DeepSeek => RigProvider::DeepSeek { client, model },
            ProviderType::Ollama => RigProvider::Ollama { client, model },
            ProviderType::OpenAiCompat => RigProvider::OpenAI { client, model },
        };

        Ok(provider)
    }

    /// 获取 Provider 的 API key
    ///
    /// Ollama 不需要真实 API key，使用占位符。
    /// 其他 Provider 从 Keychain 获取，缺失则视为不可用。
    fn get_api_key_for_provider(
        config: &ProviderConfig,
        settings_mgr: &SettingsManager,
    ) -> Result<String, AgentError> {
        if config.provider_type == ProviderType::Ollama {
            // Ollama 不需要 API key，使用占位符
            return Ok("ollama".to_string());
        }

        settings_mgr
            .get_api_key(&config.id)
            .map_err(|e| AgentError::LlmFailure(format!("Keychain 访问失败: {}", e)))?
            .ok_or_else(|| AgentError::LlmFailure(format!(
                "Provider '{}' 缺少 API key",
                config.id
            )))
    }
}

impl ProviderRouter {
    /// 测试专用构造器 — 直接注入 providers map 和 routing 配置，跳过 Client 构建
    #[cfg(test)]
    pub(crate) fn new_for_test(
        providers: HashMap<String, RigProvider>,
        routing: AgentRouting,
    ) -> Self {
        Self { providers, routing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::{AgentRouting, AppSettings, ProviderConfig, ProviderType};

    /// 构造一个不访问 Keychain 的测试辅助 — 仅测试 Ollama（不需要 API key）
    fn ollama_config() -> ProviderConfig {
        ProviderConfig {
            id: "ollama-local".to_string(),
            name: "Ollama Local".to_string(),
            provider_type: ProviderType::Ollama,
            endpoint: "http://localhost:11434/v1".to_string(),
            model: "qwen2.5".to_string(),
            temperature: 0.7,
            enabled: true,
        }
    }

    fn disabled_config() -> ProviderConfig {
        ProviderConfig {
            id: "disabled-1".to_string(),
            name: "Disabled".to_string(),
            provider_type: ProviderType::Ollama,
            endpoint: "http://localhost:11434/v1".to_string(),
            model: "test".to_string(),
            temperature: 0.5,
            enabled: false,
        }
    }

    #[test]
    fn test_disabled_provider_is_skipped() {
        let settings = AppSettings {
            providers: vec![disabled_config()],
            routing: AgentRouting {
                default_provider: Some("disabled-1".to_string()),
                overrides: HashMap::new(),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();
        assert!(router.providers.is_empty());
    }

    #[test]
    fn test_ollama_provider_builds_without_keychain() {
        let settings = AppSettings {
            providers: vec![ollama_config()],
            routing: AgentRouting {
                default_provider: Some("ollama-local".to_string()),
                overrides: HashMap::new(),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();
        assert_eq!(router.providers.len(), 1);
        assert!(router.providers.contains_key("ollama-local"));
    }

    #[test]
    fn test_resolve_uses_default_provider() {
        let settings = AppSettings {
            providers: vec![ollama_config()],
            routing: AgentRouting {
                default_provider: Some("ollama-local".to_string()),
                overrides: HashMap::new(),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();
        let provider = router.resolve("writing").unwrap();
        assert_eq!(provider.model(), "qwen2.5");
    }

    #[test]
    fn test_resolve_uses_override() {
        let mut ollama2 = ollama_config();
        ollama2.id = "ollama-fast".to_string();
        ollama2.model = "llama3".to_string();

        let settings = AppSettings {
            providers: vec![ollama_config(), ollama2],
            routing: AgentRouting {
                default_provider: Some("ollama-local".to_string()),
                overrides: HashMap::from([("curator".to_string(), "ollama-fast".to_string())]),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();

        // curator 走 override
        let curator_provider = router.resolve("curator").unwrap();
        assert_eq!(curator_provider.model(), "llama3");

        // writing 走 default
        let writing_provider = router.resolve("writing").unwrap();
        assert_eq!(writing_provider.model(), "qwen2.5");
    }

    #[test]
    fn test_resolve_no_provider_returns_error() {
        let settings = AppSettings {
            providers: vec![],
            routing: AgentRouting {
                default_provider: None,
                overrides: HashMap::new(),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();
        let result = router.resolve("writing");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_with_fallback_when_override_unavailable() {
        // 只有 ollama-local 可用，curator 的 override 指向不存在的 provider
        let settings = AppSettings {
            providers: vec![ollama_config()],
            routing: AgentRouting {
                default_provider: Some("ollama-local".to_string()),
                overrides: HashMap::from([("curator".to_string(), "nonexistent".to_string())]),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();

        // resolve() 会失败因为 "nonexistent" 不在 providers map 中
        let result = router.resolve("curator");
        assert!(result.is_err());

        // resolve_with_fallback() 会回退到 defaultProvider
        let provider = router.resolve_with_fallback("curator").unwrap();
        assert_eq!(provider.model(), "qwen2.5");
    }

    #[test]
    fn test_resolve_with_fallback_no_default_returns_error() {
        let settings = AppSettings {
            providers: vec![ollama_config()],
            routing: AgentRouting {
                default_provider: None,
                overrides: HashMap::from([("curator".to_string(), "nonexistent".to_string())]),
            },
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let router = ProviderRouter::from_config(&settings, &mgr).unwrap();
        let result = router.resolve_with_fallback("curator");
        assert!(result.is_err());
    }
}

/// Property-based tests for ProviderRouter routing logic.
///
/// These tests construct `ProviderRouter` via `new_for_test` to test routing
/// resolution logic in isolation (no network, no keychain).
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::core::settings::AgentRouting;
    use proptest::prelude::*;
    use proptest::collection::hash_map;

    // ─── Generators ─────────────────────────────────────────────────────────

    /// Generate a valid identifier string (1-20 alphanumeric + hyphen chars)
    fn gen_id() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9\\-]{0,19}".prop_filter("non-empty", |s| !s.is_empty())
    }

    /// Build a dummy RigProvider (Ollama variant) with a known model name.
    /// We use a real `openai::CompletionsClient` pointed to localhost — it's never called.
    fn dummy_provider(model: &str) -> RigProvider {
        let client = openai::CompletionsClient::builder()
            .api_key("test")
            .base_url("http://127.0.0.1:1/v1")
            .build()
            .unwrap();
        RigProvider::Ollama {
            client,
            model: model.to_string(),
        }
    }

    // ─── Property 2: Provider 路由正确解析 ──────────────────────────────────

    proptest! {
        /// **Validates: Requirements 2.2, 2.3**
        ///
        /// Property 2: Provider 路由正确解析
        /// For any agent name with an override → resolve() returns the override provider.
        /// For any agent name without an override → resolve() returns the defaultProvider.
        #[test]
        fn prop_resolve_returns_override_or_default(
            default_id in gen_id(),
            override_entries in hash_map(gen_id(), gen_id(), 1..5),
            query_agent in gen_id(),
        ) {
            // Build providers map: include default + all override targets
            let mut providers = HashMap::new();
            providers.insert(default_id.clone(), dummy_provider(&format!("model-{}", default_id)));
            for (_, provider_id) in &override_entries {
                providers.insert(provider_id.clone(), dummy_provider(&format!("model-{}", provider_id)));
            }

            let routing = AgentRouting {
                default_provider: Some(default_id.clone()),
                overrides: override_entries.clone(),
            };

            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve(&query_agent);

            if let Some(expected_provider_id) = override_entries.get(&query_agent) {
                // Agent has an override → should resolve to that provider's model
                let provider = result.unwrap();
                prop_assert_eq!(provider.model(), format!("model-{}", expected_provider_id));
            } else {
                // No override → should resolve to defaultProvider's model
                let provider = result.unwrap();
                prop_assert_eq!(provider.model(), format!("model-{}", default_id));
            }
        }

        /// **Validates: Requirements 2.2, 2.3**
        ///
        /// Property 2 (supplemental): When no defaultProvider is set and agent has no override,
        /// resolve() returns NoProvider error.
        #[test]
        fn prop_resolve_no_default_no_override_returns_error(
            agent_name in gen_id(),
        ) {
            let providers = HashMap::new();
            let routing = AgentRouting {
                default_provider: None,
                overrides: HashMap::new(),
            };
            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve(&agent_name);
            prop_assert!(result.is_err());
        }
    }

    // ─── Property 3: Provider 回退逻辑 ──────────────────────────────────────

    proptest! {
        /// **Validates: Requirements 2.4, 2.5**
        ///
        /// Property 3: Provider 回退逻辑 — 目标 Provider 不可用时回退到 defaultProvider
        /// When override target is NOT in providers map (simulating unavailability),
        /// resolve_with_fallback() returns defaultProvider.
        #[test]
        fn prop_fallback_to_default_when_override_unavailable(
            default_id in gen_id(),
            agent_name in gen_id(),
            unavailable_override_id in gen_id(),
        ) {
            // Only include defaultProvider in providers map; override target is absent (unavailable)
            let mut providers = HashMap::new();
            providers.insert(default_id.clone(), dummy_provider(&format!("model-{}", default_id)));
            // Ensure override points to a provider NOT in providers map
            let override_id = format!("{}-missing", unavailable_override_id);

            let routing = AgentRouting {
                default_provider: Some(default_id.clone()),
                overrides: HashMap::from([(agent_name.clone(), override_id)]),
            };

            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve_with_fallback(&agent_name);
            let provider = result.unwrap();
            // Should have fallen back to default
            prop_assert_eq!(provider.model(), format!("model-{}", default_id));
        }

        /// **Validates: Requirements 2.4, 2.5**
        ///
        /// Property 3: Provider 回退逻辑 — 所有 Provider 均不可用时返回 NoProvider 错误
        /// When override target AND default are both absent from providers map,
        /// resolve_with_fallback() returns NoProvider error.
        #[test]
        fn prop_all_unavailable_returns_no_provider(
            agent_name in gen_id(),
            override_id in gen_id(),
            default_id in gen_id(),
        ) {
            // Empty providers map — nothing is available
            let providers = HashMap::new();

            let routing = AgentRouting {
                default_provider: Some(default_id),
                overrides: HashMap::from([(agent_name.clone(), override_id)]),
            };

            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve_with_fallback(&agent_name);
            prop_assert!(result.is_err());
        }

        /// **Validates: Requirements 2.4, 2.5**
        ///
        /// Property 3 (supplemental): When no defaultProvider is configured and
        /// override is unavailable, resolve_with_fallback() returns NoProvider.
        #[test]
        fn prop_no_default_provider_configured_returns_no_provider(
            agent_name in gen_id(),
            override_id in gen_id(),
        ) {
            let providers = HashMap::new();
            let routing = AgentRouting {
                default_provider: None,
                overrides: HashMap::from([(agent_name.clone(), override_id)]),
            };

            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve_with_fallback(&agent_name);
            prop_assert!(result.is_err());
        }

        /// **Validates: Requirements 2.4**
        ///
        /// Property 3 (supplemental): When override IS available,
        /// resolve_with_fallback() returns the override provider (no fallback needed).
        #[test]
        fn prop_fallback_uses_override_when_available(
            default_id in gen_id(),
            agent_name in gen_id(),
            override_id in gen_id(),
        ) {
            // Include both default and override providers
            let mut providers = HashMap::new();
            providers.insert(default_id.clone(), dummy_provider(&format!("model-{}", default_id)));
            providers.insert(override_id.clone(), dummy_provider(&format!("model-{}", override_id)));

            let routing = AgentRouting {
                default_provider: Some(default_id.clone()),
                overrides: HashMap::from([(agent_name.clone(), override_id.clone())]),
            };

            let router = ProviderRouter::new_for_test(providers, routing);

            let result = router.resolve_with_fallback(&agent_name);
            let provider = result.unwrap();
            // Should use the override, not fall back
            prop_assert_eq!(provider.model(), format!("model-{}", override_id));
        }
    }
}
