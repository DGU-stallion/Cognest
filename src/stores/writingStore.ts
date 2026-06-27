import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ─── Types ──────────────────────────────────────────────────────────────────

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  streaming?: boolean;
}

export interface RecommendedFragment {
  id: string;
  content: string;
  similarity: number;
  tags: string[];
}

/** Streaming chunk payload emitted by Tauri event `writing_chunk` */
interface StreamChunkPayload {
  type: 'Delta' | 'Done' | 'Error';
  content?: string;
  usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number };
  error?: Record<string, unknown>; // LlmError enum — e.g. { Timeout: { provider: "..." } }
  partial_tokens?: number;
}

export interface WritingStore {
  messages: ChatMessage[];
  streaming: boolean;
  error: string | null;
  panelOpen: boolean;
  recommendedFragments: RecommendedFragment[];

  togglePanel: () => void;
  sendMessage: (articleId: string, message: string) => Promise<void>;
  streamMessage: (articleId: string, message: string) => void;
  retryLast: () => void;
  quickAction: (action: 'outline' | 'expand' | 'recommend', articleContent: string) => void;
  loadRecommendations: (articleContent: string) => Promise<void>;
  clearHistory: () => void;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
}

function buildHistory(messages: ChatMessage[]): Array<{ role: string; content: string }> {
  return messages
    .filter((m) => !m.streaming)
    .slice(-10)
    .map((m) => ({ role: m.role, content: m.content }));
}

// ─── Store ──────────────────────────────────────────────────────────────────

