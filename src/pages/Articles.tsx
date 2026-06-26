import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useArticlesStore } from '../stores/articlesStore';
import { useComposeStore } from '../stores/composeStore';
import { useAppStore } from '../stores/appStore';
import type { Article, ArticleStatus } from '../stores/articlesStore';
import './Articles.css';

const STATUS_OPTIONS: { key: ArticleStatus | 'all'; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'draft', label: '草稿' },
  { key: 'archived', label: '已归档' },
];

const STATUS_LABELS: Record<ArticleStatus, string> = {
  draft: '草稿',
  archived: '已归档',
};

function formatWordCount(count: number): string {
  return (count ?? 0).toLocaleString('zh-CN');
}

function formatDate(isoStr: string): string {
  if (!isoStr) return '';
  const date = new Date(isoStr);
  const now = new Date();
  const toKey = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const todayKey = toKey(now);
  const yday = new Date(now); yday.setDate(yday.getDate() - 1);
  const key = toKey(date);
  if (key === todayKey) return '今天';
  if (key === toKey(yday)) return '昨天';
  return `${date.getMonth() + 1}月${date.getDate()}日`;
}

function formatDateTime(isoStr: string): string {
  if (!isoStr) return '';
  const d = new Date(isoStr);
  return `最后编辑：${formatDate(isoStr)} ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}`;
}

function extractAllTags(articles: Article[]): string[] {
  const s = new Set<string>();
  articles.forEach(a => a.tags.forEach(t => s.add(t)));
  return [...s];
}

