import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AISettingsTab } from './AISettingsTab';
import './SettingsModal.css';

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
}

type SettingsTab = 'account' | 'shortcuts' | 'vault' | 'ai' | 'plugins';

const TABS: { id: SettingsTab; label: string; icon: React.ReactNode }[] = [
  {
    id: 'account',
    label: '账户',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="9" cy="6" r="3" />
        <path d="M3 16c0-3.3 2.7-6 6-6s6 2.7 6 6" />
      </svg>
    ),
  },
  {
    id: 'shortcuts',
    label: '快捷键',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="2" y="5" width="14" height="9" rx="2" />
        <path d="M5 8h1M8 8h2M12 8h1M5 11h8" />
      </svg>
    ),
  },
  {
    id: 'vault',
    label: '知识库',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M3 4h12v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4z" />
        <path d="M3 4V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" />
        <path d="M7 8h4" />
      </svg>
    ),
  },
  {
    id: 'ai',
    label: 'AI 模型',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="9" cy="9" r="3" />
        <path d="M9 2v2M9 14v2M2 9h2M14 9h2M4.2 4.2l1.4 1.4M12.4 12.4l1.4 1.4M4.2 13.8l1.4-1.4M12.4 5.6l1.4-1.4" />
      </svg>
    ),
  },
  {
    id: 'plugins',
    label: '插件',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="3" width="5" height="5" rx="1" />
        <rect x="10" y="3" width="5" height="5" rx="1" />
        <rect x="3" y="10" width="5" height="5" rx="1" />
        <rect x="10" y="10" width="5" height="5" rx="1" />
      </svg>
    ),
  },
];

const SHORTCUTS = [
  { name: '快速记录', key: '⌘⇧Space' },
  { name: '新建文章', key: '⌘N' },
  { name: '切换侧边栏', key: '⌘\\' },
  { name: '沉浸模式', key: '⌘⇧F' },
  { name: '关闭/返回', key: 'Esc' },
];

export default function SettingsModal({ open, onClose }: SettingsModalProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>('account');
  const [closing, setClosing] = useState(false);
  const [vaultPath, setVaultPath] = useState<string>('');
  const [fragmentCount, setFragmentCount] = useState<number>(0);
  const [articleCount, setArticleCount] = useState<number>(0);

  // Reset state when modal opens
  useEffect(() => {
    if (open) {
      setActiveTab('account');
      setClosing(false);
    }
  }, [open]);

  // Fetch vault data when vault tab is active
  useEffect(() => {
    if (open && activeTab === 'vault') {
      invoke<string>('get_vault_path')
        .then(setVaultPath)
        .catch(() => setVaultPath('未配置'));

      invoke<{ fragments: number; articles: number }>('get_counts')
        .then((counts) => {
          setFragmentCount(counts.fragments);
          setArticleCount(counts.articles);
        })
        .catch(() => {
          setFragmentCount(0);
          setArticleCount(0);
        });
    }
  }, [open, activeTab]);

  const handleClose = useCallback(() => {
    setClosing(true);
    setTimeout(() => {
      setClosing(false);
      onClose();
    }, 200);
  }, [onClose]);

  // Keyboard: Esc to close
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        handleClose();
      }
    },
    [handleClose],
  );

  // Click overlay to close
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) {
        handleClose();
      }
    },
    [handleClose],
  );

  if (!open && !closing) return null;

  return (
    <div
      className="settings-overlay"
      data-closing={closing}
      onClick={handleOverlayClick}
      onKeyDown={handleKeyDown}
      role="dialog"
      aria-modal="true"
      aria-label="设置"
    >
      <div className="settings-modal">
        {/* Left tab navigation */}
        <nav className="settings-tabs">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              className="settings-tab"
              data-active={activeTab === tab.id}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.icon}
              <span>{tab.label}</span>
            </button>
          ))}
        </nav>

        {/* Right content area */}
        <div className="settings-content">
          {activeTab === 'account' && <AccountTab />}
          {activeTab === 'shortcuts' && <ShortcutsTab />}
          {activeTab === 'vault' && (
            <VaultTab
              vaultPath={vaultPath}
              fragmentCount={fragmentCount}
              articleCount={articleCount}
            />
          )}
          {activeTab === 'ai' && <AISettingsTab />}
          {activeTab === 'plugins' && <PluginsTab />}
        </div>
      </div>
    </div>
  );
}

function AccountTab() {
  return (
    <>
      <h3 className="settings-content-title">账户</h3>
      <div className="settings-section">
        <div className="settings-field">
          <span className="settings-field-label">用户名</span>
          <span className="settings-field-value">本地用户</span>
        </div>
        <div className="settings-field">
          <span className="settings-field-label">邮箱</span>
          <span className="settings-field-value">—</span>
        </div>
        <div className="settings-field">
          <span className="settings-field-label">版本</span>
          <span className="settings-field-value">Cognest MVP 0.1.0</span>
        </div>
      </div>
    </>
  );
}

function ShortcutsTab() {
  return (
    <>
      <h3 className="settings-content-title">快捷键</h3>
      <ul className="settings-shortcut-list">
        {SHORTCUTS.map((s) => (
          <li key={s.key} className="settings-shortcut-item">
            <span className="settings-shortcut-name">{s.name}</span>
            <span className="settings-shortcut-key">{s.key}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

interface VaultTabProps {
  vaultPath: string;
  fragmentCount: number;
  articleCount: number;
}

function VaultTab({ vaultPath, fragmentCount, articleCount }: VaultTabProps) {
  return (
    <>
      <h3 className="settings-content-title">知识库</h3>
      <div className="settings-section">
        <div className="settings-section-label">Vault 路径</div>
        <div className="settings-vault-path">{vaultPath || '加载中…'}</div>
      </div>
      <div className="settings-section">
        <div className="settings-section-label">统计</div>
        <div className="settings-field">
          <span className="settings-field-label">碎片总数</span>
          <span className="settings-field-value">{fragmentCount}</span>
        </div>
        <div className="settings-field">
          <span className="settings-field-label">文章总数</span>
          <span className="settings-field-value">{articleCount}</span>
        </div>
      </div>
    </>
  );
}

function PluginsTab() {
  return (
    <>
      <h3 className="settings-content-title">插件</h3>
      <div className="settings-empty">
        <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
          <rect x="3" y="3" width="5" height="5" rx="1" />
          <rect x="10" y="3" width="5" height="5" rx="1" />
          <rect x="3" y="10" width="5" height="5" rx="1" />
          <rect x="10" y="10" width="5" height="5" rx="1" />
        </svg>
        <span>插件生态将在后续版本开放</span>
      </div>
    </>
  );
}
