import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface AppState {
  currentPage: 'discover' | 'compose' | 'capture' | 'articles';
  sidebarExpanded: boolean;
  counts: { fragments: number; articles: number };
  setCurrentPage: (page: AppState['currentPage']) => void;
  setSidebarExpanded: (expanded: boolean) => void;
  refreshCounts: () => Promise<void>;
}

export const useAppStore = create<AppState>((set) => ({
  currentPage: 'discover',
  sidebarExpanded: true,
  counts: { fragments: 0, articles: 0 },

  setCurrentPage: (page) => set({ currentPage: page }),

  setSidebarExpanded: (expanded) => set({ sidebarExpanded: expanded }),

  refreshCounts: async () => {
    try {
      const counts = await invoke<{ fragments: number; articles: number }>('get_counts');
      set({ counts });
    } catch (e) {
      console.error('Failed to refresh counts:', e);
    }
  },
}));
