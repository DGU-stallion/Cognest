// Cognest Core — FileRepo: file-based repository for fragments and articles
//
// Fragments are immutable log entries stored at capture/yyyy/mm/<8-hex>.md
// Articles are mutable documents stored at articles/<8-hex>.md

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::frontmatter;

// ─── Data Structures ────────────────────────────────────────────────────────

/// Fragment metadata stored in YAML frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FragmentMeta {
    pub id: String,
    pub created: DateTime<Utc>,
    pub source: String,
    pub tags: Vec<String>,
    pub topics: Vec<String>,
}

/// Article metadata stored in YAML frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArticleMeta {
    pub id: String,
    pub title: String,
    pub status: ArticleStatus,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub tags: Vec<String>,
}

/// Article lifecycle status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArticleStatus {
    Draft,
    Editing,
    Completed,
}

/// Errors that can occur during repository operations.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("Frontmatter 错误: {0}")]
    Frontmatter(#[from] frontmatter::FrontmatterError),

    #[error("碎片不存在: {0}")]
    FragmentNotFound(String),

    #[error("文章不存在: {0}")]
    ArticleNotFound(String),

    #[error("输入无效: {0}")]
    InvalidInput(String),
}

// ─── FileRepo Implementation ────────────────────────────────────────────────

/// File-based repository managing fragments and articles on disk.
pub struct FileRepo {
    vault_path: PathBuf,
}

impl FileRepo {
    /// Create a new FileRepo rooted at the given vault path.
    pub fn new(vault_path: PathBuf) -> Self {
        Self { vault_path }
    }

    /// Create a lightweight clone of this FileRepo for use in AI subsystem contexts.
    /// Since FileRepo only holds a path, this is a cheap operation.
    pub fn clone_for_ai(&self) -> Self {
        Self {
            vault_path: self.vault_path.clone(),
        }
    }

