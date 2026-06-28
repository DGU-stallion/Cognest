import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { showToast } from './Toast';
import './QuickCaptureModal.css';

interface QuickCaptureModalProps {
  open: boolean;
  onClose: () => void;
}

export default function QuickCaptureModal({ open, onClose }: QuickCaptureModalProps) {
  const [content, setContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [closing, setClosing] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Focus textarea within 100ms of opening
  useEffect(() => {
    if (open) {
      const timer = setTimeout(() => {
        textareaRef.current?.focus();
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [open]);

  // Reset state when modal opens
  useEffect(() => {
    if (open) {
      setContent('');
      setClosing(false);
    }
  }, [open]);

  const handleClose = useCallback(() => {
    setClosing(true);
    setTimeout(() => {
      setClosing(false);
      onClose();
    }, 200);
  }, [onClose]);

  const handleSave = useCallback(async () => {
    const trimmed = content.trim();
    if (!trimmed) return;

    setSaving(true);
    try {
      const fragmentId = await invoke<string>('create_fragment', { content: trimmed });
      showToast('已保存碎片', 'success');
      handleClose();
      // Fire-and-forget: trigger AI auto-tagging for the new fragment
      invoke('curate_fragment', { fragmentId }).catch(() => {
        // Non-critical — silently ignore if AI is unavailable
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      showToast(`保存失败: ${message}`, 'error');
      // Keep modal open, preserve user input
    } finally {
      setSaving(false);
    }
  }, [content, handleClose]);

  // Keyboard shortcuts within the modal
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      // ⌘↵ to save
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        handleSave();
        return;
      }
      // Esc to close
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        handleClose();
      }
    },
    [handleSave, handleClose],
  );

  // Click on overlay to close
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
      className="quick-capture-overlay"
      data-closing={closing}
      onClick={handleOverlayClick}
      onKeyDown={handleKeyDown}
      role="dialog"
      aria-modal="true"
      aria-label="快速记录"
    >
      <div className="quick-capture-modal">
        {/* Header */}
        <div className="quick-capture-header">
          <h2 className="quick-capture-title">快速记录</h2>
          <button
            className="quick-capture-close"
            onClick={handleClose}
            aria-label="关闭"
          >
            <svg viewBox="0 0 18 18" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M4.5 4.5l9 9M13.5 4.5l-9 9" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="quick-capture-body">
          <textarea
            ref={textareaRef}
            className="quick-capture-textarea"
            placeholder="记下你的想法…什么都可以，不需要整理。"
            value={content}
            onChange={(e) => setContent(e.target.value)}
            disabled={saving}
          />
        </div>

        {/* Footer */}
        <div className="quick-capture-footer">
          <button
            className="quick-capture-btn quick-capture-btn--secondary"
            onClick={handleClose}
            disabled={saving}
          >
            取消
          </button>
          <button
            className="quick-capture-btn quick-capture-btn--primary"
            onClick={handleSave}
            disabled={saving}
          >
            保存 <kbd>⌘↵</kbd>
          </button>
        </div>
      </div>
    </div>
  );
}
