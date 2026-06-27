// Requirements: 7.2, 7.3, 7.7
// GenerationBar: natural language input (max 500 chars) + generate button
// - Loading state with spinner
// - Generated view rendered via ViewRenderer
// - Pin/Clear buttons on generated view
// - Error/timeout → error message + retry button, preserves original prompt

import { useCallback, useState } from 'react';
import { useViewStore } from '../stores/viewStore';
import { ViewRenderer } from './ViewRenderer';
import './GenerationBar.css';

const MAX_PROMPT_LENGTH = 500;

export function GenerationBar() {
  const {
    currentView,
    generating,
    error,
    prompt,
    setPrompt,
    generateView,
    pinView,
    clearCurrent,
  } = useViewStore();

  // Keep the last submitted prompt for retry
  const [lastPrompt, setLastPrompt] = useState('');

  const canGenerate = prompt.trim().length > 0 && !generating;

  const handleGenerate = useCallback(() => {
    if (!canGenerate) return;
    setLastPrompt(prompt);
    generateView(prompt);
  }, [canGenerate, prompt, generateView]);

  const handleRetry = useCallback(() => {
    if (lastPrompt) {
      setPrompt(lastPrompt);
      generateView(lastPrompt);
    }
  }, [lastPrompt, setPrompt, generateView]);

  const handlePin = useCallback(() => {
    if (currentView) {
      pinView(currentView);
    }
  }, [currentView, pinView]);

  const handleClear = useCallback(() => {
    clearCurrent();
  }, [clearCurrent]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter' && canGenerate) {
        handleGenerate();
      }
    },
    [canGenerate, handleGenerate],
  );

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setPrompt(e.target.value);
    },
    [setPrompt],
  );

  return (
    <div className="generation-bar-wrapper">
      {/* Input bar */}
      <div className="generation-bar">
        <span className="generation-bar-icon" aria-hidden="true">🔮</span>
        <input
          type="text"
          className="generation-bar-input"
          placeholder="输入描述生成视图…"
          maxLength={MAX_PROMPT_LENGTH}
          value={prompt}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          disabled={generating}
          aria-label="视图生成描述"
        />
        <button
          type="button"
          className="generation-bar-btn"
          onClick={handleGenerate}
          disabled={!canGenerate}
          aria-label="生成视图"
        >
          {generating ? (
            <span className="generation-spinner" aria-label="生成中" />
          ) : (
            '生成'
          )}
        </button>
      </div>

      {/* Error state */}
      {error && !generating && (
        <div className="generation-error">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
            <circle cx="12" cy="12" r="10" />
            <path d="M12 8v4M12 16h.01" />
          </svg>
          <span className="generation-error-text">{error}</span>
          <button
            type="button"
            className="generation-retry-btn"
            onClick={handleRetry}
          >
            重试
          </button>
        </div>
      )}

      {/* Generated view result */}
      {currentView && !generating && (
        <div className="generation-result">
          <div className="generation-result-header">
            <h3 className="generation-result-title">{currentView.title}</h3>
            <div className="generation-result-actions">
              {!currentView.pinned && (
                <button
                  type="button"
                  className="generation-action-btn generation-pin-btn"
                  onClick={handlePin}
                  aria-label="固定视图"
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
                    <path d="M12 2v8m0 0l4-3m-4 3l-4-3M5 21l7-4 7 4V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16z" />
                  </svg>
                  固定
                </button>
              )}
              <button
                type="button"
                className="generation-action-btn generation-clear-btn"
                onClick={handleClear}
                aria-label="清除视图"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
                  <path d="M18 6L6 18M6 6l12 12" />
                </svg>
                清除
              </button>
            </div>
          </div>
          <div className="generation-result-body">
            <ViewRenderer spec={currentView} />
          </div>
        </div>
      )}
    </div>
  );
}
