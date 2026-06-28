import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { Fragment } from './captureStore';

/** Extract keywords from article text for relevance matching */
function extractKeywords(text: string): string[] {
  // Split on common delimiters, filter short words
  return text
    .replace(/[#*_`>~\[\](){}|\\\/\-=+]/g, ' ')
    .split(/\s+/)
    .filter(w => w.length >= 2)
    .map(w => w.toLowerCase());
}

/** Compute a simple relevance score for a fragment against keywords */
function computeRelevanceScore(fragment: Fragment, keywords: string[]): number {
  let score = 0;
  const content = fragment.content.toLowerCase();
  const tags = fragment.tags.map(t => t.toLowerCase());

  for (const kw of keywords) {
    if (content.includes(kw)) score += 1;
    if (tags.some(t => t.includes(kw))) score += 3; // Tag matches are stronger
  }
  return score;
}

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
  loadRelated: (articleContent?: string) => Promise<void>;
  loadArticle: (id: string) => Promise<void>;
  saveArticle: (body: string) => Promise<void>;
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

  loadRelated: async (articleContent?: string) => {
    try {
      const fragments = await invoke<Fragment[]>('list_fragments', {
        filter: 'all',
        offset: 0,
        limit: 200,
      });

      if (articleContent && articleContent.trim()) {
        // Sort fragments by relevance to current article content
        // Simple keyword matching: fragments whose tags or content overlap with article text
        const keywords = extractKeywords(articleContent);
        const scored = fragments.map(f => ({
          fragment: f,
          score: computeRelevanceScore(f, keywords),
        }));
        scored.sort((a, b) => b.score - a.score);
        set({ relatedFragments: scored.map(s => s.fragment) });
      } else {
        // No article content yet — show recent fragments
        set({ relatedFragments: fragments });
      }
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

  saveArticle: async (body: string) => {
    const { currentArticleId, title, status } = get();
    if (!currentArticleId) return;
    try {
      await invoke('save_article', {
        id: currentArticleId,
        title,
        status,
        tags: [] as string[],
        body,
      });
      set({ updatedAt: new Date().toISOString() });
    } catch (e) {
      console.error('Failed to save article:', e);
    }
  },
}));
