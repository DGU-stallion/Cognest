import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

// Requirements: 7.2, 7.4

// ─── Data Interfaces ────────────────────────────────────────────────────────

export interface GraphNode {
  id: string;
  label: string;
  type: 'topic' | 'fragment' | 'article';
  size?: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  label?: string;
  weight?: number;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface TimelineItem {
  id: string;
  date: string;
  title: string;
  content: string;
  type: 'fragment' | 'topic' | 'article';
}

export interface TimelineData {
  items: TimelineItem[];
}

export interface ListItem {
  id: string;
  title: string;
  subtitle?: string;
  tags?: string[];
}

export interface ListData {
  items: ListItem[];
  groupBy?: string;
}

export interface ChartSeries {
  name: string;
  data: number[];
  color?: string;
}

export interface ChartData {
  chartType: 'bar' | 'line' | 'pie' | 'area';
  series: ChartSeries[];
  xAxis?: { label: string; data: string[] };
}

export interface SummaryData {
  markdown: string;
  stats?: Record<string, number | string>;
}

export type ViewData = GraphData | TimelineData | ListData | ChartData | SummaryData;

export interface ViewSpec {
  id: string;
  type: 'graph' | 'timeline' | 'list' | 'chart' | 'summary';
  title: string;
  query: string;
  created: string;
  pinned: boolean;
  config: Record<string, unknown>;
  data: ViewData;
}

// ─── Store Interface ────────────────────────────────────────────────────────

export interface ViewStore {
  currentView: ViewSpec | null;
  pinnedViews: ViewSpec[];
  generating: boolean;
  error: string | null;
  prompt: string;

  setPrompt: (prompt: string) => void;
  generateView: (prompt: string) => Promise<void>;
  pinView: (view: ViewSpec) => Promise<void>;
  unpinView: (viewId: string) => Promise<void>;
  loadPinnedViews: () => Promise<void>;
  clearCurrent: () => void;
}

// ─── Store Implementation ───────────────────────────────────────────────────

export const useViewStore = create<ViewStore>((set, get) => ({
  currentView: null,
  pinnedViews: [],
  generating: false,
  error: null,
  prompt: '',

  setPrompt: (prompt) => set({ prompt }),

  generateView: async (prompt) => {
    set({ generating: true, error: null });
    try {
      const viewSpec = await invoke<ViewSpec>('generate_view', { prompt });
      set({ currentView: viewSpec, generating: false, prompt: '' });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ generating: false, error: message });
    }
  },

  pinView: async (view) => {
    try {
      const pinnedSpec: ViewSpec = { ...view, pinned: true };
      await invoke('pin_view', { viewSpec: pinnedSpec });
      set((state) => ({
        pinnedViews: [...state.pinnedViews, pinnedSpec],
        currentView: state.currentView?.id === view.id ? pinnedSpec : state.currentView,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ error: message });
    }
  },

  unpinView: async (viewId) => {
    try {
      // Remove from pinned list client-side and persist via pin_view with pinned=false
      const { pinnedViews } = get();
      const target = pinnedViews.find((v) => v.id === viewId);
      if (target) {
        const unpinnedSpec: ViewSpec = { ...target, pinned: false };
        await invoke('pin_view', { viewSpec: unpinnedSpec });
      }
      set((state) => ({
        pinnedViews: state.pinnedViews.filter((v) => v.id !== viewId),
        currentView:
          state.currentView?.id === viewId
            ? { ...state.currentView, pinned: false }
            : state.currentView,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ error: message });
    }
  },

  loadPinnedViews: async () => {
    try {
      const views = await invoke<ViewSpec[]>('list_pinned_views');
      set({ pinnedViews: views });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ error: message });
    }
  },

  clearCurrent: () => set({ currentView: null, error: null }),
}));
