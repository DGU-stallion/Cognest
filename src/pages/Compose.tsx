import { useCallback, useEffect, useRef, useState } from 'react';
import { useComposeStore } from '../stores/composeStore';
import type { Fragment } from '../stores/captureStore';
import type { EditorHandle } from '../components/Editor';
import Editor from '../components/Editor';
import './Compose.css';

type RefTab = 'inspirations' | 'topics' | 'articles';

const TAB_LABELS: { key: RefTab; label: string }[] = [
  { key: 'inspirations', label: '灵感' },
  { key: 'topics', label: '话题' },
  { key: 'articles', label: '文章' },
];

const STATUS_LABELS: Record<string, string> = {
  draft: '草稿',
  editing: '编辑中',
  completed: '已完成',
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
    createNewArticle,
  } = useComposeStore();

  const [activeTab, setActiveTab] = useState<RefTab>('inspirations');
  const [searchQuery, setSearchQuery] = useState('');
  const [insertingId, setInsertingId] = useState<string | null>(null);
  const editorRef = useRef<EditorHandle>(null);
  const titleSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Load related fragments on mount
  useEffect(() => {
    loadRelated();
  }, [loadRelated]);

  // When currentArticleId changes and bodyContent is loaded, push it into the editor
  useEffect(() => {
    if (currentArticleId && bodyContent !== undefined && editorRef.current) {
      editorRef.current.setContent(bodyContent);
    }
  }, [currentArticleId, bodyContent]);

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

  // Editor update handler (auto-save)
  const handleEditorUpdate = useCallback(
    (html: string) => {
      saveArticle(html);
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

      // Debounce title save (1s)
      if (titleSaveTimerRef.current) {
        clearTimeout(titleSaveTimerRef.current);
      }
      titleSaveTimerRef.current = setTimeout(() => {
        // Trigger a save with current editor content
        const html = editorRef.current?.getHTML() ?? '';
        saveArticle(html);
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

  // Toolbar button handlers
  const handleBold = useCallback(() => {
    editorRef.current?.getEditor()?.chain().focus().toggleBold().run();
  }, []);

  const handleItalic = useCallback(() => {
    editorRef.current?.getEditor()?.chain().focus().toggleItalic().run();
  }, []);

  const handleCode = useCallback(() => {
    editorRef.current?.getEditor()?.chain().focus().toggleCode().run();
  }, []);

  const handleBlockquote = useCallback(() => {
    editorRef.current?.getEditor()?.chain().focus().toggleBlockquote().run();
  }, []);

  // Handle new article creation
  const handleNewArticle = useCallback(() => {
    createNewArticle();
  }, [createNewArticle]);

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
      <div className="ref-panel">
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

      {/* Center: editor */}
      <div className="editor-area">
        <div className="editor-toolbar">
          <button title="新建文章 (⌘N)" onClick={handleNewArticle} className="new-article-btn">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 5v14M5 12h14" />
            </svg>
            新建
          </button>
          <div className="sep" />
          <button title="加粗" onClick={handleBold}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <path d="M6 4h8a4 4 0 014 4 4 4 0 01-4 4H6zM6 12h9a4 4 0 014 4 4 4 0 01-4 4H6z" />
            </svg>
          </button>
          <button title="斜体" onClick={handleItalic}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M19 4h-9M14 20H5M15 4L9 20" />
            </svg>
          </button>
          <button title="代码" onClick={handleCode}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M16 18l6-6-6-6M8 6l-6 6 6 6" />
            </svg>
          </button>
          <button title="引用" onClick={handleBlockquote}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M10 11h-4a1 1 0 01-1-1V6a1 1 0 011-1h3a1 1 0 011 1v5zm0 0a4 4 0 01-4 4M19 11h-4a1 1 0 01-1-1V6a1 1 0 011-1h3a1 1 0 011 1v5zm0 0a4 4 0 01-4 4" />
            </svg>
          </button>
          <div className="sep" />
          <button title="插入碎片引用">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M13.19 8.688a4.5 4.5 0 011.242 7.244l-4.5 4.5a4.5 4.5 0 01-6.364-6.364l1.757-1.757m9.9-1.026a4.5 4.5 0 00-1.242-7.244l-4.5-4.5a4.5 4.5 0 00-6.364 6.364l1.757 1.757" />
            </svg>
          </button>
          <div className="spacer" />
          <button className="immerse" onClick={toggleImmersive} title="⌘⇧F">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
            </svg>
            沉浸模式
          </button>
        </div>

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
              <input
                className="doc-title"
                type="text"
                placeholder="标题…"
                value={title}
                onChange={handleTitleChange}
              />
              <div className="doc-meta">
                <span className={`status ${status}`} onClick={cycleStatus}>
                  {STATUS_LABELS[status]}
                </span>
                <span>{wordCount} 字</span>
                {updatedAt && <span>最后编辑：{formatUpdatedAt(updatedAt)}</span>}
              </div>
              <Editor
                ref={editorRef}
                content={bodyContent}
                onUpdate={handleEditorUpdate}
                onWordCountChange={handleWordCountChange}
              />
            </>
          )}
        </div>
      </div>

      {/* Right: AI panel */}
      <div className="ai-side">
        <div className="ai-header">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4" />
          </svg>
          <span>Writing Agent</span>
        </div>
        <div className="ai-placeholder">
          <p>AI 辅助将在后续版本启用</p>
        </div>
      </div>
    </div>
  );
}

/** Format a date string to "M月D日" */
function formatDate(iso: string): string {
  const d = new Date(iso);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}
