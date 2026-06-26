import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { Fragment } from './captureStore';

export type ArticleStatus = 'draft' | 'archived';

export interface ArticleMeta {
  id: string;
  title: string;
  status: ArticleStatus;
  word_count: number;
  updated_at: string;
  tags: string[];
}

export interface ArticleResponse {
  id: string;
  title: string;
  status: string;
  created: string;
  updated: string;
  tags: string[];
  body: string;
}

export interface ComposeState {
  currentArticleId: string | null;
  title: string;
  status: ArticleStatus;
  wordCount: number;
  updatedAt: string;
  immersiveMode: boolean;
  relatedFragments: Fragment[];
  loading: boolean;
  /** The article body content loaded from backend */
  bodyContent: string;

  toggleImmersive: () => void;
  setTitle: (title: string) => void;
  setWordCount: (count: number) => void;
  cycleStatus: () => void;
  loadRelated: () => Promise<void>;
  loadArticle: (id: string) => Promise<void>;
  saveArticle: (html: string) => Promise<void>;
  createNewArticle: () => Promise<void>;
}

const STATUS_CYCLE: ArticleStatus[] = ['draft', 'archived'];

function normalizeStatus(status?: string): ArticleStatus {
  switch (status) {
    case 'completed':
    case 'done':
    case 'archived':
      return 'archived';
    case 'draft':
    default:
      return 'draft';
  }
}

export const useComposeStore = create<ComposeState>((set, get) => ({
  currentArticleId: null,
  title: '',
  status: 'draft',
  wordCount: 0,
  updatedAt: '',
  immersiveMode: false,
  relatedFragments: [],
  loading: false,
  bodyContent: '',

  toggleImmersive: () => set((s) => ({ immersiveMode: !s.immersiveMode })),

  setTitle: (title) => set({ title }),

  setWordCount: (count) => set({ wordCount: count }),

  cycleStatus: () => {
    const { status } = get();
    const idx = STATUS_CYCLE.indexOf(status);
    const next = STATUS_CYCLE[(idx + 1) % STATUS_CYCLE.length];
    set({ status: next });
  },

  loadRelated: async () => {
    try {
      const fragments = await invoke<Fragment[]>('list_fragments', {
        filter: 'all',
        offset: 0,
        limit: 50,
      });
      set({ relatedFragments: fragments });
    } catch (e) {
      console.error('Failed to load related fragments:', e);
    }
  },

  loadArticle: async (id: string) => {
    set({ loading: true, currentArticleId: id });
    try {
      const article = await invoke<ArticleResponse>('get_article', { id });
      set({
        title: article.title,
        status: normalizeStatus(article.status),
        wordCount: 0,
        updatedAt: article.updated,
        bodyContent: article.body,
        loading: false,
      });
    } catch (e) {
      console.error('Failed to load article:', e);
      set({ loading: false });
    }
  },

  createNewArticle: async () => {
    try {
      const id = await invoke<string>('create_article', { title: '无标题文章' });
      // Load the newly created article
      await get().loadArticle(id);
    } catch (e) {
      console.error('Failed to create article:', e);
    }
  },

  saveArticle: async (html: string) => {
    const { currentArticleId, title, status } = get();
    if (!currentArticleId) return;
    try {
      await invoke('save_article', {
        id: currentArticleId,
        title,
        status,
        tags: [] as string[],
        body: html,
      });
      set({ updatedAt: new Date().toISOString() });
    } catch (e) {
      console.error('Failed to save article:', e);
    }
  },
}));
