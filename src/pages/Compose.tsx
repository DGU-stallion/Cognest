import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useComposeStore } from '../stores/composeStore';
import { useWritingStore } from '../stores/writingStore';
import type { Fragment } from '../stores/captureStore';
import type { EditorHandle, EditorMode } from '../components/Editor';
import Editor from '../components/Editor';
import { WritingPanel } from '../components/WritingPanel';
import './Compose.css';

type RefTab = 'inspirations' | 'topics' | 'articles';

const TAB_LABELS: { key: RefTab; label: string }[] = [
  { key: 'inspirations', label: '灵感' },
  { key: 'topics', label: '话题' },
  { key: 'articles', label: '文章' },
];

const STATUS_LABELS: Record<string, string> = {
  draft: '草稿',
  archived: '已归档',
};

function formatUpdatedAt(iso: string): string {
  if (!iso) return '';
  const d = new Date(iso);
  const now = new Date();
  const isToday =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  const time = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  return isToday ? `今天 ${time}` : `${d.getMonth() + 1}月${d.getDate()}日 ${time}`;
}

/** Drag-to-resize hook (fixed closure bug — no width in deps) */
function useResizable(initial: number, min: number, max: number, invert = false) {
  const [width, setWidth] = useState(initial);
  const widthRef = useRef(initial);
  const dragging = useRef(false);
  const startX = useRef(0);
  const startW = useRef(initial);

  // Keep ref in sync
  useEffect(() => { widthRef.current = width; }, [width]);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragging.current = true;
    startX.current = e.clientX;
    startW.current = widthRef.current;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';

    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      const delta = ev.clientX - startX.current;
      // invert: for right-side panels, dragging right = shrinking the panel
      const adjustedDelta = invert ? -delta : delta;
      const newWidth = Math.min(max, Math.max(min, startW.current + adjustedDelta));
      setWidth(newWidth);
    };
    const onUp = () => {
      dragging.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }, [min, max, invert]);

  return { width, onMouseDown };
}

