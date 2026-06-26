import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useCaptureStore } from '../stores/captureStore';
import { useDiscoverStore } from '../stores/discoverStore';
import { useAppStore } from '../stores/appStore';

interface InitialData {
  fragments: Array<{
    id: string;
    content: string;
    created_at: string;
    source: string;
    tags: string[];
    topics: string[];
  }>;
  fragment_count: number;
  article_count: number;
}

export type StartupStatus = 'loading' | 'rebuilding' | 'ready';

/**
 * Progressive startup hook.
 *
 * Flow:
 * 1. App shell renders immediately (UI responsive <100ms)
 * 2. Call `get_initial_data` to quickly fetch recent 50 fragments
 * 3. Listen for "index_rebuilding" → show progress indicator
 * 4. Listen for "index_updated" → refresh all page data
 */
export function useStartup() {
  const [status, setStatus] = useState<StartupStatus>('loading');

  useEffect(() => {
    let unmounted = false;

    // Fetch initial data for quick first-screen render
    async function fetchInitialData() {
      try {
        const data = await invoke<InitialData>('get_initial_data');
        if (unmounted) return;

        // Populate capture store with initial fragments
        // Directly set the store state for instant render
        useCaptureStore.setState({
          fragments: data.fragments,
          totalCount: data.fragment_count,
          loading: false,
        });

        // Update app counts
        useAppStore.setState({
          counts: {
            fragments: data.fragment_count,
            articles: data.article_count,
          },
        });

        if (!unmounted) {
          setStatus('ready');
        }
      } catch (e) {
        console.error('Failed to fetch initial data:', e);
        if (!unmounted) {
          setStatus('ready'); // Proceed even on error
        }
      }
    }

    fetchInitialData();

    // Listen for "index_rebuilding" event — show progress indicator
    const unlistenRebuilding = listen('index_rebuilding', () => {
      if (!unmounted) {
        setStatus('rebuilding');
      }
    });

    // Listen for "index_updated" event — refresh all data
    const unlistenUpdated = listen('index_updated', () => {
      if (!unmounted) {
        setStatus('ready');
        // Refresh all stores with fresh data
        useCaptureStore.getState().loadFragments();
        useDiscoverStore.getState().loadCards();
        useAppStore.getState().refreshCounts();
      }
    });

    return () => {
      unmounted = true;
      unlistenRebuilding.then((fn) => fn());
      unlistenUpdated.then((fn) => fn());
    };
  }, []);

  return status;
}
