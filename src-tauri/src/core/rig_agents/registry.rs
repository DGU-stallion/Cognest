//! Agent 注册中心 — 管理所有 Rig Agent 实例的创建、获取和热重载

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::curator::CuratorRigAgent;
use super::reflection::ReflectionRigAgent;
use super::router::ProviderRouter;
use super::writing::WritingRigAgent;
use super::{AgentError, AgentStatus};
use crate::core::settings::{AppSettings, SettingsManager};

/// Agent 名称常量
const AGENT_WRITING: &str = "writing";
const AGENT_CURATOR: &str = "curator";
const AGENT_REFLECTION: &str = "reflection";

/// 注册中心内部状态
struct RegistryInner {
    writing_agent: Option<WritingRigAgent>,
    curator_agent: Option<CuratorRigAgent>,
    reflection_agent: Option<ReflectionRigAgent>,
    statuses: HashMap<String, AgentStatus>,
    initialized: bool,
}

/// Agent 注册中心 — 无阻塞的 async 访问（基于 `Arc<RwLock>`）
///
/// 支持延迟初始化：在 Tauri setup（无 tokio runtime）中创建空壳，
/// 首次调用 `writing_agent()` 等方法时在 async 上下文中完成实际构建。
#[derive(Clone)]
pub struct AgentRegistry {
    inner: Arc<RwLock<RegistryInner>>,
    /// Deferred init: settings + settings_mutex for lazy build
    deferred: Option<Arc<DeferredInit>>,
}

/// 延迟初始化所需的配置
struct DeferredInit {
    settings: AppSettings,
    settings_mutex: std::sync::Arc<std::sync::Mutex<SettingsManager>>,
}

impl AgentRegistry {
    /// 根据 Provider 配置初始化所有 Agent
    pub async fn new(settings: &AppSettings, settings_mgr: &SettingsManager) -> Self {
        let inner = Self::build_inner(settings, settings_mgr);
        Self {
            inner: Arc::new(RwLock::new(inner)),
            deferred: None,
        }
    }

    /// 同步版本的构造函数（用于 Tauri setup 等同步上下文）
    pub fn new_sync(settings: &AppSettings, settings_mgr: &SettingsManager) -> Self {
        let inner = Self::build_inner(settings, settings_mgr);
        Self {
            inner: Arc::new(RwLock::new(inner)),
            deferred: None,
        }
    }

    /// 延迟初始化构造函数 — 在同步上下文中创建空壳，首次使用时构建 agents
    ///
    /// 这避免了在 Tauri setup 闭包（无 tokio runtime）中调用 rig agent builder。
    pub fn new_deferred(
        settings: AppSettings,
        settings_mutex: std::sync::Arc<std::sync::Mutex<SettingsManager>>,
    ) -> Self {
        // 创建空的 inner（无任何 agent）
        let empty_inner = RegistryInner {
            writing_agent: None,
            curator_agent: None,
            reflection_agent: None,
            statuses: std::collections::HashMap::new(),
            initialized: false,
        };
        Self {
            inner: Arc::new(RwLock::new(empty_inner)),
            deferred: Some(Arc::new(DeferredInit { settings, settings_mutex })),
        }
    }

    /// 确保 agents 已初始化（延迟初始化的核心）
    async fn ensure_initialized(&self) {
        // Quick check with read lock
        {
            let guard = self.inner.read().await;
            if guard.initialized {
                return;
            }
        }
        // Need to initialize — acquire write lock
        let mut guard = self.inner.write().await;
        if guard.initialized {
            return; // Double-check after acquiring write lock
        }
        if let Some(deferred) = &self.deferred {
            let new_inner = {
                let settings_mgr = deferred.settings_mutex.lock().expect("无法获取 settings 锁");
                Self::build_inner(&deferred.settings, &settings_mgr)
            };
            *guard = new_inner;
        }
    }

    /// 热重载：在新实例就绪后替换旧实例，旧实例在替换前继续服务
    pub async fn reload(&self, settings: &AppSettings, settings_mgr: &SettingsManager) {
        // 先在 lock 外部构建新的内部状态
        let new_inner = Self::build_inner(settings, settings_mgr);

        // 构建完成后才获取写锁并原子替换
        let mut guard = self.inner.write().await;
        *guard = new_inner;
    }

