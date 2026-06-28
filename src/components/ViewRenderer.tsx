// Requirements: 7.1, 7.3, 7.5, 7.6
// ViewRenderer dispatches to sub-components by ViewSpec.type:
// GraphView (@xyflow/react), ChartView (recharts), SummaryView (react-markdown),
// TimelineView, ListView
// Data truncation: nodes > 200 → truncate + "显示 200/N 个节点" indicator
// Schema validation failure → error state + "重新生成" button

import { useCallback, useMemo } from 'react';
import {
  ReactFlow,
  type Node,
  type Edge,
  Background,
  Controls,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {
  ResponsiveContainer,
  BarChart,
  Bar,
  LineChart,
  Line,
  PieChart,
  Pie,
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  Cell,
  Legend,
} from 'recharts';
import Markdown from 'react-markdown';

import type {
  ViewSpec,
  GraphData,
  TimelineData,
  ListData,
  ChartData,
  SummaryData,
} from '../stores/viewStore';
import { useViewStore } from '../stores/viewStore';
import './ViewRenderer.css';

// ─── Constants ────────────────────────────────────────────────────────────────

const MAX_GRAPH_NODES = 200;
const DEFAULT_COLORS = [
  '#0071e3', '#34c759', '#ff9500', '#af52de',
  '#ff3b30', '#5ac8fa', '#ffcc00', '#007aff',
];

// ─── Schema Validation ────────────────────────────────────────────────────────

function validateGraphData(data: unknown): data is GraphData {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  // Accept if nodes array exists (edges optional — some graphs may have none)
  return Array.isArray(d.nodes);
}

function validateTimelineData(data: unknown): data is TimelineData {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return Array.isArray(d.items);
}

function validateListData(data: unknown): data is ListData {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return Array.isArray(d.items);
}

function validateChartData(data: unknown): data is ChartData {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return (
    typeof d.chartType === 'string' &&
    ['bar', 'line', 'pie', 'area'].includes(d.chartType as string) &&
    Array.isArray(d.series)
  );
}

function validateSummaryData(data: unknown): data is SummaryData {
  if (!data || typeof data !== 'object') return false;
  const d = data as Record<string, unknown>;
  return typeof d.markdown === 'string';
}

/**
 * Try to coerce AI-generated view data into the expected format.
 * AI often returns data in slightly different shapes — this normalizer
 * handles common variations.
 */
function normalizeViewData(spec: ViewSpec): ViewSpec {
  const data = spec.data as Record<string, unknown>;
  if (!data) return spec;

  switch (spec.type) {
    case 'graph': {
      // If nodes is missing but there's a top-level array, try to use it
      if (!Array.isArray(data.nodes) && !Array.isArray(data.edges)) {
        // Fallback: if data has markdown, treat as summary
        if (typeof data.markdown === 'string') {
          return { ...spec, type: 'summary', data: data as any };
        }
      }
      // Ensure edges array exists
      if (!Array.isArray(data.edges)) {
        return { ...spec, data: { ...data, edges: [] } as any };
      }
      return spec;
    }
    case 'timeline':
    case 'list': {
      // If items is missing but data has markdown, downgrade to summary
      if (!Array.isArray(data.items)) {
        if (typeof data.markdown === 'string') {
          return { ...spec, type: 'summary', data: data as any };
        }
        // Try to extract items from nested structure
        if (Array.isArray(data.list)) {
          return { ...spec, data: { items: data.list } as any };
        }
        // Last resort: show as summary with a message
        return { ...spec, type: 'summary', data: { markdown: JSON.stringify(data, null, 2) } as any };
      }
      return spec;
    }
    case 'chart': {
      if (!Array.isArray(data.series) && typeof data.markdown === 'string') {
        return { ...spec, type: 'summary', data: data as any };
      }
      return spec;
    }
    case 'summary': {
      // If markdown is missing, try to construct from available data
      if (typeof data.markdown !== 'string') {
        const content = data.content ?? data.text ?? JSON.stringify(data, null, 2);
        return { ...spec, data: { markdown: String(content), stats: data.stats } as any };
      }
      return spec;
    }
    default:
      return spec;
  }
}

// ─── Error State ──────────────────────────────────────────────────────────────

interface ErrorStateProps {
  message: string;
  query?: string;
  onRegenerate?: () => void;
}

export function ErrorState({ message, query, onRegenerate }: ErrorStateProps) {
  return (
    <div className="view-error-state">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="12" cy="12" r="10" />
        <path d="M12 8v4M12 16h.01" />
      </svg>
      <p className="error-message">{message}</p>
      {query && <p className="error-query">原始查询：{query}</p>}
      {onRegenerate && (
        <button className="btn-regenerate" onClick={onRegenerate}>
          重新生成
        </button>
      )}
    </div>
  );
}

// ─── Truncation Indicator ─────────────────────────────────────────────────────

function TruncationIndicator({ shown, total }: { shown: number; total: number }) {
  return (
    <div className="truncation-indicator">
      显示 {shown}/{total} 个节点
    </div>
  );
}

// ─── GraphView ────────────────────────────────────────────────────────────────

interface GraphViewProps {
  data: GraphData;
  config: Record<string, unknown>;
}

export function GraphView({ data, config }: GraphViewProps) {
  const totalNodes = data.nodes.length;
  const truncated = totalNodes > MAX_GRAPH_NODES;
  const visibleNodeIds = useMemo(() => {
    if (!truncated) return new Set(data.nodes.map((n) => n.id));
    // Keep top 200 nodes by connectivity (most edges)
    const edgeCount = new Map<string, number>();
    for (const edge of data.edges) {
      edgeCount.set(edge.source, (edgeCount.get(edge.source) ?? 0) + 1);
      edgeCount.set(edge.target, (edgeCount.get(edge.target) ?? 0) + 1);
    }
    const sorted = [...data.nodes].sort(
      (a, b) => (edgeCount.get(b.id) ?? 0) - (edgeCount.get(a.id) ?? 0),
    );
    return new Set(sorted.slice(0, MAX_GRAPH_NODES).map((n) => n.id));
  }, [data, truncated]);

  const nodes: Node[] = useMemo(() => {
    const filtered = data.nodes.filter((n) => visibleNodeIds.has(n.id));
    // Simple grid layout as auto-layout baseline
    const cols = Math.ceil(Math.sqrt(filtered.length));
    return filtered.map((n, i) => ({
      id: n.id,
      data: { label: n.label },
      position: {
        x: (i % cols) * 180,
        y: Math.floor(i / cols) * 100,
      },
      type: 'default',
      className: `graph-node-${n.type}`,
      style: {
        width: n.size ? Math.max(80, n.size * 2) : undefined,
      },
    }));
  }, [data.nodes, visibleNodeIds]);

  const edges: Edge[] = useMemo(() => {
    return data.edges
      .filter((e) => visibleNodeIds.has(e.source) && visibleNodeIds.has(e.target))
      .map((e, i) => ({
        id: `e-${e.source}-${e.target}-${i}`,
        source: e.source,
        target: e.target,
        label: e.label,
        animated: (e.weight ?? 0) > 0.8,
      }));
  }, [data.edges, visibleNodeIds]);

  const fitView = config?.fitView !== false;

  return (
    <div className="view-graph">
      {truncated && <TruncationIndicator shown={MAX_GRAPH_NODES} total={totalNodes} />}
      <div className="graph-container">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          fitView={fitView}
          proOptions={{ hideAttribution: true }}
        >
          <Background />
          <Controls />
        </ReactFlow>
      </div>
    </div>
  );
}

// ─── ChartView ────────────────────────────────────────────────────────────────

interface ChartViewProps {
  data: ChartData;
}

export function ChartView({ data }: ChartViewProps) {
  const { chartType, series, xAxis } = data;

  // Transform series data for recharts
  const chartData = useMemo(() => {
    if (!series.length) return [];
    const length = series[0].data.length;
    return Array.from({ length }, (_, i) => {
      const point: Record<string, unknown> = {
        name: xAxis?.data?.[i] ?? `${i + 1}`,
      };
      for (const s of series) {
        point[s.name] = s.data[i] ?? 0;
      }
      return point;
    });
  }, [series, xAxis]);

  // Pie data needs a different shape
  const pieData = useMemo(() => {
    if (chartType !== 'pie') return [];
    return series.map((s, i) => ({
      name: s.name,
      value: s.data.reduce((sum, v) => sum + v, 0),
      color: s.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length],
    }));
  }, [series, chartType]);

  return (
    <div className="view-chart">
      <ResponsiveContainer width="100%" height={320}>
        {chartType === 'bar' ? (
          <BarChart data={chartData}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey="name" />
            <YAxis />
            <Tooltip />
            <Legend />
            {series.map((s, i) => (
              <Bar
                key={s.name}
                dataKey={s.name}
                fill={s.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length]}
              />
            ))}
          </BarChart>
        ) : chartType === 'line' ? (
          <LineChart data={chartData}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey="name" />
            <YAxis />
            <Tooltip />
            <Legend />
            {series.map((s, i) => (
              <Line
                key={s.name}
                type="monotone"
                dataKey={s.name}
                stroke={s.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length]}
                strokeWidth={2}
              />
            ))}
          </LineChart>
        ) : chartType === 'area' ? (
          <AreaChart data={chartData}>
            <CartesianGrid strokeDasharray="3 3" />
            <XAxis dataKey="name" />
            <YAxis />
            <Tooltip />
            <Legend />
            {series.map((s, i) => (
              <Area
                key={s.name}
                type="monotone"
                dataKey={s.name}
                stroke={s.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length]}
                fill={s.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length]}
                fillOpacity={0.3}
              />
            ))}
          </AreaChart>
        ) : (
          <PieChart>
            <Tooltip />
            <Legend />
            <Pie
              data={pieData}
              dataKey="value"
              nameKey="name"
              cx="50%"
              cy="50%"
              outerRadius={120}
              label
            >
              {pieData.map((entry, i) => (
                <Cell key={entry.name} fill={entry.color ?? DEFAULT_COLORS[i % DEFAULT_COLORS.length]} />
              ))}
            </Pie>
          </PieChart>
        )}
      </ResponsiveContainer>
    </div>
  );
}

