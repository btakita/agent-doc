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

/// True when a queue prompt's text is a recognized directive / id-bearing prompt
/// (a legitimate queue entry shape) rather than free-text prose.
pub fn is_queue_directive_prompt(text: &str) -> bool {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    lower.starts_with("do ")
        || lower.starts_with("preset ")
        || lower.starts_with("dispatch ")
        || lower.starts_with("run ")
        || t.starts_with('#')
        || t.contains("[#")
}

/// True when text references a slash command (e.g. `/agent-doc`, `/clear`,
/// `/compact`, `/loop`) at a token boundary.
///
/// This is broader than [`is_slash_command`]: it detects command references
/// inside a user-authored prompt so queue contamination checks do not mistake
/// ordinary command instructions for copied assistant answer prose.
pub fn mentions_slash_command_reference(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        // The slash must begin a token (start of string or after a non-word char)
        // so `src/agent-doc` (a path segment after a word char) is not matched.
        let at_token_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !at_token_start {
            continue;
        }
        // Require at least two command-name chars after the slash so a bare
        // separator `/` is not treated as a command reference.
        let cmd_len = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .count();
        if cmd_len >= 2 && text[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            return true;
        }
    }
    false
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

    #[test]
    fn directive_prompt_classifier_accepts_queue_control_shapes() {
        for text in [
            "do [#fix1]",
            "preset #spec-test",
            "dispatch #spec-test",
            "run smoke tests",
            "#bare-id directive",
            "please finish [#fix1]",
        ] {
            assert!(is_queue_directive_prompt(text), "{text}");
        }

        assert!(!is_queue_directive_prompt(
            "Explain why the queue churned yesterday"
        ));
    }

    #[test]
    fn slash_command_reference_requires_command_token() {
        assert!(mentions_slash_command_reference("/agent-doc tasks/foo.md"));
        assert!(mentions_slash_command_reference(
            "Please run /clear before continuing"
        ));
        assert!(mentions_slash_command_reference(
            "Use `/compact` after this turn"
        ));

        assert!(!mentions_slash_command_reference("src/agent-doc/main.rs"));
        assert!(!mentions_slash_command_reference("divide / conquer"));
        assert!(!mentions_slash_command_reference("/ x"));
    }
}