    /// 热重载变体：接受 `Arc<Mutex<SettingsManager>>` 而非 `&SettingsManager`。
    ///
    /// 这个方法确保 `std::sync::MutexGuard` 不会跨越 `.await` 点持有，
    /// 适用于 Tauri async command 上下文中调用。
    pub async fn reload_with_sync_settings(
        &self,
        settings: &AppSettings,
        settings_mutex: &std::sync::Arc<std::sync::Mutex<crate::core::settings::SettingsManager>>,
    ) -> Result<(), String> {
        // 同步部分：获取 std::sync::Mutex 锁，构建新的内部状态，然后立即释放锁
        let new_inner = {
            let settings_mgr = settings_mutex.lock().map_err(|e| e.to_string())?;
            Self::build_inner(settings, &settings_mgr)
        }; // MutexGuard 在此处释放

        // 异步部分：获取 tokio RwLock 并原子替换
        let mut guard = self.inner.write().await;
        *guard = new_inner;
        Ok(())
    }

    /// 获取 Writing Agent（无阻塞读锁）
    pub async fn writing_agent(&self) -> Result<WritingRigAgent, AgentError> {
        self.ensure_initialized().await;
        let guard = self.inner.read().await;
        match &guard.writing_agent {
            Some(agent) => Ok(agent.clone()),
            None => Err(Self::unavailable_error(&guard.statuses, AGENT_WRITING)),
        }
    }

    /// 获取 Curator Agent（无阻塞读锁）
    pub async fn curator_agent(&self) -> Result<CuratorRigAgent, AgentError> {
        self.ensure_initialized().await;
        let guard = self.inner.read().await;
        match &guard.curator_agent {
            Some(agent) => Ok(agent.clone()),
            None => Err(Self::unavailable_error(&guard.statuses, AGENT_CURATOR)),
        }
    }

    /// 获取 Reflection Agent（无阻塞读锁）
    pub async fn reflection_agent(&self) -> Result<ReflectionRigAgent, AgentError> {
        self.ensure_initialized().await;
        let guard = self.inner.read().await;
        match &guard.reflection_agent {
            Some(agent) => Ok(agent.clone()),
            None => Err(Self::unavailable_error(&guard.statuses, AGENT_REFLECTION)),
        }
    }

    // ─── Private ────────────────────────────────────────────────────────────

    /// 构建 RegistryInner：尝试创建 ProviderRouter 并初始化各 Agent
    fn build_inner(settings: &AppSettings, settings_mgr: &SettingsManager) -> RegistryInner {
        let mut statuses = HashMap::new();
        let mut writing_agent = None;
        let mut curator_agent = None;
        let mut reflection_agent = None;

        let router = match ProviderRouter::from_config(settings, settings_mgr) {
            Ok(r) => Some(r),
            Err(e) => {
                let reason = format!("Provider 路由器构建失败: {}", e);
                log::error!("{}", reason);
                statuses.insert(AGENT_WRITING.to_string(), AgentStatus::Unavailable { reason: reason.clone() });
                statuses.insert(AGENT_CURATOR.to_string(), AgentStatus::Unavailable { reason: reason.clone() });
                statuses.insert(AGENT_REFLECTION.to_string(), AgentStatus::Unavailable { reason });
                None
            }
        };

        if let Some(router) = &router {
            // Writing Agent
            match router.resolve_with_fallback(AGENT_WRITING) {
                Ok(provider) => {
                    writing_agent = Some(WritingRigAgent::new(provider.client(), provider.model()));
                    statuses.insert(AGENT_WRITING.to_string(), AgentStatus::Available);
                }
                Err(e) => {
                    let reason = format!("无可用 Provider: {}", e);
                    statuses.insert(AGENT_WRITING.to_string(), AgentStatus::Unavailable { reason });
                }
            }

            // Curator Agent
            match router.resolve_with_fallback(AGENT_CURATOR) {
                Ok(provider) => {
                    // Note: EmbeddingSearchTool 的完整集成在后续集成任务中完成
                    // 此处构建不带 embedding 工具的 Curator Agent（工具在 Tauri 状态注入时连接）
                    curator_agent = Some(CuratorRigAgent::new_without_tool(provider.client(), provider.model()));
                    statuses.insert(AGENT_CURATOR.to_string(), AgentStatus::Available);
                }
                Err(e) => {
                    let reason = format!("无可用 Provider: {}", e);
                    statuses.insert(AGENT_CURATOR.to_string(), AgentStatus::Unavailable { reason });
                }
            }

            // Reflection Agent
            match router.resolve_with_fallback(AGENT_REFLECTION) {
                Ok(provider) => {
                    reflection_agent = Some(ReflectionRigAgent::new(provider.client(), provider.model()));
                    statuses.insert(AGENT_REFLECTION.to_string(), AgentStatus::Available);
                }
                Err(e) => {
                    let reason = format!("无可用 Provider: {}", e);
                    statuses.insert(AGENT_REFLECTION.to_string(), AgentStatus::Unavailable { reason });
                }
            }
        }

        RegistryInner {
            writing_agent,
            curator_agent,
            reflection_agent,
            statuses,
            initialized: true,
        }
    }

