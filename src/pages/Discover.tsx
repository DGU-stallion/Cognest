import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useDiscoverStore } from '../stores/discoverStore';
import type { FeedCard } from '../stores/discoverStore';
import { useViewStackStore } from '../stores/viewStackStore';
import { useViewStore } from '../stores/viewStore';
import { GenerationBar } from '../components/GenerationBar';
import { ViewRenderer } from '../components/ViewRenderer';
import './Discover.css';

/** Format today's date as "2026年6月24日 · 周二" style */
function formatDateEyebrow(): string {
  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth() + 1;
  const day = now.getDate();
  const weekdays = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
  const weekday = weekdays[now.getDay()];
  return `${year}年${month}月${day}日 · ${weekday}`;
}

export default function Discover() {
  const { cards, dismissedCards, loading, loadCards, dismissCard } = useDiscoverStore();
  const { pinnedViews, loadPinnedViews, unpinView } = useViewStore();

  useEffect(() => {
    loadCards();
    loadPinnedViews();
  }, [loadCards, loadPinnedViews]);

  // Listen for new_feed_card Tauri event → refresh pinned views & feed cards
  useEffect(() => {
    let unmounted = false;

    const unlistenPromise = listen('new_feed_card', () => {
      if (!unmounted) {
        loadPinnedViews();
        loadCards();
      }
    });

    return () => {
      unmounted = true;
      unlistenPromise.then((fn) => fn());
    };
  }, [loadPinnedViews, loadCards]);

  // Visible cards (not dismissed)
  const visibleCards = cards.filter((c) => !dismissedCards.has(c.id));

  // Check if we're in empty state: no fragments at all (no cards means no data)
  const hasNoData = !loading && cards.length === 0;
  // If all cards are the top-tags card with empty tags, it's also effectively "no data"
  const statsCard = cards.find((c) => c.type === 'stats');
  const isEmpty = hasNoData || (!loading && !statsCard);

  // Check if there's any content at all (pinned views count as content)
  const hasPinnedViews = pinnedViews.length > 0;

  return (
    <div className="discover-feed">
      {/* Header */}
      <div className="discover-head">
        <div className="eyebrow">{formatDateEyebrow()}</div>
        <h1>发现</h1>
        <p>规则引擎正在帮你整理碎片、发现模式。这里是本周的洞察。</p>
      </div>

      {/* AI View Generation */}
      <GenerationBar />

      {/* Pinned Views Section */}
      {hasPinnedViews && (
        <div className="pinned-views-section">
          <h2 className="pinned-views-title">固定视图</h2>
          {pinnedViews.map((view) => (
            <PinnedViewCard key={view.id} view={view} onUnpin={unpinView} />
          ))}
        </div>
      )}

      {/* Loading state */}
      {loading && (
        <div className="discover-empty">
          <p>加载中…</p>
        </div>
      )}

      {/* Empty state */}
      {isEmpty && !loading && !hasPinnedViews && (
        <div className="discover-empty">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          <p>还没有新发现——继续记录碎片，AI 会帮你整理</p>
        </div>
      )}

      {/* Feed cards */}
      {!loading && !isEmpty && visibleCards.map((card) => (
        <FeedCardItem key={card.id} card={card} onDismiss={dismissCard} />
      ))}
    </div>
  );
}

// ─── Pinned View Card ─────────────────────────────────────────────────────────

interface PinnedViewCardProps {
  view: import('../stores/viewStore').ViewSpec;
  onUnpin: (viewId: string) => Promise<void>;
}

function PinnedViewCard({ view, onUnpin }: PinnedViewCardProps) {
  const handleUnpin = useCallback(() => {
    onUnpin(view.id);
  }, [view.id, onUnpin]);

  return (
    <div className="discover-card pinned-view-card">
      <div className="card-top">
        <span className="card-kind">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 2v8m0 0l4-3m-4 3l-4-3M5 21l7-4 7 4V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16z" />
          </svg>
          {view.type === 'summary' ? '回顾' : '视图'}
        </span>
        <button className="card-dismiss" onClick={handleUnpin} title="取消固定" aria-label="取消固定视图">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
      <h3>{view.title}</h3>
      <div className="pinned-view-body">
        <ViewRenderer spec={view} />
      </div>
    </div>
  );
}