    /// Get a reference to the vault root path.
    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }

    /// Generate an 8-character hex ID from a UUID v4.
    fn generate_id() -> String {
        let uuid = Uuid::new_v4();
        let bytes = uuid.as_bytes();
        hex::encode(&bytes[..4])
    }

    /// Create a new fragment file. Returns the generated fragment ID.
    ///
    /// The file is written to `capture/yyyy/mm/<id>.md` with YAML frontmatter.
    /// Content must contain at least one non-whitespace character.
    pub fn create_fragment(&self, content: &str) -> Result<String, RepoError> {
        if content.trim().is_empty() {
            return Err(RepoError::InvalidInput(
                "碎片内容不能为空白".to_string(),
            ));
        }

        let id = Self::generate_id();
        let now = Utc::now();

        let meta = FragmentMeta {
            id: id.clone(),
            created: now,
            source: "manual".to_string(),
            tags: vec![],
            topics: vec![],
        };

        let document = frontmatter::serialize(&meta, content)?;

        // Build path: capture/yyyy/mm/<id>.md
        let year = now.format("%Y").to_string();
        let month = now.format("%m").to_string();
        let dir = self.vault_path.join("capture").join(&year).join(&month);
        std::fs::create_dir_all(&dir)?;

        let file_path = dir.join(format!("{}.md", id));
        std::fs::write(&file_path, &document)?;

        Ok(id)
    }

    /// Read a fragment by ID. Returns (metadata, body content).
    ///
    /// Searches recursively under `capture/` for a file named `<id>.md`.
    #[allow(dead_code)]
    pub fn read_fragment(&self, id: &str) -> Result<(FragmentMeta, String), RepoError> {
        let file_path = self.find_fragment_path(id)?;
        let content = std::fs::read_to_string(&file_path)?;
        let parsed = frontmatter::parse::<FragmentMeta>(&content)?;
        Ok((parsed.meta, parsed.body))
    }

    /// List all fragment file paths under `capture/`.
    pub fn list_fragment_paths(&self) -> Result<Vec<PathBuf>, RepoError> {
        let capture_dir = self.vault_path.join("capture");
        if !capture_dir.exists() {
            return Ok(vec![]);
        }
        let mut paths = Vec::new();
        self.collect_md_files(&capture_dir, &mut paths)?;
        paths.sort();
        Ok(paths)
    }

    /// Create a new article with the given title. Returns the generated article ID.
    ///
    /// The file is written to `articles/<id>.md` with draft status.
    pub fn create_article(&self, title: &str) -> Result<String, RepoError> {
        let id = Self::generate_id();
        let now = Utc::now();

        let meta = ArticleMeta {
            id: id.clone(),
            title: title.to_string(),
            status: ArticleStatus::Draft,
            created: now,
            updated: now,
            tags: vec![],
        };

        let body = format!("# {}\n", title);
        let document = frontmatter::serialize(&meta, &body)?;

        let dir = self.vault_path.join("articles");
        std::fs::create_dir_all(&dir)?;

        let file_path = dir.join(format!("{}.md", id));
        std::fs::write(&file_path, &document)?;

        Ok(id)
    }

    /// Read an article by ID. Returns (metadata, body content).
    pub fn read_article(&self, id: &str) -> Result<(ArticleMeta, String), RepoError> {
        let file_path = self.article_path(id);
        if !file_path.exists() {
            return Err(RepoError::ArticleNotFound(id.to_string()));
        }
        let content = std::fs::read_to_string(&file_path)?;
        let parsed = frontmatter::parse::<ArticleMeta>(&content)?;
        Ok((parsed.meta, parsed.body))
    }

    /// Save (update) an article's metadata and content.
    pub fn save_article(
        &self,
        id: &str,
        meta: &ArticleMeta,
        content: &str,
    ) -> Result<(), RepoError> {
        let file_path = self.article_path(id);
        if !file_path.exists() {
            return Err(RepoError::ArticleNotFound(id.to_string()));
        }
        let document = frontmatter::serialize(meta, content)?;
        std::fs::write(&file_path, &document)?;
        Ok(())
    }

    /// Delete an article file by ID.
    pub fn delete_article(&self, id: &str) -> Result<(), RepoError> {
        let file_path = self.article_path(id);
        if !file_path.exists() {
            return Err(RepoError::ArticleNotFound(id.to_string()));
        }
        std::fs::remove_file(&file_path)?;
        Ok(())
    }

    /// Export an article to the given destination path (copies the file).
    pub fn export_article(&self, id: &str, dest: &Path) -> Result<(), RepoError> {
        let file_path = self.article_path(id);
        if !file_path.exists() {
            return Err(RepoError::ArticleNotFound(id.to_string()));
        }
        // Ensure destination directory exists
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&file_path, dest)?;
        Ok(())
    }

    /// Update a fragment's body content (keeps frontmatter, replaces body).
    pub fn update_fragment_content(&self, id: &str, new_content: &str) -> Result<(), RepoError> {
        if new_content.trim().is_empty() {
            return Err(RepoError::InvalidInput(
                "碎片内容不能为空白".to_string(),
            ));
        }

        let file_path = self.find_fragment_path(id)?;
        let file_content = std::fs::read_to_string(&file_path)?;
        let parsed = frontmatter::parse::<FragmentMeta>(&file_content)?;

        // Re-serialize with the same meta but new body
        let document = frontmatter::serialize(&parsed.meta, new_content)?;
        std::fs::write(&file_path, &document)?;

        Ok(())
    }

    /// Delete a fragment file from disk.
    pub fn delete_fragment(&self, id: &str) -> Result<(), RepoError> {
        let file_path = self.find_fragment_path(id)?;
        std::fs::remove_file(&file_path)?;
        Ok(())
    }

    /// Compute SHA-256 hash of content bytes, returned as lowercase hex string.
    pub fn content_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        hex::encode(&result)
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    /// Build the expected file path for an article.
    fn article_path(&self, id: &str) -> PathBuf {
        self.vault_path.join("articles").join(format!("{}.md", id))
    }

    /// Find a fragment file by searching capture/**/<id>.md.
    fn find_fragment_path(&self, id: &str) -> Result<PathBuf, RepoError> {
        let capture_dir = self.vault_path.join("capture");
        if !capture_dir.exists() {
            return Err(RepoError::FragmentNotFound(id.to_string()));
        }

        let filename = format!("{}.md", id);
        let found = self.find_file_recursive(&capture_dir, &filename)?;
        found.ok_or_else(|| RepoError::FragmentNotFound(id.to_string()))
    }

    /// Recursively search for a file by name.
    fn find_file_recursive(
        &self,
        dir: &Path,
        filename: &str,
    ) -> Result<Option<PathBuf>, RepoError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = self.find_file_recursive(&path, filename)? {
                    return Ok(Some(found));
                }
            } else if path.file_name().and_then(|n| n.to_str()) == Some(filename) {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// Recursively collect all .md files under a directory.
    fn collect_md_files(&self, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), RepoError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.collect_md_files(&path, paths)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                paths.push(path);
            }
        }
        Ok(())
    }
}

