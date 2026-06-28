import { MarkdownSerializer, MarkdownSerializerState } from 'prosemirror-markdown';
import type { Node as PMNode, Mark, Schema } from '@tiptap/pm/model';

/**
 * Create a MarkdownSerializer for Cognest's ProseMirror schema.
 *
 * Supports:
 * - Nodes: heading (h1-h6), paragraph, blockquote, code_block, bullet_list,
 *          ordered_list (≥4 levels), horizontal_rule, image, list_item,
 *          hard_break, referenceChip
 * - Marks: bold(**), italic(*), code(`), link([text](url)), strikethrough(~~)
 * - Unsupported nodes/marks fallback to plain text paragraph output
 */
export function createMarkdownSerializer(_schema: Schema): MarkdownSerializer {
  const nodes: Record<
    string,
    (state: MarkdownSerializerState, node: PMNode, parent: PMNode, index: number) => void
  > = {
    // Document root — just render children
    doc(state, node) {
      state.renderContent(node);
    },

    // Paragraph — render inline content, close block
    paragraph(state, node, _parent, _index) {
      state.renderInline(node);
      state.closeBlock(node);
    },

    // Heading — ATX style: # prefix
    heading(state, node, _parent, _index) {
      const level = node.attrs.level as number;
      state.write(state.repeat('#', level) + ' ');
      state.renderInline(node);
      state.closeBlock(node);
    },

    // Blockquote — > prefix
    blockquote(state, node) {
      state.wrapBlock('> ', null, node, () => state.renderContent(node));
    },

    // Code block — fenced with optional language identifier
    code_block(state, node) {
      const language = (node.attrs.language as string) || '';
      state.write('```' + language + '\n');
      state.text(node.textContent, false);
      state.ensureNewLine();
      state.write('```');
      state.closeBlock(node);
    },

    // Bullet list — delegates to renderList
    bullet_list(state, node) {
      state.renderList(node, '  ', () => '- ');
    },

    // Ordered list — 1. 2. 3. style, supports deep nesting via indentation
    ordered_list(state, node) {
      const start = (node.attrs.start as number) || 1;
      state.renderList(node, '   ', (i) => {
        const num = start + i;
        return num + '. ';
      });
    },

    // List item — render its block content
    list_item(state, node, _parent, _index) {
      state.renderContent(node);
    },

    // Horizontal rule
    horizontal_rule(state, node) {
      state.write('---');
      state.closeBlock(node);
    },

    // Image — ![alt](src)
    image(state, node) {
      const alt = state.esc((node.attrs.alt as string) || '');
      const src = (node.attrs.src as string) || '';
      const title = node.attrs.title as string;
      state.write(
        '![' + alt + '](' + src + (title ? ' "' + title.replace(/"/g, '\\"') + '"' : '') + ')',
      );
    },

    // Hard break
    hard_break(state) {
      state.write('\\\n');
    },

    // ReferenceChip — custom inline node → @[fragmentId]
    referenceChip(state, node) {
      state.write(`@[${node.attrs.fragmentId}]`);
    },

    // Text node — should be handled by renderInline, but included for completeness
    text(state, node) {
      state.text(node.text || '');
    },
  };

  const marks: Record<string, {
    open: string | ((state: MarkdownSerializerState, mark: Mark, parent: PMNode, index: number) => string);
    close: string | ((state: MarkdownSerializerState, mark: Mark, parent: PMNode, index: number) => string);
    mixable?: boolean;
    expelEnclosingWhitespace?: boolean;
    escape?: boolean;
  }> = {
    bold: {
      open: '**',
      close: '**',
      mixable: true,
      expelEnclosingWhitespace: true,
    },
    italic: {
      open: '*',
      close: '*',
      mixable: true,
      expelEnclosingWhitespace: true,
    },
    code: {
      open: '`',
      close: '`',
      escape: false,
    },
    link: {
      open: (_state, _mark, _parent, _index) => '[',
      close: (_state, mark, _parent, _index) => `](${mark.attrs.href})`,
    },
    strikethrough: {
      open: '~~',
      close: '~~',
      mixable: true,
    },
  };

  // Use strict: false so unsupported nodes/marks don't throw errors.
  // Instead, they'll be rendered as their text content.
  return new MarkdownSerializer(nodes, marks, { strict: false });
}

/**
 * Serialize a ProseMirror document to a Markdown string.
 *
 * This only processes body content — frontmatter is handled by the Rust backend.
 */
export function serializeDocument(doc: PMNode): string {
  const serializer = createMarkdownSerializer(doc.type.schema);
  return serializer.serialize(doc);
}