// ─── Feed Card Item ───────────────────────────────────────────────────────────

interface FeedCardItemProps {
  card: FeedCard;
  onDismiss: (id: string) => void;
}

function FeedCardItem({ card, onDismiss }: FeedCardItemProps) {
  const push = useViewStackStore((s) => s.push);
  const [dismissing, setDismissing] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);

  const handleDismiss = useCallback(() => {
    setDismissing(true);
    setTimeout(() => {
      onDismiss(card.id);
    }, 220);
  }, [card.id, onDismiss]);

  const handleViewDetail = useCallback(() => {
    push('discover', {
      id: `discover-detail-${card.id}`,
      component: 'DiscoverDetail',
      props: { cardId: card.id, cardType: card.type, cardTitle: card.title, cardData: card.data },
    });
  }, [push, card]);

  return (
    <div ref={cardRef} className={`discover-card ${dismissing ? 'dismissing' : ''}`}>
      {/* Top row: kind badge + dismiss button */}
      <div className="card-top">
        <CardKindBadge type={card.type} />
        <button className="card-dismiss" onClick={handleDismiss} title="关闭" aria-label="关闭卡片">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Card content by type */}
      {card.type === 'stats' && <StatsCardContent card={card} />}
      {card.type === 'top-tags' && <TopTagsCardContent card={card} />}
      {card.type === 'activity' && <ActivityCardContent card={card} />}

      {/* Actions */}
      <div className="card-actions">
        <button className="btn-primary" onClick={handleViewDetail}>
          查看详情
        </button>
      </div>
    </div>
  );
}

function CardKindBadge({ type }: { type: FeedCard['type'] }) {
  switch (type) {
    case 'activity':
      return (
        <span className="card-kind">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M12 20V10M18 20V4M6 20v-4" />
          </svg>
          活跃度
        </span>
      );
    case 'stats':
      return (
        <span className="card-kind">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="12" cy="12" r="10" />
            <path d="M12 6v6l4 2" />
          </svg>
          碎片统计
        </span>
      );
    case 'top-tags':
      return (
        <span className="card-kind">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M20.59 13.41l-7.17 7.17a2 2 0 01-2.83 0L2 12V2h10l8.59 8.59a2 2 0 010 2.82z" />
            <circle cx="7" cy="7" r="1" />
          </svg>
          高频标签
        </span>
      );
  }
}

function StatsCardContent({ card }: { card: FeedCard }) {
  const fragmentCount = card.data.fragmentCount as number;
  const tagCount = card.data.tagCount as number;

  return (
    <>
      <h3>最近 7 天碎片统计</h3>
      <div className="card-body">
        过去 7 天你记录了 <b>{fragmentCount} 条碎片</b>，涉及 <b>{tagCount} 个标签</b>。
      </div>
      <div className="card-stats">
        <div className="stat">
          <div className="n">{fragmentCount}</div>
          <div className="k">碎片数</div>
        </div>
        <div className="stat">
          <div className="n">{tagCount}</div>
          <div className="k">标签数</div>
        </div>
      </div>
    </>
  );
}

function TopTagsCardContent({ card }: { card: FeedCard }) {
  const tags = card.data.tags as { tag: string; count: number }[];

  return (
    <>
      <h3>高频标签 Top 5</h3>
      <div className="card-body">过去 7 天使用最多的标签：</div>
      {tags.length === 0 ? (
        <div className="tag-empty">暂无标签数据——为碎片添加标签后这里会自动更新</div>
      ) : (
        <div className="tag-list">
          {tags.map((t) => (
            <span key={t.tag} className="tag-item">
              {t.tag}
              <span className="tag-count">{t.count}</span>
            </span>
          ))}
        </div>
      )}
    </>
  );
}

function ActivityCardContent({ card }: { card: FeedCard }) {
  const fragmentCount = card.data.fragmentCount as number;

  return (
    <>
      <h3>你最近非常活跃！</h3>
      <div className="card-body">
        过去 7 天你记录了 <b>{fragmentCount} 条碎片</b>，保持了很高的记录频率。继续保持这个节奏！
      </div>
      <div className="card-stats">
        <div className="stat">
          <div className="n">{fragmentCount}</div>
          <div className="k">7 天碎片</div>
        </div>
      </div>
    </>
  );
}
