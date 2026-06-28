//! Property-Based Test: AGENTS.md 内容完整性
//!
//! **Property 14: AGENTS.md 内容完整性**
//! 验证生成内容包含目录结构、字段说明、topics
//!
//! For any CognestVault state (containing directory structure and topics list),
//! the generated AGENTS.md file SHALL contain:
//! - Vault top-level directory structure (depth ≤ 2)
//! - Fragment and article frontmatter field descriptions
//! - All current topic names
//!
//! **Validates: Requirements 12.4**

use app_lib::core::cli_agents::context::generate_agents_md;
use proptest::prelude::*;
use proptest::collection::vec as prop_vec;
use std::fs;
use tempfile::TempDir;

// ─── Generators ─────────────────────────────────────────────────────────────

/// Generate a valid directory name (alphanumeric, no dots at start)
fn gen_dir_name() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9_-]{0,15}")
        .unwrap()
        .prop_filter("non-empty dir name", |s| !s.is_empty())
}

/// Generate a valid topic name (alphanumeric + Chinese characters)
fn gen_topic() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::string::string_regex("[a-zA-Z][a-zA-Z0-9-]{0,20}").unwrap(),
        prop::string::string_regex("[a-z]{1,10}").unwrap(),
    ]
    .prop_filter("non-empty topic", |s| !s.is_empty())
}

/// Generate a list of topics (0-20)
fn gen_topics() -> impl Strategy<Value = Vec<String>> {
    prop_vec(gen_topic(), 0..20)
}

