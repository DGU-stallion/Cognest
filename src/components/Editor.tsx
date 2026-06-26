import { useEffect, useRef, useCallback, useImperativeHandle, forwardRef } from 'react';
import { useEditor, EditorContent } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import Placeholder from '@tiptap/extension-placeholder';
import ReferenceChip, { deserializeReferenceChips, serializeReferenceChips } from './extensions/ReferenceChip';
import './Editor.css';

/**
 * Count words in mixed Chinese/English text.
 * Chinese characters are counted individually.
 * English words are space-separated sequences.
 */
export function countWords(text: string): number {
  if (!text || !text.trim()) return 0;
  // Count Chinese characters individually
  const chineseChars = (text.match(/[\u4e00-\u9fff\u3400-\u4dbf]/g) || []).length;
  // Count English words (space-separated, after removing Chinese chars)
  const englishWords = text
    .replace(/[\u4e00-\u9fff\u3400-\u4dbf]/g, ' ')
    .trim()
    .split(/\s+/)
    .filter((w) => w.length > 0).length;
  return chineseChars + englishWords;
}

export interface EditorProps {
  /** Initial content in HTML format (with @[id] already deserialized to ref-chip spans) */
  content: string;
  /** Called with serialized markdown-like HTML after 1s debounce */
  onUpdate: (html: string) => void;
  /** Called when word count changes */
  onWordCountChange?: (count: number) => void;
  /** Set of existing fragment IDs — used to mark invalid references */
  existingFragmentIds?: Set<string>;
}

export interface EditorHandle {
  /** Insert a ReferenceChip at current cursor position (or end if no focus) */
  insertReference: (fragmentId: string, invalid?: boolean) => void;
  /** Get current editor text content */
  getText: () => string;
  /** Get current HTML content */
  getHTML: () => string;
  /** Set editor content programmatically */
  setContent: (html: string) => void;
  /** Get the TipTap editor instance for toolbar commands */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getEditor: () => any;
}

/**
 * TipTap Editor component with:
 * - StarterKit (H1-H3, Bold, Italic, Code, CodeBlock, Blockquote, Lists)
 * - ReferenceChip custom node (@[fragment-id] syntax)
 * - Placeholder extension
 * - AutoSave (1s debounce)
 * - Word count
 */
const Editor = forwardRef<EditorHandle, EditorProps>(function Editor(
  { content, onUpdate, onWordCountChange, existingFragmentIds },
  ref,
) {
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onUpdateRef = useRef(onUpdate);
  const onWordCountChangeRef = useRef(onWordCountChange);

  // Keep refs in sync
  useEffect(() => {
    onUpdateRef.current = onUpdate;
  }, [onUpdate]);
  useEffect(() => {
    onWordCountChangeRef.current = onWordCountChange;
  }, [onWordCountChange]);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        bold: {},
        italic: {},
        code: {},
        codeBlock: {},
        blockquote: {},
        orderedList: {},
        bulletList: {},
      }),
      Placeholder.configure({
        placeholder: '开始写作…',
      }),
      ReferenceChip,
    ],
    content: deserializeReferenceChips(content || '', existingFragmentIds),
    onUpdate: ({ editor: ed }) => {
      // Word count
      const text = ed.getText();
      const count = countWords(text);
      onWordCountChangeRef.current?.(count);

      // AutoSave with 1s debounce
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        const html = ed.getHTML();
        // Serialize ReferenceChip spans back to @[id] syntax for storage
        const serialized = serializeReferenceChips(html);
        onUpdateRef.current(serialized);
      }, 1000);
    },
  });

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  // Emit initial word count
  useEffect(() => {
    if (editor) {
      const text = editor.getText();
      const count = countWords(text);
      onWordCountChangeRef.current?.(count);
    }
  }, [editor]);

  // Expose imperative handle for parent to insert references
  const insertReference = useCallback(
    (fragmentId: string, invalid = false) => {
      if (!editor) return;

      if (editor.isFocused) {
        editor.commands.insertReferenceChip(fragmentId, invalid);
      } else {
        // No focus — append to end of document
        editor.commands.focus('end');
        editor.commands.insertReferenceChip(fragmentId, invalid);
      }
    },
    [editor],
  );

  useImperativeHandle(
    ref,
    () => ({
      insertReference,
      getText: () => editor?.getText() ?? '',
      getHTML: () => {
        const html = editor?.getHTML() ?? '';
        return serializeReferenceChips(html);
      },
      setContent: (html: string) => {
        if (editor) {
          editor.commands.setContent(deserializeReferenceChips(html || '', existingFragmentIds));
        }
      },
      getEditor: () => editor ?? null,
    }),
    [editor, insertReference, existingFragmentIds],
  );

  return (
    <div className="editor-wrapper">
      <EditorContent editor={editor} />
    </div>
  );
});

export default Editor;
