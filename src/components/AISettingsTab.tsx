import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '../stores/settingsStore';
import type { ProviderConfig, AiSettings } from '../stores/settingsStore';
import './AISettingsTab.css';

// ─── Audit Record Type ──────────────────────────────────────────────────────

interface AuditRecord {
  id: number;
  timestamp: string;
  provider_name: string;
  operation: string;
  token_count: number;
  success: boolean;
}

// ─── Provider Templates ─────────────────────────────────────────────────────

const PROVIDER_TEMPLATES: Record<string, Partial<ProviderConfig>> = {
  deepseek: {
    name: 'DeepSeek',
    providerType: 'deepseek',
    endpoint: 'https://api.deepseek.com',
    model: 'deepseek-chat',
    temperature: 0.7,
  },
  ollama: {
    name: 'Ollama',
    providerType: 'ollama',
    endpoint: 'http://localhost:11434',
    model: '',
    temperature: 0.7,
  },
};

const AGENT_NAMES = [
  { key: 'curator', label: 'Curator' },
  { key: 'writing', label: 'Writing' },
  { key: 'reflection', label: 'Reflection' },
];

// ─── Data Routing Labels ────────────────────────────────────────────────────

const DATA_ROUTING: Record<string, { operations: string[]; route: 'cloud' | 'local' }[]> = {
  curator: [
    { operations: ['分类命名', '标签生成'], route: 'cloud' },
    { operations: ['Embedding', '相似度搜索', '聚类算法'], route: 'local' },
  ],
  writing: [
    { operations: ['写作辅助', '内容生成'], route: 'cloud' },
  ],
  reflection: [
    { operations: ['回顾总结', '洞察生成'], route: 'cloud' },
  ],
};

const GLOBAL_OPERATIONS: { label: string; route: 'cloud' | 'local' }[] = [
  { label: 'Embedding 计算', route: 'local' },
  { label: '相似度搜索', route: 'local' },
  { label: '向量聚类', route: 'local' },
  { label: '分类命名', route: 'cloud' },
  { label: '写作辅助', route: 'cloud' },
  { label: '视图生成', route: 'cloud' },
  { label: '回顾洞察', route: 'cloud' },
];

// ─── Helpers ────────────────────────────────────────────────────────────────

function maskApiKey(key: string): string {
  if (!key || key.length <= 4) return key ? '••••' : '';
  return '••••' + key.slice(-4);
}

function generateId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
}

// ─── Types ──────────────────────────────────────────────────────────────────

type ValidationStatus = 'idle' | 'validating' | 'success' | 'failure';

interface ProviderFormState {
  template: string;
  name: string;
  endpoint: string;
  model: string;
  apiKey: string;
}

// ─── Component ──────────────────────────────────────────────────────────────

