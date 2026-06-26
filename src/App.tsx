import { useCallback, useEffect, useState } from 'react';
import { useAppStore } from './stores/appStore';
import { useStartup } from './hooks/useStartup';
import Sidebar from './components/Sidebar';
import ViewStack from './components/ViewStack';
import StatusBar from './components/StatusBar';
import QuickCaptureModal from './components/QuickCaptureModal';
import SettingsModal from './components/SettingsModal';
import ToastContainer from './components/Toast';
import Discover from './pages/Discover';
import Compose from './pages/Compose';
import Capture from './pages/Capture';
import Articles from './pages/Articles';

type AppPage = 'discover' | 'compose' | 'capture' | 'articles';

const PAGE_MAP: Record<AppPage, React.ComponentType> = {
  discover: Discover,
  compose: Compose,
  capture: Capture,
  articles: Articles,
};

const PAGE_IDS: AppPage[] = ['discover', 'capture', 'compose', 'articles'];

function App() {
  const { currentPage } = useAppStore();
  const [quickCaptureOpen, setQuickCaptureOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  useStartup();

  const openQuickCapture = useCallback(() => {
    setQuickCaptureOpen(true);
  }, []);

  const closeQuickCapture = useCallback(() => {
    setQuickCaptureOpen(false);
  }, []);

  // Global shortcut ⌘⇧Space to open quick capture
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === 'Space') {
        e.preventDefault();
        setQuickCaptureOpen(true);
      }
    }
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Expose openQuickCapture on window for Sidebar button
  useEffect(() => {
    (window as unknown as Record<string, unknown>).__openQuickCapture = openQuickCapture;
    return () => {
      delete (window as unknown as Record<string, unknown>).__openQuickCapture;
    };
  }, [openQuickCapture]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Sidebar */}
        <Sidebar onQuickCapture={openQuickCapture} />

        {/* Main content area — ViewStack per page, only active page visible */}
        <main style={{ flex: 1, overflow: 'hidden', background: 'var(--surface-warm)', position: 'relative' }}>
          {PAGE_IDS.map((id) => {
            const Comp = PAGE_MAP[id];
            return (
              <div
                key={id}
                style={{
                  display: currentPage === id ? 'block' : 'none',
                  width: '100%',
                  height: '100%',
                }}
              >
                <ViewStack pageId={id} rootComponent={<Comp />} />
              </div>
            );
          })}
        </main>
      </div>

      {/* Status bar */}
      <StatusBar onSettingsClick={() => setSettingsOpen(true)} />

      {/* Quick Capture Modal — always mounted at top level */}
      <QuickCaptureModal open={quickCaptureOpen} onClose={closeQuickCapture} />

      {/* Settings Modal */}
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />

      {/* Toast notifications */}
      <ToastContainer />
    </div>
  );
}

export default App;
