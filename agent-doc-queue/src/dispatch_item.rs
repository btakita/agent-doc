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
}