// ─── SummaryView ──────────────────────────────────────────────────────────────

interface SummaryViewProps {
  data: SummaryData;
}

export function SummaryView({ data }: SummaryViewProps) {
  return (
    <div className="view-summary">
      {data.stats && Object.keys(data.stats).length > 0 && (
        <div className="summary-stats">
          {Object.entries(data.stats).map(([key, value]) => (
            <span key={key} className="stat-badge">
              <span className="stat-label">{key}</span>
              <span className="stat-value">{value}</span>
            </span>
          ))}
        </div>
      )}
      <div className="summary-markdown">
        <Markdown>{data.markdown}</Markdown>
      </div>
    </div>
  );
}

// ─── TimelineView ─────────────────────────────────────────────────────────────

interface TimelineViewProps {
  data: TimelineData;
}

export function TimelineView({ data }: TimelineViewProps) {
  const sortedItems = useMemo(() => {
    return [...data.items].sort(
      (a, b) => new Date(b.date).getTime() - new Date(a.date).getTime(),
    );
  }, [data.items]);

  return (
    <div className="view-timeline">
      <div className="timeline-track">
        {sortedItems.map((item) => (
          <div key={item.id} className={`timeline-item timeline-type-${item.type}`}>
            <div className="timeline-dot" />
            <div className="timeline-content">
              <time className="timeline-date">{item.date}</time>
              <h4 className="timeline-title">{item.title}</h4>
              <p className="timeline-body">{item.content}</p>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── ListView ─────────────────────────────────────────────────────────────────

interface ListViewProps {
  data: ListData;
}

export function ListView({ data }: ListViewProps) {
  const grouped = useMemo(() => {
    if (!data.groupBy) return { '': data.items };
    const groups: Record<string, typeof data.items> = {};
    for (const item of data.items) {
      // Group by first matching tag or subtitle if groupBy matches
      const key =
        item.tags?.find((t) => t.toLowerCase().includes(data.groupBy!.toLowerCase())) ??
        item.subtitle ??
        '其他';
      if (!groups[key]) groups[key] = [];
      groups[key].push(item);
    }
    return groups;
  }, [data]);

  return (
    <div className="view-list">
      {Object.entries(grouped).map(([group, items]) => (
        <div key={group} className="list-group">
          {group && <h4 className="list-group-title">{group}</h4>}
          <ul className="list-items">
            {items.map((item) => (
              <li key={item.id} className="list-item">
                <div className="list-item-main">
                  <span className="list-item-title">{item.title}</span>
                  {item.subtitle && (
                    <span className="list-item-subtitle">{item.subtitle}</span>
                  )}
                </div>
                {item.tags && item.tags.length > 0 && (
                  <div className="list-item-tags">
                    {item.tags.map((tag) => (
                      <span key={tag} className="list-tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

// ─── ViewRenderer (Main) ──────────────────────────────────────────────────────

interface ViewRendererProps {
  spec: ViewSpec;
}

export function ViewRenderer({ spec }: ViewRendererProps) {
  const generateView = useViewStore((s) => s.generateView);

  const handleRegenerate = useCallback(() => {
    generateView(spec.query);
  }, [generateView, spec.query]);

  // Normalize data before validation (handle AI format variations)
  const normalizedSpec = useMemo(() => normalizeViewData(spec), [spec]);

  // Schema validation per type
  switch (normalizedSpec.type) {
    case 'graph': {
      if (!validateGraphData(normalizedSpec.data)) {
        return (
          <ErrorState
            message="图谱数据格式无效"
            query={normalizedSpec.query}
            onRegenerate={handleRegenerate}
          />
        );
      }
      return <GraphView data={normalizedSpec.data} config={normalizedSpec.config} />;
    }
    case 'timeline': {
      if (!validateTimelineData(normalizedSpec.data)) {
        return (
          <ErrorState
            message="时间线数据格式无效"
            query={normalizedSpec.query}
            onRegenerate={handleRegenerate}
          />
        );
      }
      return <TimelineView data={normalizedSpec.data} />;
    }
    case 'list': {
      if (!validateListData(normalizedSpec.data)) {
        return (
          <ErrorState
            message="列表数据格式无效"
            query={normalizedSpec.query}
            onRegenerate={handleRegenerate}
          />
        );
      }
      return <ListView data={normalizedSpec.data} />;
    }
    case 'chart': {
      if (!validateChartData(normalizedSpec.data)) {
        return (
          <ErrorState
            message="图表数据格式无效"
            query={normalizedSpec.query}
            onRegenerate={handleRegenerate}
          />
        );
      }
      return <ChartView data={normalizedSpec.data} />;
    }
    case 'summary': {
      if (!validateSummaryData(normalizedSpec.data)) {
        return (
          <ErrorState
            message="摘要数据格式无效"
            query={normalizedSpec.query}
            onRegenerate={handleRegenerate}
          />
        );
      }
      return <SummaryView data={normalizedSpec.data} />;
    }
    default:
      return <ErrorState message="不支持的视图类型" query={normalizedSpec.query} onRegenerate={handleRegenerate} />;
  }
}
