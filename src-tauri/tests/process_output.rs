//! Property-Based Test: CLI Agent 输出转发事件序列化
//!
//! **Property 13: 进程输出事件忠实转发**
//! 验证每行 stdout/stderr 对应一个 Tauri event (AgentOutputEvent::Line) 且内容匹配；
//! 进程退出时发送包含实际退出状态码的 Exit 事件。
//!
//! 测试策略：由于实际转发依赖 Tauri AppHandle，此处验证 AgentOutputEvent 序列化格式
//! 的正确性——确保每个事件变体序列化后包含原始内容不变、stream 标识正确、exit code 完整。
//!
//! **Validates: Requirements 11.3, 11.8**

use app_lib::core::cli_agents::process_manager::AgentOutputEvent;
use proptest::prelude::*;

// ─── Generators ─────────────────────────────────────────────────────────────

/// Generate random line content including unicode, empty strings, and ANSI escape codes
fn gen_line_content() -> impl Strategy<Value = String> {
    prop_oneof![
        // Normal ASCII text
        prop::string::string_regex("[a-zA-Z0-9 \\t!@#$%^&*()]{0,200}").unwrap(),
        // Unicode content (CJK, emoji, accented chars)
        prop::string::string_regex("[\\p{Han}\\p{Hiragana}\\p{Latin}\\p{Emoji}]{0,100}").unwrap(),
        // Empty string
        Just(String::new()),
        // ANSI escape codes (color sequences)
        prop::string::string_regex("\\x1b\\[[0-9;]*m[a-zA-Z ]{0,50}\\x1b\\[0m").unwrap(),
        // Mixed content with special chars
        prop::string::string_regex("[\\x20-\\x7e\\u{4e00}-\\u{9fff}]{0,150}").unwrap(),
    ]
}

/// Generate stream identifier — only "stdout" or "stderr"
fn gen_stream() -> impl Strategy<Value = String> {
    prop_oneof![Just("stdout".to_string()), Just("stderr".to_string()),]
}

/// Generate exit codes (including negative values for signals)
fn gen_exit_code() -> impl Strategy<Value = i32> {
    prop_oneof![
        // Common exit codes
        Just(0i32),
        Just(1i32),
        Just(2i32),
        Just(127i32),
        Just(130i32),
        Just(137i32),
        Just(143i32),
        // Signal-killed processes (negative on some systems)
        Just(-1i32),
        // Arbitrary exit codes
        -128..128i32,
    ]
}

/// Generate duration in seconds
fn gen_duration_secs() -> impl Strategy<Value = u64> {
    0..u64::MAX
}

