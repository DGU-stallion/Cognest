/**
 * Pure utility functions extracted for testability.
 * These are used across the application and property-tested.
 */

import type { Fragment } from '../stores/captureStore';

// ─── Count Formatting (Property 14) ─────────────────────────────────────────

/** Format count for sidebar display: N if ≤ 999, "999+" otherwise */
export function formatCount(n: number): string {
  return n > 999 ? '999+' : String(n);
}

// ─── Date Grouping (Property 7) ─────────────────────────────────────────────

/** Get date key (YYYY-MM-DD) from ISO string for grouping */
export function getDateKey(isoStr: string): string {
  const d = new Date(isoStr);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export interface DateGroup {
  dateKey: string;
  items: Fragment[];
}

/** Group fragments by date, sorted descending (most recent day first),
 *  items within each group sorted time-descending */
export function groupByDate(fragments: Fragment[]): DateGroup[] {
  const groups = new Map<string, Fragment[]>();

  for (const frag of fragments) {
    const key = getDateKey(frag.created_at);
    const existing = groups.get(key);
    if (existing) {
      existing.push(frag);
    } else {
      groups.set(key, [frag]);
    }
  }

  // Sort groups by date descending
  const sortedKeys = [...groups.keys()].sort((a, b) => b.localeCompare(a));

  return sortedKeys.map((key) => {
    const items = groups.get(key)!;
    // Sort items within group by time descending
    items.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
    return { dateKey: key, items };
  });
}

// ─── Fragment Filtering (Property 8) ────────────────────────────────────────

export type FragmentFilterType = 'all' | 'uncategorized' | 'categorized';

/** Filter fragments by category type */
export function filterFragments(fragments: Fragment[], filter: FragmentFilterType): Fragment[] {
  switch (filter) {
    case 'all':
      return fragments;
    case 'uncategorized':
      return fragments.filter((f) => f.topics.length === 0);
    case 'categorized':
      return fragments.filter((f) => f.topics.length > 0);
  }
}

// ─── Tag Intersection (Property 11) ─────────────────────────────────────────

export interface ArticleLike {
  id: string;
  tags: string[];
}

/** Filter articles by tag intersection: result must contain ALL selected tags */
export function filterByTagIntersection<T extends ArticleLike>(
  articles: T[],
  selectedTags: string[],
): T[] {
  if (selectedTags.length === 0) return articles;
  return articles.filter((a) => selectedTags.every((t) => a.tags.includes(t)));
}

// ─── Top Tags Ranking (Property 12) ─────────────────────────────────────────

export interface TagWithCount {
  tag: string;
  count: number;
}

/** Sort tags by count descending */
export function sortTagsByCount(tags: TagWithCount[]): TagWithCount[] {
  return [...tags].sort((a, b) => b.count - a.count);
}

// ─── ViewStack (Property 13) ────────────────────────────────────────────────

export const MAX_STACK_DEPTH = 10;

export interface ViewEntry {
  id: string;
  component: string;
  props: Record<string, unknown>;
}

/** Push to a stack, enforcing max depth of 10 */
export function pushToStack(stack: ViewEntry[], entry: ViewEntry): ViewEntry[] {
  if (stack.length >= MAX_STACK_DEPTH) {
    // Replace top instead of growing
    return [...stack.slice(0, stack.length - 1), entry];
  }
  return [...stack, entry];
}
