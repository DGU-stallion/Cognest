import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useArticlesStore } from '../stores/articlesStore';
import { useComposeStore } from '../stores/composeStore';
import { useAppStore } from '../stores/appStore';
import type { Article, ArticleStatus } from '../stores/articlesStore';
import './Articles.css';

const STATUS_OPTIONS: { key: ArticleStatus | 'all'; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'draft', label: '草稿' },
  { key: 'editing', label: '修改中' },
  { key: 'done', label: '已完成' },
];

const STATUS_LABELS: Record<ArticleStatus, string> = {
  draft: '草稿',
  editing: '修改中',
  done: '已完成',
};

/** Format word count with thousands separator */
function formatWordCount(count: number): string {
  return count.toLocaleString('zh-CN');
}

/** Format date for article list row */
function formatDate(isoStr: string): string {
  const date = new Date(isoStr);
  const now = new Date();
  const toDateKey = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

  const todayKey = toDateKey(now);
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const yesterdayKey = toDateKey(yesterday);
  const dateKey = toDateKey(date);

  if (dateKey === todayKey) return '今天';
  if (dateKey === yesterdayKey) return '昨天';
  return `${date.getMonth() + 1}月${date.getDate()}日`;
}

/** Format date with time for preview panel */
function formatDateTime(isoStr: string): string {
  const date = new Date(isoStr);
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  return `最后编辑：${formatDate(isoStr)} ${hours}:${minutes}`;
}

/** Extract all unique tags from articles */
function extractAllTags(articles: Article[]): string[] {
  const tagSet = new Set<string>();
  for (const article of articles) {
    for (const tag of article.tags) {
      tagSet.add(tag);
    }
  }
  return [...tagSet];
}

