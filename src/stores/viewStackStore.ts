import { create } from 'zustand';

export interface ViewEntry {
  id: string;
  component: string;
  props: Record<string, unknown>;
}

interface ViewStackState {
  stacks: Record<string, ViewEntry[]>;
  push: (page: string, entry: ViewEntry) => void;
  pop: (page: string) => void;
  current: (page: string) => ViewEntry | null;
  clear: (page: string) => void;
}

const MAX_DEPTH = 10;

export const useViewStackStore = create<ViewStackState>((set, get) => ({
  stacks: {},

  push: (page, entry) =>
    set((state) => {
      const stack = state.stacks[page] ?? [];
      let newStack: ViewEntry[];

      if (stack.length >= MAX_DEPTH) {
        // Replace top instead of growing
        newStack = [...stack.slice(0, stack.length - 1), entry];
      } else {
        newStack = [...stack, entry];
      }

      return { stacks: { ...state.stacks, [page]: newStack } };
    }),

  pop: (page) =>
    set((state) => {
      const stack = state.stacks[page] ?? [];
      if (stack.length === 0) return state;
      return { stacks: { ...state.stacks, [page]: stack.slice(0, -1) } };
    }),

  current: (page) => {
    const stack = get().stacks[page] ?? [];
    return stack.length > 0 ? stack[stack.length - 1] : null;
  },

  clear: (page) =>
    set((state) => ({ stacks: { ...state.stacks, [page]: [] } })),
}));
