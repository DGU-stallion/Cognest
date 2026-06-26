import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export type ArticleStatus = 'draft' | 'editing' | 'done';

export interface Article {
  id: string;
  title: string;
  status: ArticleStatus;
  content: string;
  word_count: number;
  tags: string[];
  created_at: string; // ISO 8601
  updated_at: string; // ISO 8601
}

export interface ArticlesState {
  articles: Article[];
  selectedId: string | null;
  statusFilter: ArticleStatus | 'all';
  tagFilter: string[];
  searchQuery: string;
  loading: boolean;

  loadArticles: () => Promise<void>;
  setStatusFilter: (status: ArticleStatus | 'all') => void;
  toggleTag: (tag: string) => void;
  setSearchQuery: (query: string) => void;
  selectArticle: (id: string | null) => void;
  deleteArticle: (id: string) => Promise<void>;
}

export const useArticlesStore = create<ArticlesState>((set, get) => ({
  articles: [],
  selectedId: null,
  statusFilter: 'all',
  tagFilter: [],
  searchQuery: '',
  loading: false,

  loadArticles: async () => {
    set({ loading: true });
    try {
      const articles = await invoke<Article[]>('list_articles');
      set({ articles, loading: false });
    } catch (e) {
      console.error('Failed to load articles:', e);
      set({ loading: false });
    }
  },

  setStatusFilter: (status) => {
    set({ statusFilter: status });
  },

  toggleTag: (tag) => {
    const current = get().tagFilter;
    if (current.includes(tag)) {
      set({ tagFilter: current.filter((t) => t !== tag) });
    } else {
      set({ tagFilter: [...current, tag] });
    }
  },

  setSearchQuery: (query) => {
    set({ searchQuery: query });
  },

  selectArticle: (id) => {
    set({ selectedId: id });
  },

  deleteArticle: async (id) => {
    try {
      await invoke('delete_article', { id });
      const articles = get().articles.filter((a) => a.id !== id);
      const selectedId = get().selectedId === id ? null : get().selectedId;
      set({ articles, selectedId });
    } catch (e) {
      console.error('Failed to delete article:', e);
    }
  },
}));
