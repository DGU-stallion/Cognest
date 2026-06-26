// Cognest Core — YAML Frontmatter parser and serializer
//
// Parses Markdown files with YAML frontmatter delimited by `---` lines.
// Supports generic deserialization via serde.

use serde::{de::DeserializeOwned, Serialize};

/// Parsed document containing frontmatter metadata and body content.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument<T> {
    pub meta: T,
    pub body: String,
}

/// Errors that can occur during frontmatter parsing or serialization.
#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("文件不包含合法的 Frontmatter 分隔符: {path} 第 {line} 行")]
    MissingDelimiter { path: String, line: usize },

    #[error("YAML 解析失败: {path} 第 {line} 行 - {reason}")]
    YamlParseError {
        path: String,
        line: usize,
        reason: String,
    },

    #[error("序列化失败: {0}")]
    SerializeError(String),
}

/// Parse a Markdown document's YAML frontmatter into a typed struct.
///
/// Rules:
/// - The first line must be exactly `---`
/// - The next line containing only `---` marks the end of the frontmatter block
/// - Content between delimiters is parsed as YAML using serde_yaml
/// - Everything after the closing delimiter is returned as the body
/// - If the first line after the closing delimiter is blank, it is skipped
pub fn parse<T: DeserializeOwned>(input: &str) -> Result<ParsedDocument<T>, FrontmatterError> {
    let lines: Vec<&str> = input.lines().collect();

    // Check opening delimiter
    if lines.is_empty() || lines[0].trim() != "---" {
        return Err(FrontmatterError::MissingDelimiter {
            path: String::new(),
            line: 1,
        });
    }

    // Find closing delimiter (starting from line index 1)
    let closing_index = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| line.trim() == "---")
        .map(|(i, _)| i);

    let closing_index = match closing_index {
        Some(idx) => idx,
        None => {
            return Err(FrontmatterError::MissingDelimiter {
                path: String::new(),
                line: lines.len(),
            });
        }
    };

    // Extract YAML content between delimiters
    let yaml_content = lines[1..closing_index].join("\n");

    // Parse YAML
    let meta: T = serde_yaml::from_str(&yaml_content).map_err(|e| {
        // Try to extract line number from serde_yaml error
        let line = e
            .location()
            .map(|loc| loc.line() + 2) // +2: 1 for 0-index, 1 for opening ---
            .unwrap_or(2);
        FrontmatterError::YamlParseError {
            path: String::new(),
            line,
            reason: e.to_string(),
        }
    })?;

    // Extract body: everything after closing delimiter
    let body_start = closing_index + 1;
    let body = if body_start < lines.len() {
        // Skip one blank line after closing delimiter if present
        let actual_start = if lines[body_start].is_empty() {
            body_start + 1
        } else {
            body_start
        };

        if actual_start < lines.len() {
            // Reconstruct body preserving original line endings
            let mut body_lines = lines[actual_start..].join("\n");
            // If the original input ended with a newline after the body, preserve it
            if input.ends_with('\n') {
                body_lines.push('\n');
            }
            body_lines
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(ParsedDocument { meta, body })
}

/// Serialize frontmatter metadata and body into a complete Markdown document.
///
/// Output format:
/// ```text
/// ---
/// {yaml fields}
/// ---
///
/// {body}
/// ```
pub fn serialize<T: Serialize>(meta: &T, body: &str) -> Result<String, FrontmatterError> {
    let yaml =
        serde_yaml::to_string(meta).map_err(|e| FrontmatterError::SerializeError(e.to_string()))?;

    // serde_yaml::to_string already ends with a newline
    let mut output = String::new();
    output.push_str("---\n");
    output.push_str(&yaml);
    output.push_str("---\n");
    output.push('\n');
    output.push_str(body);

    // Ensure the document ends with a newline
    if !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestMeta {
        id: String,
        title: String,
        tags: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct SimpleMeta {
        id: String,
        source: String,
    }

    // --- Round-trip tests ---

    #[test]
    fn test_roundtrip_basic() {
        let meta = TestMeta {
            id: "a1b2c3d4".to_string(),
            title: "Test Article".to_string(),
            tags: vec!["rust".to_string(), "testing".to_string()],
        };
        let body = "This is the body content.\n";

        let serialized = serialize(&meta, body).unwrap();
        let parsed: ParsedDocument<TestMeta> = parse(&serialized).unwrap();

        assert_eq!(parsed.meta, meta);
        assert_eq!(parsed.body.trim(), body.trim());
    }

    #[test]
    fn test_roundtrip_empty_tags() {
        let meta = TestMeta {
            id: "deadbeef".to_string(),
            title: "Empty Tags".to_string(),
            tags: vec![],
        };
        let body = "Body with empty tags.\n";

        let serialized = serialize(&meta, body).unwrap();
        let parsed: ParsedDocument<TestMeta> = parse(&serialized).unwrap();

        assert_eq!(parsed.meta, meta);
        assert_eq!(parsed.body.trim(), body.trim());
    }

    #[test]
    fn test_roundtrip_multiline_body() {
        let meta = SimpleMeta {
            id: "12345678".to_string(),
            source: "manual".to_string(),
        };
        let body = "Line one.\nLine two.\n\nLine four after blank.\n";

        let serialized = serialize(&meta, body).unwrap();
        let parsed: ParsedDocument<SimpleMeta> = parse(&serialized).unwrap();

        assert_eq!(parsed.meta, meta);
        assert_eq!(parsed.body.trim(), body.trim());
    }

    // --- Error handling tests ---

    #[test]
    fn test_missing_opening_delimiter() {
        let input = "no frontmatter here\njust plain text\n";
        let result = parse::<SimpleMeta>(input);

        assert!(result.is_err());
        match result.unwrap_err() {
            FrontmatterError::MissingDelimiter { line, .. } => {
                assert_eq!(line, 1);
            }
            other => panic!("Expected MissingDelimiter, got: {:?}", other),
        }
    }

    #[test]
    fn test_missing_closing_delimiter() {
        let input = "---\nid: abc\nsource: manual\n";
        let result = parse::<SimpleMeta>(input);

        assert!(result.is_err());
        match result.unwrap_err() {
            FrontmatterError::MissingDelimiter { .. } => {}
            other => panic!("Expected MissingDelimiter, got: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_yaml_content() {
        let input = "---\n: invalid: [yaml: content\n---\n\nBody text\n";
        let result = parse::<SimpleMeta>(input);

        assert!(result.is_err());
        match result.unwrap_err() {
            FrontmatterError::YamlParseError { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("Expected YamlParseError, got: {:?}", other),
        }
    }

    // --- Empty body handling ---

    #[test]
    fn test_empty_body() {
        let input = "---\nid: abc123\nsource: manual\n---\n";
        let parsed: ParsedDocument<SimpleMeta> = parse(input).unwrap();

        assert_eq!(parsed.meta.id, "abc123");
        assert_eq!(parsed.meta.source, "manual");
        assert_eq!(parsed.body, "");
    }

    #[test]
    fn test_empty_body_with_blank_line() {
        let input = "---\nid: abc123\nsource: manual\n---\n\n";
        let parsed: ParsedDocument<SimpleMeta> = parse(input).unwrap();

        assert_eq!(parsed.meta.id, "abc123");
        assert_eq!(parsed.body, "");
    }

    // --- Body with multiple --- inside ---

    #[test]
    fn test_body_with_triple_dashes() {
        let input = "---\nid: abc123\nsource: manual\n---\n\nSome body text\n---\nMore text after dashes\n";
        let parsed: ParsedDocument<SimpleMeta> = parse(input).unwrap();

        assert_eq!(parsed.meta.id, "abc123");
        assert_eq!(parsed.meta.source, "manual");
        // Body should contain the --- and text after it
        assert!(parsed.body.contains("---"));
        assert!(parsed.body.contains("Some body text"));
        assert!(parsed.body.contains("More text after dashes"));
    }

    #[test]
    fn test_body_with_multiple_triple_dashes() {
        let input =
            "---\nid: test01\nsource: manual\n---\n\nFirst section\n---\nSecond\n---\nThird\n";
        let parsed: ParsedDocument<SimpleMeta> = parse(input).unwrap();

        assert_eq!(parsed.meta.id, "test01");
        // Only the first closing delimiter should split frontmatter from body
        assert!(parsed.body.contains("First section"));
        assert!(parsed.body.contains("---"));
        assert!(parsed.body.contains("Second"));
        assert!(parsed.body.contains("Third"));
    }

    // --- Serialize error test ---

    #[test]
    fn test_serialize_produces_valid_format() {
        let meta = SimpleMeta {
            id: "aabbccdd".to_string(),
            source: "manual".to_string(),
        };
        let body = "Hello, world!";

        let result = serialize(&meta, body).unwrap();

        assert!(result.starts_with("---\n"));
        // Should contain the closing delimiter
        let after_first = &result[4..]; // skip "---\n"
        assert!(after_first.contains("\n---\n"));
        // Should end with newline
        assert!(result.ends_with('\n'));
    }
}
