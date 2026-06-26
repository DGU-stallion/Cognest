// Cognest Core — SQLite Index with FTS5 full-text search
//
// Provides a derived index over the file-system vault.
// The index is disposable and can be rebuilt from vault files at any time.

use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::frontmatter;
use super::repo::FileRepo;

// ─── Data Structures ────────────────────────────────────────────────────────

/// Fragment record stored in the SQLite index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentRecord {
    pub id: String,
    pub content: String,
    pub created_at: String,
    pub source: String,
    pub tags: Vec<String>,
    pub topics: Vec<String>,
    pub content_hash: String,
}

/// Article record stored in the SQLite index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleRecord {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
    pub content_hash: String,
}

/// Search result with snippet and match positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub snippet: String,
    pub match_start: usize,
    pub match_end: usize,
    pub rank: f64,
}

/// Statistics for the Discover page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub fragment_count: u64,
    pub article_count: u64,
    pub days: Vec<DayStat>,
}

/// Per-day activity count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayStat {
    pub date: String,
    pub count: u64,
}

/// Report returned after a full index rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildReport {
    pub fragments_indexed: u64,
    pub articles_indexed: u64,
    pub skipped: Vec<String>,
}

/// Filter for listing fragments.
#[derive(Debug, Clone)]
pub enum FragmentFilter {
    All,
    Uncategorized, // topics is empty
    Categorized,   // topics is non-empty
}

/// Filter for listing articles.
#[derive(Debug, Clone)]
pub enum ArticleFilter {
    All,
    ByStatus(String),
    ByTags(Vec<String>),
}

/// Errors from index operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("Frontmatter 错误: {0}")]
    Frontmatter(#[from] frontmatter::FrontmatterError),

    #[error("索引损坏")]
    Corrupted,
}

// ─── IndexDb Implementation ─────────────────────────────────────────────────

/// SQLite-backed index providing full-text search over vault content.
pub struct IndexDb {
    conn: Connection,
}