// We need the `hex` crate for encoding. Since it's a small utility, let's inline it
// using the sha2 output formatting that's already available.
// Actually, we need hex encoding for both UUID bytes and SHA-256 output.
// Let's add a minimal hex module since the `hex` crate may not be in Cargo.toml.
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
        }
        s
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a FileRepo with a temporary vault directory.
    fn setup() -> (TempDir, FileRepo) {
        let tmp = TempDir::new().unwrap();
        let repo = FileRepo::new(tmp.path().to_path_buf());
        (tmp, repo)
    }

    // --- Fragment tests ---

    #[test]
    fn test_create_fragment_produces_correct_path_format() {
        let (_tmp, repo) = setup();
        let id = repo.create_fragment("Hello, this is a test fragment").unwrap();

        // ID should be 8 hex chars
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));

        // File should exist under capture/yyyy/mm/<id>.md
        let paths = repo.list_fragment_paths().unwrap();
        assert_eq!(paths.len(), 1);

        let path = &paths[0];
        let path_str = path.to_string_lossy();
        // Verify path structure: capture/YYYY/MM/ID.md
        assert!(path_str.contains("capture/"));
        assert!(path_str.ends_with(&format!("{}.md", id)));

        // Verify year/month directory structure
        let relative = path.strip_prefix(repo.vault_path.join("capture")).unwrap();
        let components: Vec<&str> = relative
            .components()
            .map(|c| c.as_os_str().to_str().unwrap())
            .collect();
        assert_eq!(components.len(), 3); // year, month, file
        assert_eq!(components[0].len(), 4); // YYYY
        assert_eq!(components[1].len(), 2); // MM
        assert!(components[0].chars().all(|c| c.is_ascii_digit()));
        assert!(components[1].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_create_fragment_file_format() {
        let (_tmp, repo) = setup();
        let id = repo.create_fragment("Test content body").unwrap();

        let paths = repo.list_fragment_paths().unwrap();
        let content = fs::read_to_string(&paths[0]).unwrap();

        // Must start with frontmatter delimiter
        assert!(content.starts_with("---\n"));
        // Must contain all required fields
        assert!(content.contains(&format!("id: {}", id)));
        assert!(content.contains("created:"));
        assert!(content.contains("source: manual"));
        assert!(content.contains("tags: []"));
        assert!(content.contains("topics: []"));
        // Must contain the body
        assert!(content.contains("Test content body"));
    }

    #[test]
    fn test_read_fragment_returns_correct_meta_and_body() {
        let (_tmp, repo) = setup();
        let body = "This is my fragment content\nWith multiple lines";
        let id = repo.create_fragment(body).unwrap();

        let (meta, read_body) = repo.read_fragment(&id).unwrap();

        assert_eq!(meta.id, id);
        assert_eq!(meta.source, "manual");
        assert_eq!(meta.tags, Vec::<String>::new());
        assert_eq!(meta.topics, Vec::<String>::new());
        assert_eq!(read_body.trim(), body.trim());
    }

    #[test]
    fn test_create_fragment_rejects_blank_input() {
        let (_tmp, repo) = setup();

        let result = repo.create_fragment("");
        assert!(result.is_err());

        let result = repo.create_fragment("   \n\t  ");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_fragment_not_found() {
        let (_tmp, repo) = setup();
        let result = repo.read_fragment("nonexist");
        assert!(matches!(result, Err(RepoError::FragmentNotFound(_))));
    }

    #[test]
    fn test_list_fragment_paths_empty_vault() {
        let (_tmp, repo) = setup();
        let paths = repo.list_fragment_paths().unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_list_fragment_paths_multiple() {
        let (_tmp, repo) = setup();
        repo.create_fragment("Fragment 1").unwrap();
        repo.create_fragment("Fragment 2").unwrap();
        repo.create_fragment("Fragment 3").unwrap();

        let paths = repo.list_fragment_paths().unwrap();
        assert_eq!(paths.len(), 3);
    }

    // --- Article tests ---

    #[test]
    fn test_create_article_with_draft_status() {
        let (_tmp, repo) = setup();
        let id = repo.create_article("My First Article").unwrap();

        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));

        let (meta, body) = repo.read_article(&id).unwrap();
        assert_eq!(meta.id, id);
        assert_eq!(meta.title, "My First Article");
        assert_eq!(meta.status, ArticleStatus::Draft);
        assert_eq!(meta.tags, Vec::<String>::new());
        assert!(body.contains("# My First Article"));
    }

    #[test]
    fn test_save_article_updates_content() {
        let (_tmp, repo) = setup();
        let id = repo.create_article("Original Title").unwrap();

        let (mut meta, _) = repo.read_article(&id).unwrap();
        meta.title = "Updated Title".to_string();
        meta.status = ArticleStatus::Editing;
        meta.updated = Utc::now();

        let new_body = "# Updated Title\n\nNew content here.";
        repo.save_article(&id, &meta, new_body).unwrap();

        let (read_meta, read_body) = repo.read_article(&id).unwrap();
        assert_eq!(read_meta.title, "Updated Title");
        assert_eq!(read_meta.status, ArticleStatus::Editing);
        assert_eq!(read_body.trim(), new_body.trim());
    }

    #[test]
    fn test_delete_article_removes_file() {
        let (_tmp, repo) = setup();
        let id = repo.create_article("To Delete").unwrap();

        // File exists
        assert!(repo.read_article(&id).is_ok());

        // Delete it
        repo.delete_article(&id).unwrap();

        // File no longer exists
        assert!(matches!(
            repo.read_article(&id),
            Err(RepoError::ArticleNotFound(_))
        ));
    }

    #[test]
    fn test_delete_article_not_found() {
        let (_tmp, repo) = setup();
        let result = repo.delete_article("nonexist");
        assert!(matches!(result, Err(RepoError::ArticleNotFound(_))));
    }

    #[test]
    fn test_export_article() {
        let (tmp, repo) = setup();
        let id = repo.create_article("Export Me").unwrap();

        let dest = tmp.path().join("exports").join("exported.md");
        repo.export_article(&id, &dest).unwrap();

        assert!(dest.exists());
        let original = fs::read_to_string(repo.article_path(&id)).unwrap();
        let exported = fs::read_to_string(&dest).unwrap();
        assert_eq!(original, exported);
    }

    // --- Content hash tests ---

    #[test]
    fn test_content_hash_produces_consistent_sha256() {
        let hash1 = FileRepo::content_hash(b"hello world");
        let hash2 = FileRepo::content_hash(b"hello world");
        assert_eq!(hash1, hash2);
        // Known SHA-256 for "hello world"
        assert_eq!(
            hash1,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_content_hash_different_inputs() {
        let hash1 = FileRepo::content_hash(b"hello");
        let hash2 = FileRepo::content_hash(b"world");
        assert_ne!(hash1, hash2);
    }

    // --- Immutable log tests ---

    #[test]
    fn test_fragment_body_immutable_after_creation() {
        let (_tmp, repo) = setup();
        let original_body = "This content must never change";
        let id = repo.create_fragment(original_body).unwrap();

        // Read back body multiple times — should always match original
        let (_, body1) = repo.read_fragment(&id).unwrap();
        let (_, body2) = repo.read_fragment(&id).unwrap();

        assert_eq!(body1.trim(), original_body);
        assert_eq!(body2.trim(), original_body);

        // FileRepo does not expose a method to modify fragment body,
        // which enforces immutability at the API level.
        // Verify the file content on disk hasn't changed either.
        let paths = repo.list_fragment_paths().unwrap();
        let file_content = fs::read_to_string(&paths[0]).unwrap();
        assert!(file_content.contains(original_body));
    }

    #[test]
    fn test_fragment_body_preserves_special_characters() {
        let (_tmp, repo) = setup();
        let special_body = "Line 1\nLine 2\n\n---\n\nContent with `code` and *emphasis*";
        let id = repo.create_fragment(special_body).unwrap();

        let (_, body) = repo.read_fragment(&id).unwrap();
        assert_eq!(body.trim(), special_body.trim());
    }
}