export function AISettingsTab() {
  const { settings, loading, loadSettings, saveSettings, validateProvider, listOllamaModels } =
    useSettingsStore();

  const [localSettings, setLocalSettings] = useState<AiSettings | null>(null);
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [showAddForm, setShowAddForm] = useState(false);
  const [formState, setFormState] = useState<ProviderFormState>({
    template: 'deepseek',
    name: '',
    endpoint: '',
    model: '',
    apiKey: '',
  });
  const [validationStatus, setValidationStatus] = useState<Record<string, ValidationStatus>>({});
  const [ollamaModels, setOllamaModels] = useState<Record<string, string[]>>({});
  const [disableWarning, setDisableWarning] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [auditLog, setAuditLog] = useState<AuditRecord[]>([]);
  const [auditLoading, setAuditLoading] = useState(false);

  // Load settings on mount
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Sync local copy from store
  useEffect(() => {
    if (settings) {
      setLocalSettings(structuredClone(settings));
    }
  }, [settings]);

  // Fetch audit log on mount
  useEffect(() => {
    async function fetchAuditLog() {
      setAuditLoading(true);
      try {
        const records = await invoke<AuditRecord[]>('get_audit_log', { limit: 50 });
        setAuditLog(records);
      } catch {
        // Silently fail — audit log is non-critical
        setAuditLog([]);
      } finally {
        setAuditLoading(false);
      }
    }
    fetchAuditLog();
  }, []);

  // ─── Provider Enable/Disable ────────────────────────────────────────────

  const handleToggleProvider = useCallback(
    (providerId: string) => {
      if (!localSettings) return;

      const provider = localSettings.providers.find((p) => p.id === providerId);
      if (!provider) return;

      // Prevent disabling the only enabled provider
      if (provider.enabled) {
        const enabledCount = localSettings.providers.filter((p) => p.enabled).length;
        if (enabledCount <= 1) {
          setDisableWarning(providerId);
          setTimeout(() => setDisableWarning(null), 3000);
          return;
        }
      }

      setLocalSettings({
        ...localSettings,
        providers: localSettings.providers.map((p) =>
          p.id === providerId ? { ...p, enabled: !p.enabled } : p,
        ),
      });
      setDirty(true);
    },
    [localSettings],
  );

  // ─── Validation ─────────────────────────────────────────────────────────

  const handleValidate = useCallback(
    async (provider: ProviderConfig) => {
      setValidationStatus((prev) => ({ ...prev, [provider.id]: 'validating' }));
      const key = apiKeys[provider.id] || '';
      const valid = await validateProvider(provider, key);
      setValidationStatus((prev) => ({
        ...prev,
        [provider.id]: valid ? 'success' : 'failure',
      }));
      // Reset status after 4s
      setTimeout(() => {
        setValidationStatus((prev) => ({ ...prev, [provider.id]: 'idle' }));
      }, 4000);
    },
    [apiKeys, validateProvider],
  );

  // ─── Ollama Model Refresh ───────────────────────────────────────────────

  const handleRefreshOllamaModels = useCallback(
    async (provider: ProviderConfig) => {
      const models = await listOllamaModels(provider.endpoint);
      setOllamaModels((prev) => ({ ...prev, [provider.id]: models }));
    },
    [listOllamaModels],
  );

  const handleOllamaModelSelect = useCallback(
    (providerId: string, model: string) => {
      if (!localSettings) return;
      setLocalSettings({
        ...localSettings,
        providers: localSettings.providers.map((p) =>
          p.id === providerId ? { ...p, model } : p,
        ),
      });
      setDirty(true);
    },
    [localSettings],
  );

  // ─── Add Provider ──────────────────────────────────────────────────────

  const handleTemplateChange = useCallback((template: string) => {
    const tpl = PROVIDER_TEMPLATES[template];
    setFormState({
      template,
      name: tpl?.name || '',
      endpoint: tpl?.endpoint || '',
      model: tpl?.model || '',
      apiKey: '',
    });
  }, []);

  const handleAddProvider = useCallback(() => {
    if (!localSettings) return;
    const newProvider: ProviderConfig = {
      id: generateId(),
      name: formState.name || PROVIDER_TEMPLATES[formState.template]?.name || 'Provider',
      providerType: (PROVIDER_TEMPLATES[formState.template]?.providerType || 'openai_compat') as ProviderConfig['providerType'],
      endpoint: formState.endpoint,
      model: formState.model,
      temperature: 0.7,
      enabled: true,
    };

    if (formState.apiKey) {
      setApiKeys((prev) => ({ ...prev, [newProvider.id]: formState.apiKey }));
    }

    setLocalSettings({
      ...localSettings,
      providers: [...localSettings.providers, newProvider],
    });
    setShowAddForm(false);
    setFormState({ template: 'deepseek', name: '', endpoint: '', model: '', apiKey: '' });
    setDirty(true);
  }, [localSettings, formState]);

  // ─── Remove Provider ───────────────────────────────────────────────────

  const handleRemoveProvider = useCallback(
    (providerId: string) => {
      if (!localSettings) return;
      setLocalSettings({
        ...localSettings,
        providers: localSettings.providers.filter((p) => p.id !== providerId),
      });
      setDirty(true);
    },
    [localSettings],
  );

  // ─── Agent Routing ─────────────────────────────────────────────────────

  const handleDefaultRouting = useCallback(
    (providerId: string) => {
      if (!localSettings) return;
      setLocalSettings({
        ...localSettings,
        routing: { ...localSettings.routing, defaultProvider: providerId || null },
      });
      setDirty(true);
    },
    [localSettings],
  );

  const handleAgentOverride = useCallback(
    (agent: string, providerId: string) => {
      if (!localSettings) return;
      const overrides = { ...localSettings.routing.overrides };
      if (providerId) {
        overrides[agent] = providerId;
      } else {
        delete overrides[agent];
      }
      setLocalSettings({
        ...localSettings,
        routing: { ...localSettings.routing, overrides },
      });
      setDirty(true);
    },
    [localSettings],
  );

  // ─── Save ──────────────────────────────────────────────────────────────

  const handleSave = useCallback(async () => {
    if (!localSettings) return;
    await saveSettings(localSettings, apiKeys);
    setDirty(false);
  }, [localSettings, apiKeys, saveSettings]);

  // ─── Render ────────────────────────────────────────────────────────────

  if (loading && !localSettings) {
    return (
      <div className="ai-settings-tab">
        <h3 className="settings-content-title">AI 模型</h3>
        <div className="ai-settings-loading">加载中…</div>
      </div>
    );
  }

  if (!localSettings) {
    return (
      <div className="ai-settings-tab">
        <h3 className="settings-content-title">AI 模型</h3>
        <div className="ai-settings-loading">无法加载设置</div>
      </div>
    );
  }

  const enabledProviders = localSettings.providers.filter((p) => p.enabled);

  return (
    <div className="ai-settings-tab">
      <h3 className="settings-content-title">AI 模型</h3>

      {/* Provider List */}
      <div className="ai-settings-section">
        <div className="ai-settings-section-label">Provider 列表</div>
        <div className="ai-provider-list">
          {localSettings.providers.map((provider) => (
            <div key={provider.id} className="ai-provider-row">
              <label className="ai-provider-toggle">
                <input
                  type="checkbox"
                  checked={provider.enabled}
                  onChange={() => handleToggleProvider(provider.id)}
                />
                <span className="ai-provider-toggle-track" />
              </label>

              <span className="ai-provider-name">{provider.name}</span>
              <span className="ai-provider-endpoint">{provider.endpoint.replace(/^https?:\/\//, '')}</span>

              {provider.providerType === 'ollama' ? (
                <span className="ai-provider-model">
                  {ollamaModels[provider.id] && ollamaModels[provider.id].length > 0 ? (
                    <select
                      className="ai-provider-model-select"
                      value={provider.model}
                      onChange={(e) => handleOllamaModelSelect(provider.id, e.target.value)}
                    >
                      {!provider.model && <option value="">选择模型</option>}
                      {ollamaModels[provider.id].map((m) => (
                        <option key={m} value={m}>{m}</option>
                      ))}
                    </select>
                  ) : (
                    <span className="ai-provider-model-text">{provider.model || '—'}</span>
                  )}
                </span>
              ) : (
                <span className="ai-provider-model">
                  {apiKeys[provider.id]
                    ? maskApiKey(apiKeys[provider.id])
                    : provider.model || '—'}
                </span>
              )}

              <div className="ai-provider-actions">
                {provider.providerType === 'ollama' ? (
                  <button
                    className="ai-provider-action-btn"
                    onClick={() => handleRefreshOllamaModels(provider)}
                    title="刷新模型列表"
                  >
                    刷新
                  </button>
                ) : (
                  <button
                    className="ai-provider-action-btn"
                    data-status={validationStatus[provider.id] || 'idle'}
                    onClick={() => handleValidate(provider)}
                    disabled={validationStatus[provider.id] === 'validating'}
                  >
                    {validationStatus[provider.id] === 'validating' && '验证中…'}
                    {validationStatus[provider.id] === 'success' && '✓ 成功'}
                    {validationStatus[provider.id] === 'failure' && '✗ 失败'}
                    {(!validationStatus[provider.id] || validationStatus[provider.id] === 'idle') && '测试'}
                  </button>
                )}

                <button
                  className="ai-provider-remove-btn"
                  onClick={() => handleRemoveProvider(provider.id)}
                  title="移除 Provider"
                >
                  ×
                </button>
              </div>

              {disableWarning === provider.id && (
                <div className="ai-provider-warning">
                  ⚠️ 无法禁用唯一启用的 Provider
                </div>
              )}
            </div>
          ))}

          {localSettings.providers.length === 0 && (
            <div className="ai-provider-empty">暂无 Provider，请添加一个</div>
          )}
        </div>

        {/* Add Provider */}
        {!showAddForm ? (
          <button className="ai-add-provider-btn" onClick={() => setShowAddForm(true)}>
            + 添加 Provider
          </button>
        ) : (
          <div className="ai-add-form">
            <div className="ai-add-form-row">
              <label className="ai-add-form-label">模板</label>
              <select
                className="ai-add-form-select"
                value={formState.template}
                onChange={(e) => handleTemplateChange(e.target.value)}
              >
                <option value="deepseek">DeepSeek</option>
                <option value="ollama">Ollama</option>
              </select>
            </div>

            <div className="ai-add-form-row">
              <label className="ai-add-form-label">名称</label>
              <input
                className="ai-add-form-input"
                value={formState.name}
                onChange={(e) => setFormState((s) => ({ ...s, name: e.target.value }))}
                placeholder={PROVIDER_TEMPLATES[formState.template]?.name}
              />
            </div>

            <div className="ai-add-form-row">
              <label className="ai-add-form-label">Endpoint</label>
              <input
                className="ai-add-form-input"
                value={formState.endpoint}
                onChange={(e) => setFormState((s) => ({ ...s, endpoint: e.target.value }))}
                placeholder={PROVIDER_TEMPLATES[formState.template]?.endpoint}
              />
            </div>

            <div className="ai-add-form-row">
              <label className="ai-add-form-label">模型</label>
              <input
                className="ai-add-form-input"
                value={formState.model}
                onChange={(e) => setFormState((s) => ({ ...s, model: e.target.value }))}
                placeholder={PROVIDER_TEMPLATES[formState.template]?.model || '自动探测'}
              />
            </div>

            {formState.template !== 'ollama' && (
              <div className="ai-add-form-row">
                <label className="ai-add-form-label">API Key</label>
                <input
                  className="ai-add-form-input"
                  type="password"
                  value={formState.apiKey}
                  onChange={(e) => setFormState((s) => ({ ...s, apiKey: e.target.value }))}
                  placeholder="sk-..."
                />
              </div>
            )}

            <div className="ai-add-form-actions">
              <button className="ai-add-form-cancel" onClick={() => setShowAddForm(false)}>
                取消
              </button>
              <button
                className="ai-add-form-submit"
                onClick={handleAddProvider}
                disabled={!formState.endpoint}
              >
                添加
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Agent Routing */}
      <div className="ai-settings-section">
        <div className="ai-settings-section-label">Agent 路由</div>
        <div className="ai-routing-list">
          <div className="ai-routing-row">
            <span className="ai-routing-agent">默认</span>
            <select
              className="ai-routing-select"
              value={localSettings.routing.defaultProvider || ''}
              onChange={(e) => handleDefaultRouting(e.target.value)}
            >
              <option value="">未指定</option>
              {enabledProviders.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </select>
          </div>

          {AGENT_NAMES.map(({ key, label }) => (
            <div key={key} className="ai-routing-row">
              <span className="ai-routing-agent">{label}</span>
              <div className="ai-routing-badges">
                {DATA_ROUTING[key]?.map((group) =>
                  group.operations.map((op) => (
                    <span
                      key={op}
                      className={`ai-route-badge ai-route-badge--${group.route}`}
                    >
                      {group.route === 'cloud' ? '☁ Cloud' : '🖥 Local'}: {op}
                    </span>
                  ))
                )}
              </div>
              <select
                className="ai-routing-select"
                value={localSettings.routing.overrides[key] || ''}
                onChange={(e) => handleAgentOverride(key, e.target.value)}
              >
                <option value="">使用默认</option>
                {enabledProviders.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
            </div>
          ))}
        </div>
      </div>

      {/* Data Routing Overview */}
      <div className="ai-settings-section">
        <div className="ai-settings-section-label">数据路由</div>
        <div className="ai-data-routing-list">
          {GLOBAL_OPERATIONS.map((op) => (
            <div key={op.label} className="ai-data-routing-item">
              <span className="ai-data-routing-label">{op.label}</span>
              <span className={`ai-route-badge ai-route-badge--${op.route}`}>
                {op.route === 'cloud' ? '☁ Cloud' : '🖥 Local'}
              </span>
            </div>
          ))}
        </div>
      </div>

      {/* Privacy Notice */}
      <div className="ai-settings-section">
        <div className="ai-privacy-notice">
          <span className="ai-privacy-icon">⚠️</span>
          <div className="ai-privacy-text">
            <strong>隐私说明</strong>
            <p>
              Embedding 始终本地计算。云端 Provider 仅接收分类/写作/视图生成相关的碎片内容（每次最多
              20 条 / 8000 tokens）。如使用 Ollama，所有数据留在本地。
            </p>
          </div>
        </div>
      </div>

      {/* Audit Log */}
      <div className="ai-settings-section">
        <div className="ai-settings-section-label">审计日志</div>
        {auditLoading ? (
          <div className="ai-audit-loading">加载中…</div>
        ) : auditLog.length === 0 ? (
          <div className="ai-audit-empty">暂无审计记录</div>
        ) : (
          <div className="ai-audit-table-wrap">
            <table className="ai-audit-table">
              <thead>
                <tr>
                  <th>时间</th>
                  <th>Provider</th>
                  <th>操作</th>
                  <th>Tokens</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                {auditLog.map((record) => (
                  <tr key={record.id}>
                    <td className="ai-audit-timestamp">
                      {new Date(record.timestamp).toLocaleString()}
                    </td>
                    <td className="ai-audit-provider">{record.provider_name}</td>
                    <td className="ai-audit-operation">{record.operation}</td>
                    <td className="ai-audit-tokens">{record.token_count}</td>
                    <td className="ai-audit-status">
                      {record.success ? (
                        <span className="ai-audit-success">✓</span>
                      ) : (
                        <span className="ai-audit-failure">✗</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Save Button */}
      {dirty && (
        <div className="ai-settings-save-bar">
          <button className="ai-settings-save-btn" onClick={handleSave} disabled={loading}>
            {loading ? '保存中…' : '保存设置'}
          </button>
        </div>
      )}
    </div>
  );
}
