import { useAppStore } from '../stores/appStore';
import { useViewStackStore } from '../stores/viewStackStore';
import './NavBar.css';

export default function NavBar() {
  const { currentPage } = useAppStore();
  const { stacks, pop } = useViewStackStore();

  const stack = stacks[currentPage] ?? [];
  const canGoBack = stack.length > 0;

  return (
    <div className="nav-bar">
      <button
        className="nav-btn"
        disabled={!canGoBack}
        onClick={() => pop(currentPage)}
        title="返回"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M15 18l-6-6 6-6" />
        </svg>
      </button>
      <button className="nav-btn" disabled title="前进">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M9 18l6-6-6-6" />
        </svg>
      </button>
    </div>
  );
}
