import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Fragment {
  id: string;
  content: string;
  created_at: string; // ISO 8601
  source: string;
  tags: string[];
  topics: string[];
}

export type FragmentFilter = 'all' | 'uncategorized' | 'categorized';

export interface CaptureState {
  fragments: Fragment[];
  filter: FragmentFilter;
  totalCount: number;
  loading: boolean;
  loadFragments: () => Promise<void>;
  setFilter: (filter: FragmentFilter) => void;
  addFragment: (content: string) => Promise<string>;
}

export const useCaptureStore = create<CaptureState>((set, get) => ({
  fragments: [],
  filter: 'all',
  totalCount: 0,
  loading: false,

  loadFragments: async () => {
    set({ loading: true });
    try {
      const filter = get().filter;
      const fragments = await invoke<Fragment[]>('list_fragments', {
        filter,
        offset: 0,
        limit: 200,
      });
      set({ fragments, totalCount: fragments.length, loading: false });
    } catch (e) {
      console.error('Failed to load fragments:', e);
      set({ loading: false });
    }
  },

  setFilter: (filter) => {
    set({ filter });
    get().loadFragments();
  },

  addFragment: async (content: string) => {
    const id = await invoke<string>('create_fragment', { content });
    // Reload after creation
    await get().loadFragments();
    return id;
  },
}));
