import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

// ─── Interfaces (Requirements 3.1, 3.6) ────────────────────────────────────

export interface ProviderConfig {
  id: string;
  name: string;
  providerType: 'deepseek' | 'ollama' | 'openai_compat';
  endpoint: string;
  model: string;
  temperature: number;
  enabled: boolean;
}

export interface AgentRouting {
  defaultProvider: string | null;
  overrides: Record<string, string>; // agent_name → provider_id
}

export interface AiSettings {
  providers: ProviderConfig[];
  routing: AgentRouting;
}

// ─── Store Interface ────────────────────────────────────────────────────────

export interface SettingsStore {
  settings: AiSettings | null;
  loading: boolean;
  error: string | null;

  loadSettings: () => Promise<void>;
  saveSettings: (settings: AiSettings, apiKeys: Record<string, string>) => Promise<void>;
  validateProvider: (provider: ProviderConfig, apiKey: string) => Promise<boolean>;
  listOllamaModels: (endpoint: string) => Promise<string[]>;
}

// ─── Store Implementation ───────────────────────────────────────────────────

export const useSettingsStore = create<SettingsStore>((set) => ({
  settings: null,
  loading: false,
  error: null,

  loadSettings: async () => {
    set({ loading: true, error: null });
    try {
      const result = await invoke<AiSettings>('get_ai_settings');
      set({ settings: result, loading: false });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.error('Failed to load AI settings:', message);
      set({ error: message, loading: false });
    }
  },

  saveSettings: async (settings: AiSettings, apiKeys: Record<string, string>) => {
    set({ loading: true, error: null });
    try {
      await invoke('save_ai_settings', { settings, apiKeys });
      set({ settings, loading: false });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.error('Failed to save AI settings:', message);
      set({ error: message, loading: false });
    }
  },

  validateProvider: async (provider: ProviderConfig, apiKey: string) => {
    try {
      const valid = await invoke<boolean>('validate_provider', { provider, apiKey });
      return valid;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.error('Provider validation failed:', message);
      return false;
    }
  },

  listOllamaModels: async (endpoint: string) => {
    try {
      const models = await invoke<string[]>('list_ollama_models', { endpoint });
      return models;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      console.error('Failed to list Ollama models:', message);
      return [];
    }
  },
}));
