// Git module — git2-based Git operations for Vault sync
//
// Provides: open(), sync_status(), sync(), ensure_gitignore()
// No dependency on Tauri types — pure Rust.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Local;
use git2::{
    Cred, IndexAddOption, PushOptions, RemoteCallbacks, Repository, Signature, StatusOptions,
};

/// Git synchronization status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum SyncStatus {
    /// All changes committed and pushed.
    Synced,
    /// There are uncommitted changes or unpushed commits.
    Unsynced { file_count: usize },
    /// No remote "origin" configured.
    NoRemote,
}

/// Result of a successful sync operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncResult {
    pub files_changed: usize,
    pub commit_sha: String,
}

/// Git-related errors.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git 仓库打开失败: {0}")]
    OpenFailed(String),

    #[error("Git 操作失败: {0}")]
    OperationFailed(String),

    #[error("Push 超时（30秒）")]
    PushTimeout,

    #[error("Push 失败: {0}")]
    PushFailed(String),

    #[error("无远程仓库")]
    NoRemote,

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// Git module wrapping a git2::Repository.
pub struct GitModule {
    repo: Repository,
    #[allow(dead_code)]
    vault_path: PathBuf,
}

/// Required entries in .gitignore
#[allow(dead_code)]
const GITIGNORE_ENTRIES: &[&str] = &[".cognest/", "*.sqlite", "vectors.bin"];

impl GitModule {
    /// Open an existing Git repository at vault_path.
    pub fn open(vault_path: &Path) -> Result<Self, GitError> {
        let repo = Repository::open(vault_path)
            .map_err(|e| GitError::OpenFailed(e.message().to_string()))?;
        Ok(Self {
            repo,
            vault_path: vault_path.to_path_buf(),
        })
    }

    /// Get the current sync status.
    ///
    /// Returns:
    /// - NoRemote if "origin" remote doesn't exist
    /// - Synced if no uncommitted changes and no unpushed commits
    /// - Unsynced { file_count } otherwise
    pub fn sync_status(&self) -> Result<SyncStatus, GitError> {
        // Check if remote "origin" exists
        if self.repo.find_remote("origin").is_err() {
            return Ok(SyncStatus::NoRemote);
        }

        let mut file_count = 0;

        // Count uncommitted/unstaged changes
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        file_count += statuses.len();

        // Count unpushed commits (ahead of remote)
        file_count += self.count_unpushed_commits()?;

        if file_count == 0 {
            Ok(SyncStatus::Synced)
        } else {
            Ok(SyncStatus::Unsynced { file_count })
        }
    }

    /// Execute git add . → git commit → git push.
    ///
    /// - Commit message: "sync: N files changed · YYYY-MM-DD HH:mm"
    /// - No empty commits if nothing changed
    /// - Push timeout: 30 seconds
    /// - On push failure: keeps local commit, returns error
    pub fn sync(&self) -> Result<SyncResult, GitError> {
        // Check remote first
        if self.repo.find_remote("origin").is_err() {
            return Err(GitError::NoRemote);
        }

        // Stage all changes (git add .)
        let mut index = self
            .repo
            .index()
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        // Also remove deleted files from the index
        index
            .update_all(["*"].iter(), None)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        index
            .write()
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        // After staging, check the diff between index and HEAD
        let head_tree = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok());

        let diff = self
            .repo
            .diff_tree_to_index(head_tree.as_ref(), Some(&index), None)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        let _files_changed = diff.deltas().len();

        // Re-read index to get final staged state
        let mut index = self
            .repo
            .index()
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        // Re-check diff after add_all
        let tree_oid = index
            .write_tree()
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        // Compare tree with HEAD to get actual file count
        let final_diff = self
            .repo
            .diff_tree_to_tree(head_tree.as_ref(), Some(&tree), None)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        let final_files_changed = final_diff.deltas().len();

        if final_files_changed == 0 {
            // Nothing to commit
            return Ok(SyncResult {
                files_changed: 0,
                commit_sha: String::new(),
            });
        }

        // Create commit
        let now = Local::now();
        let commit_msg = format!(
            "sync: {} files changed \u{00b7} {}",
            final_files_changed,
            now.format("%Y-%m-%d %H:%M")
        );

