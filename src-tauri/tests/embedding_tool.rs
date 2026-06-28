//! Property 10: EmbeddingSearch Tool 输入输出约束
//!
//! 验证 EmbeddingSearch Tool 的核心辅助函数满足设计约束：
//! - `truncate_chars` 仅处理前 2000 字符
//! - `normalize_similarity` 始终产生 [0.0, 1.0] 范围内的值
//! - `MAX_RESULTS` 常量为 5
//!
//! **Validates: Requirements 4.3**

use app_lib::core::rig_agents::curator::{
    normalize_similarity, truncate_chars, MAX_QUERY_CHARS, MAX_RESULTS,
};
use proptest::prelude::*;

// ─── Generators ──────────────────────────────────────────────────────────────

/// 生成随机长度的查询文本（0-5000 字符，含 ASCII 和中文）
fn gen_query_text() -> impl Strategy<Value = String> {
    prop_oneof![
        // ASCII-only strings of varying length
        ".{0,5000}",
        // Mix including multibyte characters
        proptest::collection::vec(
            prop_oneof![
                // ASCII chars
                prop::char::range('\x20', '\x7e'),
                // CJK Unified Ideographs (common Chinese chars)
                prop::char::range('\u{4e00}', '\u{9fff}'),
            ],
            0..5000
        )
        .prop_map(|chars| chars.into_iter().collect::<String>()),
    ]
}

/// 生成任意 f32 值作为原始余弦相似度（包含边界和超范围值）
fn gen_raw_similarity() -> impl Strategy<Value = f32> {
    prop_oneof![
        // Normal range [-1.0, 1.0]
        (-1.0f32..=1.0f32),
        // Edge values
        Just(-1.0f32),
        Just(0.0f32),
        Just(1.0f32),
        // Out-of-range values (should still be clamped)
        (-10.0f32..=-1.0f32),
        (1.0f32..=10.0f32),
        // Special float values
        Just(f32::MIN),
        Just(f32::MAX),
    ]
}

// ─── Property 10: EmbeddingSearch Tool 输入输出约束 ──────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 4.3**
    ///
    /// Property 10a: truncate_chars 仅保留前 MAX_QUERY_CHARS (2000) 个字符
    ///
    /// For any query text, truncate_chars(text, MAX_QUERY_CHARS) SHALL produce
    /// a result with at most MAX_QUERY_CHARS characters.
    #[test]
    fn prop_truncate_chars_limits_to_max_query_chars(
        text in gen_query_text(),
    ) {
        let truncated = truncate_chars(&text, MAX_QUERY_CHARS);
        let char_count = truncated.chars().count();

        // Result must not exceed MAX_QUERY_CHARS
        prop_assert!(
            char_count <= MAX_QUERY_CHARS,
            "truncated text has {} chars, expected <= {}",
            char_count,
            MAX_QUERY_CHARS
        );

        // If original is within limit, result equals original
        let original_char_count = text.chars().count();
        if original_char_count <= MAX_QUERY_CHARS {
            prop_assert_eq!(truncated, text.as_str());
        } else {
            // If original exceeds limit, result is exactly MAX_QUERY_CHARS chars
            prop_assert_eq!(
                char_count,
                MAX_QUERY_CHARS,
                "truncated text has {} chars, expected exactly {} when original exceeds limit",
                char_count,
                MAX_QUERY_CHARS
            );
        }
    }

    /// **Validates: Requirements 4.3**
    ///
    /// Property 10b: truncate_chars 保留的是原始字符串的前缀
    ///
    /// The truncated result SHALL be a valid prefix of the original string.
    #[test]
    fn prop_truncate_chars_is_prefix(
        text in gen_query_text(),
    ) {
        let truncated = truncate_chars(&text, MAX_QUERY_CHARS);

        // Truncated result must be a prefix of the original
        prop_assert!(
            text.starts_with(truncated),
            "truncated '{}' is not a prefix of original",
            truncated
        );
    }

    /// **Validates: Requirements 4.3**
    ///
    /// Property 10c: normalize_similarity 始终产生 [0.0, 1.0] 范围内的值
    ///
    /// For any raw cosine similarity value, normalize_similarity SHALL produce
    /// a value in [0.0, 1.0].
    #[test]
    fn prop_normalize_similarity_in_unit_range(
        raw_sim in gen_raw_similarity(),
    ) {
        let normalized = normalize_similarity(raw_sim);

        prop_assert!(
            normalized >= 0.0,
            "normalize_similarity({}) = {} which is < 0.0",
            raw_sim,
            normalized
        );
        prop_assert!(
            normalized <= 1.0,
            "normalize_similarity({}) = {} which is > 1.0",
            raw_sim,
            normalized
        );
    }

    /// **Validates: Requirements 4.3**
    ///
    /// Property 10d: normalize_similarity 保持单调性
    ///
    /// For any two raw similarity values a < b, normalize_similarity(a) <= normalize_similarity(b).
    /// (Monotonicity ensures ordering is preserved after normalization)
    #[test]
    fn prop_normalize_similarity_monotone(
        a in -1.0f32..=1.0f32,
        b in -1.0f32..=1.0f32,
    ) {
        let norm_a = normalize_similarity(a);
        let norm_b = normalize_similarity(b);

        if a <= b {
            prop_assert!(
                norm_a <= norm_b,
                "Monotonicity violated: normalize_similarity({}) = {} > normalize_similarity({}) = {}",
                a, norm_a, b, norm_b
            );
        } else {
            prop_assert!(
                norm_a >= norm_b,
                "Monotonicity violated: normalize_similarity({}) = {} < normalize_similarity({}) = {}",
                a, norm_a, b, norm_b
            );
        }
    }
}

// ─── 常量验证 ─────────────────────────────────────────────────────────────────

/// Verify MAX_RESULTS constant is 5 as specified in requirements
#[test]
fn test_max_results_is_five() {
    assert_eq!(MAX_RESULTS, 5, "MAX_RESULTS must be 5 per Requirements 4.3");
}

/// Verify MAX_QUERY_CHARS constant is 2000 as specified in requirements
#[test]
fn test_max_query_chars_is_2000() {
    assert_eq!(
        MAX_QUERY_CHARS, 2000,
        "MAX_QUERY_CHARS must be 2000 per Requirements 4.3"
    );
}

/// Verify normalize_similarity known values
#[test]
fn test_normalize_similarity_known_values() {
    // cosine similarity -1.0 → normalized 0.0
    assert!((normalize_similarity(-1.0) - 0.0).abs() < f32::EPSILON);
    // cosine similarity 0.0 → normalized 0.5
    assert!((normalize_similarity(0.0) - 0.5).abs() < f32::EPSILON);
    // cosine similarity 1.0 → normalized 1.0
    assert!((normalize_similarity(1.0) - 1.0).abs() < f32::EPSILON);
}
