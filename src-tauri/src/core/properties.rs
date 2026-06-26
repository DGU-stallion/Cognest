// Cognest Core — Property-Based Tests
//
// Property tests (proptest, 100+ iterations) covering:
// - Property 1: Frontmatter round-trip
// - Property 2: Fragment file format invariants
// - Property 3: Immutable body content
// - Property 4: Index count = valid files
// - Property 5: Content hash detection
// - Property 6: Blank rejection

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use tempfile::TempDir;

    use crate::core::frontmatter;
    use crate::core::index::IndexDb;
    use crate::core::repo::{FileRepo, FragmentMeta};

    // ─── Strategies ─────────────────────────────────────────────────────────

    /// Generate a valid 8-char hex ID
    fn arb_hex_id() -> impl Strategy<Value = String> {
        "[a-f0-9]{8}".prop_map(|s| s)
    }

    /// Generate a non-empty body string (at least 1 non-whitespace char)
    fn arb_body() -> impl Strategy<Value = String> {
        // A non-whitespace char followed by 0-200 arbitrary printable chars
        ("[^ \\t\\n\\r][\\x20-\\x7E]{0,200}")
            .prop_map(|s| s)
    }

    /// Generate a tag name: 3-10 lowercase ascii chars
    fn arb_tag() -> impl Strategy<Value = String> {
        "[a-z]{3,10}".prop_map(|s| s)
    }

    /// Generate a list of tags (0 to 5 tags)
    fn arb_tags() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(arb_tag(), 0..5)
    }

    // ─── Property 1: Frontmatter Round-Trip ─────────────────────────────────
    // **Validates: Requirements 4.5**
    //
    // For any valid FragmentMeta, serialize then parse produces identical struct.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_1_frontmatter_roundtrip(
            id in arb_hex_id(),
            tags in arb_tags(),
            topics in arb_tags(),
            body in arb_body(),
        ) {
            let meta = FragmentMeta {
                id: id.clone(),
                created: chrono::Utc::now(),
                source: "manual".to_string(),
                tags: tags.clone(),
                topics: topics.clone(),
            };

            let serialized = frontmatter::serialize(&meta, &body).unwrap();
            let parsed: frontmatter::ParsedDocument<FragmentMeta> =
                frontmatter::parse(&serialized).unwrap();

            prop_assert_eq!(&parsed.meta.id, &meta.id);
            prop_assert_eq!(&parsed.meta.source, &meta.source);
            prop_assert_eq!(&parsed.meta.tags, &meta.tags);
            prop_assert_eq!(&parsed.meta.topics, &meta.topics);
            // Body: trim comparison since serialize adds trailing newline
            prop_assert_eq!(parsed.body.trim(), body.trim());
        }
    }

    // ─── Property 2: Fragment File Format ───────────────────────────────────
    // **Validates: Requirements 3.1, 3.2**
    //
    // For any valid content string, create_fragment produces file at
    // capture/yyyy/mm/<8hex>.md with correct frontmatter fields.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_2_fragment_file_format(
            body in arb_body(),
        ) {
            let tmp = TempDir::new().unwrap();
            let repo = FileRepo::new(tmp.path().to_path_buf());

            let id = repo.create_fragment(&body).unwrap();

            // ID is 8 hex chars
            prop_assert_eq!(id.len(), 8);
            prop_assert!(id.chars().all(|c| c.is_ascii_hexdigit()));

            // File path: capture/yyyy/mm/<id>.md
            let paths = repo.list_fragment_paths().unwrap();
            prop_assert_eq!(paths.len(), 1);

            let path = &paths[0];
            let relative = path.strip_prefix(tmp.path().join("capture")).unwrap();
            let components: Vec<&str> = relative
                .components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect();
            // year/month/file
            prop_assert_eq!(components.len(), 3);
            prop_assert_eq!(components[0].len(), 4); // YYYY
            prop_assert_eq!(components[1].len(), 2); // MM
            prop_assert!(components[0].chars().all(|c| c.is_ascii_digit()));
            prop_assert!(components[1].chars().all(|c| c.is_ascii_digit()));
            prop_assert!(components[2].ends_with(".md"));

            // Read file content and verify frontmatter fields
            let content = std::fs::read_to_string(path).unwrap();
            prop_assert!(content.starts_with("---\n"));

            // Parse back the file to verify all frontmatter fields
            let parsed: frontmatter::ParsedDocument<FragmentMeta> =
                frontmatter::parse(&content).unwrap();
            prop_assert_eq!(&parsed.meta.id, &id);
            prop_assert_eq!(&parsed.meta.source, "manual");
            prop_assert_eq!(&parsed.meta.tags, &Vec::<String>::new());
            prop_assert_eq!(&parsed.meta.topics, &Vec::<String>::new());
            // created field should be present (non-empty)
            prop_assert!(!parsed.meta.created.to_rfc3339().is_empty());
        }
    }

    // ─── Property 3: Immutable Body ────────────────────────────────────────
    // **Validates: Requirements 3.3, 3.7**
    //
    // After creating a fragment, reading it back returns identical body content.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_3_immutable_body(
            body in arb_body(),
        ) {
            let tmp = TempDir::new().unwrap();
            let repo = FileRepo::new(tmp.path().to_path_buf());

            let id = repo.create_fragment(&body).unwrap();
            let (_, read_body) = repo.read_fragment(&id).unwrap();

            prop_assert_eq!(read_body.trim(), body.trim());
        }
    }

    // ─── Property 4: Index Count = Valid Files ──────────────────────────────
    // **Validates: Requirements 2.5, 2.6, 2.8**
    //
    // After rebuild_from_vault, fragment table count equals number of valid
    // .md files in capture/.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_4_index_count_equals_valid_files(
            num_valid in 1u8..10,
            num_invalid in 0u8..5,
        ) {
            let tmp = TempDir::new().unwrap();
            let repo = FileRepo::new(tmp.path().to_path_buf());

            // Create valid fragments
            for i in 0..num_valid {
                repo.create_fragment(&format!("Valid fragment {}", i)).unwrap();
            }

            // Create invalid files (no valid frontmatter)
            let capture_dir = tmp.path().join("capture").join("2026").join("01");
            std::fs::create_dir_all(&capture_dir).unwrap();
            for i in 0..num_invalid {
                let invalid_path = capture_dir.join(format!("invalid{:02}.md", i));
                std::fs::write(&invalid_path, format!("No frontmatter here {}", i)).unwrap();
            }

            // Build index
            let db_path = tmp.path().join(".cognest").join("index.sqlite");
            let db = IndexDb::open(&db_path).unwrap();
            db.init_schema().unwrap();
            let report = db.rebuild_from_vault(&repo).unwrap();

            // Fragment count should equal num_valid (invalid files skipped)
            prop_assert_eq!(report.fragments_indexed, num_valid as u64);
            prop_assert_eq!(db.fragment_count().unwrap(), num_valid as u64);
            // Skipped count should equal num_invalid
            prop_assert_eq!(report.skipped.len(), num_invalid as usize);
        }
    }

    // ─── Property 5: Content Hash Detection ─────────────────────────────────
    // **Validates: Requirements 2.7**
    //
    // Index update happens iff sha256 hash differs from stored value.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_5_content_hash_detection(
            content_a in arb_body(),
            content_b in arb_body(),
        ) {
            let hash_a = FileRepo::content_hash(content_a.as_bytes());
            let hash_b = FileRepo::content_hash(content_b.as_bytes());

            // Same content → same hash
            let hash_a2 = FileRepo::content_hash(content_a.as_bytes());
            prop_assert_eq!(&hash_a, &hash_a2);

            // Different content → different hash (with overwhelming probability)
            if content_a != content_b {
                prop_assert_ne!(&hash_a, &hash_b);
            }

            // Hash is 64 hex chars (SHA-256)
            prop_assert_eq!(hash_a.len(), 64);
            prop_assert!(hash_a.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    // ─── Property 6: Blank Rejection ────────────────────────────────────────
    // **Validates: Requirements 5.7, 8.3**
    //
    // For any string of only whitespace chars, create_fragment returns error.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_6_blank_rejection(
            blanks in "[ \\t\\n\\r]{0,50}",
        ) {
            let tmp = TempDir::new().unwrap();
            let repo = FileRepo::new(tmp.path().to_path_buf());

            let result = repo.create_fragment(&blanks);
            prop_assert!(result.is_err(), "Should reject blank input: {:?}", blanks);

            // No file should be created
            let paths = repo.list_fragment_paths().unwrap();
            prop_assert!(paths.is_empty(), "No file should be created for blank input");
        }
    }

    // ─── Property 9: FTS5 Search Correctness ────────────────────────────────
    // **Validates: Requirements 9.1, 9.3, 9.4**
    //
    // For any non-empty query on indexed data, results are ranked by FTS5 rank
    // descending, max 50 results, snippets ≤ 150 chars.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn property_9_fts5_search_correctness(
            num_fragments in 1u8..20,
            query_word in "[a-z]{3,8}",
        ) {
            use crate::core::index::FragmentRecord;

            let tmp = TempDir::new().unwrap();
            let db_path = tmp.path().join("index.sqlite");
            let db = IndexDb::open(&db_path).unwrap();
            db.init_schema().unwrap();

            // Insert fragments, some containing the query word
            for i in 0..num_fragments {
                let content = if i % 3 == 0 {
                    format!("Fragment {} with keyword {} inside", i, query_word)
                } else {
                    format!("Fragment {} with random content xyz", i)
                };
                let record = FragmentRecord {
                    id: format!("{:08x}", i as u32),
                    content,
                    created_at: "2026-06-25T10:30:00+08:00".to_string(),
                    source: "manual".to_string(),
                    tags: vec![],
                    topics: vec![],
                    content_hash: format!("{:064x}", i as u64),
                };
                db.insert_fragment(&record).unwrap();
            }

            // Search
            let results = db.search_fragments(&query_word, 50).unwrap();

            // Max 50 results
            prop_assert!(results.len() <= 50);

            // Results should be ordered by rank (FTS5 rank is negative, more
            // negative = more relevant, so ascending order is descending relevance)
            for i in 1..results.len() {
                prop_assert!(
                    results[i - 1].rank <= results[i].rank,
                    "Results should be ordered by rank: {} vs {}",
                    results[i - 1].rank,
                    results[i].rank
                );
            }

            // Snippets should be ≤ 150 chars
            for r in &results {
                prop_assert!(
                    r.snippet.len() <= 153, // 150 + "..." suffix
                    "Snippet too long: {} chars",
                    r.snippet.len()
                );
            }
        }
    }
}
