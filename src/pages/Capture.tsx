import { useCallback, useEffect, useRef, useState } from 'react';
import { useCaptureStore } from '../stores/captureStore';
import type { Fragment, FragmentFilter } from '../stores/captureStore';
import './Capture.css';

/** Format date label like "今天 · 6月24日", "昨天 · 6月23日", or "6月22日" */
function formatDayLabel(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();

  const toDateKey = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

  const todayKey = toDateKey(now);
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const yesterdayKey = toDateKey(yesterday);
  const dateKey = toDateKey(date);

  const month = date.getMonth() + 1;
  const day = date.getDate();
  const datePart = `${month}月${day}日`;

  if (dateKey === todayKey) return `今天 · ${datePart}`;
  if (dateKey === yesterdayKey) return `昨天 · ${datePart}`;
  return datePart;
}

/** Get date key (YYYY-MM-DD) from ISO string for grouping */
function getDateKey(isoStr: string): string {
  const d = new Date(isoStr);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** Format time as HH:mm */
function formatTime(isoStr: string): string {
  const d = new Date(isoStr);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** Group fragments by date, sorted descending (most recent day first), items within each group sorted time-descending */
function groupByDate(fragments: Fragment[]): { dateKey: string; label: string; items: Fragment[] }[] {
  const groups = new Map<string, Fragment[]>();

  for (const frag of fragments) {
    const key = getDateKey(frag.created_at);
    const existing = groups.get(key);
    if (existing) {
      existing.push(frag);
    } else {
      groups.set(key, [frag]);
    }
  }

  // Sort groups by date descending
  const sortedKeys = [...groups.keys()].sort((a, b) => b.localeCompare(a));

  return sortedKeys.map((key) => {
    const items = groups.get(key)!;
    // Sort items within group by time descending
    items.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
    return {
      dateKey: key,
      label: formatDayLabel(items[0].created_at),
      items,
    };
  });
}

const FILTER_OPTIONS: { key: FragmentFilter; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'uncategorized', label: '未整理' },
  { key: 'categorized', label: '已归类' },
];

export default function Capture() {
  const { fragments, filter, totalCount, loading, loadFragments, setFilter, addFragment } =
    useCaptureStore();
  const [inputValue, setInputValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    loadFragments();
  }, [loadFragments]);

  const handleSubmit = useCallback(async () => {
    const trimmed = inputValue.trim();
    if (!trimmed) return;
    try {
      await addFragment(trimmed);
      setInputValue('');
    } catch (e) {
      console.error('Failed to create fragment:', e);
    }
  }, [inputValue, addFragment]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        handleSubmit();
      }
    },
    [handleSubmit],
  );

  const groups = groupByDate(fragments);

  return (
    <div className="inbox" style={{ overflowY: 'auto', height: '100%' }}>
      {/* Header */}
      <div className="inbox-head">
        <h1>碎片</h1>
        <p>你的所有原始想法。记录即完成，AI 会帮你整理。</p>
      </div>

      {/* Quick input */}
      <div className="quick-input">
        <input
          ref={inputRef}
          type="text"
          placeholder="写下你的想法…"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onKeyDown={handleKeyDown}
        />
        <button className="btn-primary" onClick={handleSubmit}>
          记录
        </button>
      </div>

      {/* Filter bar */}
      <div className="filter-bar">
        {FILTER_OPTIONS.map((opt) => (
          <button
            key={opt.key}
            className={`chip ${filter === opt.key ? 'active' : ''}`}
            onClick={() => setFilter(opt.key)}
          >
            {opt.label}
          </button>
        ))}
        <span className="count">共 {totalCount} 条</span>
      </div>

      {/* Fragment list */}
      {loading ? (
        <div className="capture-empty">
          <p>加载中…</p>
        </div>
      ) : groups.length === 0 ? (
        <div className="capture-empty">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M4 4h16v16H4z" />
            <path d="M9 9h6M9 13h4" />
          </svg>
          <p>还没有碎片</p>
          <p>在上方输入你的第一条想法</p>
        </div>
      ) : (
        <div>
          {groups.map((group) => (
            <div className="day-group" key={group.dateKey}>
              <div className="day-label">{group.label}</div>
              {group.items.map((frag) => (
                <FragmentItem key={frag.id} fragment={frag} />
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function FragmentItem({ fragment }: { fragment: Fragment }) {
  const isCategorized = fragment.topics.length > 0;

  return (
    <div className="frag">
      <div className="frag-content">{fragment.content}</div>
      <div className="frag-meta">
        <span className="frag-time">{formatTime(fragment.created_at)}</span>
        {fragment.tags.map((tag) => (
          <span key={tag} className="frag-tag">
            {tag}
          </span>
        ))}
        {fragment.topics.map((topic) => (
          <span key={topic} className="frag-tag topic">
            {topic}
          </span>
        ))}
      </div>

      {/* Categorized check icon (hidden on hover) */}
      {isCategorized && (
        <svg className="frag-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M20 6L9 17l-5-5" />
        </svg>
      )}

      {/* Hover action buttons */}
      <div className="frag-actions">
        <button title="引用到创作">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 20h9M16.5 3.5a2.12 2.12 0 013 3L7 19l-4 1 1-4z" />
          </svg>
        </button>
        <button title="查看关联">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="6" cy="12" r="3" />
            <circle cx="18" cy="6" r="3" />
            <path d="M8.6 10.5l6.3-3" />
          </svg>
        </button>
      </div>
    </div>
  );
}
