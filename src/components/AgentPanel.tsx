import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import Ansi from 'ansi-to-react';
import './AgentPanel.css';

// ─── Types ──────────────────────────────────────────────────────────────────

interface CliAgentInfo {
  name: string;
  command: string;
  path: string;
  version: string;
  available: boolean;
}

interface AgentOutputEvent {
  type: 'Line' | 'Exit' | 'Error';
  content?: string;
  stream?: string;
  code?: number;
  duration_secs?: number;
  reason?: string;
}

type ProcessState = 'idle' | 'running' | 'finished';

// ─── Constants ──────────────────────────────────────────────────────────────

const MAX_PROMPT_LENGTH = 10_000;
const MAX_OUTPUT_LINES = 5000;

const INSTALL_LINKS: Record<string, string> = {
  claude: 'https://docs.anthropic.com/en/docs/claude-code',
  opencode: 'https://github.com/opencode-ai/opencode',
  kiro: 'https://kiro.dev',
};

// ─── Component ──────────────────────────────────────────────────────────────

export function AgentPanel() {
  const [agents, setAgents] = useState<CliAgentInfo[]>([]);
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const [prompt, setPrompt] = useState('');
  const [outputLines, setOutputLines] = useState<string[]>([]);
  const [processState, setProcessState] = useState<ProcessState>('idle');
  const [exitCode, setExitCode] = useState<number | null>(null);
  const [duration, setDuration] = useState<number | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const outputRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  // ─── Detect agents on mount ───────────────────────────────────────────────

  const detectAgents = useCallback(async () => {
    setDetecting(true);
    setError(null);
    try {
      const result = await invoke<CliAgentInfo[]>('detect_cli_agents');
      setAgents(result);
      // Auto-select first available agent if none selected
      if (!selectedAgent) {
        const firstAvailable = result.find((a) => a.available);
        if (firstAvailable) setSelectedAgent(firstAvailable.command);
      }
    } catch (e) {
      setError(`检测失败: ${e}`);
    } finally {
      setDetecting(false);
    }
  }, [selectedAgent]);

  useEffect(() => {
    detectAgents();
  }, []);

  // ─── Listen to agent output events ────────────────────────────────────────

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    listen<AgentOutputEvent>('agent_output', (event) => {
      const payload = event.payload;

      if (payload.type === 'Line') {
        setOutputLines((prev) => {
          const next = [...prev, payload.content ?? ''];
          // Trim to max buffer
          if (next.length > MAX_OUTPUT_LINES) {
            return next.slice(next.length - MAX_OUTPUT_LINES);
          }
          return next;
        });
      } else if (payload.type === 'Exit') {
        setProcessState('finished');
        setExitCode(payload.code ?? null);
        setDuration(payload.duration_secs ?? null);
      } else if (payload.type === 'Error') {
        setProcessState('finished');
        setError(payload.reason ?? '未知错误');
      }
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  // ─── Auto-scroll output ───────────────────────────────────────────────────

  useEffect(() => {
    if (autoScrollRef.current && outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [outputLines]);

  const handleOutputScroll = useCallback(() => {
    if (!outputRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = outputRef.current;
    // If user scrolled up more than 40px from bottom, disable auto-scroll
    autoScrollRef.current = scrollHeight - scrollTop - clientHeight < 40;
  }, []);

  // ─── Submit prompt ────────────────────────────────────────────────────────

  const handleSubmit = useCallback(async () => {
    if (!selectedAgent || !prompt.trim() || processState === 'running') return;

    setOutputLines([]);
    setExitCode(null);
    setDuration(null);
    setError(null);
    setProcessState('running');
    autoScrollRef.current = true;

    try {
      await invoke('spawn_cli_agent', {
        command: selectedAgent,
        prompt: prompt.trim(),
        articleContent: null,
      });
    } catch (e) {
      setProcessState('finished');
      setError(`启动失败: ${e}`);
    }
  }, [selectedAgent, prompt, processState]);

  // ─── Stop process ─────────────────────────────────────────────────────────

  const handleStop = useCallback(async () => {
    try {
      await invoke('kill_cli_agent');
    } catch (e) {
      setError(`终止失败: ${e}`);
    }
  }, []);

  // ─── Key handler for textarea ─────────────────────────────────────────────

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        handleSubmit();
      }
    },
    [handleSubmit],
  );

  // ─── Derived state ────────────────────────────────────────────────────────

  const isRunning = processState === 'running';
  const canSubmit = !!selectedAgent && !!prompt.trim() && !isRunning;

  // ─── Render ───────────────────────────────────────────────────────────────

  return (
    <div className="agent-panel">
      {/* Header */}
      <div className="agent-panel__header">
        <div className="agent-panel__header-left">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.3 24.3 0 014.5 0m0 0v5.714a2.25 2.25 0 00.659 1.591L19 14.5m-4.75-11.396c.251.023.501.05.75.082M5 14.5l-1.456 1.456a2.25 2.25 0 00-.659 1.591v.663c0 1.452 1.047 2.686 2.467 2.93a49 49 0 0013.296 0 2.745 2.745 0 002.467-2.93v-.663a2.25 2.25 0 00-.659-1.591L19 14.5" />
          </svg>
          <span>CLI Agent</span>
        </div>
        <button
          className="agent-panel__refresh-btn"
          onClick={detectAgents}
          disabled={detecting}
          title="刷新 Agent 列表"
        >
          <svg
            className={detecting ? 'spinning' : ''}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
          >
            <path d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182m0-4.991v4.99" />
          </svg>
        </button>
      </div>

      {/* Agent list */}
      <div className="agent-panel__agents">
        {agents.length === 0 && !detecting && (
          <div className="agent-panel__empty">
            未检测到 CLI Agent
          </div>
        )}
        {detecting && agents.length === 0 && (
          <div className="agent-panel__empty">检测中…</div>
        )}
        {agents.map((agent) => (
          <div
            key={agent.command}
            className={`agent-panel__agent-item${
              selectedAgent === agent.command ? ' selected' : ''
            }${!agent.available ? ' unavailable' : ''}`}
            onClick={() => agent.available && setSelectedAgent(agent.command)}
          >
            <div className="agent-panel__agent-info">
              <span className="agent-panel__agent-name">{agent.name}</span>
              {agent.available ? (
                <span className="agent-panel__agent-version">{agent.version}</span>
              ) : (
                <a
                  className="agent-panel__install-link"
                  href={INSTALL_LINKS[agent.command] ?? '#'}
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(e) => e.stopPropagation()}
                >
                  安装 →
                </a>
              )}
            </div>
            {agent.available && (
              <span className="agent-panel__agent-path">{agent.path}</span>
            )}
          </div>
        ))}
      </div>

      {/* Output area */}
      <div
        className="agent-panel__output"
        ref={outputRef}
        onScroll={handleOutputScroll}
      >
        {outputLines.length === 0 && processState === 'idle' && (
          <div className="agent-panel__output-placeholder">
            选择 Agent 并输入 prompt 开始
          </div>
        )}
        {outputLines.map((line, i) => (
          <div key={i} className="agent-panel__output-line">
            <Ansi>{line}</Ansi>
          </div>
        ))}
      </div>

      {/* Process result */}
      {processState === 'finished' && (
        <div className="agent-panel__result">
          {exitCode !== null && (
            <span className={`agent-panel__exit-code${exitCode === 0 ? ' success' : ' error'}`}>
              退出码: {exitCode}
            </span>
          )}
          {duration !== null && (
            <span className="agent-panel__duration">
              耗时: {duration}s
            </span>
          )}
        </div>
      )}

      {/* Error display */}
      {error && (
        <div className="agent-panel__error">
          {error}
        </div>
      )}

      {/* Input area */}
      <div className="agent-panel__input-area">
        <div className="agent-panel__input-wrapper">
          <textarea
            className="agent-panel__input"
            placeholder="输入 prompt（⌘+Enter 发送）…"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value.slice(0, MAX_PROMPT_LENGTH))}
            onKeyDown={handleKeyDown}
            disabled={isRunning}
            rows={3}
          />
          <span className="agent-panel__char-count">
            {prompt.length}/{MAX_PROMPT_LENGTH}
          </span>
        </div>
        <div className="agent-panel__actions">
          {isRunning ? (
            <button
              className="agent-panel__stop-btn"
              onClick={handleStop}
              title="停止进程"
            >
              <svg viewBox="0 0 24 24" fill="currentColor">
                <rect x="6" y="6" width="12" height="12" rx="2" />
              </svg>
              停止
            </button>
          ) : (
            <button
              className="agent-panel__submit-btn"
              onClick={handleSubmit}
              disabled={!canSubmit}
              title="运行 Agent"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M5 3l14 9-14 9V3z" />
              </svg>
              运行
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
