import { useCallback, useEffect, useRef, useState } from 'react';
import { useWritingStore } from '../stores/writingStore';
import { useComposeStore } from '../stores/composeStore';
import type { RecommendedFragment } from '../stores/writingStore';
import './WritingPanel.css';

const MAX_INPUT_LENGTH = 2000;

export function WritingPanel() {
  const {
    messages,
    streaming,
    error,
    panelOpen,
    recommendedFragments,
    togglePanel,
    streamMessage,
    retryLast,
    quickAction,
    loadRecommendations,
    clearHistory,
  } = useWritingStore();

  const { currentArticleId, bodyContent } = useComposeStore();

  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Auto-scroll to bottom on new messages or streaming content
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Handle send message
  const handleSend = useCallback(() => {
    const trimmed = input.trim();
    if (!trimmed || streaming) return;
    if (!currentArticleId) {
      console.warn('[WritingPanel] No article selected, cannot send');
      return;
    }

    console.log('[WritingPanel] sending:', { currentArticleId, message: trimmed });
    streamMessage(currentArticleId, trimmed);
    setInput('');
  }, [input, streaming, currentArticleId, streamMessage]);

  // Handle Enter key (Shift+Enter for newline)
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  // Handle quick action buttons
  const handleQuickAction = useCallback(
    (action: 'outline' | 'expand' | 'recommend') => {
      if (streaming) return;
      if (action === 'recommend') {
        loadRecommendations(bodyContent);
      } else {
        quickAction(action, bodyContent);
      }
    },
    [streaming, bodyContent, quickAction, loadRecommendations],
  );

  // Handle fragment card click — insert reference chip
  const handleInsertFragment = useCallback(
    (fragment: RecommendedFragment) => {
      // Dispatch a custom event that the Editor can listen for
      const event = new CustomEvent('insert-reference-chip', {
        detail: { fragmentId: fragment.id },
      });
      window.dispatchEvent(event);
    },
    [],
  );

  // Check if error indicates no provider
  const isNoProvider = error?.includes('NoProvider') || error?.includes('no available provider');

  // Navigate to settings
  const handleGoToSettings = useCallback(() => {
    // Dispatch a custom event to open settings modal on AI tab
    const event = new CustomEvent('open-settings', { detail: { tab: 'ai' } });
    window.dispatchEvent(event);
  }, []);

  if (!panelOpen) {
    return (
      <div className="writing-panel writing-panel--collapsed">
        <button
          className="writing-panel__expand-btn"
          onClick={togglePanel}
          title="展开 Writing Agent"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4" />
          </svg>
          <span>✨</span>
        </button>
      </div>
    );
  }

  return (
    <div className="writing-panel">
      {/* Header */}
      <div className="writing-panel__header">
        <div className="writing-panel__header-left">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4" />
          </svg>
          <span>Writing Agent</span>
        </div>
        <button className="writing-panel__collapse-btn" onClick={togglePanel} title="折叠面板">
          ▶
        </button>
      </div>

      {/* Messages area */}
      <div className="writing-panel__messages">
        {messages.length === 0 && !isNoProvider && (
          <div className="writing-panel__empty">
            <p>向 AI 提问以获得写作帮助</p>
            <p className="writing-panel__empty-hint">试试点击下方的快捷按钮开始</p>
          </div>
        )}

        {/* No Provider notice */}
        {isNoProvider && (
          <div className="writing-panel__no-provider">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M12 9v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <p>尚未配置 AI 模型</p>
            <p className="writing-panel__no-provider-hint">
              请前往设置页配置 LLM Provider 以启用 AI 写作辅助
            </p>
            <button className="writing-panel__settings-btn" onClick={handleGoToSettings}>
              前往 AI 模型设置
            </button>
          </div>
        )}

        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`writing-panel__bubble writing-panel__bubble--${msg.role}`}
          >
            <div className="writing-panel__bubble-content">
              {msg.content}
              {msg.streaming && <span className="writing-panel__cursor" />}
            </div>
            <div className="writing-panel__bubble-time">
              {new Date(msg.timestamp).toLocaleTimeString('zh-CN', {
                hour: '2-digit',
                minute: '2-digit',
              })}
            </div>
          </div>
        ))}

        {/* Error display */}
        {error && !isNoProvider && (
          <div className="writing-panel__error">
            <span>{error}</span>
            <button className="writing-panel__retry-btn" onClick={retryLast}>
              重试
            </button>
          </div>
        )}

        {/* Recommended fragments */}
        {recommendedFragments.length > 0 && (
          <div className="writing-panel__recommendations">
            <div className="writing-panel__recommendations-title">推荐素材</div>
            {recommendedFragments.map((frag) => (
              <div
                key={frag.id}
                className="writing-panel__fragment-card"
                onClick={() => handleInsertFragment(frag)}
                title="点击插入引用"
              >
                <div className="writing-panel__fragment-content">{frag.content}</div>
                <div className="writing-panel__fragment-meta">
                  <span className="writing-panel__fragment-similarity">
                    {Math.round(frag.similarity * 100)}% 相关
                  </span>
                  {frag.tags.map((tag) => (
                    <span key={tag} className="writing-panel__fragment-tag">
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Quick actions */}
      <div className="writing-panel__actions">
        <button
          className="writing-panel__action-btn"
          onClick={() => handleQuickAction('outline')}
          disabled={streaming || !currentArticleId}
        >
          推荐结构
        </button>
        <button
          className="writing-panel__action-btn"
          onClick={() => handleQuickAction('expand')}
          disabled={streaming || !currentArticleId}
        >
          扩展段落
        </button>
        <button
          className="writing-panel__action-btn"
          onClick={() => handleQuickAction('recommend')}
          disabled={streaming || !currentArticleId}
        >
          推荐素材
        </button>
      </div>

      {/* Input area */}
      <div className="writing-panel__input-area">
        <div className="writing-panel__input-wrapper">
          <textarea
            ref={inputRef}
            className="writing-panel__input"
            placeholder="输入消息…"
            value={input}
            onChange={(e) => setInput(e.target.value.slice(0, MAX_INPUT_LENGTH))}
            onKeyDown={handleKeyDown}
            disabled={streaming || isNoProvider}
            rows={1}
          />
          <span className="writing-panel__char-count">
            {input.length}/{MAX_INPUT_LENGTH}
          </span>
        </div>
        <button
          className="writing-panel__send-btn"
          onClick={handleSend}
          disabled={!input.trim() || streaming || !currentArticleId || isNoProvider}
          title="发送"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
          </svg>
        </button>
      </div>

      {/* Clear history */}
      {messages.length > 0 && (
        <button className="writing-panel__clear-btn" onClick={clearHistory}>
          清除对话
        </button>
      )}
    </div>
  );
}
