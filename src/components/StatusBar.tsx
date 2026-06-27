import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../stores/appStore';
import { showToast } from './Toast';
import './StatusBar.css';

interface SyncStatus {
  status: 'synced' | 'unsynced' | 'no-remote';
  fileCount?: number;
}

// AI Job event payloads
interface JobStatusChangedPayload {
  job_id: string;
  status: string;
  progress?: number;
}

interface JobFailedPayload {
  job_id: string;
  job_type: string;
  reason: string;
}

interface ProviderNeededPayload {
  job_id: string;
  job_type: string;
}

interface EmbeddingProgressPayload {
  completed: number;
  total: number;
}

interface StatusBarProps {
  onSettingsClick?: () => void;
}

export default function StatusBar({ onSettingsClick }: StatusBarProps) {
  const { counts, refreshCounts } = useAppStore();
  const [taskText, setTaskText] = useState<string>('');
  const [gitStatus, setGitStatus] = useState<SyncStatus>({ status: 'synced' });

  // AI status state
  const [aiStatus, setAiStatus] = useState<string>('');
  const [providerNeeded, setProviderNeeded] = useState(false);

  const totalCount = counts.fragments + counts.articles;

  // Fetch git status from backend
  const fetchGitStatus = useCallback(async () => {
    try {
      const status = await invoke<SyncStatus>('git_status');
      setGitStatus(status);
    } catch {
      // git may not be configured yet — leave default
    }
  }, []);

  // Listen for Tauri events: index_updated and sync_status
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    // When index updates, refresh knowledge base counts
    listen('index_updated', () => {
      refreshCounts();
      setTaskText('');
    }).then((unlisten) => unlisteners.push(unlisten));

    // Index rebuilding in progress
    listen('index_rebuilding', () => {
      setTaskText('正在构建索引…');
    }).then((unlisten) => unlisteners.push(unlisten));

    // Sync status changes from backend
    listen<{ status: string; message?: string }>('sync_status', (event) => {
      const { status } = event.payload;
      if (status === 'syncing') {
        setTaskText('同步中…');
      } else if (status === 'indexing') {
        setTaskText('索引更新中…');
      } else {
        setTaskText('');
        fetchGitStatus();
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [refreshCounts, fetchGitStatus]);

  // Listen for AI job events
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    // Job status changes — show progress when running
    listen<JobStatusChangedPayload>('job_status_changed', (event) => {
      const { status, progress } = event.payload;
      if (status === 'running' && progress != null) {
        setAiStatus(`AI ${progress}%`);
        setProviderNeeded(false);
      } else if (status === 'completed') {
        setAiStatus('');
      } else if (status === 'blocked') {
        setAiStatus('');
      } else if (status === 'pending') {
        setAiStatus('AI 排队中…');
      } else {
        setAiStatus('');
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    // Job failed — show toast notification
    listen<JobFailedPayload>('job_failed', (event) => {
      const { job_type, reason } = event.payload;
      showToast(`AI 任务失败 [${job_type}]: ${reason}`, 'error');
      setAiStatus('');
    }).then((unlisten) => unlisteners.push(unlisten));

    // Provider needed — show configure link
    listen<ProviderNeededPayload>('provider_needed', () => {
      setProviderNeeded(true);
      setAiStatus('');
    }).then((unlisten) => unlisteners.push(unlisten));

    // Embedding batch progress
    listen<EmbeddingProgressPayload>('embedding_progress', (event) => {
      const { completed, total } = event.payload;
      if (completed < total) {
        setAiStatus(`Embedding: ${completed}/${total}`);
      } else {
        setAiStatus('');
      }
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  // Initial data fetch
  useEffect(() => {
    refreshCounts();
    fetchGitStatus();
  }, [refreshCounts, fetchGitStatus]);

  // Git status display text
  const gitLabel = (() => {
    switch (gitStatus.status) {
      case 'synced':
        return '已同步';
      case 'unsynced':
        return `未同步(${gitStatus.fileCount ?? 0})`;
      case 'no-remote':
        return '无远程';
    }
  })();

  return (
    <footer className="status-bar">
      {/* Left: background task indicator */}
      <div className="status-bar-left">
        {taskText && (
          <span className="status-bar-task">
            <svg className="status-bar-task-spinner" viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M9 2a7 7 0 1 1-7 7" strokeLinecap="round" />
            </svg>
            {taskText}
          </span>
        )}

        {/* AI activity indicator */}
        {aiStatus && (
          <span className="status-bar-ai">
            <svg className="status-bar-task-spinner" viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M9 2a7 7 0 1 1-7 7" strokeLinecap="round" />
            </svg>
            {aiStatus}
          </span>
        )}

        {/* Provider needed — configure link */}
        {providerNeeded && (
          <button
            className="status-bar-provider-link"
            onClick={onSettingsClick}
            title="AI 模型未配置，点击前往设置"
          >
            <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M9 3v4M9 11v.01" strokeLinecap="round" />
              <circle cx="9" cy="9" r="7" />
            </svg>
            配置 AI 模型
          </button>
        )}
      </div>

      {/* Right: counts + git status + settings */}
      <div className="status-bar-right">
        {/* Knowledge base count */}
        <span className="status-bar-counts">
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M3 4h12M3 9h12M3 14h8" strokeLinecap="round" />
          </svg>
          知识库 {totalCount} 条
        </span>

        {/* Git sync status — click to sync if unsynced */}
        <button
          className={`status-bar-git ${gitStatus.status === 'no-remote' ? 'no-remote' : gitStatus.status}`}
          onClick={async () => {
            if (gitStatus.status === 'unsynced') {
              setTaskText('同步中…');
              try {
                await invoke('git_sync');
                setTaskText('');
                fetchGitStatus();
              } catch {
                setTaskText('');
              }
            }
          }}
          title={gitStatus.status === 'unsynced' ? '点击同步到 GitHub' : ''}
          style={{ cursor: gitStatus.status === 'unsynced' ? 'pointer' : 'default' }}
        >
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="9" cy="4" r="2" />
            <circle cx="9" cy="14" r="2" />
            <path d="M9 6v6" />
          </svg>
          {gitLabel}
        </button>

        {/* Settings button */}
        <button
          className="status-bar-settings"
          onClick={onSettingsClick}
          aria-label="设置"
          title="设置"
        >
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.5">
            <circle cx="9" cy="9" r="2.5" />
            <path d="M9 1.5v2M9 14.5v2M1.5 9h2M14.5 9h2M3.4 3.4l1.4 1.4M13.2 13.2l1.4 1.4M3.4 14.6l1.4-1.4M13.2 4.8l1.4-1.4" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </footer>
  );
}
