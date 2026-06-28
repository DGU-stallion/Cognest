//! Property Test: 写作上下文注入约束
//!
//! **Property 9**: 对于任意文章内容字符串(0-10000字符)和任意相关碎片列表(带各种相似度)，
//! WritingRigAgent::build_prompt() SHALL:
//! - 截取文章上下文至最多 4000 字符
//! - 最多包含 5 条碎片
//! - 仅包含相似度 >= 0.5 的碎片
//!
//! **Validates: Requirements 3.4**

use proptest::prelude::*;
use proptest::collection::vec as prop_vec;

use app_lib::core::rig_agents::writing::{
    WritingRigAgent, CONTEXT_MAX_CHARS, CONTEXT_MAX_FRAGMENTS, CONTEXT_MIN_SIMILARITY,
};

// ─── Generators ─────────────────────────────────────────────────────────────

/// 生成随机文章内容 (0-10000 字符)
fn arb_article_content() -> impl Strategy<Value = String> {
    // 使用混合 ASCII + Unicode 字符生成策略
    prop::string::string_regex("[\\w\\s\\p{Han}]{0,10000}")
        .unwrap()
}

/// 生成单个碎片 (内容, 相似度)
fn arb_fragment() -> impl Strategy<Value = (String, f64)> {
    (
        prop::string::string_regex("[\\w\\s\\p{Han}]{1,500}").unwrap(),
        0.0..=1.0_f64,
    )
}

/// 生成碎片列表 (0-20 条)
fn arb_fragments() -> impl Strategy<Value = Vec<(String, f64)>> {
    prop_vec(arb_fragment(), 0..20)
}

/// 生成用户消息 (1-200 字符)
fn arb_message() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\\w\\s\\p{Han}]{1,200}").unwrap()
}

// ─── Property 9: 写作上下文注入约束 ─────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 3.4**
    ///
    /// Property 9: 写作上下文注入约束
    /// For any article content string (0-10000 chars) and any list of related
    /// fragments with various similarities, WritingRigAgent::build_prompt() SHALL:
    /// - Truncate article context to max 4000 chars
    /// - Include at most 5 fragments
    /// - Only include fragments with similarity >= 0.5
    #[test]
    fn prop_writing_context_injection_constraints(
        article in arb_article_content(),
        fragments in arb_fragments(),
        message in arb_message(),
    ) {
        let prompt = WritingRigAgent::build_prompt(&article, &fragments, &message);

        // ─── Constraint 1: 文章上下文不超过 4000 字符 ───────────────────
        // If article is non-empty, the context section should contain at most
        // CONTEXT_MAX_CHARS characters of the article.
        if !article.is_empty() {
            // Extract the article context section from the prompt
            if let Some(context_start) = prompt.find("【当前文章上下文】\n") {
                let after_header = &prompt[context_start + "【当前文章上下文】\n".len()..];
                // Context section ends at "\n\n"
                let context_section = after_header.split("\n\n").next().unwrap_or("");
                let context_char_count = context_section.chars().count();
                prop_assert!(
                    context_char_count <= CONTEXT_MAX_CHARS,
                    "Article context has {} chars, exceeds max {}",
                    context_char_count,
                    CONTEXT_MAX_CHARS
                );
            }
        }

        // ─── Constraint 2: 最多注入 5 条碎片 ────────────────────────────
        // Count how many "碎片 N" entries appear in the prompt
        let fragment_count = (1..=20)
            .filter(|i| prompt.contains(&format!("碎片 {} (相似度:", i)))
            .count();
        prop_assert!(
            fragment_count <= CONTEXT_MAX_FRAGMENTS,
            "Prompt contains {} fragments, exceeds max {}",
            fragment_count,
            CONTEXT_MAX_FRAGMENTS
        );

        // ─── Constraint 3: 仅包含相似度 >= 0.5 的碎片 ───────────────────
        // Verify that no fragment with similarity < 0.5 appears in the prompt
        for (content, similarity) in &fragments {
            if *similarity < CONTEXT_MIN_SIMILARITY {
                // Fragments with low similarity should NOT appear in the prompt
                // We check by looking for the fragment content in the 相关知识碎片 section
                if let Some(frag_section_start) = prompt.find("【相关知识碎片】\n") {
                    let frag_section = &prompt[frag_section_start..];
                    // Only check if the content is non-trivial (avoid false positives
                    // with very short strings that could appear elsewhere)
                    if content.len() > 10 {
                        prop_assert!(
                            !frag_section.contains(content.as_str()),
                            "Fragment with similarity {:.2} (< {}) was included in prompt",
                            similarity,
                            CONTEXT_MIN_SIMILARITY
                        );
                    }
                }
            }
        }

        // ─── Additional: 所有包含的碎片确实满足 >= 0.5 约束 ──────────────
        // Verify included fragments have similarity >= 0.5
        let included_fragments: Vec<&(String, f64)> = fragments
            .iter()
            .filter(|(_, sim)| *sim >= CONTEXT_MIN_SIMILARITY)
            .take(CONTEXT_MAX_FRAGMENTS)
            .collect();

        // The number of fragments in prompt should match the filtered count
        prop_assert!(
            fragment_count <= included_fragments.len(),
            "Fragment count in prompt ({}) exceeds expected filtered count ({})",
            fragment_count,
            included_fragments.len()
        );
    }
}