export const useWritingStore = create<WritingStore>((set, get) => ({
  messages: [],
  streaming: false,
  error: null,
  panelOpen: false,
  recommendedFragments: [],

  togglePanel: () => set((state) => ({ panelOpen: !state.panelOpen })),

  sendMessage: async (articleId: string, message: string) => {
    const userMsg: ChatMessage = {
      id: generateId(),
      role: 'user',
      content: message,
      timestamp: new Date().toISOString(),
    };

    set((state) => ({
      messages: [...state.messages, userMsg],
      streaming: true,
      error: null,
    }));

    try {
      const history = buildHistory(get().messages);
      const response = await invoke<string>('writing_chat', {
        articleId,
        message,
        history,
      });

      const assistantMsg: ChatMessage = {
        id: generateId(),
        role: 'assistant',
        content: response,
        timestamp: new Date().toISOString(),
      };

      set((state) => ({
        messages: [...state.messages, assistantMsg],
        streaming: false,
      }));
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      set({ streaming: false, error: errorMessage });
    }
  },

  streamMessage: (articleId: string, message: string) => {
    const userMsg: ChatMessage = {
      id: generateId(),
      role: 'user',
      content: message,
      timestamp: new Date().toISOString(),
    };

    const assistantMsgId = generateId();
    const assistantMsg: ChatMessage = {
      id: assistantMsgId,
      role: 'assistant',
      content: '',
      timestamp: new Date().toISOString(),
      streaming: true,
    };

    set((state) => ({
      messages: [...state.messages, userMsg, assistantMsg],
      streaming: true,
      error: null,
    }));

    let unlisten: UnlistenFn | null = null;

    // Set up event listener for streaming chunks
    listen<string>('writing_chunk', (event) => {
      // Payload comes as a JSON string (double-serialized by Tauri emit)
      let chunk: StreamChunkPayload;
      try {
        const raw = event.payload;
        chunk = typeof raw === 'string' ? JSON.parse(raw) : raw;
      } catch {
        console.error('[WritingStore] Failed to parse writing_chunk:', event.payload);
        return;
      }
      console.log('[WritingStore] received writing_chunk:', chunk);

      if (chunk.type === 'Delta' && chunk.content) {
        // Append delta content to the streaming assistant message
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId
              ? { ...m, content: m.content + chunk.content }
              : m
          ),
        }));
      } else if (chunk.type === 'Done') {
        // Stream complete — mark message as finished
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId ? { ...m, streaming: false } : m
          ),
          streaming: false,
        }));
        // Clean up listener
        if (unlisten) unlisten();
      } else if (chunk.type === 'Error') {
        // Stream error — mark message as finished with error
        const errorMessage = chunk.error?.message || chunk.error?.Timeout?.provider
          ? `${chunk.error?.Timeout?.provider || 'Provider'} 请求超时，请重试`
          : '操作失败，请重试';
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId ? { ...m, streaming: false } : m
          ),
          streaming: false,
          error: errorMessage,
        }));
        // Clean up listener
        if (unlisten) unlisten();
      }
    }).then((unlistenFn) => {
      unlisten = unlistenFn;
    });

    // Invoke the stream command (it emits events, doesn't return content directly)
    const history = buildHistory(get().messages.slice(0, -2)); // Exclude the just-added user+assistant msgs
    console.log('[WritingStore] invoking writing_stream_chat:', { articleId, message, historyLen: history.length });
    invoke('writing_stream_chat', {
      articleId,
      message,
      history,
    }).then(() => {
      console.log('[WritingStore] writing_stream_chat invoke resolved (OK)');
    }).catch((e) => {
      const errorMessage = e instanceof Error ? e.message : String(e);
      console.error('[WritingStore] writing_stream_chat invoke FAILED:', errorMessage);
      set((state) => ({
        messages: state.messages.map((m) =>
          m.id === assistantMsgId ? { ...m, streaming: false } : m
        ),
        streaming: false,
        error: errorMessage,
      }));
      if (unlisten) unlisten();
    });
  },

  retryLast: () => {
    const { messages } = get();
    if (messages.length === 0) return;

    // Find the last user message to retry
    let lastUserMsgIndex = -1;
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'user') {
        lastUserMsgIndex = i;
        break;
      }
    }

    if (lastUserMsgIndex === -1) return;

    const lastUserMsg = messages[lastUserMsgIndex];

    // Remove the last user message and any assistant response after it
    const trimmedMessages = messages.slice(0, lastUserMsgIndex);
    set({ messages: trimmedMessages, error: null });

    // Re-send using streamMessage (needs articleId — use empty string as fallback,
    // the actual articleId should be managed by the component)
    // Note: retryLast re-dispatches the message via streamMessage
    // The component should call streamMessage directly with the articleId if needed
    // Here we add the user message back and let the caller handle it
    const retryMsg: ChatMessage = {
      ...lastUserMsg,
      id: generateId(),
      timestamp: new Date().toISOString(),
    };
    set((state) => ({ messages: [...state.messages, retryMsg] }));
  },

  quickAction: (action: 'outline' | 'expand' | 'recommend', articleContent: string) => {
    const prompts: Record<string, string> = {
      outline: `请为以下文章推荐一个写作结构/大纲：\n\n${articleContent.slice(0, 2000)}`,
      expand: `请帮我扩展以下内容的当前段落，使其更丰富详实：\n\n${articleContent.slice(0, 2000)}`,
      recommend: `请基于以下文章内容，推荐相关的素材和参考：\n\n${articleContent.slice(0, 2000)}`,
    };

    const message = prompts[action];
    if (!message) return;

    // Use streamMessage with an empty articleId — the panel should provide the real one
    // This is a convenience dispatch; the component wrapping this store action
    // should supply the actual articleId
    const userMsg: ChatMessage = {
      id: generateId(),
      role: 'user',
      content: message,
      timestamp: new Date().toISOString(),
    };

    const assistantMsgId = generateId();
    const assistantMsg: ChatMessage = {
      id: assistantMsgId,
      role: 'assistant',
      content: '',
      timestamp: new Date().toISOString(),
      streaming: true,
    };

    set((state) => ({
      messages: [...state.messages, userMsg, assistantMsg],
      streaming: true,
      error: null,
    }));

    let unlisten: UnlistenFn | null = null;

    listen<StreamChunkPayload>('writing_chunk', (event) => {
      const chunk = event.payload;

      if (chunk.type === 'Delta' && chunk.content) {
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId
              ? { ...m, content: m.content + chunk.content }
              : m
          ),
        }));
      } else if (chunk.type === 'Done') {
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId ? { ...m, streaming: false } : m
          ),
          streaming: false,
        }));
        if (unlisten) unlisten();
      } else if (chunk.type === 'Error') {
        const errorMessage = chunk.error?.message || 'Stream error occurred';
        set((state) => ({
          messages: state.messages.map((m) =>
            m.id === assistantMsgId ? { ...m, streaming: false } : m
          ),
          streaming: false,
          error: errorMessage,
        }));
        if (unlisten) unlisten();
      }
    }).then((unlistenFn) => {
      unlisten = unlistenFn;
    });

    const history = buildHistory(get().messages.slice(0, -2));
    invoke('writing_stream_chat', {
      articleId: '',
      message,
      history,
    }).catch((e) => {
      const errorMessage = e instanceof Error ? e.message : String(e);
      set((state) => ({
        messages: state.messages.map((m) =>
          m.id === assistantMsgId ? { ...m, streaming: false } : m
        ),
        streaming: false,
        error: errorMessage,
      }));
      if (unlisten) unlisten();
    });
  },

  loadRecommendations: async (articleContent: string) => {
    try {
      const results = await invoke<RecommendedFragment[]>('writing_recommend', {
        articleContent,
        limit: 5,
      });
      set({ recommendedFragments: results });
    } catch (e) {
      console.error('Failed to load recommendations:', e);
      set({ recommendedFragments: [] });
    }
  },

  clearHistory: () => set({ messages: [], error: null, recommendedFragments: [] }),
}));
