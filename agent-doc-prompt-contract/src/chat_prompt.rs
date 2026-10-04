//! # Module: chat_prompt (`#chatprompt`, GH #125)
//!
//! ## Spec
//! - An operator prompt that reaches the harness chat instead of the session
//!   document is still a session turn. "Document is the UI" assumes every prompt
//!   arrives as a document edit; a chat-originated prompt produces no document
//!   diff by construction, so without this module the agent did the work, the
//!   document never learned the turn happened, and the next `agent-doc <FILE>`
//!   cycle reported an idle `no_changes` stop.
//! - [`chat_prompt_text`] classifies a submitted harness prompt: it is a chat
//!   prompt unless it is empty, a harness slash command (`/clear`, `/loop ...`,
//!   another skill), or an `agent-doc` invocation (those own their own
//!   admission path).
//! - [`chat_prompt_recorded`] decides whether the document body (frontmatter
//!   excluded, so a `prompt_presets` key never counts as its own record) already
//!   carries every non-blank line of the chat prompt.
//! - [`chat_prompt_presets`] resolves `prompt_presets` keys the chat prompt
//!   references, using the same preset reference rules as preflight.
//! - [`chat_prompt_context`] / [`unrecorded_chat_prompts_context`] render the
//!   hook context: the first for the chat turn itself, the second prepended to
//!   the next admitted cycle contract so `no_changes: true` is not read as idle.
//!
//! ## Agentic Contracts
//! - Pure: no I/O. The hook adapter owns state and stdout.
//! - Never classifies an `agent-doc` trigger as a chat prompt, so admission and
//!   chat recording cannot both claim one prompt.

use indexmap::IndexMap;

/// Marker that begins the chat-turn hook context.
pub const CHAT_PROMPT_MARKER: &str = "[agent-doc] chat prompt for session document";

/// Marker that begins the trigger-turn notice for unrecorded chat prompts.
pub const UNRECORDED_CHAT_PROMPTS_MARKER: &str =
    "[agent-doc] unrecorded chat prompt(s) for this document";

/// Envelope elements a harness injects into the prompt stream itself: a
/// background task's completion event, system reminders, cross-session
/// messages, and slash-command / local-command wrappers. None of them is text
/// the operator typed.
pub const HARNESS_ENVELOPE_TAGS: &[&str] = &[
    "task-notification",
    "system-reminder",
    "cross-session-message",
    "command-message",
    "command-name",
    "command-args",
    "command-contents",
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "user-prompt-submit-hook",
    "bash-input",
    "bash-stdout",
    "bash-stderr",
];

fn envelope_tag_at(text: &str) -> Option<&'static str> {
    let rest = text.strip_prefix('<')?;
    HARNESS_ENVELOPE_TAGS.iter().copied().find(|tag| {
        rest.strip_prefix(tag)
            .and_then(|after| after.chars().next())
            .is_some_and(|ch| ch == '>' || ch == '/' || ch.is_whitespace())
    })
}

