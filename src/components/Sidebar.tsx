import { useEffect, useMemo } from 'react';
import { useAppStore } from '../stores/appStore';
import type { AppPage } from '../stores/appStore';
import './Sidebar.css';

interface NavItem {
  id: AppPage;
  label: string;
  icon: React.ReactNode;
  countKey?: 'fragments' | 'articles';
}

const STORAGE_KEY = 'cognest-sidebar-expanded';

function formatCount(n: number): string {
  return n > 999 ? '999+' : String(n);
}

/** Generate stable heatmap data (seeded to avoid flicker) */
function generateHeatmapData(): number[][] {
  const seed = Math.floor(Date.now() / (7 * 24 * 3600 * 1000));
  const weeks: number[][] = [];
  let s = seed;
  for (let w = 0; w < 16; w++) {
    const week: number[] = [];
    for (let d = 0; d < 7; d++) {
      s = (s * 1103515245 + 12345) & 0x7fffffff;
      week.push(s % 5);
    }
    weeks.push(week);
  }
  return weeks;
}

const NAV_ITEMS: NavItem[] = [
  {
    id: 'discover',
    label: '发现',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="8" cy="8" r="5.5" />
        <path d="m15 15-2.7-2.7" />
      </svg>
    ),
  },
  {
    id: 'compose',
    label: '创作',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M9 15h6.75M12.375 2.625a1.59 1.59 0 0 1 2.25 2.25L5.25 14.25l-3 .75.75-3 9.375-9.375z" />
      </svg>
    ),
  },
  {
    id: 'capture',
    label: '碎片',
    countKey: 'fragments',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="3" width="12" height="12" rx="1.5" />
        <path d="M6.75 6.75h4.5M6.75 9.75h3" />
      </svg>
    ),
  },
  {
    id: 'articles',
    label: '文章',
    countKey: 'articles',
    icon: (
      <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M3 4.5h12M3 9h12M3 13.5h9" />
      </svg>
    ),
  },
];

interface SidebarProps {
  onQuickCapture?: () => void;
}

export default function Sidebar({ onQuickCapture }: SidebarProps) {
  const { currentPage, setCurrentPage, sidebarExpanded, setSidebarExpanded, counts, canGoBack, canGoForward, goBack, goForward } = useAppStore();

  const navigateBack = canGoBack();
  const navigateForward = canGoForward();

  const heatmapData = useMemo(() => generateHeatmapData(), []);

  useEffect(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored !== null) setSidebarExpanded(stored === 'true');
  }, [setSidebarExpanded]);

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, String(sidebarExpanded));
  }, [sidebarExpanded]);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === '\\') {
        e.preventDefault();
        setSidebarExpanded(!sidebarExpanded);
      }
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [sidebarExpanded, setSidebarExpanded]);

  const toggleSidebar = () => setSidebarExpanded(!sidebarExpanded);

  return (
    <aside className={`sidebar${sidebarExpanded ? '' : ' collapsed'}`}>
      {/* Window controls — top bar, centered, 1/3 each */}
      <div className="sidebar-top-bar">
        <button
          className="top-btn top-toggle"
          onClick={toggleSidebar}
          title={sidebarExpanded ? '收起侧边栏 ⌘\\' : '展开侧边栏 ⌘\\'}
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4">
            <rect x="2" y="2" width="12" height="12" rx="2" />
            <path d="M6 2v12" />
          </svg>
        </button>
        <button className="top-btn" disabled={!navigateBack} onClick={goBack} title="返回">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M10 12 6 8l4-4" />
          </svg>
        </button>
        <button className="top-btn" disabled={!navigateForward} onClick={goForward} title="前进">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M6 12l4-4-4-4" />
          </svg>
        </button>
      </div>

      {/* Activity Heatmap — aligned with quick-capture below */}
      {sidebarExpanded && (
        <div className="sidebar-heatmap">
          {heatmapData.map((week, wi) => (
            <div key={wi} className="hm-col">
              {week.map((level, di) => (
                <div key={di} className={`hm-cell hm-${level}`} />
              ))}
            </div>
          ))}
        </div>
      )}

      {/* Quick capture button */}
      <button className="sidebar-capture-btn" aria-label="快速记录" onClick={onQuickCapture}>
        <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path d="M9 3.75v10.5M3.75 9h10.5" />
        </svg>
        <span className="capture-text">快速记录</span>
        <kbd>⌘⇧Space</kbd>
      </button>

      {/* Navigation */}
      <nav className="sidebar-nav">
        {NAV_ITEMS.map((item) => {
          const isActive = currentPage === item.id;
          const count = item.countKey ? counts[item.countKey] : null;
          return (
            <button
              key={item.id}
              className={`sidebar-nav-item${isActive ? ' active' : ''}`}
              onClick={() => setCurrentPage(item.id)}
              aria-current={isActive ? 'page' : undefined}
            >
              <span className="nav-icon">{item.icon}</span>
              <span className="nav-label">{item.label}</span>
              {count !== null && <span className="nav-count">{formatCount(count)}</span>}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
