import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

/** IPC response from get_stats */
interface StatsResult {
  fragment_count: number;
  article_count: number;
  days: { date: string; count: number }[];
}

/** IPC response item from get_top_tags */
interface TagCount {
  tag: string;
  count: number;
}

export interface FeedCard {
  id: string;
  type: 'activity' | 'stats' | 'top-tags';
  title: string;
  priority: number; // lower = higher priority
  data: Record<string, unknown>;
}

export interface DiscoverState {
  cards: FeedCard[];
  dismissedCards: Set<string>;
  loading: boolean;
  loadCards: () => Promise<void>;
  dismissCard: (cardId: string) => void;
}

export const useDiscoverStore = create<DiscoverState>((set, get) => ({
  cards: [],
  dismissedCards: new Set(),
  loading: false,

  loadCards: async () => {
    set({ loading: true });
    try {
      const [stats, topTags] = await Promise.all([
        invoke<StatsResult>('get_stats', { days: 7 }),
        invoke<TagCount[]>('get_top_tags', { days: 7, limit: 5 }),
      ]);

      const cards: FeedCard[] = [];
      const totalFragments7d = stats.fragment_count;

      // Unique tags count from top tags result
      const tagCount = topTags.length;

      // "最近碎片统计" card — always shown if any data exists
      if (totalFragments7d > 0 || tagCount > 0) {
        cards.push({
          id: 'stats',
          type: 'stats',
          title: '最近碎片统计',
          priority: 2,
          data: {
            fragmentCount: totalFragments7d,
            tagCount,
          },
        });
      }

      // "高频标签" card — always shown (with empty state if no tags)
      cards.push({
        id: 'top-tags',
        type: 'top-tags',
        title: '高频标签 Top 5',
        priority: 3,
        data: {
          tags: topTags,
        },
      });

      // "活跃度提示" card — only shown if 7-day fragments > 20
      if (totalFragments7d > 20) {
        // Compare with previous 7 days by looking at daily breakdown
        // stats.days contains per-day counts for the past 7 days
        const currentTotal = totalFragments7d;
        cards.push({
          id: 'activity',
          type: 'activity',
          title: '活跃度提示',
          priority: 1,
          data: {
            fragmentCount: currentTotal,
          },
        });
      }

      // Sort by priority (ascending = higher priority first)
      cards.sort((a, b) => a.priority - b.priority);

      set({ cards, loading: false });
    } catch (e) {
      console.error('Failed to load discover cards:', e);
      set({ cards: [], loading: false });
    }
  },

  dismissCard: (cardId: string) => {
    const dismissed = new Set(get().dismissedCards);
    dismissed.add(cardId);
    set({ dismissedCards: dismissed });
  },
}));
