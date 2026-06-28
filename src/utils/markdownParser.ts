import { MarkdownParser } from 'prosemirror-markdown';
import markdownit from 'markdown-it';
import type MarkdownIt from 'markdown-it';
import type StateInline from 'markdown-it/lib/rules_inline/state_inline.mjs';
import { Node as PMNode, Schema } from '@tiptap/pm/model';

/**
 * Custom markdown-it inline rule plugin for @[hex8] reference chip syntax.
 * Matches `@[` followed by exactly 8 hex characters and `]`.
 */
function referenceChipPlugin(md: MarkdownIt): void {
  md.inline.ruler.push('reference_chip', (state: StateInline, silent: boolean): boolean => {
    const start = state.pos;
    const max = state.posMax;

    // Must start with @[
    if (state.src.charCodeAt(start) !== 0x40 /* @ */) return false;
    if (start + 1 >= max || state.src.charCodeAt(start + 1) !== 0x5B /* [ */) return false;

    // Need at least @[ + 8 hex chars + ] = 11 chars total
    if (start + 10 >= max) return false;

    // Extract and validate 8 hex characters
    const hexPart = state.src.slice(start + 2, start + 10);
    if (!/^[a-f0-9]{8}$/.test(hexPart)) return false;

    // Must end with ]
    if (state.src.charCodeAt(start + 10) !== 0x5D /* ] */) return false;

    if (!silent) {
      const token = state.push('reference_chip', '', 0);
      token.content = hexPart;
    }

    state.pos = start + 11;
    return true;
  });
}

/**
 * Strip YAML frontmatter from a markdown string.
 * Frontmatter is defined as content between the first `---` at line start
 * and the second `---` at line start. Only the content after the second
 * delimiter is returned.
 */
export function stripFrontmatter(md: string): string {
  // Match frontmatter: starts with --- on first line, ends with --- on its own line
  const lines = md.split('\n');

  if (lines.length === 0) return md;

  // First line must be exactly '---' (possibly with trailing whitespace)
  if (lines[0].trim() !== '---') return md;

  // Find the closing ---
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') {
      // Return everything after the closing ---
      return lines.slice(i + 1).join('\n');
    }
  }

  // No closing --- found — return original (no valid frontmatter)
  return md;
}

/**
 * Create a MarkdownParser for Cognest's ProseMirror/TipTap schema.
 *
 * Parses CommonMark + GFM strikethrough + @[hex8] reference chips.
 * Token keys are markdown-it token names; block/node/mark values are
 * TipTap schema node type names (camelCase).
 */
export function createMarkdownParser(schema: Schema): MarkdownParser {
  const md = markdownit('commonmark', { html: false })
    .enable('strikethrough')
    .use(referenceChipPlugin);

  return new MarkdownParser(schema, md, {
    // Block nodes
    blockquote: { block: 'blockquote' },
    paragraph: { block: 'paragraph' },
    list_item: { block: 'listItem' },
    bullet_list: { block: 'bulletList' },
    ordered_list: {
      block: 'orderedList',
      getAttrs: (tok) => ({ start: +(tok.attrGet('start') || 1) }),
    },
    heading: {
      block: 'heading',
      getAttrs: (tok) => ({ level: +tok.tag.slice(1) }),
    },
    code_block: { block: 'codeBlock', noCloseToken: true },
    fence: {
      block: 'codeBlock',
      getAttrs: (tok) => ({ language: tok.info || '' }),
      noCloseToken: true,
    },
    // Inline/leaf nodes
    hr: { node: 'horizontalRule' },
    hardbreak: { node: 'hardBreak' },
    // Custom inline node
    reference_chip: {
      node: 'referenceChip',
      getAttrs: (tok) => ({ fragmentId: tok.content }),
    },
    // Marks
    em: { mark: 'italic' },
    strong: { mark: 'bold' },
    link: {
      mark: 'link',
      getAttrs: (tok) => ({
        href: tok.attrGet('href'),
        title: tok.attrGet('title') || null,
      }),
    },
    code_inline: { mark: 'code', noCloseToken: true },
    s: { mark: 'strikethrough' },
  });
}

/**
 * Parse a Markdown string into a ProseMirror Document.
 *
 * - Strips YAML frontmatter (content between first and second `---`)
 * - Returns a doc with a single empty paragraph for empty/whitespace input
 * - Otherwise parses using prosemirror-markdown + markdown-it
 */
export function parseMarkdown(schema: Schema, markdown: string): PMNode {
  const body = stripFrontmatter(markdown);

  // Empty or whitespace-only → single empty paragraph doc
  if (!body.trim()) {
    return schema.node('doc', null, [schema.node('paragraph')]);
  }

  const parser = createMarkdownParser(schema);
  return parser.parse(body)!;
}
