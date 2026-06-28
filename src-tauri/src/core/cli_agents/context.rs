// Cognest Core — CLI Agent Context Generator
//
// Generates AGENTS.md content for CLI agents to understand the vault structure.
// Write failures degrade gracefully (log error, continue).

use std::fmt::Write as FmtWrite;
use std::path::Path;

/// Generate the AGENTS.md content string.
///
/// Includes:
/// - CognestVault top-level directory structure (depth ≤ 2)
/// - Fragment and article YAML frontmatter field descriptions
/// - Current topics list
pub fn generate_agents_md(vault_path: &Path, topics: &[String]) -> String {
    let mut md = String::new();

    md.push_str("# CognestVault — Agent Context\n\n");
    md.push_str("This file provides context about the knowledge vault structure for AI agents.\n\n");

    // Section 1: Directory structure
    md.push_str("## Directory Structure\n\n");
    md.push_str("```\n");
    write_directory_tree(&mut md, vault_path, "", 0, 2);
    md.push_str("```\n\n");

    // Section 2: Frontmatter field descriptions
    md.push_str("## Frontmatter Fields\n\n");
    write_frontmatter_descriptions(&mut md);

    // Section 3: Topics list
    md.push_str("## Topics\n\n");
    if topics.is_empty() {
        md.push_str("_No topics defined yet._\n");
    } else {
        for topic in topics {
            let _ = writeln!(md, "- {}", topic);
        }
    }

    md
}

/// Write AGENTS.md to the vault root directory.
///
/// On failure, logs a warning and returns the error — caller should continue
/// without the context file (graceful degradation per Requirement 12.5).
pub fn write_agents_md(vault_path: &Path, content: &str) -> Result<(), std::io::Error> {
    let agents_md_path = vault_path.join("AGENTS.md");
    match std::fs::write(&agents_md_path, content) {
        Ok(()) => Ok(()),
        Err(e) => {
            log::warn!(
                "Failed to write AGENTS.md at {}: {}",
                agents_md_path.display(),
                e
            );
            Err(e)
        }
    }
}

/// Recursively write directory tree entries up to max_depth.
/// Only includes directories and skips hidden files/dirs (starting with '.').
fn write_directory_tree(
    output: &mut String,
    dir: &Path,
    prefix: &str,
    current_depth: usize,
    max_depth: usize,
) {
    if current_depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    // Collect and sort entries, filtering out hidden ones
    let mut items: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| !n.starts_with('.'))
                .unwrap_or(false)
        })
        .collect();

    items.sort_by_key(|e| e.file_name());

    for entry in &items {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let path = entry.path();

        if path.is_dir() {
            let _ = writeln!(output, "{}{}/", prefix, name_str);
            if current_depth < max_depth {
                let child_prefix = format!("{}  ", prefix);
                write_directory_tree(output, &path, &child_prefix, current_depth + 1, max_depth);
            }
        } else if current_depth == 0 {
            // Only show top-level files (like AGENTS.md itself, README, etc.)
            let _ = writeln!(output, "{}{}", prefix, name_str);
        }
    }
}

