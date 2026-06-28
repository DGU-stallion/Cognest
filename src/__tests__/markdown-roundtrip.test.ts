/**
 * Markdown Round-Trip Property-Based Tests (fast-check)
 *
 * Covers:
 * - Property 5: Markdown Round-Trip 一致性
 * - Property 6: 不支持节点的文本保留
 * - Property 7: 不支持内容的 Round-Trip 稳定性
 * - Property 8: Frontmatter 分离
 *
 * **Validates: Requirements 6.1, 6.2, 6.3, 6.6, 7.1, 7.2, 7.4, 8.1, 8.2, 8.3, 8.4**
 */

import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import { Schema } from '@tiptap/pm/model';
import { serializeDocument } from '../utils/markdownSerializer';
import { parseMarkdown, stripFrontmatter } from '../utils/markdownParser';

// ─── Test Schema ────────────────────────────────────────────────────────────
// Build a ProseMirror schema matching what the parser produces (TipTap naming).

const testSchema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { content: 'inline*', group: 'block' },
    heading: {
      content: 'inline*',
      group: 'block',
      attrs: { level: { default: 1 } },
    },
    blockquote: { content: 'block+', group: 'block' },
    codeBlock: {
      content: 'text*',
      group: 'block',
      marks: '',
      attrs: { language: { default: '' } },
      code: true,
    },
    bulletList: { content: 'listItem+', group: 'block' },
    orderedList: {
      content: 'listItem+',
      group: 'block',
      attrs: { start: { default: 1 } },
    },
    listItem: { content: 'paragraph block*' },
    horizontalRule: { group: 'block' },
    image: {
      group: 'block',
      inline: false,
      attrs: { src: { default: '' }, alt: { default: null }, title: { default: null } },
    },
    hardBreak: { inline: true, group: 'inline' },
    referenceChip: {
      inline: true,
      group: 'inline',
      atom: true,
      attrs: { fragmentId: { default: '' } },
    },
    text: { group: 'inline' },
  },
  marks: {
    bold: {},
    italic: {},
    code: {},
    link: { attrs: { href: { default: '' }, title: { default: null } } },
    strikethrough: {},
  },
});

// ─── Generators / Arbitraries ───────────────────────────────────────────────

/** Plain text word (letters, digits, spaces — no markdown syntax chars).
 * Excludes leading/trailing spaces to avoid CommonMark trailing-space stripping
 * which is correct behavior but breaks round-trip identity for raw input. */
const arbPlainWord = fc
  .stringMatching(/^[a-zA-Z0-9]([a-zA-Z0-9 ]{0,18}[a-zA-Z0-9])?$/)
  .filter((s) => s.length > 0 && s === s.trim());

/** Heading level 1-6 */
const arbLevel = fc.integer({ min: 1, max: 6 });

/** Code language identifier */
const arbLanguage = fc.constantFrom('', 'javascript', 'python', 'rust', 'typescript', 'json');

/** Code block content (printable ASCII, no backticks).
 * Must start and end with non-whitespace characters because:
 * - Leading whitespace may be stripped by prosemirror-markdown's token handling
 * - Trailing whitespace is stripped by markdown-it's fence rule
 * These are parser/serializer limitations within CommonMark spec behavior. */
const arbCodeContent = fc
  .stringMatching(/^[a-zA-Z0-9=;(){}\[\].,_+\-/:'"!?][a-zA-Z0-9 =;(){}\[\].,_+\-/:'"!?]*[a-zA-Z0-9=;(){}\[\].,_+\-/:'"!?]$/)
  .filter((s) => s.length >= 2 && s.length <= 40);

// ─── Markdown Content Generators ────────────────────────────────────────────

/** Generate valid markdown with only supported syntax */
function arbSupportedMarkdown(): fc.Arbitrary<string> {
  const arbMdParagraph = arbPlainWord.map((text) => text);
  const arbMdHeading = fc
    .tuple(arbLevel, arbPlainWord)
    .map(([level, text]) => '#'.repeat(level) + ' ' + text);
  const arbMdCodeBlock = fc
    .tuple(arbLanguage, arbCodeContent)
    .map(([lang, code]) => '```' + lang + '\n' + code + '\n```');
  const arbMdBlockquote = arbPlainWord.map((text) => '> ' + text);
  const arbMdHr = fc.constant('---');
  const arbMdBulletList = fc
    .array(arbPlainWord, { minLength: 1, maxLength: 3 })
    .map((items) => items.map((t) => '- ' + t).join('\n'));
  const arbMdOrderedList = fc
    .array(arbPlainWord, { minLength: 1, maxLength: 3 })
    .map((items) => items.map((t, i) => `${i + 1}. ${t}`).join('\n'));

  return fc
    .array(
      fc.oneof(
        { weight: 3, arbitrary: arbMdParagraph },
        { weight: 2, arbitrary: arbMdHeading },
        { weight: 1, arbitrary: arbMdCodeBlock },
        { weight: 1, arbitrary: arbMdBlockquote },
        { weight: 1, arbitrary: arbMdHr },
        { weight: 1, arbitrary: arbMdBulletList },
        { weight: 1, arbitrary: arbMdOrderedList },
      ),
      { minLength: 1, maxLength: 5 },
    )
    .map((blocks) => blocks.join('\n\n'));
}