/// `prompt` with every leading/trailing/interleaved harness envelope element
/// removed (`#chatprompt`). An unterminated envelope swallows the rest.
fn strip_harness_envelopes(prompt: &str) -> String {
    let mut out = String::new();
    let mut rest = prompt;
    while let Some(start) = rest.find('<') {
        let (before, candidate) = rest.split_at(start);
        match envelope_tag_at(candidate) {
            Some(tag) => {
                out.push_str(before);
                let close = format!("</{tag}>");
                rest = match candidate.find(&close) {
                    Some(end) => &candidate[end + close.len()..],
                    None => match candidate.find("/>") {
                        Some(end) if !candidate[..end].contains('\n') => &candidate[end + 2..],
                        _ => "",
                    },
                };
            }
            None => {
                out.push_str(before);
                out.push('<');
                rest = &candidate[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether a submitted prompt is harness-generated rather than typed: it
/// starts with a `<task-notification>` (a background subagent's completion
/// event), or nothing but harness envelope elements remain once they are
/// removed (`#chatprompt`).
pub fn is_harness_generated_prompt(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    trimmed.starts_with("<task-notification>")
        || (envelope_tag_at(trimmed).is_some()
            && strip_harness_envelopes(trimmed).trim().is_empty())
}

/// The operator text of a harness prompt that is not an `agent-doc` trigger.
///
/// Harness-injected envelopes are not operator text: a prompt made only of
/// them (a `<task-notification>` completion event, a `<system-reminder>`, a
/// `<cross-session-message>`, a `<command-name>` wrapper) is never a chat
/// prompt, and envelopes around real operator text are dropped from it.
pub fn chat_prompt_text(prompt: &str) -> Option<String> {
    if is_harness_generated_prompt(prompt) {
        return None;
    }
    let stripped = strip_harness_envelopes(prompt);
    let trimmed = stripped.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    let first_word = trimmed.split_whitespace().next().unwrap_or_default();
    if first_word == "agent-doc" {
        return None;
    }
    if crate::harness_prompt::agent_doc_invocation_file_from_text(trimmed).is_some() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Does the document body already carry every non-blank line of `chat`?
pub fn chat_prompt_recorded(document: &str, chat: &str) -> bool {
    let (_, body) = agent_doc_frontmatter::frontmatter::split_frontmatter_parts(document);
    let mut lines = chat
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    if lines.peek().is_none() {
        return true;
    }
    lines.all(|line| body.contains(line))
}

/// `prompt_presets` entries (`(key, body)`) the chat prompt references.
pub fn chat_prompt_presets(document: &str, chat: &str) -> Vec<(String, String)> {
    let Ok((frontmatter, _)) = agent_doc_frontmatter::frontmatter::parse(document) else {
        return Vec::new();
    };
    let presets: &IndexMap<String, String> = &frontmatter.prompt_presets;
    crate::requested_prompt_presets(&[], &[chat.to_string()], presets)
        .into_iter()
        .filter_map(|key| presets.get(&key).map(|body| (key.clone(), body.clone())))
        .collect()
}

/// Hook context for a chat-originated prompt in a document-bound session.
pub fn chat_prompt_context(document: &str, chat: &str, presets: &[(String, String)]) -> String {
    let mut context = format!(
        "{CHAT_PROMPT_MARKER} `{document}` (#chatprompt): this operator prompt arrived in the \
         harness chat, not as a document edit, and it is still a session turn. Record it: insert \
         the prompt verbatim into `agent:exchange` as `{record} <prompt>` (that shape is never read \
         back as operator steering), do the work, and persist the response through \
         `agent-doc respond {document}` / `agent-doc write --commit {document}` with its \
         queue/backlog mutations. Do not leave the turn only in the chat transcript.\n\
         chat_prompt: {chat:?}",
        record = agent_doc_prompt_lines::CHAT_PROMPT_RECORD_PREFIX,
    );
    for (key, body) in presets {
        context.push_str(&format!("\nprompt_preset {key:?}: {body:?}"));
    }
    context
}

/// Notice prepended to an admitted cycle contract when chat prompts since the
/// previous trigger never reached the document.
pub fn unrecorded_chat_prompts_context(document: &str, prompts: &[String]) -> Option<String> {
    if prompts.is_empty() {
        return None;
    }
    let mut context = format!(
        "{UNRECORDED_CHAT_PROMPTS_MARKER} `{document}` (#chatprompt): the operator prompted in \
         the harness chat since the last cycle and the document does not record it. This cycle is \
         not idle even if `no_changes` is true: insert each prompt below into `agent:exchange` \
         as `{record} <prompt>` with its response (or a note of the work already done) and persist through \
         `agent-doc respond {document}` / `agent-doc write --commit {document}`.",
        record = agent_doc_prompt_lines::CHAT_PROMPT_RECORD_PREFIX,
    );
    for prompt in prompts {
        context.push_str(&format!("\nchat_prompt: {prompt:?}"));
    }
    Some(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nprompt_presets:\n  '#upgrade': 'Upgrade agent-doc. File a gh issue.'\n---\n\n## Exchange\n\n<!-- agent:exchange -->\nhello\n<!-- /agent:exchange -->\n";

    #[test]
    fn a_trigger_is_never_a_chat_prompt() {
        assert_eq!(chat_prompt_text("/agent-doc tasks/a.md"), None);
        assert_eq!(chat_prompt_text("agent-doc tasks/a.md"), None);
        assert_eq!(chat_prompt_text("/loop agent-doc tasks/a.md"), None);
        assert_eq!(chat_prompt_text("agent-doc compact tasks/a.md"), None);
        assert_eq!(chat_prompt_text("agent-doc status"), None);
    }

    #[test]
    fn slash_commands_and_blank_prompts_are_not_chat_prompts() {
        assert_eq!(chat_prompt_text("   "), None);
        assert_eq!(chat_prompt_text("/clear"), None);
        assert_eq!(chat_prompt_text("/loop 5m check the deploy"), None);
    }

    #[test]
    fn a_preset_key_or_prose_typed_in_chat_is_a_chat_prompt() {
        assert_eq!(
            chat_prompt_text(" #upgrade \n"),
            Some("#upgrade".to_string())
        );
        assert_eq!(
            chat_prompt_text("what changed in the last release?"),
            Some("what changed in the last release?".to_string())
        );
    }

    #[test]
    fn a_preset_key_is_not_recorded_by_its_own_frontmatter_definition() {
        assert!(!chat_prompt_recorded(DOC, "#upgrade"));
        let recorded = DOC.replace("hello\n", "hello\n#upgrade\n");
        assert!(chat_prompt_recorded(&recorded, "#upgrade"));
    }

    #[test]
    fn multi_line_chat_prompts_need_every_line_recorded() {
        let doc = DOC.replace("hello\n", "hello\nfirst line\n");
        assert!(!chat_prompt_recorded(&doc, "first line\nsecond line"));
        let doc = doc.replace("first line\n", "first line\nsecond line\n");
        assert!(chat_prompt_recorded(&doc, "first line\n\nsecond line"));
    }

    #[test]
    fn a_chat_preset_key_resolves_to_its_document_body() {
        assert_eq!(
            chat_prompt_presets(DOC, "#upgrade"),
            vec![(
                "#upgrade".to_string(),
                "Upgrade agent-doc. File a gh issue.".to_string()
            )]
        );
        assert!(chat_prompt_presets(DOC, "what changed?").is_empty());
    }

    #[test]
    fn chat_context_names_the_recording_path_and_preset_body() {
        let presets = chat_prompt_presets(DOC, "#upgrade");
        let context = chat_prompt_context("tasks/a.md", "#upgrade", &presets);
        assert!(context.starts_with(CHAT_PROMPT_MARKER));
        assert!(context.contains("agent:exchange"));
        assert!(context.contains("agent-doc respond tasks/a.md"));
        assert!(context.contains("agent-doc write --commit tasks/a.md"));
        assert!(context.contains("chat_prompt: \"#upgrade\""));
        assert!(context.contains("Upgrade agent-doc. File a gh issue."));
    }

    #[test]
    fn unrecorded_notice_says_no_changes_is_not_idle() {
        assert_eq!(unrecorded_chat_prompts_context("tasks/a.md", &[]), None);
        let notice =
            unrecorded_chat_prompts_context("tasks/a.md", &["#upgrade".to_string()]).unwrap();
        assert!(notice.starts_with(UNRECORDED_CHAT_PROMPTS_MARKER));
        assert!(notice.contains("not idle even if `no_changes` is true"));
        assert!(notice.contains("chat_prompt: \"#upgrade\""));
    }

    /// `#steerworks`: Claude Code injected a background subagent's completion
    /// event into the coordinator's prompt stream; the UserPromptSubmit hook
    /// flagged it as an operator chat prompt to record in the document.
    #[test]
    fn harness_envelopes_are_never_chat_prompts() {
        let task_notification = "<task-notification>\n<task-id>af0c40c5</task-id>\n<status>completed</status>\n<summary>Agent \"Fix #126\" finished</summary>\n<result>done</result>\n</task-notification>";
        assert_eq!(chat_prompt_text(task_notification), None);
        // Wrapped in a system-reminder, as Claude Code delivers it.
        assert_eq!(
            chat_prompt_text(&format!(
                "<system-reminder>\n[SYSTEM NOTIFICATION]\n{task_notification}\n</system-reminder>"
            )),
            None
        );
        // Trailing prose after the event is still the event, not operator text.
        assert_eq!(
            chat_prompt_text(&format!("{task_notification}\nFull transcript at /tmp/x")),
            None
        );
        assert_eq!(
            chat_prompt_text("<system-reminder>The date changed.</system-reminder>"),
            None
        );
        assert_eq!(
            chat_prompt_text("<cross-session-message from=\"pane-2\">ping</cross-session-message>"),
            None
        );
        assert_eq!(
            chat_prompt_text(
                "<command-message>agent-doc is running…</command-message>\n<command-name>/agent-doc</command-name>\n<command-args>tasks/a.md</command-args>"
            ),
            None
        );
        assert_eq!(
            chat_prompt_text("<local-command-stdout>ok</local-command-stdout>"),
            None
        );
    }

    #[test]
    fn operator_text_around_an_envelope_is_still_a_chat_prompt() {
        assert_eq!(
            chat_prompt_text(
                "<system-reminder>context</system-reminder>\nwhat changed in the last release?"
            ),
            Some("what changed in the last release?".to_string())
        );
        // Ordinary angle brackets in operator text survive.
        assert_eq!(
            chat_prompt_text("is a < b in the <queue> marker?"),
            Some("is a < b in the <queue> marker?".to_string())
        );
    }
}
