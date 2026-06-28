import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export type AppPage = 'discover' | 'compose' | 'capture' | 'articles';

export interface AppState {
  currentPage: AppPage;
  sidebarExpanded: boolean;
  counts: { fragments: number; articles: number };
  /** Navigation history for back/forward */
  pageHistory: AppPage[];
  historyIndex: number;
  setCurrentPage: (page: AppPage) => void;
  setSidebarExpanded: (expanded: boolean) => void;
  refreshCounts: () => Promise<void>;
  canGoBack: () => boolean;
  canGoForward: () => boolean;
  goBack: () => void;
  goForward: () => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  currentPage: 'discover',
  sidebarExpanded: true,
  counts: { fragments: 0, articles: 0 },
  pageHistory: ['discover'],
  historyIndex: 0,

  setCurrentPage: (page) => {
    const { currentPage, pageHistory, historyIndex } = get();
    if (page === currentPage) return;
    // Truncate forward history and push new page
    const newHistory = [...pageHistory.slice(0, historyIndex + 1), page];
    set({
      currentPage: page,
      pageHistory: newHistory,
      historyIndex: newHistory.length - 1,
    });
  },

  setSidebarExpanded: (expanded) => set({ sidebarExpanded: expanded }),

  refreshCounts: async () => {
    try {
      const counts = await invoke<{ fragments: number; articles: number }>('get_counts');
      set({ counts });
    } catch (e) {
      console.error('Failed to refresh counts:', e);
    }
  },

  canGoBack: () => {
    const { historyIndex } = get();
    return historyIndex > 0;
  },

  canGoForward: () => {
    const { historyIndex, pageHistory } = get();
    return historyIndex < pageHistory.length - 1;
  },

  goBack: () => {
    const { historyIndex, pageHistory } = get();
    if (historyIndex <= 0) return;
    const newIndex = historyIndex - 1;
    set({
      historyIndex: newIndex,
      currentPage: pageHistory[newIndex],
    });
  },

  goForward: () => {
    const { historyIndex, pageHistory } = get();
    if (historyIndex >= pageHistory.length - 1) return;
    const newIndex = historyIndex + 1;
    set({
      historyIndex: newIndex,
      currentPage: pageHistory[newIndex],
    });
  },
}));
