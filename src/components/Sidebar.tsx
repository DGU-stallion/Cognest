import { useEffect } from 'react';
import { useAppStore } from '../stores/appStore';
import './Sidebar.css';

type AppPage = 'discover' | 'compose' | 'capture' | 'articles';

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
  const { currentPage, setCurrentPage, sidebarExpanded, setSidebarExpanded, counts } = useAppStore();

  // Persist sidebar state to localStorage
  useEffect(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored !== null) {
      setSidebarExpanded(stored === 'true');
    }
  }, [setSidebarExpanded]);

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, String(sidebarExpanded));
  }, [sidebarExpanded]);

  // ⌘\ keyboard shortcut to toggle sidebar
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

  const toggleSidebar = () => {
    setSidebarExpanded(!sidebarExpanded);
  };

  return (
    <aside className={`sidebar${sidebarExpanded ? '' : ' collapsed'}`}>
      {/* Brand */}
      <div className="sidebar-brand">
        <div className="sidebar-brand-mark">
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M9 1.5 1.5 5.25 9 9l7.5-3.75L9 1.5zM1.5 12.75 9 16.5l7.5-3.75M1.5 9 9 12.75 16.5 9" />
          </svg>
        </div>
        <b className="sidebar-brand-text">Cognest</b>
        <button
          className="sidebar-toggle"
          onClick={toggleSidebar}
          title={sidebarExpanded ? '收起侧边栏' : '展开侧边栏'}
          aria-label={sidebarExpanded ? '收起侧边栏' : '展开侧边栏'}
        >
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
            {sidebarExpanded ? (
              <path d="M11.25 13.5 6.75 9l4.5-4.5" />
            ) : (
              <path d="M6.75 13.5 11.25 9 6.75 4.5" />
            )}
          </svg>
        </button>
      </div>

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
              {count !== null && (
                <span className="nav-count">{formatCount(count)}</span>
              )}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
