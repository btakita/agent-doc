//! Pure agent streaming output parsing.
//!
//! This module owns the shared chunk vocabulary plus the line parsers for
//! agent CLI streaming protocols. It deliberately contains no process,
//! terminal, document, or orchestration side effects.

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
    /// Session ID, when the agent protocol provides one.
    pub session_id: Option<String>,
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

    let msg_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

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
        _ => Ok(StreamChunk {
            text: String::new(),
            thinking: None,
            is_final: false,
            session_id: None,
        }),
    }
}

/// Parse a single JSONL line from Codex output into a stream chunk.
pub fn parse_codex_line(line: &str) -> Result<StreamChunk> {
    let json: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| anyhow::anyhow!("failed to parse Codex JSONL: {}: {}", e, line))?;

    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "thread.started" => {
            let session_id = json
                .get("thread_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(StreamChunk {
                text: String::new(),
                thinking: None,
                is_final: false,
                session_id,
            })
        }
        "item.completed" => {
            let item = json.get("item");
            let item_type = item
                .and_then(|i| i.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if item_type == "agent_message" {
                let text = item
                    .and_then(|i| i.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(StreamChunk {
                    text,
                    thinking: None,
                    is_final: false,
                    session_id: None,
                })
            } else {
                Ok(StreamChunk {
                    text: String::new(),
                    thinking: None,
                    is_final: false,
                    session_id: None,
                })
            }
        }
        "turn.completed" => Ok(StreamChunk {
            text: String::new(),
            thinking: None,
            is_final: true,
            session_id: None,
        }),
        _ => Ok(StreamChunk {
            text: String::new(),
            thinking: None,
            is_final: false,
            session_id: None,
        }),
    }
}

/// Extract text and thinking content from an assistant message's content blocks.
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
    fn parse_claude_result_line() {
        let line = r#"{"type":"result","result":"Hello, world!","session_id":"abc-123"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Hello, world!");
        assert!(chunk.thinking.is_none());
        assert!(chunk.is_final);
        assert_eq!(chunk.session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn parse_claude_assistant_line() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Partial output"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Partial output");
        assert!(chunk.thinking.is_none());
        assert!(!chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_claude_unknown_type() {
        let line = r#"{"type":"system","message":"starting"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_claude_malformed_json_errors() {
        let result = parse_stream_line("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_claude_empty_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(chunk.thinking.is_none());
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_claude_multiple_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello "},{"type":"text","text":"world"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Hello world");
    }

    #[test]
    fn parse_claude_result_with_no_session_id() {
        let line = r#"{"type":"result","result":"Done"}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert!(chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_claude_thinking_block() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me reason about this..."},{"type":"text","text":"Here is the answer."}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "Here is the answer.");
        assert_eq!(
            chunk.thinking.as_deref(),
            Some("Let me reason about this...")
        );
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_claude_thinking_only_no_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Reasoning..."}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert_eq!(chunk.thinking.as_deref(), Some("Reasoning..."));
    }

    #[test]
    fn parse_claude_no_thinking_returns_none() {
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Just text"}]}}"#;
        let chunk = parse_stream_line(line).unwrap();
        assert!(chunk.thinking.is_none());
    }

    #[test]
    fn parse_codex_thread_started() {
        let line =
            r#"{"type":"thread.started","thread_id":"019db613-e57b-77d2-844c-9e7dca83ad01"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
        assert_eq!(
            chunk.session_id.as_deref(),
            Some("019db613-e57b-77d2-844c-9e7dca83ad01")
        );
    }

    #[test]
    fn parse_codex_agent_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello world"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "hello world");
        assert!(!chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_codex_command_execution() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"foo\n","exit_code":0,"status":"completed"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_codex_turn_completed() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":20}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_codex_turn_started() {
        let line = r#"{"type":"turn.started"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_codex_item_started() {
        let line = r#"{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_codex_unknown_event() {
        let line = r#"{"type":"some.future.event","data":42}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_codex_malformed_json() {
        let result = parse_codex_line("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_codex_agent_message_missing_text() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }
}
