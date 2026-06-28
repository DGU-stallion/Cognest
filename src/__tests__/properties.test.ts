/**
 * Frontend Property-Based Tests (fast-check, 100+ iterations)
 *
 * Covers:
 * - Property 7: Date grouping & sorting
 * - Property 8: Filter correctness
 * - Property 10: Reference round-trip
 * - Property 11: Tag intersection
 * - Property 12: Top tags ranking
 * - Property 13: ViewStack max depth
 * - Property 14: Count format
 */

import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import {
  formatCount,
  groupByDate,
  getDateKey,
  filterFragments,
  filterByTagIntersection,
  sortTagsByCount,
  pushToStack,
  MAX_STACK_DEPTH,
} from '../utils/helpers';
import type { ViewEntry, TagWithCount, ArticleLike } from '../utils/helpers';
import type { Fragment } from '../stores/captureStore';
import {
  serializeReferenceChips,
  deserializeReferenceChips,
} from '../components/extensions/ReferenceChip';

// ─── Strategies / Arbitraries ───────────────────────────────────────────────

/** Generate a random ISO 8601 date string within a reasonable range */
const arbIsoDate = fc.date({
  min: new Date('2020-01-01T00:00:00Z'),
  max: new Date('2030-12-31T23:59:59Z'),
}).filter((d) => !isNaN(d.getTime())).map((d) => d.toISOString());

/** Generate a valid 8-char hex id */
const arbHexId = fc.stringMatching(/^[a-f0-9]{8}$/);

/** Generate a tag (3-10 lowercase ascii letters) */
const arbTag = fc.stringMatching(/^[a-z]{3,10}$/);

/** Generate a list of tags (0 to 5) */
const arbTags = fc.array(arbTag, { minLength: 0, maxLength: 5 });

/** Generate a Fragment for testing */
const arbFragment: fc.Arbitrary<Fragment> = fc.record({
  id: arbHexId,
  content: fc.string({ minLength: 1, maxLength: 200 }),
  created_at: arbIsoDate,
  source: fc.constant('manual'),
  tags: arbTags,
  topics: fc.array(arbTag, { minLength: 0, maxLength: 3 }),
});

/** Generate a ViewEntry */
const arbViewEntry: fc.Arbitrary<ViewEntry> = fc.record({
  id: arbHexId,
  component: fc.constantFrom('detail', 'preview', 'settings', 'search'),
  props: fc.constant({}),
});

// ─── Property 7: Date Grouping & Sorting ────────────────────────────────────
// **Validates: Requirements 8.4**

describe('Property 7: Date grouping & sorting', () => {
  it('groups are sorted date-descending, items within each group sorted time-descending', () => {
    fc.assert(
      fc.property(fc.array(arbFragment, { minLength: 1, maxLength: 50 }), (fragments) => {
        const groups = groupByDate(fragments);

        // Groups are sorted by dateKey descending
        for (let i = 1; i < groups.length; i++) {
          expect(groups[i - 1].dateKey >= groups[i].dateKey).toBe(true);
        }

        // Items within each group are sorted by time descending
        for (const group of groups) {
          for (let i = 1; i < group.items.length; i++) {
            const prev = new Date(group.items[i - 1].created_at).getTime();
            const curr = new Date(group.items[i].created_at).getTime();
            expect(prev >= curr).toBe(true);
          }
        }

        // All items in a group share the same date key
        for (const group of groups) {
          for (const item of group.items) {
            expect(getDateKey(item.created_at)).toBe(group.dateKey);
          }
        }

        // Total items across groups equals input length
        const totalItems = groups.reduce((sum, g) => sum + g.items.length, 0);
        expect(totalItems).toBe(fragments.length);
      }),
      { numRuns: 100 },
    );
  });
});

// ─── Property 8: Filter Correctness ─────────────────────────────────────────
// **Validates: Requirements 8.6, 8.7**

describe('Property 8: Filter correctness', () => {
  it('"all" returns all; "uncategorized" returns only empty topics; "categorized" returns only non-empty topics', () => {
    fc.assert(
      fc.property(fc.array(arbFragment, { minLength: 0, maxLength: 50 }), (fragments) => {
        // "all" returns everything
        const all = filterFragments(fragments, 'all');
        expect(all.length).toBe(fragments.length);

        // "uncategorized" returns only those with empty topics
        const uncategorized = filterFragments(fragments, 'uncategorized');
        for (const f of uncategorized) {
          expect(f.topics.length).toBe(0);
        }

        // "categorized" returns only those with non-empty topics
        const categorized = filterFragments(fragments, 'categorized');
        for (const f of categorized) {
          expect(f.topics.length).toBeGreaterThan(0);
        }

        // Partition is complete: uncategorized + categorized = all
        expect(uncategorized.length + categorized.length).toBe(fragments.length);
      }),
      { numRuns: 100 },
    );
  });
});