    /// 根据 statuses map 生成 AgentUnavailable 错误
    fn unavailable_error(statuses: &HashMap<String, AgentStatus>, agent_name: &str) -> AgentError {
        let reason = match statuses.get(agent_name) {
            Some(AgentStatus::Unavailable { reason }) => reason.clone(),
            _ => "未知原因".to_string(),
        };
        AgentError::AgentUnavailable {
            agent: agent_name.to_string(),
            reason,
        }
    }
}

/// 测试辅助：允许直接构造包含特定 agent 状态的 AgentRegistry（跳过 Provider 构建）
#[cfg(test)]
impl AgentRegistry {
    pub(crate) fn new_for_test(
        writing: Option<WritingRigAgent>,
        curator: Option<CuratorRigAgent>,
        reflection: Option<ReflectionRigAgent>,
        statuses: HashMap<String, AgentStatus>,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RegistryInner {
                writing_agent: writing,
                curator_agent: curator,
                reflection_agent: reflection,
                statuses,
                initialized: true,
            })),
            deferred: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::{AgentRouting, AppSettings, ProviderConfig, ProviderType};

    fn ollama_settings() -> AppSettings {
        AppSettings {
            providers: vec![ProviderConfig {
                id: "ollama-local".to_string(),
                name: "Ollama Local".to_string(),
                provider_type: ProviderType::Ollama,
                endpoint: "http://localhost:11434/v1".to_string(),
                model: "qwen2.5".to_string(),
                temperature: 0.7,
                enabled: true,
            }],
            routing: AgentRouting {
                default_provider: Some("ollama-local".to_string()),
                overrides: HashMap::new(),
            },
        }
    }

    fn empty_settings() -> AppSettings {
        AppSettings {
            providers: vec![],
            routing: AgentRouting {
                default_provider: None,
                overrides: HashMap::new(),
            },
        }
    }

    #[tokio::test]
    async fn test_new_with_valid_provider_all_agents_available() {
        let settings = ollama_settings();
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let registry = AgentRegistry::new(&settings, &mgr).await;

        assert!(registry.writing_agent().await.is_ok());
        assert!(registry.curator_agent().await.is_ok());
        assert!(registry.reflection_agent().await.is_ok());
    }

    #[tokio::test]
    async fn test_new_with_no_provider_all_agents_unavailable() {
        let settings = empty_settings();
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let registry = AgentRegistry::new(&settings, &mgr).await;

        let err = registry.writing_agent().await.unwrap_err();
        assert!(matches!(err, AgentError::AgentUnavailable { .. }));

        let err = registry.curator_agent().await.unwrap_err();
        assert!(matches!(err, AgentError::AgentUnavailable { .. }));

        let err = registry.reflection_agent().await.unwrap_err();
        assert!(matches!(err, AgentError::AgentUnavailable { .. }));
    }

    #[tokio::test]
    async fn test_reload_swaps_inner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        // 初始无 Provider
        let registry = AgentRegistry::new(&empty_settings(), &mgr).await;
        assert!(registry.writing_agent().await.is_err());

        // 热重载后有 Provider
        registry.reload(&ollama_settings(), &mgr).await;
        assert!(registry.writing_agent().await.is_ok());
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let settings = ollama_settings();
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = SettingsManager::new(tmp.path());

        let registry = AgentRegistry::new(&settings, &mgr).await;
        let cloned = registry.clone();

        // 通过原实例 reload 到空配置
        registry.reload(&empty_settings(), &mgr).await;

        // clone 也能观察到变更（共享 Arc）
        assert!(cloned.writing_agent().await.is_err());
    }
}

