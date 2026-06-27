import './Discover.css';

interface DiscoverDetailProps {
  cardId: string;
  cardType: 'stats' | 'top-tags' | 'activity';
  cardTitle: string;
  cardData: Record<string, unknown>;
}

export default function DiscoverDetail({ cardType, cardTitle, cardData }: DiscoverDetailProps) {
  return (
    <div className="discover-detail">
      <h1>{cardTitle}</h1>
      
      {cardType === 'stats' && <StatsDetail data={cardData} />}
      {cardType === 'top-tags' && <TopTagsDetail data={cardData} />}
      {cardType === 'activity' && <ActivityDetail data={cardData} />}
    </div>
  );
}

function StatsDetail({ data }: { data: Record<string, unknown> }) {
  const fragmentCount = (data.fragmentCount as number) ?? 0;
  const tagCount = (data.tagCount as number) ?? 0;
  
  return (
    <div className="detail-section">
      <div className="detail-stats-grid">
        <div className="detail-stat-card">
          <div className="detail-stat-number">{fragmentCount}</div>
          <div className="detail-stat-label">最近 7 天碎片数</div>
          <div className="detail-stat-desc">平均每天 {Math.round(fragmentCount / 7 * 10) / 10} 条</div>
        </div>
        <div className="detail-stat-card">
          <div className="detail-stat-number">{tagCount}</div>
          <div className="detail-stat-label">涉及标签</div>
          <div className="detail-stat-desc">知识覆盖面</div>
        </div>
      </div>
      <div className="detail-insight">
        <h3>💡 洞察</h3>
        <p>你在过去一周保持了稳定的记录频率。{fragmentCount > 10 ? '产出相当高效！' : '可以尝试更频繁地记录灵感。'}</p>
      </div>
    </div>
  );
}

function TopTagsDetail({ data }: { data: Record<string, unknown> }) {
  const tags = (data.tags as { tag: string; count: number }[]) ?? [];
  const maxCount = tags.length > 0 ? Math.max(...tags.map(t => t.count)) : 1;
  
  return (
    <div className="detail-section">
      <div className="detail-tag-bars">
        {tags.map(t => (
          <div key={t.tag} className="detail-tag-bar-item">
            <span className="detail-tag-name">{t.tag}</span>
            <div className="detail-tag-bar-track">
              <div 
                className="detail-tag-bar-fill" 
                style={{ width: `${(t.count / maxCount) * 100}%` }} 
              />
            </div>
            <span className="detail-tag-count">{t.count}</span>
          </div>
        ))}
      </div>
      {tags.length === 0 && (
        <p className="detail-empty">暂无标签数据——为碎片添加标签后这里会自动更新</p>
      )}
      <div className="detail-insight">
        <h3>💡 洞察</h3>
        <p>
          {tags.length > 0 
            ? `「${tags[0]?.tag}」是你最活跃的方向，已累计 ${tags[0]?.count} 条相关碎片。`
            : '开始为碎片添加标签，AI 将帮你发现知识模式。'}
        </p>
      </div>
    </div>
  );
}

function ActivityDetail({ data }: { data: Record<string, unknown> }) {
  const fragmentCount = (data.fragmentCount as number) ?? 0;
  
  return (
    <div className="detail-section">
      <div className="detail-stats-grid">
        <div className="detail-stat-card accent">
          <div className="detail-stat-number">{fragmentCount}</div>
          <div className="detail-stat-label">7 天碎片总数</div>
          <div className="detail-stat-desc">你非常活跃！</div>
        </div>
      </div>
      <div className="detail-insight">
        <h3>🔥 活跃度分析</h3>
        <p>过去 7 天你保持了很高的记录频率。持续的碎片输入是知识积累的基础——AI 正在后台帮你整理这些内容。</p>
      </div>
    </div>
  );
}