export default function Compose() {
  const {
    currentArticleId,
    title,
    status,
    wordCount,
    updatedAt,
    immersiveMode,
    relatedFragments,
    bodyContent,
    loading,
    toggleImmersive,
    setTitle,
    setWordCount,
    cycleStatus,
    loadRelated,
    saveArticle,
  } = useComposeStore();

  const { panelOpen, togglePanel } = useWritingStore();

  const [activeTab, setActiveTab] = useState<RefTab>('inspirations');
  const [searchQuery, setSearchQuery] = useState('');
  const [insertingId, setInsertingId] = useState<string | null>(null);
  const [editorMode, setEditorMode] = useState<EditorMode>('wysiwyg');
  const [rightPanelOpen, setRightPanelOpen] = useState(false);
  const editorRef = useRef<EditorHandle>(null);
  const titleSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const leftPanel = useResizable(280, 180, 400);
  const rightPanel = useResizable(320, 200, 500, true);

  // Auto-create a new article when entering compose page with no article
  useEffect(() => {
    if (!currentArticleId && !loading) {
      useComposeStore.getState().createNewArticle();
    }
  }, []); // Only on mount

  // Load related fragments reactively based on article content
  useEffect(() => {
    loadRelated(bodyContent || title);
  }, [loadRelated]);

  // Re-load related when content changes (debounced)
  useEffect(() => {
    if (!bodyContent && !title) return;
    const timer = setTimeout(() => {
      loadRelated(bodyContent || title);
    }, 2000);
    return () => clearTimeout(timer);
  }, [bodyContent, title, loadRelated]);

  // Refresh related fragments when a fragment is deleted or index is updated
  useEffect(() => {
    const handleFragmentDeleted = () => { loadRelated(bodyContent || title); };
    window.addEventListener('fragment-deleted', handleFragmentDeleted);

    let unlisten: (() => void) | undefined;
    listen('index_updated', () => { loadRelated(bodyContent || title); }).then((fn) => { unlisten = fn; });

    return () => {
      window.removeEventListener('fragment-deleted', handleFragmentDeleted);
      unlisten?.();
    };
  }, [loadRelated, bodyContent, title]);

  // When currentArticleId changes and bodyContent is loaded, push it into the editor
  // Use the editor's setContent which handles markdown parsing properly
  useEffect(() => {
    if (currentArticleId && !loading && bodyContent !== undefined && editorRef.current) {
      editorRef.current.setContent(bodyContent);
    }
  }, [currentArticleId, bodyContent, loading]);

  // Keyboard shortcut ⌘⇧F for immersive mode
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
        e.preventDefault();
        toggleImmersive();
      }
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [toggleImmersive]);

  // Insert reference from left panel click
  const handleInsertRef = useCallback((fragment: Fragment) => {
    setInsertingId(fragment.id);
    editorRef.current?.insertReference(fragment.id);
    setTimeout(() => setInsertingId(null), 300);
  }, []);

  // Editor update handler (auto-save) — receives Markdown body from Editor
  const handleEditorUpdate = useCallback(
    (markdown: string) => {
      saveArticle(markdown);
    },
    [saveArticle],
  );

  // Word count handler
  const handleWordCountChange = useCallback(
    (count: number) => {
      setWordCount(count);
    },
    [setWordCount],
  );

  // Title change handler with debounced save
  const handleTitleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setTitle(e.target.value);

      if (titleSaveTimerRef.current) {
        clearTimeout(titleSaveTimerRef.current);
      }
      titleSaveTimerRef.current = setTimeout(() => {
        const markdown = editorRef.current?.getMarkdown() ?? '';
        saveArticle(markdown);
      }, 1000);
    },
    [setTitle, saveArticle],
  );

  // Cleanup title save timer
  useEffect(() => {
    return () => {
      if (titleSaveTimerRef.current) {
        clearTimeout(titleSaveTimerRef.current);
      }
    };
  }, []);

  // Mode toggle handler — delegates to the Editor component
  const handleModeToggle = useCallback((targetMode: EditorMode) => {
    if (targetMode === editorMode) return;
    // The Editor exposes mode switching via imperative handle indirectly,
    // but since mode is internal to Editor, we need a different approach.
    // We'll lift mode state to Compose and pass it to Editor.
    setEditorMode(targetMode);
  }, [editorMode]);

  // Toggle right AI panel
  const handleToggleRightPanel = useCallback(() => {
    setRightPanelOpen((v) => !v);
    if (!panelOpen) {
      togglePanel();
    }
  }, [panelOpen, togglePanel]);

  // Filter fragments by search query
  const filteredFragments = searchQuery
    ? relatedFragments.filter(
        (f) =>
          f.content.toLowerCase().includes(searchQuery.toLowerCase()) ||
          f.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase())),
      )
    : relatedFragments;

  return (
    <div className={`compose-shell${immersiveMode ? ' immersive' : ''}`}>
      {/* Left panel: related items */}
      <div className="ref-panel" style={{ width: leftPanel.width, flexShrink: 0 }}>
        <div className="ref-tabs">
          {TAB_LABELS.map((tab) => (
            <button
              key={tab.key}
              className={activeTab === tab.key ? 'on' : ''}
              onClick={() => setActiveTab(tab.key)}
            >
              {tab.label}
            </button>
          ))}
        </div>
        <input
          className="ref-search"
          type="text"
          placeholder="搜索相关内容…"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />
        <div className="ref-list">
          {activeTab === 'inspirations' && (
            <>
              {filteredFragments.length === 0 ? (
                <div className="ref-empty">暂无相关灵感碎片</div>
              ) : (
                filteredFragments.map((frag) => (
                  <div
                    key={frag.id}
                    className={`ref-item${insertingId === frag.id ? ' inserting' : ''}`}
                    onClick={() => handleInsertRef(frag)}
                  >
                    <div className="ri-text">{frag.content}</div>
                    <div className="ri-meta">
                      {frag.tags.map((tag) => (
                        <span key={tag} className="ri-tag">
                          {tag}
                        </span>
                      ))}
                      <span>{formatDate(frag.created_at)}</span>
                    </div>
                  </div>
                ))
              )}
            </>
          )}
          {activeTab === 'topics' && (
            <div className="ref-empty">话题功能将在后续版本启用</div>
          )}
          {activeTab === 'articles' && (
            <div className="ref-empty">相关文章功能将在后续版本启用</div>
          )}
        </div>
      </div>

      {/* Resize handle: left ↔ center */}
      <div className="resize-handle" onMouseDown={leftPanel.onMouseDown} />

      {/* Center: editor */}
      <div className="editor-area">
        {/* Top bar with controls */}
        <div className="compose-topbar">
          {/* Immersive mode button */}
          <button
            className={`compose-topbar__btn${immersiveMode ? ' compose-topbar__btn--active' : ''}`}
            onClick={toggleImmersive}
            title="沉浸式写作 ⌘⇧F"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
            </svg>
          </button>

          <div className="compose-topbar__spacer" />

          {/* Mode toggle: single button */}
          <button
            className="compose-topbar__btn"
            onClick={() => handleModeToggle(editorMode === 'wysiwyg' ? 'source' : 'wysiwyg')}
            title={editorMode === 'wysiwyg' ? '查看源码' : '返回编辑器'}
          >
            {editorMode === 'wysiwyg' ? (
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M16 18l6-6-6-6M8 6l-6 6 6 6" />
              </svg>
            ) : (
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                <path d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
              </svg>
            )}
          </button>

          {/* More actions */}
          <button className="compose-topbar__btn" title="更多操作">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <circle cx="12" cy="5" r="1.5" fill="currentColor" />
              <circle cx="12" cy="12" r="1.5" fill="currentColor" />
              <circle cx="12" cy="19" r="1.5" fill="currentColor" />
            </svg>
          </button>

          {/* AI panel toggle */}
          <button
            className={`compose-topbar__btn${rightPanelOpen ? ' compose-topbar__btn--active' : ''}`}
            onClick={handleToggleRightPanel}
            title="AI 写作助手"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4" />
            </svg>
          </button>

          {/* Collapse right panel */}
          <button
            className="compose-topbar__btn"
            onClick={() => setRightPanelOpen(false)}
            title="收起右侧面板"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <rect x="4" y="4" width="16" height="16" rx="2" />
              <path d="M15 4v16" />
              <path d="M19 12l-2 2M19 12l-2-2" />
            </svg>
          </button>
        </div>

        {currentArticleId && (
          <div className="compose-title-bar">
            <input
              className="doc-title"
              type="text"
              placeholder="标题…"
              value={title}
              onChange={handleTitleChange}
            />
            <span className={`status ${status}`} onClick={cycleStatus}>
              {STATUS_LABELS[status] ?? status}
            </span>
          </div>
        )}

        <div className="editor-body">
          {!currentArticleId && !loading ? (
            <div className="compose-empty">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M12 20h9M16.5 3.5a2.12 2.12 0 013 3L7 19l-4 1 1-4L16.5 3.5z" />
              </svg>
              <p>点击「+ 新建」或按 ⌘N 开始创作</p>
            </div>
          ) : (
            <>
              <div className="doc-meta">
                <span>{wordCount} 字</span>
                {updatedAt && <span>最后编辑：{formatUpdatedAt(updatedAt)}</span>}
              </div>
              <Editor
                ref={editorRef}
                content={bodyContent}
                onUpdate={handleEditorUpdate}
                onWordCountChange={handleWordCountChange}
                mode={editorMode}
                onModeChange={setEditorMode}
              />
            </>
          )}
        </div>
      </div>

      {/* Resize handle: center ↔ right (only when right panel is open) */}
      {rightPanelOpen && (
        <div className="resize-handle" onMouseDown={rightPanel.onMouseDown} />
      )}

      {/* Right: AI panel (default collapsed) */}
      <div
        className={`ai-side${rightPanelOpen ? '' : ' collapsed'}`}
        style={rightPanelOpen ? { width: rightPanel.width, flexShrink: 0 } : undefined}
      >
        <WritingPanel />
      </div>
    </div>
  );
}

/** Format a date string to "M月D日" */
function formatDate(iso: string): string {
  const d = new Date(iso);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}
