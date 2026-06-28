//! Property-Based Test: StreamChunk 格式兼容性
//!
//! **Property 4**: For any StreamChunk enum value (Delta/Done/Error), its serde
//! serialization SHALL satisfy:
//! - Delta: {"type":"delta","content":"..."}
//! - Done: {"type":"done","usage":{"prompt_tokens":N,"completion_tokens":N,"total_tokens":N}}
//! - Error: {"type":"error","error":{...},"partial_tokens":N}
//! Tag field is "type", values are lowercase variant names.
//!
//! **Validates: Requirements 3.2, 5.5, 5.6**

use app_lib::core::rig_agents::types::{LlmError, StreamChunk, TokenUsage};
use proptest::prelude::*;

// ─── Generators ─────────────────────────────────────────────────────────────

/// Generate arbitrary UTF-8 content strings (including empty and unicode)
fn gen_content() -> impl Strategy<Value = String> {
    prop::string::string_regex(".{0,200}").unwrap()
}

/// Generate arbitrary token count values
fn gen_token_count() -> impl Strategy<Value = u32> {
    0..u32::MAX
}

/// Generate a valid TokenUsage
fn gen_token_usage() -> impl Strategy<Value = TokenUsage> {
    (gen_token_count(), gen_token_count(), gen_token_count()).prop_map(
        |(prompt_tokens, completion_tokens, total_tokens)| TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        },
    )
}

/// Generate a provider name string
fn gen_provider() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9_]{0,15}").unwrap()
}

/// Generate a reason string
fn gen_reason() -> impl Strategy<Value = String> {
    prop::string::string_regex(".{0,100}").unwrap()
}

/// Generate an LlmError variant
fn gen_llm_error() -> impl Strategy<Value = LlmError> {
    prop_oneof![
        gen_provider().prop_map(|provider| LlmError::Timeout { provider }),
        gen_provider().prop_map(|provider| LlmError::RateLimit { provider }),
        gen_provider().prop_map(|provider| LlmError::AuthFailure { provider }),
        (gen_provider(), gen_reason())
            .prop_map(|(provider, reason)| LlmError::NetworkError { provider, reason }),
        Just(LlmError::NoProvider),
        gen_reason().prop_map(|details| LlmError::SchemaValidation { details }),
        (gen_provider(), gen_reason())
            .prop_map(|(provider, reason)| LlmError::Unknown { provider, reason }),
    ]
}

/// Generate any StreamChunk variant
fn gen_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        gen_content().prop_map(|content| StreamChunk::Delta { content }),
        gen_token_usage().prop_map(|usage| StreamChunk::Done { usage }),
        (gen_llm_error(), gen_token_count())
            .prop_map(|(error, partial_tokens)| StreamChunk::Error {
                error,
                partial_tokens
            }),
    ]
}

// ─── Property 4: StreamChunk 格式兼容性 ─────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 3.2, 5.5, 5.6**
    ///
    /// Property 4: StreamChunk Delta variant — serializes to {"type":"delta","content":"..."}
    #[test]
    fn prop_delta_format(content in gen_content()) {
        let chunk = StreamChunk::Delta { content: content.clone() };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "delta"
        prop_assert_eq!(parsed["type"].as_str().unwrap(), "delta");
        // Must have "content" field matching the input
        prop_assert_eq!(parsed["content"].as_str().unwrap(), &content);
        // Must have exactly 2 top-level keys
        prop_assert_eq!(parsed.as_object().unwrap().len(), 2);
    }

    /// **Validates: Requirements 3.2, 5.5, 5.6**
    ///
    /// Property 4: StreamChunk Done variant — serializes to
    /// {"type":"done","usage":{"prompt_tokens":N,"completion_tokens":N,"total_tokens":N}}
    #[test]
    fn prop_done_format(usage in gen_token_usage()) {
        let chunk = StreamChunk::Done { usage: usage.clone() };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "done"
        prop_assert_eq!(parsed["type"].as_str().unwrap(), "done");
        // Must have "usage" object with exactly 3 fields
        let usage_obj = parsed["usage"].as_object().unwrap();
        prop_assert_eq!(usage_obj.len(), 3);
        prop_assert_eq!(usage_obj["prompt_tokens"].as_u64().unwrap(), usage.prompt_tokens as u64);
        prop_assert_eq!(usage_obj["completion_tokens"].as_u64().unwrap(), usage.completion_tokens as u64);
        prop_assert_eq!(usage_obj["total_tokens"].as_u64().unwrap(), usage.total_tokens as u64);
        // Must have exactly 2 top-level keys
        prop_assert_eq!(parsed.as_object().unwrap().len(), 2);
    }

    /// **Validates: Requirements 3.2, 5.5, 5.6**
    ///
    /// Property 4: StreamChunk Error variant — serializes to
    /// {"type":"error","error":{...},"partial_tokens":N}
    /// The "error" field contains the LlmError payload (object for struct variants,
    /// string for unit variants like NoProvider).
    #[test]
    fn prop_error_format(error in gen_llm_error(), partial_tokens in gen_token_count()) {
        let chunk = StreamChunk::Error { error, partial_tokens };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "error"
        prop_assert_eq!(parsed["type"].as_str().unwrap(), "error");
        // Must have "error" field present (object for struct variants, string for unit variants)
        prop_assert!(!parsed["error"].is_null(), "error field must be present");
        prop_assert!(
            parsed["error"].is_object() || parsed["error"].is_string(),
            "error field must be an object or string, got: {:?}", parsed["error"]
        );
        // Must have "partial_tokens" field as a number
        prop_assert_eq!(parsed["partial_tokens"].as_u64().unwrap(), partial_tokens as u64);
        // Must have exactly 3 top-level keys
        prop_assert_eq!(parsed.as_object().unwrap().len(), 3);
    }

    /// **Validates: Requirements 3.2, 5.5, 5.6**
    ///
    /// Property 4 (supplemental): All StreamChunk variants deserialize back correctly (roundtrip)
    #[test]
    fn prop_stream_chunk_roundtrip(chunk in gen_stream_chunk()) {
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: StreamChunk = serde_json::from_str(&json).unwrap();
        // Re-serialize to verify roundtrip stability
        let json2 = serde_json::to_string(&deserialized).unwrap();
        prop_assert_eq!(&json, &json2);
    }

    /// **Validates: Requirements 5.5, 5.6**
    ///
    /// Property 4 (supplemental): The "type" tag is always one of the three expected values
    #[test]
    fn prop_type_tag_is_valid(chunk in gen_stream_chunk()) {
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let type_val = parsed["type"].as_str().unwrap();
        prop_assert!(
            type_val == "delta" || type_val == "done" || type_val == "error",
            "Unexpected type tag: {}", type_val
        );
    }
}
