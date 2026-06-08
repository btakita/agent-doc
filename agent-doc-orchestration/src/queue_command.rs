//! Slash-command classification for prompt-bearing queue/exchange items.
//!
//! Queue/exchange commands are literal harness commands: text like `/clear` or
//! `/model sonnet` must be submitted to the owner pane after the current turn
//! reaches an idle prompt, not answered as ordinary agent-doc work.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    pub raw: String,
    pub name: String,
    pub args: Vec<String>,
}

pub fn classify(text: &str) -> Option<SlashCommand> {
    let trimmed = text.trim();
    let without_slash = trimmed.strip_prefix('/')?;
    let mut parts = without_slash.split_whitespace();
    let name = parts.next()?.to_string();
    if name.is_empty() {
        return None;
    }
    Some(SlashCommand {
        raw: trimmed.to_string(),
        name,
        args: parts.map(String::from).collect(),
    })
}

pub fn slash_command_text(text: &str) -> Option<String> {
    classify(text).map(|command| command.raw)
}

pub fn is_slash_command(text: &str) -> bool {
    classify(text).is_some()
}

pub fn is_context_clear_command(text: &str) -> bool {
    let Some(command) = classify(text) else {
        return false;
    };
    command.args.is_empty() && matches!(command.name.as_str(), "clear" | "new")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_clear_command() {
        let command = classify("  /clear  ").expect("slash command");
        assert_eq!(command.raw, "/clear");
        assert_eq!(command.name, "clear");
        assert!(command.args.is_empty());
        assert!(is_context_clear_command("/clear"));
    }

    #[test]
    fn classify_command_with_args() {
        let command = classify("/model sonnet").expect("slash command");
        assert_eq!(command.raw, "/model sonnet");
        assert_eq!(command.name, "model");
        assert_eq!(command.args, vec!["sonnet"]);
        assert!(!is_context_clear_command("/model sonnet"));
    }

    #[test]
    fn ignores_plain_prompts_and_empty_slash() {
        assert!(classify("do #fix1").is_none());
        assert!(classify("/").is_none());
        assert!(classify(" / ").is_none());
    }

    #[test]
    fn context_clear_requires_exact_builtin_without_args() {
        assert!(is_context_clear_command("/new"));
        assert!(!is_context_clear_command("/clear please"));
        assert!(!is_context_clear_command("agent-doc tasks/foo.md"));
    }
}