// ─── Property 10: Reference Round-Trip ──────────────────────────────────────
// **Validates: Requirements 11.5**

describe('Property 10: Reference round-trip', () => {
  it('serializeReferenceChips(deserializeReferenceChips(md)) preserves all @[id] references', () => {
    fc.assert(
      fc.property(
        fc.array(arbHexId, { minLength: 1, maxLength: 10 }),
        fc.string({ minLength: 0, maxLength: 100 }),
        (ids, surrounding) => {
          // Build a markdown string with references
          const refs = ids.map((id) => `@[${id}]`);
          const markdown = `${surrounding} ${refs.join(' some text ')} end`;

          // Deserialize (md → html) then serialize (html → md)
          const html = deserializeReferenceChips(markdown);
          const roundtripped = serializeReferenceChips(html);

          // All original references should be preserved
          for (const id of ids) {
            expect(roundtripped).toContain(`@[${id}]`);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 11: Tag Intersection ──────────────────────────────────────────
// **Validates: Requirements 12.3**

describe('Property 11: Tag intersection filtering', () => {
  it('filtered results all contain ALL selected tags', () => {
    const arbArticle: fc.Arbitrary<ArticleLike> = fc.record({
      id: arbHexId,
      tags: arbTags,
    });

    fc.assert(
      fc.property(
        fc.array(arbArticle, { minLength: 0, maxLength: 30 }),
        fc.array(arbTag, { minLength: 1, maxLength: 3 }),
        (articles, selectedTags) => {
          const filtered = filterByTagIntersection(articles, selectedTags);

          // Every result must contain ALL selected tags
          for (const article of filtered) {
            for (const tag of selectedTags) {
              expect(article.tags).toContain(tag);
            }
          }

          // Every article NOT in filtered must be missing at least one selected tag
          const filteredIds = new Set(filtered.map((a) => a.id));
          for (const article of articles) {
            if (!filteredIds.has(article.id)) {
              const hasAll = selectedTags.every((t) => article.tags.includes(t));
              expect(hasAll).toBe(false);
            }
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 12: Top Tags Ranking ──────────────────────────────────────────
// **Validates: Requirements 13.3**

describe('Property 12: Top tags ranking', () => {
  it('tags are sorted by count descending', () => {
    const arbTagWithCount: fc.Arbitrary<TagWithCount> = fc.record({
      tag: arbTag,
      count: fc.nat({ max: 1000 }),
    });

    fc.assert(
      fc.property(
        fc.array(arbTagWithCount, { minLength: 1, maxLength: 20 }),
        (tags) => {
          const sorted = sortTagsByCount(tags);

          // Sorted by count descending
          for (let i = 1; i < sorted.length; i++) {
            expect(sorted[i - 1].count >= sorted[i].count).toBe(true);
          }

          // Same length as input
          expect(sorted.length).toBe(tags.length);
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 13: ViewStack Max Depth ───────────────────────────────────────
// **Validates: Requirements 7.4**

describe('Property 13: ViewStack max depth', () => {
  it('after any sequence of push operations, stack depth never exceeds 10', () => {
    fc.assert(
      fc.property(
        fc.array(arbViewEntry, { minLength: 1, maxLength: 30 }),
        (entries) => {
          let stack: ViewEntry[] = [];

          for (const entry of entries) {
            stack = pushToStack(stack, entry);
            // Invariant: stack depth never exceeds MAX_STACK_DEPTH
            expect(stack.length).toBeLessThanOrEqual(MAX_STACK_DEPTH);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 14: Count Format ──────────────────────────────────────────────
// **Validates: Requirements 6.5**

describe('Property 14: Count format', () => {
  it('returns String(N) if N ≤ 999, else "999+"', () => {
    fc.assert(
      fc.property(fc.nat({ max: 100000 }), (n) => {
        const result = formatCount(n);
        if (n <= 999) {
          expect(result).toBe(String(n));
        } else {
          expect(result).toBe('999+');
        }
      }),
      { numRuns: 100 },
    );
  });
});
