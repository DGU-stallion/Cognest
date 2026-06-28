import { useEffect, useRef, useCallback, useImperativeHandle, forwardRef, useState } from 'react';
import { useEditor, EditorContent } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import Placeholder from '@tiptap/extension-placeholder';
import ReferenceChip, { deserializeReferenceChips, serializeReferenceChips } from './extensions/ReferenceChip';
import { serializeDocument } from '../utils/markdownSerializer';
import { parseMarkdown } from '../utils/markdownParser';
import { showToast } from './Toast';
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

export type EditorMode = 'wysiwyg' | 'source';

export interface EditorProps {
  /** Initial content (Markdown body string) */
  content: string;
  /** Called with Markdown body string after 1s debounce (no frontmatter) */
  onUpdate: (markdown: string) => void;
  /** Called when word count changes */
  onWordCountChange?: (count: number) => void;
  /** Set of existing fragment IDs — used to mark invalid references */
  existingFragmentIds?: Set<string>;
  /** Current editor mode (controlled by parent) */
  mode: EditorMode;
  /** Callback when mode changes */
  onModeChange: (mode: EditorMode) => void;
}

export interface EditorHandle {
  /** Insert a ReferenceChip at current cursor position (or end if no focus) */
  insertReference: (fragmentId: string, invalid?: boolean) => void;
  /** Get current editor text content */
  getText: () => string;
  /** Get current HTML content (legacy — in source mode returns raw markdown) */
  getHTML: () => string;
  /** Get current content as Markdown body string (no frontmatter) */
  getMarkdown: () => string;
  /** Set editor content programmatically (accepts markdown string) */
  setContent: (markdown: string) => void;
  /** Get the TipTap editor instance for toolbar commands */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getEditor: () => any;
  /** Get the current editor mode */
  getMode: () => EditorMode;
}

/**
 * TipTap Editor component with:
 * - StarterKit (H1-H3, Bold, Italic, Code, CodeBlock, Blockquote, Lists)
 * - ReferenceChip custom node (@[fragment-id] syntax)
 * - Placeholder extension
 * - AutoSave (1s debounce → serialize to Markdown → pass to onUpdate)
 * - Word count
 * - Dual mode: WYSIWYG / Source (raw Markdown) — mode controlled by parent
 */
const Editor = forwardRef<EditorHandle, EditorProps>(function Editor(
  { content, onUpdate, onWordCountChange, existingFragmentIds, mode, onModeChange },
  ref,
) {
  const [sourceText, setSourceText] = useState('');
  const sourceTextareaRef = useRef<HTMLTextAreaElement>(null);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onUpdateRef = useRef(onUpdate);
  const onWordCountChangeRef = useRef(onWordCountChange);
  const prevModeRef = useRef<EditorMode>(mode);

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
    content: '',
    onUpdate: ({ editor: ed }) => {
      // Only trigger auto-save in wysiwyg mode
      if (mode !== 'wysiwyg') return;

      // Word count
      const text = ed.getText();
      const count = countWords(text);
      onWordCountChangeRef.current?.(count);

      // AutoSave with 1s debounce
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        const doc = ed.state.doc;
        const markdown = serializeDocument(doc);
        onUpdateRef.current(markdown);
      }, 1000);
    },
  });

  // Handle mode switching when parent changes mode
  useEffect(() => {
    if (!editor) return;
    const prevMode = prevModeRef.current;
    prevModeRef.current = mode;

    if (prevMode === mode) return;

    if (mode === 'source' && prevMode === 'wysiwyg') {
      // Switch to source: serialize current ProseMirror doc to Markdown
      const doc = editor.state.doc;
      const markdown = serializeDocument(doc);
      setSourceText(markdown);
      setTimeout(() => {
        if (sourceTextareaRef.current) {
          sourceTextareaRef.current.focus();
          sourceTextareaRef.current.setSelectionRange(0, 0);
        }
      }, 0);
    } else if (mode === 'wysiwyg' && prevMode === 'source') {
      // Switch to WYSIWYG: parse Markdown back to ProseMirror doc
      try {
        const schema = editor.state.schema;
        const doc = parseMarkdown(schema, sourceText);
        editor.commands.setContent(doc.toJSON());
        setTimeout(() => {
          editor.commands.focus('start');
        }, 0);
      } catch (err) {
        // Parse failure: revert to source mode, show error toast
        const message = err instanceof Error ? err.message : 'Markdown 解析失败';
        showToast(message, 'error');
        onModeChange('source');
      }
    }
  }, [mode, editor, sourceText, onModeChange]);

  // Source mode auto-save with 1s debounce
  const handleSourceChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newValue = e.target.value;
    setSourceText(newValue);

    // Word count
    const count = countWords(newValue);
    onWordCountChangeRef.current?.(count);

    // AutoSave with 1s debounce
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      onUpdateRef.current(newValue);
    }, 1000);
  }, []);

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
    if (editor && mode === 'wysiwyg') {
      const text = editor.getText();
      const count = countWords(text);
      onWordCountChangeRef.current?.(count);
    }
  }, [editor, mode]);

  // Expose imperative handle for parent to insert references
  const insertReference = useCallback(
    (fragmentId: string, invalid = false) => {
      if (!editor || mode !== 'wysiwyg') return;

      if (editor.isFocused) {
        editor.commands.insertReferenceChip(fragmentId, invalid);
      } else {
        // No focus — append to end of document
        editor.commands.focus('end');
        editor.commands.insertReferenceChip(fragmentId, invalid);
      }
    },
    [editor, mode],
  );

  useImperativeHandle(
    ref,
    () => ({
      insertReference,
      getText: () => {
        if (mode === 'source') return sourceText;
        return editor?.getText() ?? '';
      },
      getHTML: () => {
        if (mode === 'source') return sourceText;
        const html = editor?.getHTML() ?? '';
        return serializeReferenceChips(html);
      },
      getMarkdown: () => {
        if (mode === 'source') return sourceText;
        if (!editor) return '';
        return serializeDocument(editor.state.doc);
      },
      setContent: (markdown: string) => {
        if (!editor) return;
        try {
          const schema = editor.state.schema;
          const doc = parseMarkdown(schema, markdown);
          editor.commands.setContent(doc.toJSON());
          if (mode === 'source') {
            setSourceText(markdown);
          }
        } catch {
          // Fallback: set as HTML (for legacy content)
          editor.commands.setContent(deserializeReferenceChips(markdown || '', existingFragmentIds));
          if (mode === 'source') {
            setSourceText(markdown);
          }
        }
      },
      getEditor: () => editor ?? null,
      getMode: () => mode,
    }),
    [editor, insertReference, existingFragmentIds, mode, sourceText],
  );

  return (
    <div className="editor-wrapper">
      {mode === 'wysiwyg' ? (
        <EditorContent editor={editor} />
      ) : (
        <textarea
          ref={sourceTextareaRef}
          className="editor-source-textarea"
          value={sourceText}
          onChange={handleSourceChange}
          placeholder="在此编辑 Markdown 源码…"
          spellCheck={false}
        />
      )}
    </div>
  );
});

export default Editor;