/// Generate a vault directory structure description
/// Returns (top-level dirs, subdirs per top-level dir)
fn gen_vault_structure() -> impl Strategy<Value = Vec<(String, Vec<String>)>> {
    prop_vec(
        (gen_dir_name(), prop_vec(gen_dir_name(), 0..5)),
        1..8,
    )
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Create a temporary vault directory with the given structure
fn create_vault(structure: &[(String, Vec<String>)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path();

    for (top_dir, sub_dirs) in structure {
        let top_path = vault.join(top_dir);
        fs::create_dir_all(&top_path).unwrap();

        for sub_dir in sub_dirs {
            let sub_path = top_path.join(sub_dir);
            fs::create_dir_all(&sub_path).unwrap();
        }
    }

    tmp
}

// ─── Property 14: AGENTS.md 内容完整性 ──────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14: Generated AGENTS.md contains directory structure section.
    /// For any vault directory structure, the output SHALL include a
    /// "Directory Structure" section listing the top-level directories.
    #[test]
    fn prop_agents_md_contains_directory_structure(
        structure in gen_vault_structure(),
    ) {
        let tmp = create_vault(&structure);
        let topics: Vec<String> = vec![];

        let result = generate_agents_md(tmp.path(), &topics);

        // Must contain directory structure section
        prop_assert!(
            result.contains("## Directory Structure"),
            "Missing '## Directory Structure' section in output:\n{}",
            &result[..result.len().min(500)]
        );

        // Must contain code block for the tree
        prop_assert!(
            result.contains("```"),
            "Missing code block for directory tree"
        );

        // Each top-level directory should appear in the output
        for (dir_name, _) in &structure {
            prop_assert!(
                result.contains(&format!("{}/", dir_name)),
                "Top-level directory '{}/' not found in output:\n{}",
                dir_name,
                &result[..result.len().min(800)]
            );
        }
    }

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14: Generated AGENTS.md contains frontmatter field descriptions.
    /// For any vault state, the output SHALL include descriptions of fragment
    /// and article YAML frontmatter fields.
    #[test]
    fn prop_agents_md_contains_frontmatter_descriptions(
        structure in gen_vault_structure(),
        topics in gen_topics(),
    ) {
        let tmp = create_vault(&structure);

        let result = generate_agents_md(tmp.path(), &topics);

        // Must contain frontmatter section
        prop_assert!(
            result.contains("## Frontmatter Fields"),
            "Missing '## Frontmatter Fields' section"
        );

        // Must contain fragment frontmatter description
        prop_assert!(
            result.contains("Fragment Frontmatter"),
            "Missing 'Fragment Frontmatter' subsection"
        );

        // Must contain article frontmatter description
        prop_assert!(
            result.contains("Article Frontmatter"),
            "Missing 'Article Frontmatter' subsection"
        );

        // Must contain key fields: id, tags, topics for fragments
        prop_assert!(
            result.contains("| `id` |"),
            "Missing 'id' field description"
        );
        prop_assert!(
            result.contains("| `tags` |"),
            "Missing 'tags' field description"
        );
        prop_assert!(
            result.contains("| `topics` |"),
            "Missing 'topics' field description"
        );

        // Must contain key fields for articles: title, status
        prop_assert!(
            result.contains("| `title` |"),
            "Missing 'title' field description"
        );
        prop_assert!(
            result.contains("| `status` |"),
            "Missing 'status' field description"
        );
    }

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14: Generated AGENTS.md contains all topics.
    /// For any non-empty topics list, the output SHALL include every topic name
    /// in the Topics section.
    #[test]
    fn prop_agents_md_contains_all_topics(
        structure in gen_vault_structure(),
        topics in gen_topics(),
    ) {
        let tmp = create_vault(&structure);

        let result = generate_agents_md(tmp.path(), &topics);

        // Must contain topics section
        prop_assert!(
            result.contains("## Topics"),
            "Missing '## Topics' section"
        );

        // Every topic must appear in the output
        for topic in &topics {
            prop_assert!(
                result.contains(&format!("- {}", topic)),
                "Topic '{}' not found in Topics section. Output:\n{}",
                topic,
                &result[..result.len().min(1000)]
            );
        }
    }

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14 (supplemental): Empty topics list shows placeholder message.
    /// When no topics are defined, the output SHALL indicate this clearly.
    #[test]
    fn prop_agents_md_empty_topics_shows_placeholder(
        structure in gen_vault_structure(),
    ) {
        let tmp = create_vault(&structure);
        let topics: Vec<String> = vec![];

        let result = generate_agents_md(tmp.path(), &topics);

        // Must contain the empty-topics placeholder
        prop_assert!(
            result.contains("_No topics defined yet._"),
            "Missing empty-topics placeholder when topics list is empty"
        );
    }

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14 (supplemental): Directory tree respects depth ≤ 2 limit.
    /// Directories nested deeper than 2 levels from the vault root SHALL NOT
    /// appear in the generated output.
    #[test]
    fn prop_agents_md_directory_depth_limit(
        top_dir in gen_dir_name(),
        sub_dir in gen_dir_name(),
        deep_dir in gen_dir_name(),
    ) {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path();

        // Create depth-0: top_dir/
        // Create depth-1: top_dir/sub_dir/
        // Create depth-2: top_dir/sub_dir/deep_dir/ (should appear)
        // Create depth-3: top_dir/sub_dir/deep_dir/very_deep/ (should NOT appear)
        let very_deep_name = "xyzverydeepdirname";
        fs::create_dir_all(
            vault.join(&top_dir).join(&sub_dir).join(&deep_dir).join(very_deep_name)
        ).unwrap();

        let result = generate_agents_md(vault, &[]);

        // Top-level dir should appear
        prop_assert!(
            result.contains(&format!("{}/", top_dir)),
            "Top-level dir '{}/' should appear in output",
            top_dir
        );

        // Sub-dir (depth 1) should appear
        prop_assert!(
            result.contains(&format!("{}/", sub_dir)),
            "Sub-dir '{}/' (depth 1) should appear in output",
            sub_dir
        );

        // Deep dir (depth 2) should appear
        prop_assert!(
            result.contains(&format!("{}/", deep_dir)),
            "Deep dir '{}/' (depth 2) should appear in output",
            deep_dir
        );

        // Very deep dir (depth 3) should NOT appear
        prop_assert!(
            !result.contains(very_deep_name),
            "Very deep dir '{}' (depth 3) should NOT appear in output",
            very_deep_name
        );
    }

    /// **Validates: Requirements 12.4**
    ///
    /// Property 14 (supplemental): All three mandatory sections are present.
    /// For any vault state, the generated AGENTS.md SHALL contain all three
    /// required sections: Directory Structure, Frontmatter Fields, Topics.
    #[test]
    fn prop_agents_md_all_sections_present(
        structure in gen_vault_structure(),
        topics in gen_topics(),
    ) {
        let tmp = create_vault(&structure);

        let result = generate_agents_md(tmp.path(), &topics);

        // Header
        prop_assert!(
            result.contains("# CognestVault"),
            "Missing top-level heading"
        );

        // All three mandatory sections
        prop_assert!(
            result.contains("## Directory Structure"),
            "Missing Directory Structure section"
        );
        prop_assert!(
            result.contains("## Frontmatter Fields"),
            "Missing Frontmatter Fields section"
        );
        prop_assert!(
            result.contains("## Topics"),
            "Missing Topics section"
        );
    }
}