/// Property-based tests for AgentRegistry.
///
/// Property 1: Agent 不可用返回明确错误
/// 验证标记为不可用的 Agent 返回 AgentUnavailable 错误，且错误包含原因描述。
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // ─── Generators ─────────────────────────────────────────────────────────

    /// Generate a non-empty reason string (1-50 characters)
    fn gen_reason() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 _\\-]{1,50}"
    }

    /// Which agent slot to test
    #[derive(Debug, Clone, Copy)]
    enum AgentSlot {
        Writing,
        Curator,
        Reflection,
    }

    fn gen_agent_slot() -> impl Strategy<Value = AgentSlot> {
        prop_oneof![
            Just(AgentSlot::Writing),
            Just(AgentSlot::Curator),
            Just(AgentSlot::Reflection),
        ]
    }

    // ─── Property 1: Agent 不可用返回明确错误 ────────────────────────────────

    proptest! {
        /// **Validates: Requirements 1.6**
        ///
        /// Property 1: Agent 不可用返回明确错误
        /// For any agent marked as Unavailable in the registry, requesting that agent
        /// SHALL return AgentUnavailable error containing the reason string.
        #[test]
        fn prop_unavailable_agent_returns_error_with_reason(
            reason in gen_reason(),
            slot in gen_agent_slot(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let agent_name = match slot {
                    AgentSlot::Writing => "writing",
                    AgentSlot::Curator => "curator",
                    AgentSlot::Reflection => "reflection",
                };

                // Build a registry with the target agent slot empty and status Unavailable
                let mut statuses = HashMap::new();
                statuses.insert(
                    agent_name.to_string(),
                    AgentStatus::Unavailable { reason: reason.clone() },
                );

                let registry = AgentRegistry::new_for_test(None, None, None, statuses);

                let result = match slot {
                    AgentSlot::Writing => registry.writing_agent().await.map(|_| ()),
                    AgentSlot::Curator => registry.curator_agent().await.map(|_| ()),
                    AgentSlot::Reflection => registry.reflection_agent().await.map(|_| ()),
                };

                // Must be an error
                let err = result.unwrap_err();

                // Must be AgentUnavailable variant
                match &err {
                    AgentError::AgentUnavailable { agent, reason: r } => {
                        // Agent name matches the queried slot
                        prop_assert_eq!(agent, agent_name);
                        // Reason matches what we set
                        prop_assert_eq!(r, &reason);
                    }
                    other => {
                        prop_assert!(false, "Expected AgentUnavailable, got {:?}", other);
                    }
                }

                Ok(())
            })?;
        }

        /// **Validates: Requirements 1.6**
        ///
        /// Property 1 (supplemental): When agent is Available, requesting it returns Ok.
        #[test]
        fn prop_available_agent_returns_ok(
            slot in gen_agent_slot(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let agent_name = match slot {
                    AgentSlot::Writing => "writing",
                    AgentSlot::Curator => "curator",
                    AgentSlot::Reflection => "reflection",
                };

                let mut statuses = HashMap::new();
                statuses.insert(agent_name.to_string(), AgentStatus::Available);

                // Build registry with the target agent present
                let (writing, curator, reflection) = match slot {
                    AgentSlot::Writing => (Some(WritingRigAgent::dummy()), None, None),
                    AgentSlot::Curator => (None, Some(CuratorRigAgent::dummy()), None),
                    AgentSlot::Reflection => (None, None, Some(ReflectionRigAgent::dummy())),
                };

                let registry = AgentRegistry::new_for_test(writing, curator, reflection, statuses);

                let result = match slot {
                    AgentSlot::Writing => registry.writing_agent().await.map(|_| ()),
                    AgentSlot::Curator => registry.curator_agent().await.map(|_| ()),
                    AgentSlot::Reflection => registry.reflection_agent().await.map(|_| ()),
                };

                prop_assert!(result.is_ok());

                Ok(())
            })?;
        }
    }
}