/** Get excerpt: first 50 characters of content (strip markdown) */
function getExcerpt(content: string): string {
  // Strip markdown syntax for cleaner excerpt
  const plain = content
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/[*_~`>]/g, '')
    .replace(/\n+/g, ' ')
    .trim();
  return plain.length > 50 ? plain.slice(0, 50) + '…' : plain;
}

export default function Articles() {
  const {
    articles,
    selectedId,
    statusFilter,
    tagFilter,
    searchQuery,
    loading,
    loadArticles,
    setStatusFilter,
    toggleTag,
    setSearchQuery,
    selectArticle,
    deleteArticle,
  } = useArticlesStore();

  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  useEffect(() => {
    loadArticles();
  }, [loadArticles]);

  // Filter articles
  const filteredArticles = articles
    .filter((a) => {
      if (statusFilter !== 'all' && a.status !== statusFilter) return false;
      if (tagFilter.length > 0 && !tagFilter.every((t) => a.tags.includes(t))) return false;
      if (searchQuery) {
        const q = searchQuery.toLowerCase();
        if (!a.title.toLowerCase().includes(q) && !a.content.toLowerCase().includes(q)) return false;
      }
      return true;
    })
    .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime());

  const selectedArticle = articles.find((a) => a.id === selectedId) ?? null;
  const allTags = extractAllTags(articles);

  const handleEdit = useCallback(() => {
    if (!selectedArticle) return;
    // Load article into compose store and switch to compose page
    useComposeStore.getState().loadArticle(selectedArticle.id);
    useAppStore.getState().setCurrentPage('compose');
  }, [selectedArticle]);

  const handleExport = useCallback(async () => {
    if (!selectedArticle) return;
    try {
      await invoke('export_article', { id: selectedArticle.id });
    } catch (e) {
      console.error('Failed to export article:', e);
    }
  }, [selectedArticle]);

  const handleDeleteConfirm = useCallback(async () => {
    if (!confirmDeleteId) return;
    await deleteArticle(confirmDeleteId);
    setConfirmDeleteId(null);
  }, [confirmDeleteId, deleteArticle]);

  return (
    <div className="articles-shell">
      {/* Left: list panel */}
      <div className="art-list-panel">
        <div className="art-header">
          <h1>文章</h1>
          <div className="art-bar">
            <input
              className="art-search"
              type="text"
              placeholder="搜索文章标题或内容…"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
            />
            <div className="filters">
              {STATUS_OPTIONS.map((opt) => (
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

        {/* Tag filter row */}
        {allTags.length > 0 && (
          <div className="tag-row">
            {allTags.map((tag) => (
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

        {/* Article list */}
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
            filteredArticles.map((article) => (
              <ArticleRow
                key={article.id}
                article={article}
                selected={article.id === selectedId}
                onSelect={() => selectArticle(article.id)}
              />
            ))
          )}
        </div>
      </div>

      {/* Right: preview panel */}
      <div className={`preview-panel ${!selectedArticle ? 'empty' : ''}`}>
        {selectedArticle ? (
          <ArticlePreview
            article={selectedArticle}
            onEdit={handleEdit}
            onExport={handleExport}
            onDelete={() => setConfirmDeleteId(selectedArticle.id)}
          />
        ) : (
          <div className="preview-empty">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M4 6h16M4 12h16M4 18h12" />
            </svg>
            <p>选择一篇文章以预览</p>
          </div>
        )}
      </div>

      {/* Delete confirmation dialog */}
      {confirmDeleteId && (
        <div className="confirm-overlay" onClick={() => setConfirmDeleteId(null)}>
          <div className="confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <h3>确认删除</h3>
            <p>确定要删除这篇文章吗？此操作不可撤销。</p>
            <div className="confirm-actions">
              <button className="btn-secondary" onClick={() => setConfirmDeleteId(null)}>
                取消
              </button>
              <button className="btn-danger" onClick={handleDeleteConfirm}>
                删除
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ArticleRow({
  article,
  selected,
  onSelect,
}: {
  article: Article;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <div className={`art-row ${selected ? 'selected' : ''}`} onClick={onSelect}>
      <div className="ar-main">
        <div className="ar-title">{article.title}</div>
        <div className="ar-excerpt">{getExcerpt(article.content)}</div>
      </div>
      <div className="ar-meta">
        {article.tags.length > 0 && (
          <div className="ar-tags">
            {article.tags.map((tag) => (
              <span key={tag} className="ar-tag">
                {tag}
              </span>
            ))}
          </div>
        )}
        <span className={`ar-status ${article.status}`}>{STATUS_LABELS[article.status]}</span>
        <span className="ar-words">{formatWordCount(article.word_count)}</span>
        <span className="ar-date">{formatDate(article.updated_at)}</span>
      </div>
    </div>
  );
}

function ArticlePreview({
  article,
  onEdit,
  onExport,
  onDelete,
}: {
  article: Article;
  onEdit: () => void;
  onExport: () => void;
  onDelete: () => void;
}) {
  return (
    <>
      <div className="preview-head">
        <h2>{article.title}</h2>
        <div className="pm">
          <span className={`ar-status ${article.status}`}>{STATUS_LABELS[article.status]}</span>
          <span>{formatWordCount(article.word_count)} 字</span>
          <span>{formatDateTime(article.updated_at)}</span>
        </div>
      </div>
      <div className="preview-actions">
        <button className="btn-primary" onClick={onEdit}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" width="14" height="14">
            <path d="M12 20h9M16.5 3.5a2.12 2.12 0 013 3L7 19l-4 1 1-4L16.5 3.5z" />
          </svg>
          编辑
        </button>
        <button className="btn-secondary" onClick={onExport}>
          导出
        </button>
        <button className="btn-secondary btn-delete" onClick={onDelete}>
          删除
        </button>
      </div>
      {article.tags.length > 0 && (
        <div className="preview-tags">
          {article.tags.map((tag) => (
            <span key={tag} className="ar-tag">
              {tag}
            </span>
          ))}
        </div>
      )}
      <div className="preview-body" dangerouslySetInnerHTML={{ __html: renderMarkdown(article.content) }} />
    </>
  );
}

/** Simple markdown renderer — handles headers, paragraphs, blockquotes, bold, italic */
function renderMarkdown(md: string): string {
  const lines = md.split('\n');
  let html = '';
  let inBlockquote = false;

  for (const line of lines) {
    const trimmed = line.trim();

    if (trimmed === '') {
      if (inBlockquote) {
        html += '</blockquote>';
        inBlockquote = false;
      }
      continue;
    }

    // Headers
    const headerMatch = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (headerMatch) {
      if (inBlockquote) {
        html += '</blockquote>';
        inBlockquote = false;
      }
      const level = headerMatch[1].length;
      html += `<h${level + 2}>${escapeHtml(headerMatch[2])}</h${level + 2}>`;
      continue;
    }

    // Blockquote
    if (trimmed.startsWith('>')) {
      const content = trimmed.slice(1).trim();
      if (!inBlockquote) {
        html += '<blockquote>';
        inBlockquote = true;
      }
      html += escapeHtml(content) + ' ';
      continue;
    }

    if (inBlockquote) {
      html += '</blockquote>';
      inBlockquote = false;
    }

    // Paragraph with inline formatting
    html += `<p>${inlineFormat(trimmed)}</p>`;
  }

  if (inBlockquote) {
    html += '</blockquote>';
  }

  return html;
}

function inlineFormat(text: string): string {
  let result = escapeHtml(text);
  // Bold
  result = result.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  // Italic
  result = result.replace(/\*(.+?)\*/g, '<em>$1</em>');
  // Inline code
  result = result.replace(/`(.+?)`/g, '<code>$1</code>');
  return result;
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
