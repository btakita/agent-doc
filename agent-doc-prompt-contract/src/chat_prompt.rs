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
//!   the next admitted cycle contract, which carries the same prompts as
//!   `chat_prompts` (never `no_changes`; [`CHAT_PROMPT_DIFF_TYPE`]).
//! - [`chat_prompt_record_present`] / [`chat_prompts_missing_record`] /
//!   [`unrecorded_chat_prompt_closeout_warning`] back the closeout warning: a
//!   committed cycle that carried chat prompts needs a
//!   `> **Chat prompt (#chatprompt):**` record block for each.
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

/// `diff_type` a preflight contract reports when the only work it carries is
/// chat prompts the document never recorded (`#chatprompt`, GH #125). A
/// document diff keeps its own classification; this names the otherwise-empty
/// cycle so it can never be mistaken for an idle `no_changes` one.
pub const CHAT_PROMPT_DIFF_TYPE: &str = "chat_prompt";

/// Seconds a chat prompt stays in a session's ledger. A prompt the next trigger
/// never carried within this window is stale: the turn it belonged to is long
/// over and re-raising it on every later trigger is noise, not a record.
pub const CHAT_PROMPT_LEDGER_TTL_SECS: u64 = 6 * 60 * 60;

/// Bodies of every `> **Chat prompt (#chatprompt):** …` record in `document`
/// (frontmatter excluded): the text after the tag plus its `>` continuation
/// lines, joined by newlines.
fn chat_prompt_record_blocks(document: &str) -> Vec<String> {
    let (_, body) = agent_doc_frontmatter::frontmatter::split_frontmatter_parts(document);
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if agent_doc_prompt_lines::is_chat_prompt_record_line(trimmed) {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            let rest = trimmed
                .split_once("(#chatprompt)")
                .map(|(_, rest)| rest)
                .unwrap_or_default()
                .trim_start_matches([':', '*', '_', ' ']);
            current = Some(rest.to_string());
            continue;
        }
        if let Some(block) = current.as_mut()
            && let Some(rest) = trimmed.strip_prefix('>')
        {
            block.push('\n');
            block.push_str(rest.trim());
            continue;
        }
        if let Some(block) = current.take() {
            blocks.push(block);
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

/// Does `document` carry a `> **Chat prompt (#chatprompt):** …` record of
/// `chat`: one record block containing every non-blank line of the prompt?
///
/// Stricter than [`chat_prompt_recorded`]: a short prompt (`yes`, `#upgrade`)
/// can appear in unrelated prose, so closeout enforcement asks for the
/// canonical record shape, not a substring match.
pub fn chat_prompt_record_present(document: &str, chat: &str) -> bool {
    let lines = chat
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return true;
    }
    chat_prompt_record_blocks(document)
        .iter()
        .any(|block| lines.iter().all(|line| block.contains(line)))
}

/// The chat prompts in `prompts` that `document` has no record line for.
pub fn chat_prompts_missing_record(document: &str, prompts: &[String]) -> Vec<String> {
    prompts
        .iter()
        .filter(|prompt| !chat_prompt_record_present(document, prompt))
        .cloned()
        .collect()
}

/// Marker that begins the closeout warning for a chat prompt a committed cycle
/// carried but never recorded.
pub const UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER: &str =
    "[session-check] warning: chat prompt not recorded (#chatprompt)";

/// Closeout warning for chat prompts a committed cycle carried without a
/// `> **Chat prompt (#chatprompt):**` record, with the repair. A warning, never
/// a failure: the work may be done and committed, only its record is missing.
pub fn unrecorded_chat_prompt_closeout_warning(
    document: &str,
    missing: &[String],
) -> Option<String> {
    if missing.is_empty() {
        return None;
    }
    let mut warning = format!(
        "{UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER}: `{document}` committed a cycle that carried {}          chat prompt(s) without a `{record} <prompt>` record. The turn is in the chat transcript          only. Repair: pipe a response whose `patch:exchange` begins with `{record} <verbatim          prompt>` (plus a note of the work done) through `agent-doc respond {document}`; on a          committed cycle it reopens a fresh one from HEAD.",
        missing.len(),
        record = agent_doc_prompt_lines::CHAT_PROMPT_RECORD_PREFIX,
    );
    for prompt in missing {
        warning.push_str(&format!("\nchat_prompt: {prompt:?}"));
    }
    Some(warning)
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
         the harness chat since the last cycle and the document does not record it. The contract \
         below carries them as `chat_prompts` (and in `user_intent_prompt_changes`), so this cycle \
         is a real turn, not idle: begin the `patch:exchange` response with `{record} <prompt>` \
         for each prompt below, answer it (or note the work already done), and persist through \
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
        assert!(notice.contains("not idle"));
        assert!(notice.contains("chat_prompts"));
        assert!(notice.contains("chat_prompt: \"#upgrade\""));
    }

    /// GH #125 gap 4: closeout asks for the canonical record shape. A short
    /// prompt that merely appears in prose is not a record.
    #[test]
    fn a_record_line_is_required_not_a_prose_substring() {
        let prose = DOC.replace("hello\n", "hello\nwe ran #upgrade yesterday\n");
        assert!(chat_prompt_recorded(&prose, "#upgrade"));
        assert!(!chat_prompt_record_present(&prose, "#upgrade"));
        let recorded = DOC.replace(
            "hello\n",
            "hello\n> **Chat prompt (#chatprompt):** #upgrade\n\n### Re: upgrade\n",
        );
        assert!(chat_prompt_record_present(&recorded, "#upgrade"));
        assert!(chat_prompts_missing_record(&recorded, &["#upgrade".to_string()]).is_empty());
        // A multi-line prompt is recorded through `>` continuation lines.
        let multi = DOC.replace(
            "hello\n",
            "> **Chat prompt (#chatprompt):** first line\n> second line\n\nbody\n",
        );
        assert!(chat_prompt_record_present(
            &multi,
            "first line\nsecond line"
        ));
        assert!(!chat_prompt_record_present(
            &multi,
            "first line\nthird line"
        ));
        // A preset key in frontmatter is never its own record.
        let fm_only =
            "---\nprompt_presets:\n  x: '> **Chat prompt (#chatprompt):** #upgrade'\n---\nbody\n";
        assert!(!chat_prompt_record_present(fm_only, "#upgrade"));
    }

    #[test]
    fn closeout_warning_names_each_prompt_and_the_repair() {
        assert_eq!(
            unrecorded_chat_prompt_closeout_warning("tasks/a.md", &[]),
            None
        );
        let warning =
            unrecorded_chat_prompt_closeout_warning("tasks/a.md", &["#upgrade".to_string()])
                .unwrap();
        assert!(warning.starts_with(UNRECORDED_CHAT_PROMPT_CLOSEOUT_MARKER));
        assert!(warning.contains("agent-doc respond tasks/a.md"));
        assert!(warning.contains(agent_doc_prompt_lines::CHAT_PROMPT_RECORD_PREFIX));
        assert!(warning.contains("chat_prompt: \"#upgrade\""));
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
