import { Node, mergeAttributes, InputRule } from '@tiptap/react';

export interface ReferenceChipOptions {
  HTMLAttributes: Record<string, unknown>;
}

declare module '@tiptap/react' {
  interface Commands<ReturnType> {
    referenceChip: {
      insertReferenceChip: (fragmentId: string, invalid?: boolean) => ReturnType;
    };
  }
}

/**
 * ReferenceChip — TipTap Node extension for @[fragment-id] inline references
 *
 * - Inline, atom (non-editable) node
 * - Renders as <span class="ref-chip"> with accent background
 * - Serializes to @[fragment-id] in markdown
 * - Parses @[fragment-id] syntax via InputRule
 * - Supports invalid state for missing fragments
 */
const ReferenceChip = Node.create<ReferenceChipOptions>({
  name: 'referenceChip',

  group: 'inline',

  inline: true,

  atom: true,

  addOptions() {
    return {
      HTMLAttributes: {},
    };
  },

  addAttributes() {
    return {
      fragmentId: {
        default: null,
        parseHTML: (element) => element.getAttribute('data-fragment-id'),
        renderHTML: (attributes) => ({
          'data-fragment-id': attributes.fragmentId as string,
        }),
      },
      invalid: {
        default: false,
        parseHTML: (element) => element.classList.contains('invalid'),
        renderHTML: (attributes) => {
          if (attributes.invalid) {
            return { class: 'invalid' };
          }
          return {};
        },
      },
    };
  },

  parseHTML() {
    return [
      {
        tag: 'span.ref-chip[data-fragment-id]',
      },
    ];
  },

  renderHTML({ node, HTMLAttributes }) {
    const fragmentId = node.attrs.fragmentId as string;
    const invalid = node.attrs.invalid as boolean;
    const classes = ['ref-chip', ...(invalid ? ['invalid'] : [])].join(' ');

    return [
      'span',
      mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
        class: classes,
        'data-fragment-id': fragmentId,
        contenteditable: 'false',
      }),
      `@${fragmentId}`,
    ];
  },

  addCommands() {
    return {
      insertReferenceChip:
        (fragmentId: string, invalid = false) =>
        ({ commands }) => {
          return commands.insertContent({
            type: this.name,
            attrs: { fragmentId, invalid },
          });
        },
    };
  },

  addInputRules() {
    // Match @[fragment-id] syntax — 8 hex chars
    const inputRegex = /@\[([a-f0-9]{8})\]$/;

    return [
      new InputRule({
        find: inputRegex,
        handler: (props) => {
          const fragmentId = props.match[1];
          const { tr } = props.state;
          const node = this.type.create({ fragmentId, invalid: false });

          tr.replaceWith(props.range.from, props.range.to, node);
        },
      }),
    ];
  },
});

export default ReferenceChip;

/**
 * Serialize ReferenceChip nodes back to @[fragment-id] markdown syntax.
 * Used when converting editor content to markdown for saving.
 */
export function serializeReferenceChips(html: string): string {
  // Replace <span class="ref-chip" data-fragment-id="xxx">@xxx</span> with @[xxx]
  return html.replace(
    /<span[^>]*class="ref-chip[^"]*"[^>]*data-fragment-id="([^"]+)"[^>]*>[^<]*<\/span>/g,
    '@[$1]',
  );
}

/**
 * Deserialize @[fragment-id] markdown syntax to HTML for the editor.
 * Used when loading markdown content into the editor.
 */
export function deserializeReferenceChips(
  markdown: string,
  existingIds?: Set<string>,
): string {
  return markdown.replace(/@\[([a-f0-9]{8})\]/g, (_match, id: string) => {
    const invalid = existingIds ? !existingIds.has(id) : false;
    const cls = invalid ? 'ref-chip invalid' : 'ref-chip';
    return `<span class="${cls}" data-fragment-id="${id}" contenteditable="false">@${id}</span>`;
  });
}