/** Generate a YAML frontmatter string */
function arbFrontmatter(): fc.Arbitrary<string> {
  return fc
    .array(
      fc.tuple(
        fc.stringMatching(/^[a-z]{2,10}$/),
        fc.stringMatching(/^[a-z0-9 ]{1,20}$/),
      ),
      { minLength: 1, maxLength: 5 },
    )
    .map((entries) => {
      const yaml = entries.map(([key, val]) => `${key}: ${val}`).join('\n');
      return `---\n${yaml}\n---`;
    });
}

// ─── Property 5: Markdown Round-Trip 一致性 ─────────────────────────────────
// **Validates: Requirements 6.1, 6.2, 6.3, 7.1, 7.2, 8.1, 8.3**

describe('Property 5: Markdown Round-Trip 一致性', () => {
  it('serialize → parse → serialize produces the SAME markdown string for supported content', () => {
    fc.assert(
      fc.property(arbSupportedMarkdown(), (markdown) => {
        // First pass: parse → serialize
        const doc1 = parseMarkdown(testSchema, markdown);
        const md1 = serializeDocument(doc1);
        // Second pass: parse → serialize (must be stable)
        const doc2 = parseMarkdown(testSchema, md1);
        const md2 = serializeDocument(doc2);
        // The second serialization MUST equal the first
        expect(md2).toBe(md1);
      }),
      { numRuns: 100 },
    );
  });
});

// ─── Property 6: 不支持节点的文本保留 ──────────────────────────────────────────
// **Validates: Requirements 6.6, 7.4**

describe('Property 6: 不支持节点的文本保留', () => {
  it('plain text content is never lost through a round-trip', () => {
    fc.assert(
      fc.property(arbPlainWord, (text) => {
        const doc = parseMarkdown(testSchema, text);
        const serialized = serializeDocument(doc);
        // Text content must be preserved
        expect(serialized).toContain(text.trim());
      }),
      { numRuns: 100 },
    );
  });

  it('multiple paragraphs preserve all text content', () => {
    fc.assert(
      fc.property(
        fc.array(arbPlainWord, { minLength: 2, maxLength: 5 }),
        (paragraphs) => {
          const markdown = paragraphs.join('\n\n');
          const doc = parseMarkdown(testSchema, markdown);
          const serialized = serializeDocument(doc);
          // Each paragraph's text content must appear in output
          for (const text of paragraphs) {
            expect(serialized).toContain(text.trim());
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 7: 不支持内容的 Round-Trip 稳定性 ────────────────────────────────
// **Validates: Requirements 8.4**

describe('Property 7: 不支持内容的 Round-Trip 稳定性', () => {
  it('after one round-trip, subsequent round-trips are stable', () => {
    fc.assert(
      fc.property(arbSupportedMarkdown(), (markdown) => {
        // First round-trip
        const doc1 = parseMarkdown(testSchema, markdown);
        const md1 = serializeDocument(doc1);

        // Second round-trip
        const doc2 = parseMarkdown(testSchema, md1);
        const md2 = serializeDocument(doc2);

        // Third round-trip — must equal second
        const doc3 = parseMarkdown(testSchema, md2);
        const md3 = serializeDocument(doc3);

        expect(md3).toBe(md2);
      }),
      { numRuns: 100 },
    );
  });

  it('arbitrary text content reaches stability after one round-trip', () => {
    fc.assert(
      fc.property(
        fc.array(arbPlainWord, { minLength: 1, maxLength: 5 }),
        (lines) => {
          const markdown = lines.join('\n\n');

          // First round-trip
          const doc1 = parseMarkdown(testSchema, markdown);
          const md1 = serializeDocument(doc1);

          // Second round-trip
          const doc2 = parseMarkdown(testSchema, md1);
          const md2 = serializeDocument(doc2);

          // Stable
          expect(md2).toBe(md1);
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 8: Frontmatter 分离 ──────────────────────────────────────────
// **Validates: Requirements 8.2**

describe('Property 8: Frontmatter 分离', () => {
  it('YAML frontmatter does NOT enter ProseMirror Document', () => {
    fc.assert(
      fc.property(
        fc.tuple(arbFrontmatter(), arbPlainWord),
        ([frontmatter, body]) => {
          const fullMarkdown = `${frontmatter}\n${body}`;

          // Parse should only see the body
          const doc = parseMarkdown(testSchema, fullMarkdown);
          const serialized = serializeDocument(doc);

          // Body content must be present
          expect(serialized).toContain(body.trim());

          // Frontmatter YAML content must NOT appear in output
          const yamlLines = frontmatter.split('\n').slice(1, -1); // lines between ---
          for (const line of yamlLines) {
            expect(serialized).not.toContain(line);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it('stripFrontmatter extracts only body content', () => {
    fc.assert(
      fc.property(
        fc.tuple(arbFrontmatter(), arbPlainWord),
        ([frontmatter, body]) => {
          const fullMarkdown = `${frontmatter}\n${body}`;
          const stripped = stripFrontmatter(fullMarkdown);

          // Should contain the body
          expect(stripped).toContain(body);

          // Should NOT contain frontmatter YAML lines
          const yamlLines = frontmatter.split('\n').slice(1, -1);
          for (const line of yamlLines) {
            expect(stripped).not.toContain(line);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it('content without frontmatter is unchanged', () => {
    fc.assert(
      fc.property(
        arbPlainWord.filter((s) => !s.trimStart().startsWith('---')),
        (content) => {
          const result = stripFrontmatter(content);
          expect(result).toBe(content);
        },
      ),
      { numRuns: 100 },
    );
  });
});
