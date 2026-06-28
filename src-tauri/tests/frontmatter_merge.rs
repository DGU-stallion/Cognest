//! Property-Based Tests — Frontmatter 合并无重复
//!
//! **Property 11: Frontmatter 合并无重复**
//! 验证 topics 合并无重复、tags 1-5 个且每个 ≤ 10 字符
//!
//! **Validates: Requirements 4.4**

use app_lib::core::rig_agents::curator::{merge_tags, merge_topics, sanitize_tags};
use proptest::prelude::*;

// ─── Generators ─────────────────────────────────────────────────────────────

/// Generate a valid topic string (1-30 chars, alphanumeric + hyphen + Chinese)
fn gen_topic() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z\u{4e00}-\u{9fff}][a-zA-Z0-9\\-\u{4e00}-\u{9fff}]{0,29}")
        .unwrap()
        .prop_filter("non-empty topic", |s| !s.is_empty())
}

/// Generate a list of topics (0 to 10)
fn gen_topics_list() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(gen_topic(), 0..10)
}

/// Generate a tag string (1-20 chars, including chars that exceed 10-char limit)
fn gen_tag() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z\u{4e00}-\u{9fff}][a-zA-Z0-9\u{4e00}-\u{9fff}]{0,19}")
        .unwrap()
        .prop_filter("non-empty tag", |s| !s.trim().is_empty())
}

/// Generate a list of tags (0 to 15, allowing more than the 5-tag limit to test truncation)
fn gen_tags_list() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(gen_tag(), 0..15)
}

/// Generate a mixed tag list that may contain empty/whitespace strings
fn gen_mixed_tags_list() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop_oneof![
            gen_tag(),
            Just("".to_string()),
            Just("   ".to_string()),
            Just("\t".to_string()),
        ],
        0..15,
    )
}

// ─── Property 11.1: merge_topics() never produces duplicates ─────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: merge_topics() never produces duplicates.
    /// For any existing topics list and new topic, the merged result SHALL
    /// contain no duplicate entries.
    #[test]
    fn prop_merge_topics_no_duplicates(
        existing in gen_topics_list(),
        new_topic in gen_topic(),
    ) {
        let result = merge_topics(&existing, &new_topic);

        // Check no duplicates in result
        let mut seen = std::collections::HashSet::new();
        for topic in &result {
            prop_assert!(
                seen.insert(topic.clone()),
                "Duplicate topic found: '{}' in {:?}",
                topic,
                result
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11 (supplemental): merge_topics preserves all existing topics
    /// and adds the new topic if not already present.
    #[test]
    fn prop_merge_topics_preserves_existing(
        existing in gen_topics_list(),
        new_topic in gen_topic(),
    ) {
        let result = merge_topics(&existing, &new_topic);

        // All existing topics should still be present
        for topic in &existing {
            prop_assert!(
                result.contains(topic),
                "Existing topic '{}' was lost after merge",
                topic
            );
        }

        // New topic should be present (unless it's empty)
        if !new_topic.is_empty() {
            prop_assert!(
                result.contains(&new_topic),
                "New topic '{}' was not added to result",
                new_topic
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: merge_topics with empty new_topic does not modify the list.
    #[test]
    fn prop_merge_topics_empty_new_is_noop(
        existing in gen_topics_list(),
    ) {
        let result = merge_topics(&existing, "");
        prop_assert_eq!(&result, &existing);
    }
}

// ─── Property 11.2: merge_tags() never produces duplicates ───────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: merge_tags() never produces duplicates.
    /// For any existing tags list and new tags, the merged result SHALL
    /// contain no duplicate entries.
    #[test]
    fn prop_merge_tags_no_duplicates(
        existing in gen_tags_list(),
        new_tags in gen_tags_list(),
    ) {
        let result = merge_tags(&existing, &new_tags);

        // Check no duplicates in result
        let mut seen = std::collections::HashSet::new();
        for tag in &result {
            prop_assert!(
                seen.insert(tag.clone()),
                "Duplicate tag found: '{}' in {:?}",
                tag,
                result
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11 (supplemental): merge_tags preserves all existing tags
    /// and adds new non-empty, non-duplicate tags.
    #[test]
    fn prop_merge_tags_preserves_existing(
        existing in gen_tags_list(),
        new_tags in gen_tags_list(),
    ) {
        let result = merge_tags(&existing, &new_tags);

        // All existing tags should still be present
        for tag in &existing {
            prop_assert!(
                result.contains(tag),
                "Existing tag '{}' was lost after merge",
                tag
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: merge_tags ignores empty/whitespace-only new tags.
    #[test]
    fn prop_merge_tags_ignores_empty(
        existing in gen_tags_list(),
        new_tags in gen_mixed_tags_list(),
    ) {
        let result = merge_tags(&existing, &new_tags);

        // No empty or whitespace-only strings in result beyond what was in existing
        for tag in &result {
            if !existing.contains(tag) {
                prop_assert!(
                    !tag.trim().is_empty(),
                    "Empty/whitespace tag '{}' was added to result",
                    tag
                );
            }
        }
    }
}

// ─── Property 11.3: sanitize_tags() output has 0-5 tags, each ≤ 10 chars ────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: sanitize_tags() output has at most 5 tags.
    /// For any input tag list, the sanitized result SHALL contain
    /// between 0 and 5 tags (inclusive).
    #[test]
    fn prop_sanitize_tags_count_constraint(
        tags in gen_tags_list(),
    ) {
        let result = sanitize_tags(tags);
        prop_assert!(
            result.len() <= 5,
            "sanitize_tags returned {} tags, expected at most 5: {:?}",
            result.len(),
            result
        );
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: sanitize_tags() output — each tag is at most 10 characters.
    /// For any input tag list, every tag in the sanitized result SHALL
    /// have a character count ≤ 10.
    #[test]
    fn prop_sanitize_tags_char_length_constraint(
        tags in gen_tags_list(),
    ) {
        let result = sanitize_tags(tags);
        for tag in &result {
            let char_count = tag.chars().count();
            prop_assert!(
                char_count <= 10,
                "Tag '{}' has {} chars, expected at most 10",
                tag,
                char_count
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11: sanitize_tags() filters out empty/whitespace-only tags.
    /// No tag in the output SHALL be empty or whitespace-only.
    #[test]
    fn prop_sanitize_tags_no_empty(
        tags in gen_mixed_tags_list(),
    ) {
        let result = sanitize_tags(tags);
        for tag in &result {
            prop_assert!(
                !tag.trim().is_empty(),
                "Empty/whitespace tag found in sanitized result: '{}'",
                tag
            );
        }
    }

    /// **Validates: Requirements 4.4**
    ///
    /// Property 11 (combined): sanitize_tags satisfies ALL constraints simultaneously.
    /// For any input, the output SHALL have 0-5 non-empty tags, each ≤ 10 chars.
    #[test]
    fn prop_sanitize_tags_all_constraints(
        tags in gen_mixed_tags_list(),
    ) {
        let result = sanitize_tags(tags);

        // Count constraint: 0-5
        prop_assert!(result.len() <= 5, "Too many tags: {}", result.len());

        for tag in &result {
            // Non-empty constraint
            prop_assert!(!tag.trim().is_empty(), "Empty tag in result");

            // Char length constraint: ≤ 10
            let char_count = tag.chars().count();
            prop_assert!(
                char_count <= 10,
                "Tag '{}' exceeds 10 chars (has {})",
                tag,
                char_count
            );
        }
    }
}