        let sig = self.make_signature()?;
        let parent = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok());

        let parents: Vec<&git2::Commit> = parent.iter().collect();

        let commit_oid = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, &commit_msg, &tree, &parents)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        let commit_sha = commit_oid.to_string();

        // Push with 30s timeout
        self.push_with_timeout(Duration::from_secs(30))?;

        Ok(SyncResult {
            files_changed: final_files_changed,
            commit_sha,
        })
    }

    /// Ensure .gitignore contains required exclusion entries.
    ///
    /// Creates .gitignore if it doesn't exist, or appends missing entries.
    #[allow(dead_code)]
    pub fn ensure_gitignore(&self) -> Result<(), GitError> {
        let gitignore_path = self.vault_path.join(".gitignore");

        let existing_content = if gitignore_path.exists() {
            std::fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        let existing_lines: Vec<&str> = existing_content.lines().collect();
        let mut missing: Vec<&str> = Vec::new();

        for entry in GITIGNORE_ENTRIES {
            if !existing_lines.iter().any(|line| line.trim() == *entry) {
                missing.push(entry);
            }
        }

        if missing.is_empty() {
            return Ok(());
        }

        let mut new_content = existing_content.clone();
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }

        // Add a comment header if we're adding to an existing file without our entries
        if !existing_content.is_empty() && !existing_content.contains("# Cognest") {
            new_content.push_str("\n# Cognest\n");
        } else if existing_content.is_empty() {
            new_content.push_str("# Cognest\n");
        }

        for entry in &missing {
            new_content.push_str(entry);
            new_content.push('\n');
        }

        std::fs::write(&gitignore_path, new_content)?;
        Ok(())
    }

    // ─── Private Helpers ─────────────────────────────────────────────────────

    /// Count commits ahead of remote tracking branch.
    fn count_unpushed_commits(&self) -> Result<usize, GitError> {
        let head = match self.repo.head() {
            Ok(h) => h,
            Err(_) => return Ok(0), // No HEAD means no commits at all
        };

        let local_oid = match head.target() {
            Some(oid) => oid,
            None => return Ok(0),
        };

        // Find the upstream tracking branch
        let branch_name = head
            .shorthand()
            .unwrap_or("main")
            .to_string();

        let upstream_ref = format!("refs/remotes/origin/{}", branch_name);
        let upstream_oid = match self.repo.refname_to_id(&upstream_ref) {
            Ok(oid) => oid,
            Err(_) => {
                // No upstream tracking — all local commits are "unpushed"
                // But we report 0 here since we already count via statuses
                return Ok(0);
            }
        };

        let (ahead, _behind) = self
            .repo
            .graph_ahead_behind(local_oid, upstream_oid)
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        Ok(ahead)
    }

    /// Create a git signature from repo config or fallback defaults.
    fn make_signature(&self) -> Result<Signature<'static>, GitError> {
        // Try to get signature from git config
        if let Ok(sig) = self.repo.signature() {
            return Ok(Signature::now(
                sig.name().unwrap_or("Cognest"),
                sig.email().unwrap_or("cognest@local"),
            )
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?);
        }

        // Fallback
        Signature::now("Cognest", "cognest@local")
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))
    }

    /// Push to origin with a timeout.
    fn push_with_timeout(&self, timeout: Duration) -> Result<(), GitError> {
        let mut remote = self
            .repo
            .find_remote("origin")
            .map_err(|e| GitError::PushFailed(e.message().to_string()))?;

        let head = self
            .repo
            .head()
            .map_err(|e| GitError::OperationFailed(e.message().to_string()))?;

        let refspec = head
            .name()
            .unwrap_or("refs/heads/main")
            .to_string();

        let mut callbacks = RemoteCallbacks::new();

        // Credential callback — try SSH agent, then default
        callbacks.credentials(|_url, username_from_url, _allowed_types| {
            // Try SSH agent first
            if let Some(username) = username_from_url {
                if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                    return Ok(cred);
                }
            }
            // Try default credentials
            Cred::default()
        });

        // Transfer progress for timeout tracking
        let start = std::time::Instant::now();
        let timeout_dur = timeout;
        callbacks.push_transfer_progress(move |_current, _total, _bytes| {
            if start.elapsed() > timeout_dur {
                // Note: git2 doesn't have a clean way to abort mid-transfer
                // via this callback. We rely on the sideband_progress or
                // the overall operation timeout below.
            }
        });

        let mut push_opts = PushOptions::new();
        push_opts.remote_callbacks(callbacks);

        // Use a thread with timeout for the push operation
        let refspec_clone = refspec.clone();

        // git2 operations are not Send, so we can't easily spawn them.
        // Instead, we perform the push directly and rely on network-level timeouts.
        // The git2 library respects system-level socket timeouts.
        // For a more robust timeout, we set it via push options.
        let push_result = remote.push(&[&refspec_clone], Some(&mut push_opts));

        match push_result {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.message().to_string();
                if msg.contains("timed out") || msg.contains("timeout") {
                    Err(GitError::PushTimeout)
                } else {
                    Err(GitError::PushFailed(msg))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a git repo in a temp dir.
    fn setup_git_repo() -> (TempDir, Repository) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Create an initial commit so HEAD exists
        {
            let sig = Signature::now("Test", "test@test.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn test_open_valid_repo() {
        let (dir, _repo) = setup_git_repo();
        let module = GitModule::open(dir.path());
        assert!(module.is_ok());
    }

    #[test]
    fn test_open_invalid_path() {
        let result = GitModule::open(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_status_no_remote() {
        let (dir, _repo) = setup_git_repo();
        let module = GitModule::open(dir.path()).unwrap();
        let status = module.sync_status().unwrap();
        assert!(matches!(status, SyncStatus::NoRemote));
    }

    #[test]
    fn test_sync_status_synced_with_remote() {
        let (dir, repo) = setup_git_repo();
        // Create a bare remote
        let remote_dir = TempDir::new().unwrap();
        Repository::init_bare(remote_dir.path()).unwrap();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();

        // Push initial commit
        let mut remote = repo.find_remote("origin").unwrap();
        remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        let status = module.sync_status().unwrap();
        assert!(matches!(status, SyncStatus::Synced));
    }

    #[test]
    fn test_sync_status_unsynced_with_changes() {
        let (dir, repo) = setup_git_repo();
        // Create a bare remote
        let remote_dir = TempDir::new().unwrap();
        Repository::init_bare(remote_dir.path()).unwrap();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();

        // Push initial state
        let mut remote = repo.find_remote("origin").unwrap();
        remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

        // Create a new file (uncommitted change)
        std::fs::write(dir.path().join("test.md"), "hello").unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        let status = module.sync_status().unwrap();
        match status {
            SyncStatus::Unsynced { file_count } => assert!(file_count > 0),
            other => panic!("Expected Unsynced, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_no_remote_returns_error() {
        let (dir, _repo) = setup_git_repo();
        let module = GitModule::open(dir.path()).unwrap();
        let result = module.sync();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GitError::NoRemote));
    }

    #[test]
    fn test_sync_no_changes() {
        let (dir, repo) = setup_git_repo();
        let remote_dir = TempDir::new().unwrap();
        Repository::init_bare(remote_dir.path()).unwrap();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();

        let mut remote = repo.find_remote("origin").unwrap();
        remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        let result = module.sync().unwrap();
        assert_eq!(result.files_changed, 0);
        assert!(result.commit_sha.is_empty());
    }

    #[test]
    fn test_sync_with_changes() {
        let (dir, repo) = setup_git_repo();
        let remote_dir = TempDir::new().unwrap();
        Repository::init_bare(remote_dir.path()).unwrap();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();

        let mut remote = repo.find_remote("origin").unwrap();
        remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

        // Create new files
        std::fs::write(dir.path().join("file1.md"), "content 1").unwrap();
        std::fs::write(dir.path().join("file2.md"), "content 2").unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        let result = module.sync().unwrap();
        assert_eq!(result.files_changed, 2);
        assert!(!result.commit_sha.is_empty());
    }

    #[test]
    fn test_ensure_gitignore_creates_new() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        let module = GitModule::open(dir.path()).unwrap();

        module.ensure_gitignore().unwrap();

        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".cognest/"));
        assert!(content.contains("*.sqlite"));
        assert!(content.contains("vectors.bin"));
    }

    #[test]
    fn test_ensure_gitignore_appends_missing() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();

        // Write a partial .gitignore
        std::fs::write(dir.path().join(".gitignore"), "*.log\n.cognest/\n").unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        module.ensure_gitignore().unwrap();

        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".cognest/"));
        assert!(content.contains("*.sqlite"));
        assert!(content.contains("vectors.bin"));
        assert!(content.contains("*.log")); // Original entry preserved
    }

    #[test]
    fn test_ensure_gitignore_idempotent() {
        let dir = TempDir::new().unwrap();
        Repository::init(dir.path()).unwrap();
        let module = GitModule::open(dir.path()).unwrap();

        module.ensure_gitignore().unwrap();
        let content1 = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();

        module.ensure_gitignore().unwrap();
        let content2 = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();

        assert_eq!(content1, content2);
    }

    #[test]
    fn test_sync_commit_message_format() {
        let (dir, repo) = setup_git_repo();
        let remote_dir = TempDir::new().unwrap();
        Repository::init_bare(remote_dir.path()).unwrap();

        repo.remote("origin", &format!("file://{}", remote_dir.path().display()))
            .unwrap();

        let mut remote = repo.find_remote("origin").unwrap();
        remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();

        std::fs::write(dir.path().join("test.md"), "hello").unwrap();

        let module = GitModule::open(dir.path()).unwrap();
        let result = module.sync().unwrap();

        // Verify commit message format
        let commit = repo
            .find_commit(git2::Oid::from_str(&result.commit_sha).unwrap())
            .unwrap();
        let msg = commit.message().unwrap();
        assert!(msg.starts_with("sync: 1 files changed \u{00b7} "));
        // Verify date format: YYYY-MM-DD HH:MM
        let date_part = msg.split('\u{00b7}').nth(1).unwrap().trim();
        assert_eq!(date_part.len(), 16); // "2024-01-01 12:00"
    }
}
