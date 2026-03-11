//! Streaming agent backend — iterates over agent output chunks.
//!
//! Claude Code supports `--output-format stream-json --include-partial-messages`
//! which emits one JSON object per line as output is generated.

use anyhow::Result;

/// A chunk of streaming agent output.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// The text content of this chunk (incremental or cumulative).
    pub text: String,
    /// Chain-of-thought (thinking) content, if present.
    pub thinking: Option<String>,
    /// True when this is the final chunk (response complete).
    pub is_final: bool,
    /// Session ID (only present on the final message).
    pub session_id: Option<String>,
}

/// Trait for agent backends that support streaming output.
pub trait StreamingAgent {
    /// Send a prompt and return an iterator over response chunks.
    fn send_streaming(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamChunk>>>>;
}

/// Parse a single stream-json line from Claude Code output.
///
/// Claude Code stream-json format emits lines like:
/// ```json
/// {"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]},"session_id":"..."}
/// {"type":"result","result":"full text","session_id":"abc-123"}
/// ```
pub fn parse_stream_line(line: &str) -> Result<StreamChunk> {
    let json: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| anyhow::anyhow!("failed to parse stream JSON: {}: {}", e, line))?;

    let msg_type = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match msg_type {
        "result" => {
            let text = json
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let session_id = json
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(StreamChunk {
                text,
                thinking: None,
                is_final: true,
                session_id,
            })
        }
        "assistant" => {
            // Extract text and thinking from content blocks
            let (text, thinking) = extract_assistant_content(&json);
            let session_id = json
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(StreamChunk {
                text,
                thinking,
                is_final: false,
                session_id,
            })
        }
        _ => {
            // Other message types (system, tool_use, etc.) — return empty chunk
            Ok(StreamChunk {
                text: String::new(),
                thinking: None,
                is_final: false,
                session_id: None,
            })
        }
    }
}

/// Extract text and thinking content from an assistant message's content blocks.
/// Returns (text, Option<thinking>).
fn extract_assistant_content(json: &serde_json::Value) -> (String, Option<String>) {
    let mut text = String::new();
    let mut thinking = String::new();
    if let Some(message) = json.get("message")
        && let Some(content) = message.get("content").and_then(|c| c.as_array())
    {
        for block in content {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
                "thinking" => {
                    if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                        thinking.push_str(t);
                    }
                }
                _ => {}
            }
        }
    }
    let thinking = if thinking.is_empty() {
        None
    } else {
        Some(thinking)
    };
    (text, thinking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_result_line() {
        let line = r#"{"type":"result","result":"Hello, world!","session_id":"abc-123"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Hello, world!");
        assert!(chunk.thinking.is_none());
        assert!(chunk.is_final);
        assert_eq!(chunk.session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn parse_assistant_line() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Partial output"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Partial output");
        assert!(chunk.thinking.is_none());
        assert!(!chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_unknown_type() {
        let line = r#"{"type":"system","message":"starting"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_malformed_json_errors() {
        let result = parse_stream_line("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(chunk.thinking.is_none());
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_multiple_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello "},{"type":"text","text":"world"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Hello world");
    }

    #[test]
    fn parse_result_with_no_session_id() {
        let line = r#"{"type":"result","result":"Done"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert!(chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_thinking_block() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me reason about this..."},{"type":"text","text":"Here is the answer."}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Here is the answer.");
        assert_eq!(chunk.thinking.as_deref(), Some("Let me reason about this..."));
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_thinking_only_no_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Reasoning..."}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert_eq!(chunk.thinking.as_deref(), Some("Reasoning..."));
    }

    #[test]
    fn parse_no_thinking_returns_none() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Just text"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert!(chunk.thinking.is_none());
    }
}
