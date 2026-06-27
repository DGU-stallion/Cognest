import { useCallback, useEffect, useRef, useState } from 'react';
import { useViewStackStore } from '../stores/viewStackStore';
import DiscoverDetail from '../pages/DiscoverDetail';
import './ViewStack.css';

interface ViewStackProps {
  pageId: string;
  rootComponent: React.ReactNode;
}

/**
 * ViewStack — manages a navigation stack per page.
 * - Renders rootComponent when the stack is empty.
 * - Renders the top-of-stack view otherwise.
 * - Shows "← 返回" button when depth > 0.
 * - Animates forward (slide in from right) and backward (slide out to right).
 * - Listens for Esc key to pop.
 * - Each page's stack is independent; switching pages preserves stacks.
 */
export default function ViewStack({ pageId, rootComponent }: ViewStackProps) {
  const stacks = useViewStackStore((s) => s.stacks);
  const pop = useViewStackStore((s) => s.pop);

  const stack = stacks[pageId] ?? [];
  const depth = stack.length;
  const currentEntry = depth > 0 ? stack[depth - 1] : null;

  // Track previous depth to determine animation direction
  const prevDepthRef = useRef(depth);
  const [animClass, setAnimClass] = useState<string>('');
  const [exitingView, setExitingView] = useState<React.ReactNode | null>(null);
  const prevViewRef = useRef<React.ReactNode | null>(null);

  useEffect(() => {
    const prevDepth = prevDepthRef.current;

    if (depth > prevDepth) {
      // Forward navigation: new view slides in from right
      setAnimClass('view-stack__layer--enter');
      setExitingView(null);
    } else if (depth < prevDepth) {
      // Backward navigation: exiting view slides out to right
      setAnimClass('');
      setExitingView(prevViewRef.current);
      // Remove exiting view after animation completes
      const timer = setTimeout(() => setExitingView(null), 220);
      prevDepthRef.current = depth;
      return () => clearTimeout(timer);
    } else {
      setAnimClass('');
    }

    prevDepthRef.current = depth;
  }, [depth]);

  // Keep track of current rendered view for exit animation
  useEffect(() => {
    prevViewRef.current = currentEntry ? (
      <div className="view-stack__layer">
        <ViewPlaceholder entry={currentEntry} />
      </div>
    ) : (
      <div className="view-stack__layer">{rootComponent}</div>
    );
  });

  // Esc key handler
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === 'Escape' && depth > 0) {
        e.preventDefault();
        pop(pageId);
      }
    },
    [depth, pageId, pop],
  );

  useEffect(() => {
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  // Clear enter animation after it completes
  useEffect(() => {
    if (animClass) {
      const timer = setTimeout(() => setAnimClass(''), 220);
      return () => clearTimeout(timer);
    }
  }, [animClass]);

  const handleBack = () => {
    if (depth > 0) {
      pop(pageId);
    }
  };

  return (
    <div className="view-stack">
      {/* Back button */}
      {depth > 0 && (
        <button className="view-stack__back-btn" onClick={handleBack} aria-label="返回">
          ← 返回
        </button>
      )}

      {/* Exiting view (backward animation) */}
      {exitingView && (
        <div className="view-stack__layer view-stack__layer--exit">
          {exitingView}
        </div>
      )}

      {/* Current view */}
      <div className={`view-stack__layer ${animClass}`}>
        {currentEntry ? (
          <ViewPlaceholder entry={currentEntry} />
        ) : (
          rootComponent
        )}
      </div>
    </div>
  );
}

function ViewPlaceholder({ entry }: { entry: { id: string; component: string; props: Record<string, unknown> } }) {
  // Resolve component by name
  switch (entry.component) {
    case 'DiscoverDetail':
      return <DiscoverDetail {...(entry.props as any)} />;
    default:
      return (
        <div style={{ padding: '48px 24px', textAlign: 'center', color: 'var(--muted)' }}>
          <p>未知视图: {entry.component}</p>
        </div>
      );
  }
}