/// Write frontmatter field descriptions for fragments and articles.
fn write_frontmatter_descriptions(output: &mut String) {
    output.push_str("### Fragment Frontmatter (`capture/**/*.md`)\n\n");
    output.push_str("| Field | Type | Description |\n");
    output.push_str("|-------|------|-------------|\n");
    output.push_str("| `id` | string | Unique 8-character hex identifier |\n");
    output.push_str("| `created` | datetime | ISO 8601 creation timestamp |\n");
    output.push_str("| `source` | string | Origin of the fragment (e.g. \"manual\") |\n");
    output.push_str("| `tags` | string[] | User or AI-assigned tags |\n");
    output.push_str("| `topics` | string[] | AI-assigned topic classifications |\n");
    output.push_str("\n");

    output.push_str("### Article Frontmatter (`articles/*.md`)\n\n");
    output.push_str("| Field | Type | Description |\n");
    output.push_str("|-------|------|-------------|\n");
    output.push_str("| `id` | string | Unique 8-character hex identifier |\n");
    output.push_str("| `title` | string | Article title |\n");
    output.push_str("| `status` | string | Lifecycle status: draft, editing, completed |\n");
    output.push_str("| `created` | datetime | ISO 8601 creation timestamp |\n");
    output.push_str("| `updated` | datetime | ISO 8601 last update timestamp |\n");
    output.push_str("| `tags` | string[] | User or AI-assigned tags |\n");
    output.push_str("\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_vault() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();

        // Create capture directory structure
        fs::create_dir_all(vault.join("capture/2024/01")).unwrap();
        fs::create_dir_all(vault.join("capture/2024/02")).unwrap();
        fs::write(vault.join("capture/2024/01/aabb0011.md"), "test").unwrap();

        // Create articles directory
        fs::create_dir_all(vault.join("articles")).unwrap();
        fs::write(vault.join("articles/ccdd2233.md"), "article").unwrap();

        // Create a top-level file
        fs::write(vault.join("README.md"), "# Vault").unwrap();

        tmp
    }

    #[test]
    fn test_generate_agents_md_contains_directory_structure() {
        let tmp = setup_vault();
        let topics = vec!["programming".to_string(), "design".to_string()];

        let result = generate_agents_md(tmp.path(), &topics);

        assert!(result.contains("## Directory Structure"));
        assert!(result.contains("capture/"));
        assert!(result.contains("articles/"));
    }

    #[test]
    fn test_generate_agents_md_contains_frontmatter_descriptions() {
        let tmp = setup_vault();
        let topics = vec![];

        let result = generate_agents_md(tmp.path(), &topics);

        assert!(result.contains("## Frontmatter Fields"));
        assert!(result.contains("Fragment Frontmatter"));
        assert!(result.contains("Article Frontmatter"));
        assert!(result.contains("| `id` |"));
        assert!(result.contains("| `topics` |"));
        assert!(result.contains("| `status` |"));
    }

    #[test]
    fn test_generate_agents_md_contains_topics() {
        let tmp = setup_vault();
        let topics = vec![
            "programming".to_string(),
            "design".to_string(),
            "ai".to_string(),
        ];

        let result = generate_agents_md(tmp.path(), &topics);

        assert!(result.contains("## Topics"));
        assert!(result.contains("- programming"));
        assert!(result.contains("- design"));
        assert!(result.contains("- ai"));
    }

    #[test]
    fn test_generate_agents_md_empty_topics() {
        let tmp = setup_vault();
        let topics: Vec<String> = vec![];

        let result = generate_agents_md(tmp.path(), &topics);

        assert!(result.contains("_No topics defined yet._"));
    }

    #[test]
    fn test_generate_agents_md_respects_depth_limit() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();

        // Create deeply nested structure
        fs::create_dir_all(vault.join("capture/2024/01/deep/nested")).unwrap();
        fs::create_dir_all(vault.join("articles")).unwrap();

        let result = generate_agents_md(vault, &[]);

        // depth=0: capture/, articles/
        // depth=1: capture/2024/
        // depth=2: capture/2024/01/
        // depth>2: "deep/nested/" should NOT appear
        assert!(result.contains("capture/"));
        assert!(result.contains("2024/"));
        assert!(result.contains("01/"));
        assert!(!result.contains("deep/"));
        assert!(!result.contains("nested/"));
    }

    #[test]
    fn test_generate_agents_md_skips_hidden_dirs() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();

        fs::create_dir_all(vault.join(".cognest")).unwrap();
        fs::create_dir_all(vault.join(".git")).unwrap();
        fs::create_dir_all(vault.join("capture")).unwrap();

        let result = generate_agents_md(vault, &[]);

        assert!(result.contains("capture/"));
        assert!(!result.contains(".cognest"));
        assert!(!result.contains(".git"));
    }

    #[test]
    fn test_write_agents_md_success() {
        let tmp = TempDir::new().unwrap();
        let content = "# Test AGENTS.md\n";

        let result = write_agents_md(tmp.path(), content);
        assert!(result.is_ok());

        let written = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert_eq!(written, content);
    }

    #[test]
    fn test_write_agents_md_failure_returns_error() {
        // Try to write to a non-existent directory
        let path = Path::new("/nonexistent/path/that/does/not/exist");
        let result = write_agents_md(path, "content");
        assert!(result.is_err());
    }
}