impl IndexDb {
    /// Open or create the index database at the given path.
    pub fn open(db_path: &Path) -> Result<Self, IndexError> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        Ok(Self { conn })
    }

    /// Create an in-memory IndexDb (useful for testing).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, IndexError> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    /// Initialize the database schema (tables + FTS virtual tables).
    pub fn init_schema(&self) -> Result<(), IndexError> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS fragments (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                source TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                topics TEXT NOT NULL DEFAULT '[]',
                content_hash TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS fragments_fts USING fts5(
                content, tags,
                content='fragments',
                content_rowid='rowid',
                tokenize='trigram'
            );

            CREATE TABLE IF NOT EXISTS articles (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                content_hash TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(
                title, tags,
                content='articles',
                content_rowid='rowid',
                tokenize='trigram'
            );

            -- Triggers to keep FTS in sync with content tables
            CREATE TRIGGER IF NOT EXISTS fragments_ai AFTER INSERT ON fragments BEGIN
                INSERT INTO fragments_fts(rowid, content, tags)
                VALUES (new.rowid, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS fragments_ad AFTER DELETE ON fragments BEGIN
                INSERT INTO fragments_fts(fragments_fts, rowid, content, tags)
                VALUES ('delete', old.rowid, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS fragments_au AFTER UPDATE ON fragments BEGIN
                INSERT INTO fragments_fts(fragments_fts, rowid, content, tags)
                VALUES ('delete', old.rowid, old.content, old.tags);
                INSERT INTO fragments_fts(rowid, content, tags)
                VALUES (new.rowid, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_ai AFTER INSERT ON articles BEGIN
                INSERT INTO articles_fts(rowid, title, tags)
                VALUES (new.rowid, new.title, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_ad AFTER DELETE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, tags)
                VALUES ('delete', old.rowid, old.title, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_au AFTER UPDATE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, tags)
                VALUES ('delete', old.rowid, old.title, old.tags);
                INSERT INTO articles_fts(rowid, title, tags)
                VALUES (new.rowid, new.title, new.tags);
            END;
            ",
        )?;
        Ok(())
    }

    /// Check database integrity. Returns true if the database is healthy.
    pub fn check_integrity(&self) -> Result<bool, IndexError> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
        Ok(result == "ok")
    }

    // ─── Fragment CRUD ───────────────────────────────────────────────────────

    /// Insert a fragment record into the index.
    pub fn insert_fragment(&self, record: &FragmentRecord) -> Result<(), IndexError> {
        let tags_json = serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string());
        let topics_json =
            serde_json::to_string(&record.topics).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "INSERT OR REPLACE INTO fragments (id, content, created_at, source, tags, topics, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.content,
                record.created_at,
                record.source,
                tags_json,
                topics_json,
                record.content_hash,
            ],
        )?;
        Ok(())
    }

    /// Update an existing fragment record (only if content_hash differs).
    pub fn update_fragment(&self, record: &FragmentRecord) -> Result<(), IndexError> {
        let tags_json = serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string());
        let topics_json =
            serde_json::to_string(&record.topics).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "UPDATE fragments SET content = ?1, created_at = ?2, source = ?3,
             tags = ?4, topics = ?5, content_hash = ?6 WHERE id = ?7",
            params![
                record.content,
                record.created_at,
                record.source,
                tags_json,
                topics_json,
                record.content_hash,
                record.id,
            ],
        )?;
        Ok(())
    }

    /// Delete a fragment by ID.
    pub fn delete_fragment(&self, id: &str) -> Result<(), IndexError> {
        self.conn
            .execute("DELETE FROM fragments WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ─── Article CRUD ────────────────────────────────────────────────────────

    /// Insert an article record into the index.
    pub fn insert_article(&self, record: &ArticleRecord) -> Result<(), IndexError> {
        let tags_json = serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "INSERT OR REPLACE INTO articles (id, title, status, created_at, updated_at, tags, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.title,
                record.status,
                record.created_at,
                record.updated_at,
                tags_json,
                record.content_hash,
            ],
        )?;
        Ok(())
    }

    /// Update an existing article record.
    pub fn update_article(&self, record: &ArticleRecord) -> Result<(), IndexError> {
        let tags_json = serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "UPDATE articles SET title = ?1, status = ?2, created_at = ?3,
             updated_at = ?4, tags = ?5, content_hash = ?6 WHERE id = ?7",
            params![
                record.title,
                record.status,
                record.created_at,
                record.updated_at,
                tags_json,
                record.content_hash,
                record.id,
            ],
        )?;
        Ok(())
    }

    /// Delete an article by ID.
    pub fn delete_article(&self, id: &str) -> Result<(), IndexError> {
        self.conn
            .execute("DELETE FROM articles WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ─── Search ──────────────────────────────────────────────────────────────

    /// Full-text search fragments using FTS5 trigram matching.
    /// Returns at most `limit` results (capped at 50), ordered by relevance.
    pub fn search_fragments(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, IndexError> {
        let effective_limit = limit.min(50);
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        // Escape the query for FTS5 — wrap in double quotes for literal matching
        let escaped_query = format!("\"{}\"", query.replace('"', "\"\""));

        let mut stmt = self.conn.prepare(
            "SELECT f.id, snippet(fragments_fts, 0, '', '', '...', 50) as snip,
                    rank
             FROM fragments_fts
             JOIN fragments f ON f.rowid = fragments_fts.rowid
             WHERE fragments_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![escaped_query, effective_limit as i64], |row| {
                let id: String = row.get(0)?;
                let snippet: String = row.get(1)?;
                let rank: f64 = row.get(2)?;
                Ok(SearchResult {
                    id,
                    snippet: truncate_snippet(&snippet, 150),
                    match_start: 0,
                    match_end: query.len().min(150),
                    rank,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Full-text search articles using FTS5 trigram matching.
    /// Returns at most `limit` results (capped at 50), ordered by relevance.
    pub fn search_articles(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, IndexError> {
        let effective_limit = limit.min(50);
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        let escaped_query = format!("\"{}\"", query.replace('"', "\"\""));

        let mut stmt = self.conn.prepare(
            "SELECT a.id, snippet(articles_fts, 0, '', '', '...', 50) as snip,
                    rank
             FROM articles_fts
             JOIN articles a ON a.rowid = articles_fts.rowid
             WHERE articles_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![escaped_query, effective_limit as i64], |row| {
                let id: String = row.get(0)?;
                let snippet: String = row.get(1)?;
                let rank: f64 = row.get(2)?;
                Ok(SearchResult {
                    id,
                    snippet: truncate_snippet(&snippet, 150),
                    match_start: 0,
                    match_end: query.len().min(150),
                    rank,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    // ─── Counts & Statistics ─────────────────────────────────────────────────

    /// Get the total number of indexed fragments.
    pub fn fragment_count(&self) -> Result<u64, IndexError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM fragments", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Get the total number of indexed articles.
    pub fn article_count(&self) -> Result<u64, IndexError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM articles", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Get activity statistics for the past N days.
    /// Returns per-day fragment creation counts.
    pub fn stats_last_days(&self, days: u32) -> Result<StatsResult, IndexError> {
        let frag_count = self.fragment_count()?;
        let art_count = self.article_count()?;

        let mut stmt = self.conn.prepare(
            "SELECT date(created_at) as day, COUNT(*) as cnt
             FROM fragments
             WHERE created_at >= date('now', ?1)
             GROUP BY day
             ORDER BY day DESC",
        )?;

        let offset = format!("-{} days", days);
        let day_stats = stmt
            .query_map(params![offset], |row| {
                let date: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok(DayStat {
                    date,
                    count: count as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(StatsResult {
            fragment_count: frag_count,
            article_count: art_count,
            days: day_stats,
        })
    }

    /// Get the top N tags by frequency within the past `days` days.
    pub fn top_tags(&self, days: u32, limit: usize) -> Result<Vec<(String, u32)>, IndexError> {
        // We store tags as JSON arrays. We need to parse them and count.
        let offset = format!("-{} days", days);
        let mut stmt = self.conn.prepare(
            "SELECT tags FROM fragments WHERE created_at >= date('now', ?1)",
        )?;

        let mut tag_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        let rows = stmt.query_map(params![offset], |row| {
            let tags_str: String = row.get(0)?;
            Ok(tags_str)
        })?;

        for row in rows {
            let tags_str = row?;
            if let Ok(tags) = serde_json::from_str::<Vec<String>>(&tags_str) {
                for tag in tags {
                    if !tag.is_empty() {
                        *tag_counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
        }

        // Sort by count descending and take top N
        let mut sorted: Vec<(String, u32)> = tag_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(limit);

        Ok(sorted)
    }

    // ─── List / Filter ───────────────────────────────────────────────────────

    /// List fragments with filtering and pagination.
    pub fn list_fragments(
        &self,
        filter: FragmentFilter,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<FragmentRecord>, IndexError> {
        let (sql, filter_params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match filter {
            FragmentFilter::All => (
                "SELECT id, content, created_at, source, tags, topics, content_hash
                 FROM fragments ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                vec![
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
            FragmentFilter::Uncategorized => (
                "SELECT id, content, created_at, source, tags, topics, content_hash
                 FROM fragments WHERE topics = '[]' OR topics = ''
                 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                vec![
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
            FragmentFilter::Categorized => (
                "SELECT id, content, created_at, source, tags, topics, content_hash
                 FROM fragments WHERE topics != '[]' AND topics != ''
                 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                vec![
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
        };

        let mut stmt = self.conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            filter_params.iter().map(|p| p.as_ref()).collect();

        let records = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(FragmentRecord {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    created_at: row.get(2)?,
                    source: row.get(3)?,
                    tags: parse_json_array(&row.get::<_, String>(4)?),
                    topics: parse_json_array(&row.get::<_, String>(5)?),
                    content_hash: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    /// List articles with optional filtering by status or tags.
    pub fn list_articles(&self, filter: ArticleFilter) -> Result<Vec<ArticleRecord>, IndexError> {
        match filter {
            ArticleFilter::All => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, title, status, created_at, updated_at, tags, content_hash
                     FROM articles ORDER BY updated_at DESC",
                )?;
                let records = stmt
                    .query_map([], |row| {
                        Ok(ArticleRecord {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            status: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                            tags: parse_json_array(&row.get::<_, String>(5)?),
                            content_hash: row.get(6)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(records)
            }
            ArticleFilter::ByStatus(ref status) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, title, status, created_at, updated_at, tags, content_hash
                     FROM articles WHERE status = ?1 ORDER BY updated_at DESC",
                )?;
                let records = stmt
                    .query_map(params![status], |row| {
                        Ok(ArticleRecord {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            status: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                            tags: parse_json_array(&row.get::<_, String>(5)?),
                            content_hash: row.get(6)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(records)
            }
            ArticleFilter::ByTags(ref tags) => {
                // Intersection semantics: articles must contain ALL specified tags
                let mut stmt = self.conn.prepare(
                    "SELECT id, title, status, created_at, updated_at, tags, content_hash
                     FROM articles ORDER BY updated_at DESC",
                )?;
                let all: Vec<ArticleRecord> = stmt
                    .query_map([], |row| {
                        Ok(ArticleRecord {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            status: row.get(2)?,
                            created_at: row.get(3)?,
                            updated_at: row.get(4)?,
                            tags: parse_json_array(&row.get::<_, String>(5)?),
                            content_hash: row.get(6)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                let filtered = all
                    .into_iter()
                    .filter(|a| tags.iter().all(|t| a.tags.contains(t)))
                    .collect();
                Ok(filtered)
            }
        }
    }

    // ─── Rebuild ─────────────────────────────────────────────────────────────

    /// Rebuild the entire index from vault files.
    ///
    /// Clears all existing index data and re-indexes all valid .md files
    /// in capture/ (fragments) and articles/ directories.
    pub fn rebuild_from_vault(&self, repo: &FileRepo) -> Result<RebuildReport, IndexError> {
        // Clear existing data
        self.conn.execute_batch(
            "DELETE FROM fragments;
             DELETE FROM articles;",
        )?;

        let mut report = RebuildReport {
            fragments_indexed: 0,
            articles_indexed: 0,
            skipped: vec![],
        };

        // Index fragments from capture/
        let fragment_paths = repo.list_fragment_paths().map_err(|e| {
            IndexError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        for path in &fragment_paths {
            match self.index_fragment_file(path) {
                Ok(()) => report.fragments_indexed += 1,
                Err(e) => {
                    let path_str = path.to_string_lossy().to_string();
                    log::warn!("跳过碎片文件 {}: {}", path_str, e);
                    report.skipped.push(path_str);
                }
            }
        }

        // Index articles from articles/
        let articles_dir = repo.vault_path().join("articles");
        if articles_dir.exists() {
            let mut article_paths = Vec::new();
            collect_md_files(&articles_dir, &mut article_paths)?;

            for path in &article_paths {
                match self.index_article_file(path) {
                    Ok(()) => report.articles_indexed += 1,
                    Err(e) => {
                        let path_str = path.to_string_lossy().to_string();
                        log::warn!("跳过文章文件 {}: {}", path_str, e);
                        report.skipped.push(path_str);
                    }
                }
            }
        }

        Ok(report)
    }

    /// Index a single fragment .md file.
    fn index_fragment_file(&self, path: &Path) -> Result<(), IndexError> {
        let content = std::fs::read_to_string(path)?;
        let parsed = frontmatter::parse::<super::repo::FragmentMeta>(&content)?;

        let file_bytes = content.as_bytes();
        let hash = compute_hash(file_bytes);

        let tags_json =
            serde_json::to_string(&parsed.meta.tags).unwrap_or_else(|_| "[]".to_string());
        let topics_json =
            serde_json::to_string(&parsed.meta.topics).unwrap_or_else(|_| "[]".to_string());

        let record = FragmentRecord {
            id: parsed.meta.id,
            content: parsed.body,
            created_at: parsed.meta.created.to_rfc3339(),
            source: parsed.meta.source,
            tags: parsed.meta.tags,
            topics: parsed.meta.topics,
            content_hash: hash,
        };

        // Store with JSON-encoded tags/topics
        self.conn.execute(
            "INSERT OR REPLACE INTO fragments (id, content, created_at, source, tags, topics, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.content,
                record.created_at,
                record.source,
                tags_json,
                topics_json,
                record.content_hash,
            ],
        )?;

        Ok(())
    }

    /// Index a single article .md file.
    fn index_article_file(&self, path: &Path) -> Result<(), IndexError> {
        let content = std::fs::read_to_string(path)?;
        let parsed = frontmatter::parse::<super::repo::ArticleMeta>(&content)?;

        let file_bytes = content.as_bytes();
        let hash = compute_hash(file_bytes);

        let tags_json =
            serde_json::to_string(&parsed.meta.tags).unwrap_or_else(|_| "[]".to_string());

        let status_str = match parsed.meta.status {
            super::repo::ArticleStatus::Draft => "draft",
            super::repo::ArticleStatus::Editing => "editing",
            super::repo::ArticleStatus::Completed => "completed",
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO articles (id, title, status, created_at, updated_at, tags, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                parsed.meta.id,
                parsed.meta.title,
                status_str,
                parsed.meta.created.to_rfc3339(),
                parsed.meta.updated.to_rfc3339(),
                tags_json,
                hash,
            ],
        )?;

        Ok(())
    }

    /// Get the stored content hash for a fragment, if it exists.
    pub fn get_fragment_hash(&self, id: &str) -> Result<Option<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM fragments WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Get the stored content hash for an article, if it exists.
    pub fn get_article_hash(&self, id: &str) -> Result<Option<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM articles WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

// ─── Helper Functions ────────────────────────────────────────────────────────

/// Truncate a snippet to the given max length, respecting char boundaries.
fn truncate_snippet(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Compute SHA-256 hash of bytes, returned as lowercase hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Parse a JSON array string into Vec<String>. Returns empty vec on failure.
fn parse_json_array(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Recursively collect all .md files under a directory.
fn collect_md_files(dir: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<(), IndexError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(&path, paths)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create an IndexDb with in-memory SQLite and initialized schema.
    fn setup_db() -> IndexDb {
        let db = IndexDb::open_in_memory().unwrap();
        db.init_schema().unwrap();
        db
    }

    fn sample_fragment(id: &str) -> FragmentRecord {
        FragmentRecord {
            id: id.to_string(),
            content: format!("Content for fragment {}", id),
            created_at: "2026-06-25T10:30:00+08:00".to_string(),
            source: "manual".to_string(),
            tags: vec!["rust".to_string(), "test".to_string()],
            topics: vec!["programming".to_string()],
            content_hash: "abc123".to_string(),
        }
    }

    fn sample_article(id: &str) -> ArticleRecord {
        ArticleRecord {
            id: id.to_string(),
            title: format!("Article {}", id),
            status: "draft".to_string(),
            created_at: "2026-06-25T14:00:00+08:00".to_string(),
            updated_at: "2026-06-25T16:00:00+08:00".to_string(),
            tags: vec!["writing".to_string()],
            content_hash: "def456".to_string(),
        }
    }

    #[test]
    fn test_init_schema_creates_tables() {
        let db = setup_db();
        // Should not error when querying the tables
        assert_eq!(db.fragment_count().unwrap(), 0);
        assert_eq!(db.article_count().unwrap(), 0);
    }

    #[test]
    fn test_check_integrity_healthy_db() {
        let db = setup_db();
        assert!(db.check_integrity().unwrap());
    }

    // --- Fragment CRUD tests ---

    #[test]
    fn test_insert_fragment() {
        let db = setup_db();
        let record = sample_fragment("aabb0011");
        db.insert_fragment(&record).unwrap();
        assert_eq!(db.fragment_count().unwrap(), 1);
    }

    #[test]
    fn test_update_fragment() {
        let db = setup_db();
        let mut record = sample_fragment("aabb0011");
        db.insert_fragment(&record).unwrap();

        record.content = "Updated content".to_string();
        record.content_hash = "newhash".to_string();
        db.update_fragment(&record).unwrap();

        let results = db
            .list_fragments(FragmentFilter::All, 0, 10)
            .unwrap();
        assert_eq!(results[0].content, "Updated content");
        assert_eq!(results[0].content_hash, "newhash");
    }

    #[test]
    fn test_delete_fragment() {
        let db = setup_db();
        db.insert_fragment(&sample_fragment("aabb0011")).unwrap();
        assert_eq!(db.fragment_count().unwrap(), 1);

        db.delete_fragment("aabb0011").unwrap();
        assert_eq!(db.fragment_count().unwrap(), 0);
    }

    // --- Article CRUD tests ---

    #[test]
    fn test_insert_article() {
        let db = setup_db();
        db.insert_article(&sample_article("xxxx0001")).unwrap();
        assert_eq!(db.article_count().unwrap(), 1);
    }

    #[test]
    fn test_update_article() {
        let db = setup_db();
        let mut record = sample_article("xxxx0001");
        db.insert_article(&record).unwrap();

        record.title = "Updated Title".to_string();
        record.status = "editing".to_string();
        db.update_article(&record).unwrap();

        let results = db.list_articles(ArticleFilter::All).unwrap();
        assert_eq!(results[0].title, "Updated Title");
        assert_eq!(results[0].status, "editing");
    }

    #[test]
    fn test_delete_article() {
        let db = setup_db();
        db.insert_article(&sample_article("xxxx0001")).unwrap();
        assert_eq!(db.article_count().unwrap(), 1);

        db.delete_article("xxxx0001").unwrap();
        assert_eq!(db.article_count().unwrap(), 0);
    }

    // --- Filter tests ---

    #[test]
    fn test_list_fragments_filter_all() {
        let db = setup_db();
        db.insert_fragment(&sample_fragment("f001")).unwrap();
        db.insert_fragment(&sample_fragment("f002")).unwrap();

        let results = db.list_fragments(FragmentFilter::All, 0, 100).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_fragments_filter_uncategorized() {
        let db = setup_db();
        // Categorized fragment (has topics)
        db.insert_fragment(&sample_fragment("f001")).unwrap();

        // Uncategorized fragment (empty topics)
        let mut uncategorized = sample_fragment("f002");
        uncategorized.topics = vec![];
        db.insert_fragment(&uncategorized).unwrap();

        let results = db
            .list_fragments(FragmentFilter::Uncategorized, 0, 100)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f002");
    }

    #[test]
    fn test_list_fragments_filter_categorized() {
        let db = setup_db();
        // Categorized fragment (has topics)
        db.insert_fragment(&sample_fragment("f001")).unwrap();

        // Uncategorized fragment
        let mut uncategorized = sample_fragment("f002");
        uncategorized.topics = vec![];
        db.insert_fragment(&uncategorized).unwrap();

        let results = db
            .list_fragments(FragmentFilter::Categorized, 0, 100)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f001");
    }

    #[test]
    fn test_list_articles_by_status() {
        let db = setup_db();
        db.insert_article(&sample_article("a001")).unwrap();

        let mut editing = sample_article("a002");
        editing.status = "editing".to_string();
        db.insert_article(&editing).unwrap();

        let drafts = db
            .list_articles(ArticleFilter::ByStatus("draft".to_string()))
            .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, "a001");
    }

    #[test]
    fn test_list_articles_by_tags_intersection() {
        let db = setup_db();
        let mut a1 = sample_article("a001");
        a1.tags = vec!["rust".to_string(), "web".to_string()];
        db.insert_article(&a1).unwrap();

        let mut a2 = sample_article("a002");
        a2.tags = vec!["rust".to_string()];
        db.insert_article(&a2).unwrap();

        // Filter by both "rust" and "web" — only a001 has both
        let results = db
            .list_articles(ArticleFilter::ByTags(vec![
                "rust".to_string(),
                "web".to_string(),
            ]))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a001");
    }

    // --- Search tests ---

    #[test]
    fn test_search_fragments_empty_query_returns_empty() {
        let db = setup_db();
        db.insert_fragment(&sample_fragment("f001")).unwrap();
        let results = db.search_fragments("", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_fragments_finds_match() {
        let db = setup_db();
        let mut frag = sample_fragment("f001");
        frag.content = "Rust programming language is great".to_string();
        db.insert_fragment(&frag).unwrap();

        let results = db.search_fragments("programming", 50).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "f001");
    }

    #[test]
    fn test_search_fragments_limit_respected() {
        let db = setup_db();
        // Insert many fragments with searchable content
        for i in 0..10 {
            let mut frag = sample_fragment(&format!("f{:03}", i));
            frag.content = format!("Common searchable text number {}", i);
            db.insert_fragment(&frag).unwrap();
        }

        let results = db.search_fragments("searchable", 3).unwrap();
        assert!(results.len() <= 3);
    }

    #[test]
    fn test_search_articles_finds_by_title() {
        let db = setup_db();
        let mut article = sample_article("a001");
        article.title = "Introduction to Rust Programming".to_string();
        db.insert_article(&article).unwrap();

        let results = db.search_articles("Rust", 50).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "a001");
    }

    #[test]
    fn test_search_max_50_results() {
        let db = setup_db();
        // Even if limit > 50, should cap at 50
        for i in 0..60 {
            let mut frag = sample_fragment(&format!("f{:03}", i));
            frag.content = format!("Identical content for search test {}", i);
            db.insert_fragment(&frag).unwrap();
        }

        let results = db.search_fragments("content", 100).unwrap();
        assert!(results.len() <= 50);
    }

    #[test]
    fn test_search_snippet_max_150_chars() {
        let db = setup_db();
        let mut frag = sample_fragment("f001");
        frag.content = "a".repeat(500);
        db.insert_fragment(&frag).unwrap();

        let results = db.search_fragments("aaa", 50).unwrap();
        if !results.is_empty() {
            assert!(results[0].snippet.len() <= 153); // 150 + "..."
        }
    }

    // --- Stats & Tags tests ---

    #[test]
    fn test_fragment_count() {
        let db = setup_db();
        assert_eq!(db.fragment_count().unwrap(), 0);
        db.insert_fragment(&sample_fragment("f001")).unwrap();
        db.insert_fragment(&sample_fragment("f002")).unwrap();
        assert_eq!(db.fragment_count().unwrap(), 2);
    }

    #[test]
    fn test_article_count() {
        let db = setup_db();
        assert_eq!(db.article_count().unwrap(), 0);
        db.insert_article(&sample_article("a001")).unwrap();
        assert_eq!(db.article_count().unwrap(), 1);
    }

    #[test]
    fn test_stats_last_days_returns_structure() {
        let db = setup_db();
        let stats = db.stats_last_days(7).unwrap();
        assert_eq!(stats.fragment_count, 0);
        assert_eq!(stats.article_count, 0);
        assert!(stats.days.is_empty());
    }

    #[test]
    fn test_top_tags_empty() {
        let db = setup_db();
        let tags = db.top_tags(7, 5).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn test_top_tags_counts_correctly() {
        let db = setup_db();
        // Insert fragments with various tags, using today's date
        let today = chrono::Utc::now().to_rfc3339();

        let mut f1 = sample_fragment("f001");
        f1.tags = vec!["rust".to_string(), "web".to_string()];
        f1.created_at = today.clone();
        db.insert_fragment(&f1).unwrap();

        let mut f2 = sample_fragment("f002");
        f2.tags = vec!["rust".to_string(), "database".to_string()];
        f2.created_at = today.clone();
        db.insert_fragment(&f2).unwrap();

        let mut f3 = sample_fragment("f003");
        f3.tags = vec!["rust".to_string()];
        f3.created_at = today;
        db.insert_fragment(&f3).unwrap();

        let tags = db.top_tags(7, 5).unwrap();
        // "rust" should be first with count 3
        assert!(!tags.is_empty());
        assert_eq!(tags[0].0, "rust");
        assert_eq!(tags[0].1, 3);
    }

    // --- Hash helper tests ---

    #[test]
    fn test_get_fragment_hash_not_found() {
        let db = setup_db();
        assert_eq!(db.get_fragment_hash("nonexist").unwrap(), None);
    }

    #[test]
    fn test_get_fragment_hash_exists() {
        let db = setup_db();
        let frag = sample_fragment("f001");
        db.insert_fragment(&frag).unwrap();
        assert_eq!(
            db.get_fragment_hash("f001").unwrap(),
            Some("abc123".to_string())
        );
    }

    // --- Rebuild test ---

    #[test]
    fn test_rebuild_from_vault_with_fragments() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let repo = FileRepo::new(tmp.path().to_path_buf());

        // Create a few fragments via the repo
        repo.create_fragment("First fragment content").unwrap();
        repo.create_fragment("Second fragment content").unwrap();

        // Create an IndexDb and rebuild
        let db = IndexDb::open_in_memory().unwrap();
        db.init_schema().unwrap();

        let report = db.rebuild_from_vault(&repo).unwrap();
        assert_eq!(report.fragments_indexed, 2);
        assert_eq!(report.skipped.len(), 0);
        assert_eq!(db.fragment_count().unwrap(), 2);
    }
}