function getExcerpt(content: string): string {
  if (!content) return '';
  const plain = content
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/[*_~`>]/g, '')
    .replace(/\n+/g, ' ')
    .trim();
  return plain.length > 60 ? plain.slice(0, 60) + '…' : plain;
}

// ─── Resizable handle hook (fixed closure bug) ────────────────────────────────

function useResizable(initialWidth: number, min: number, max: number) {
  const [width, setWidth] = useState(initialWidth);
  const widthRef = useRef(initialWidth);
  const dragging = useRef(false);
  const startX = useRef(0);
  const startW = useRef(initialWidth);

  // Keep ref in sync
  useEffect(() => { widthRef.current = width; }, [width]);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    startX.current = e.clientX;
    startW.current = widthRef.current;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';

    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      const delta = ev.clientX - startX.current;
      const newWidth = Math.min(max, Math.max(min, startW.current + delta));
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
  }, [min, max]);

  return { width, onMouseDown };
}

// ─── Main component ────────────────────────────────────────────────────────────

export default function Articles() {
  const {
    articles, selectedId, selectedArticleContent, statusFilter, tagFilter, searchQuery, loading,
    loadArticles, setStatusFilter, toggleTag, setSearchQuery, selectArticle, deleteArticle,
  } = useArticlesStore();

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const preview = useResizable(380, 240, 600);

  useEffect(() => { loadArticles(); }, [loadArticles]);

  const filteredArticles = articles
    .filter(a => {
      if (statusFilter !== 'all' && a.status !== statusFilter) return false;
      if (tagFilter.length > 0 && !tagFilter.every(t => a.tags.includes(t))) return false;
      if (searchQuery) {
        const q = searchQuery.toLowerCase();
        if (!a.title.toLowerCase().includes(q) && !(a.content || '').toLowerCase().includes(q)) return false;
      }
      return true;
    })
    .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime());

  const selectedArticle = articles.find(a => a.id === selectedId) ?? null;
  const allTags = extractAllTags(articles);

  const handleOpenEdit = useCallback((articleId?: string) => {
    const id = articleId ?? selectedArticle?.id;
    if (!id) return;
    useComposeStore.getState().loadArticle(id);
    useAppStore.getState().setCurrentPage('compose');
  }, [selectedArticle]);

  const handleExport = useCallback(async (id: string) => {
    try { await invoke('export_article', { id }); } catch (e) { console.error(e); }
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!confirmDeleteId) return;
    await deleteArticle(confirmDeleteId);
    setConfirmDeleteId(null);
  }, [confirmDeleteId, deleteArticle]);

  return (
    <div className="articles-shell">
      {/* Left: list panel (flex:1, fills remaining space) */}
      <div className="art-list-panel">
        <div className="art-header">
          <h1>文章</h1>
          <div className="art-bar">
            <input
              className="art-search"
              type="text"
              placeholder="搜索文章标题或内容…"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
            />
            <div className="filters">
              {STATUS_OPTIONS.map(opt => (
                <button
                  key={opt.key}
                  className={statusFilter === opt.key ? 'on' : ''}
                  onClick={() => setStatusFilter(opt.key)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
        </div>

        {allTags.length > 0 && (
          <div className="tag-row">
            {allTags.map(tag => (
              <span
                key={tag}
                className={`tag-chip ${tagFilter.includes(tag) ? 'on' : ''}`}
                onClick={() => toggleTag(tag)}
              >
                {tag}
              </span>
            ))}
          </div>
        )}

        <div className="art-list">
          {loading ? (
            <div className="art-empty">加载中…</div>
          ) : filteredArticles.length === 0 ? (
            <div className="art-empty">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M4 6h16M4 12h16M4 18h12" />
              </svg>
              <p>暂无文章</p>
            </div>
          ) : (
            filteredArticles.map(article => (
              <ArticleRow
                key={article.id}
                article={article}
                selected={article.id === selectedId}
                onSelect={() => selectArticle(article.id)}
                onEdit={() => {
                  handleOpenEdit(article.id);
                }}
                onExport={() => handleExport(article.id)}
                onDelete={() => setConfirmDeleteId(article.id)}
              />
            ))
          )}
        </div>
      </div>

      {/* Resize handle between list and preview */}
      <div className="resize-handle" onMouseDown={preview.onMouseDown} title="拖拽调整宽度" />

      {/* Right: preview panel (resizable width) — clicking navigates to compose */}
      <div
        className={`preview-panel ${!selectedArticle ? 'empty' : ''}`}
        style={{ width: preview.width, flexShrink: 0 }}
        onClick={selectedArticle ? () => handleOpenEdit() : undefined}
      >
        {selectedArticle ? (
          <ArticlePreview article={selectedArticle} content={selectedArticleContent} />
        ) : (
          <div className="preview-empty">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M4 6h16M4 12h16M4 18h12" />
            </svg>
            <p>选择一篇文章以预览</p>
          </div>
        )}
      </div>

      {/* Delete confirmation */}
      {confirmDeleteId && (
        <div className="confirm-overlay" onClick={() => setConfirmDeleteId(null)}>
          <div className="confirm-dialog" onClick={e => e.stopPropagation()}>
            <h3>确认删除</h3>
            <p>确定要删除这篇文章吗？此操作不可撤销。</p>
            <div className="confirm-actions">
              <button className="btn-secondary" onClick={() => setConfirmDeleteId(null)}>取消</button>
              <button className="btn-danger" onClick={handleDeleteConfirm}>删除</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── ArticleRow with hover ⋯ menu ─────────────────────────────────────────────

function ArticleRow({
  article, selected, onSelect, onEdit, onExport, onDelete,
}: {
  article: Article;
  selected: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onExport: () => void;
  onDelete: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number }>({ top: 0, left: 0 });
  const menuRef = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);

  // Close menu on outside click
  useEffect(() => {
    if (!menuOpen) return;
    function handleClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [menuOpen]);

  const openMenu = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (btnRef.current) {
      const rect = btnRef.current.getBoundingClientRect();
      setMenuPos({ top: rect.bottom + 4, left: rect.right - 120 });
    }
    setMenuOpen(v => !v);
  }, []);

  return (
    <div
      className={`art-row ${selected ? 'selected' : ''}`}
      onClick={onSelect}
      onDoubleClick={onEdit}
      onContextMenu={e => { e.preventDefault(); setMenuPos({ top: e.clientY, left: e.clientX }); setMenuOpen(true); }}
    >
      <div className="ar-main">
        <div className="ar-title">{article.title || '无标题文章'}</div>
        <div className="ar-excerpt">{getExcerpt(article.content)}</div>
      </div>
      <div className="ar-meta">
        {article.tags.length > 0 && (
          <div className="ar-tags">
            {article.tags.map(tag => <span key={tag} className="ar-tag">{tag}</span>)}
          </div>
        )}
        <span className="ar-words">{formatWordCount(article.word_count)}</span>
        <span className="ar-date">{formatDate(article.updated_at)}</span>

        {/* ⋯ more button — shown on hover */}
        <div className="ar-more-wrap">
          <button
            ref={btnRef}
            className="ar-more-btn"
            title="更多操作"
            onClick={openMenu}
          >
            <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
              <circle cx="4.5" cy="9" r="1" fill="currentColor" />
              <circle cx="9" cy="9" r="1" fill="currentColor" />
              <circle cx="13.5" cy="9" r="1" fill="currentColor" />
            </svg>
          </button>
          {menuOpen && (
            <div
              ref={menuRef}
              className="ar-dropdown"
              style={{ top: menuPos.top, left: menuPos.left }}
              onClick={e => e.stopPropagation()}
            >
              <button onClick={() => { setMenuOpen(false); onEdit(); }}>
                <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path d="M9 15h6.75M12.375 2.625a1.59 1.59 0 0 1 2.25 2.25L5.25 14.25l-3 .75.75-3z" />
                </svg>
                编辑
              </button>
              <button onClick={() => { setMenuOpen(false); onExport(); }}>
                <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path d="M3 12v3h12v-3M9 3v9M6 9l3 3 3-3" />
                </svg>
                导出
              </button>
              <button className="danger" onClick={() => { setMenuOpen(false); onDelete(); }}>
                <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path d="M3 4.5h12M7.5 7.5v6M10.5 7.5v6M4.5 4.5l.75 9h7.5l.75-9" />
                </svg>
                删除
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Preview panel — clickable to open editor, shows fetched content ─────────

function ArticlePreview({ article, content }: { article: Article; content: string }) {
  return (
    <div style={{ cursor: 'pointer' }}>
      <div className="preview-head">
        <h2>{article.title || '无标题文章'}</h2>
        <div className="pm">
          <span>{formatWordCount(article.word_count)} 字</span>
          <span>{formatDateTime(article.updated_at)}</span>
        </div>
      </div>
      {article.tags.length > 0 && (
        <div className="preview-tags">
          {article.tags.map(tag => <span key={tag} className="ar-tag">{tag}</span>)}
        </div>
      )}
      <div className="preview-body" dangerouslySetInnerHTML={{ __html: content || '' }} />
    </div>
  );
}

// ─── Markdown renderer ────────────────────────────────────────────────────────

function renderMarkdown(md: string): string {
  if (!md) return '';
  const lines = md.split('\n');
  let html = '';
  let inBlockquote = false;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed === '') {
      if (inBlockquote) { html += '</blockquote>'; inBlockquote = false; }
      continue;
    }
    const hm = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (hm) {
      if (inBlockquote) { html += '</blockquote>'; inBlockquote = false; }
      html += `<h${hm[1].length + 2}>${esc(hm[2])}</h${hm[1].length + 2}>`;
      continue;
    }
    if (trimmed.startsWith('>')) {
      if (!inBlockquote) { html += '<blockquote>'; inBlockquote = true; }
      html += esc(trimmed.slice(1).trim()) + ' ';
      continue;
    }
    if (inBlockquote) { html += '</blockquote>'; inBlockquote = false; }
    html += `<p>${fmt(trimmed)}</p>`;
  }
  if (inBlockquote) html += '</blockquote>';
  return html;
}

function fmt(t: string) {
  let r = esc(t);
  r = r.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  r = r.replace(/\*(.+?)\*/g, '<em>$1</em>');
  r = r.replace(/`(.+?)`/g, '<code>$1</code>');
  return r;
}

function esc(s: string) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}
