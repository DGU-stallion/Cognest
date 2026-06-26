import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export type ArticleStatus = 'draft' | 'archived';

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

/** Raw record shape returned by the `list_articles` IPC (from SQLite index).
 *  May lack `content`/`word_count` and uses backend status values. */
interface RawArticleRecord {
  id: string;
  title?: string;
  status?: string;
  content?: string;
  word_count?: number;
  tags?: string[];
  created_at?: string;
  updated_at?: string;
}

interface ArticleResponse {
  id: string;
  title: string;
  status: string;
  created: string;
  updated: string;
  tags: string[];
  body: string;
}

/** Map backend status values to frontend ones. */
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

export interface ArticlesState {
  articles: Article[];
  selectedId: string | null;
  selectedArticleContent: string;
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
  selectedArticleContent: '',
  statusFilter: 'all',
  tagFilter: [],
  searchQuery: '',
  loading: false,

  loadArticles: async () => {
    set({ loading: true });
    try {
      // Backend ArticleRecord lacks `content` and `word_count`, and uses
      // status "completed" instead of frontend "archived". Normalize here.
      const raw = await invoke<RawArticleRecord[]>('list_articles');
      const articles: Article[] = raw.map((r) => ({
        id: r.id,
        title: r.title ?? '无标题文章',
        status: normalizeStatus(r.status),
        content: r.content ?? '',
        word_count: r.word_count ?? 0,
        tags: r.tags ?? [],
        created_at: r.created_at ?? '',
        updated_at: r.updated_at ?? '',
      }));
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

  selectArticle: async (id) => {
    if (!id) {
      set({ selectedId: null, selectedArticleContent: '' });
      return;
    }
    set({ selectedId: id, selectedArticleContent: '' });
    try {
      const article = await invoke<ArticleResponse>('get_article', { id });
      set({ selectedArticleContent: article.body ?? '' });
    } catch (e) {
      console.error('Failed to fetch article content:', e);
    }
  },

  deleteArticle: async (id) => {
    try {
      await invoke('delete_article', { id });
      const articles = get().articles.filter((a) => a.id !== id);
      const selectedId = get().selectedId === id ? null : get().selectedId;
      set({ articles, selectedId, selectedArticleContent: selectedId ? get().selectedArticleContent : '' });
    } catch (e) {
      console.error('Failed to delete article:', e);
    }
  },
}));