// ─── Property 13: 进程输出事件忠实转发 ──────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// **Validates: Requirements 11.3, 11.8**
    ///
    /// Property 13: AgentOutputEvent::Line serializes with correct structure —
    /// contains "type":"Line", the exact original content, and correct stream identifier.
    #[test]
    fn prop_line_event_preserves_content(
        content in gen_line_content(),
        stream in gen_stream(),
    ) {
        let event = AgentOutputEvent::Line {
            content: content.clone(),
            stream: stream.clone(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "Line"
        prop_assert_eq!(
            parsed["type"].as_str().unwrap(),
            "Line",
            "Expected type tag 'Line', got: {:?}",
            parsed["type"]
        );

        // Content must be preserved exactly (no truncation, no modification)
        prop_assert_eq!(
            parsed["content"].as_str().unwrap(),
            &content,
            "Content was not preserved in serialization"
        );

        // Stream identifier must match
        prop_assert_eq!(
            parsed["stream"].as_str().unwrap(),
            &stream,
            "Stream identifier mismatch"
        );

        // Must have exactly 3 top-level keys: type, content, stream
        prop_assert_eq!(
            parsed.as_object().unwrap().len(),
            3,
            "Line event should have exactly 3 fields, got: {:?}",
            parsed.as_object().unwrap().keys().collect::<Vec<_>>()
        );
    }

    /// **Validates: Requirements 11.3, 11.8**
    ///
    /// Property 13: AgentOutputEvent::Exit serializes with correct structure —
    /// contains "type":"Exit", the exit code, and duration_secs.
    #[test]
    fn prop_exit_event_contains_code_and_duration(
        code in gen_exit_code(),
        duration_secs in gen_duration_secs(),
    ) {
        let event = AgentOutputEvent::Exit { code, duration_secs };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "Exit"
        prop_assert_eq!(
            parsed["type"].as_str().unwrap(),
            "Exit",
            "Expected type tag 'Exit', got: {:?}",
            parsed["type"]
        );

        // Exit code must match exactly
        prop_assert_eq!(
            parsed["code"].as_i64().unwrap(),
            code as i64,
            "Exit code mismatch"
        );

        // Duration must match exactly
        prop_assert_eq!(
            parsed["duration_secs"].as_u64().unwrap(),
            duration_secs,
            "Duration mismatch"
        );

        // Must have exactly 3 top-level keys: type, code, duration_secs
        prop_assert_eq!(
            parsed.as_object().unwrap().len(),
            3,
            "Exit event should have exactly 3 fields, got: {:?}",
            parsed.as_object().unwrap().keys().collect::<Vec<_>>()
        );
    }

    /// **Validates: Requirements 11.3, 11.8**
    ///
    /// Property 13: AgentOutputEvent::Error serializes with correct structure —
    /// contains "type":"Error" and a reason string.
    #[test]
    fn prop_error_event_contains_reason(reason in gen_line_content()) {
        let event = AgentOutputEvent::Error { reason: reason.clone() };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Must have "type" field with value "Error"
        prop_assert_eq!(
            parsed["type"].as_str().unwrap(),
            "Error",
            "Expected type tag 'Error', got: {:?}",
            parsed["type"]
        );

        // Reason must be preserved exactly
        prop_assert_eq!(
            parsed["reason"].as_str().unwrap(),
            &reason,
            "Error reason was not preserved"
        );

        // Must have exactly 2 top-level keys: type, reason
        prop_assert_eq!(
            parsed.as_object().unwrap().len(),
            2,
            "Error event should have exactly 2 fields, got: {:?}",
            parsed.as_object().unwrap().keys().collect::<Vec<_>>()
        );
    }

    /// **Validates: Requirements 11.3, 11.8**
    ///
    /// Property 13 (supplemental): Line events for stdout vs stderr produce distinct
    /// serialization with correct stream identifiers.
    #[test]
    fn prop_stdout_stderr_events_are_distinguishable(content in gen_line_content()) {
        let stdout_event = AgentOutputEvent::Line {
            content: content.clone(),
            stream: "stdout".to_string(),
        };
        let stderr_event = AgentOutputEvent::Line {
            content: content.clone(),
            stream: "stderr".to_string(),
        };

        let stdout_json = serde_json::to_string(&stdout_event).unwrap();
        let stderr_json = serde_json::to_string(&stderr_event).unwrap();

        let stdout_parsed: serde_json::Value = serde_json::from_str(&stdout_json).unwrap();
        let stderr_parsed: serde_json::Value = serde_json::from_str(&stderr_json).unwrap();

        // Both have same content
        prop_assert_eq!(
            stdout_parsed["content"].as_str().unwrap(),
            stderr_parsed["content"].as_str().unwrap(),
            "Content should be identical for both streams"
        );

        // Stream identifiers are different
        prop_assert_eq!(stdout_parsed["stream"].as_str().unwrap(), "stdout");
        prop_assert_eq!(stderr_parsed["stream"].as_str().unwrap(), "stderr");

        // JSON strings are different (unless content is empty, stream differs)
        prop_assert_ne!(
            stdout_json,
            stderr_json,
            "stdout and stderr events should serialize differently"
        );
    }

    /// **Validates: Requirements 11.3, 11.8**
    ///
    /// Property 13 (supplemental): The "type" tag for all AgentOutputEvent variants
    /// is always one of the expected values — Line, Exit, or Error.
    #[test]
    fn prop_type_tag_is_valid_variant(
        content in gen_line_content(),
        stream in gen_stream(),
        code in gen_exit_code(),
        duration_secs in gen_duration_secs(),
    ) {
        let events = vec![
            AgentOutputEvent::Line { content, stream },
            AgentOutputEvent::Exit { code, duration_secs },
            AgentOutputEvent::Error { reason: "test error".to_string() },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            let type_val = parsed["type"].as_str().unwrap();
            prop_assert!(
                type_val == "Line" || type_val == "Exit" || type_val == "Error",
                "Unexpected type tag: {}",
                type_val
            );
        }
    }
}
