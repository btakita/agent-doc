//! Pure queue/orchestration item classification.
//!
//! Dispatch effects live in the CLI shell. This module only decides whether a
//! text item is a prompt or a slash command and carries the parsed command
//! fields needed by the dispatcher.

/// Classification of a queue/orchestration item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemKind {
    Prompt,
    Command,
}

/// A classified queue item with its raw text and parsed components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub kind: QueueItemKind,
    pub raw: String,
    /// For commands: the command name without leading `/` (e.g., "clear", "model").
    /// For prompts: `None`.
    pub command: Option<String>,
    /// For commands: arguments after the command name. Empty for prompts.
    pub args: Vec<String>,
}

/// Inline command variants whose effects are implemented by the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineDispatchCommand {
    Model,
    Compact,
}

/// Classify a text item as a prompt or command.
pub fn classify(text: &str) -> QueueItem {
    let trimmed = text.trim();
    if let Some(command) = crate::queue_command::classify(trimmed) {
        return QueueItem {
            kind: QueueItemKind::Command,
            raw: command.raw,
            command: Some(command.name),
            args: command.args,
        };
    }
    if let Some(without_slash) = trimmed.strip_prefix('/') {
        let mut parts = without_slash.split_whitespace();
        let command = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(String::from).collect();
        QueueItem {
            kind: QueueItemKind::Command,
            raw: trimmed.to_string(),
            command: Some(command),
            args,
        }
    } else {
        QueueItem {
            kind: QueueItemKind::Prompt,
            raw: trimmed.to_string(),
            command: None,
            args: Vec::new(),
        }
    }
}

/// Classify commands that can be executed inline without a harness session.
pub fn inline_dispatch_command(command: &str) -> Option<InlineDispatchCommand> {
    match command {
        "model" => Some(InlineDispatchCommand::Model),
        "compact" => Some(InlineDispatchCommand::Compact),
        _ => None,
    }
}

/// Return whether a command must use the guarded session clear path.
pub fn is_session_clear_command(command: &str) -> bool {
    command == "clear"
}

/// Sanitize a progress log field so it stays single-token and redaction-safe.
pub fn sanitize_progress_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '%' | '=') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Redacted, stable fingerprint for a queue dispatch item.
pub fn item_fingerprint(item: &QueueItem) -> String {
    format!(
        "command={} bytes={} sha256={}",
        sanitize_progress_field(item.command.as_deref().unwrap_or("prompt")),
        item.raw.len(),
        agent_doc_hash::content_hash(&item.raw)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_prompt_item() {
        let item = classify("do #fix1");
        assert_eq!(item.kind, QueueItemKind::Prompt);
        assert_eq!(item.raw, "do #fix1");
        assert!(item.command.is_none());
        assert!(item.args.is_empty());
    }

    #[test]
    fn classify_command_item() {
        let item = classify("/clear");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.raw, "/clear");
        assert_eq!(item.command.as_deref(), Some("clear"));
        assert!(item.args.is_empty());
    }

    #[test]
    fn classify_command_with_args() {
        let item = classify("/model sonnet");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some("model"));
        assert_eq!(item.args, vec!["sonnet"]);
    }

    #[test]
    fn classify_command_with_multiple_args() {
        let item = classify("/compact tasks/agent-doc/agent-doc-bugs.md");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some("compact"));
        assert_eq!(item.args, vec!["tasks/agent-doc/agent-doc-bugs.md"]);
    }

    #[test]
    fn classify_trims_whitespace() {
        let item = classify("  /clear  ");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.raw, "/clear");
    }

    #[test]
    fn classify_empty_slash() {
        let item = classify("/");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some(""));
    }

    #[test]
    fn classify_review_prompt() {
        let item = classify("Review the pending items");
        assert_eq!(item.kind, QueueItemKind::Prompt);
    }

    #[test]
    fn inline_dispatch_command_classifies_inline_only_commands() {
        assert_eq!(
            inline_dispatch_command("model"),
            Some(InlineDispatchCommand::Model)
        );
        assert_eq!(
            inline_dispatch_command("compact"),
            Some(InlineDispatchCommand::Compact)
        );
        assert_eq!(inline_dispatch_command("clear"), None);
        assert_eq!(inline_dispatch_command("doctor"), None);
    }

    #[test]
    fn is_session_clear_command_matches_only_clear() {
        assert!(is_session_clear_command("clear"));
        assert!(!is_session_clear_command("model"));
    }

    #[test]
    fn sanitize_progress_field_keeps_safe_symbols_and_masks_unsafe_text() {
        assert_eq!(
            sanitize_progress_field("socket:/tmp/doc pane:%1 target=a b\nc"),
            "socket:/tmp/doc_pane:%1_target=a_b_c"
        );
    }

    #[test]
    fn item_fingerprint_redacts_raw_command_text() {
        let item = classify("/doctor secret value");
        let fingerprint = item_fingerprint(&item);
        assert!(fingerprint.contains("command=doctor"));
        assert!(fingerprint.contains("bytes=20"));
        assert!(fingerprint.contains(&agent_doc_hash::content_hash("/doctor secret value")));
        assert!(!fingerprint.contains("secret value"));
    }
}
