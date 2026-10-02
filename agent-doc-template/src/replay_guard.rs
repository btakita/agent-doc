//! # Module: replay_guard
//!
//! Shared shape validation for replaying assistant payloads through recovery
//! paths such as the Codex Stop hook and `agent-doc repair`.
//!
//! ## Spec
//! - `classify_replay_payload(message)` trims the candidate payload and
//!   classifies it as empty, replayable, or blocked.
//! - Block transcript/full-document shaped payloads that should never be
//!   replayed automatically: agent component dumps (a line-anchored component
//!   marker outside code and quoted prose — a marker named inside inline
//!   backticks or a fence is prose, GH 90), transcript prompt lines,
//!   `## User` / `## Assistant` headings, multiple `### Re:` headings, or
//!   malformed patch payloads.
//! - Patch-bearing payloads are replayable only when `template::parse_patches`
//!   returns at least one patch and at least one `patch:exchange` block.
//! - Patch-bearing payloads may carry only known replay guard comments outside
//!   patch blocks, such as `<!-- no-pending-capture -->`.
//! - When a patch-bearing payload has unmatched text, replay is allowed only
//!   for the narrow "plain progress commentary before the patch body" shape; in
//!   that case the replayable payload is the extracted patch-only body.
//!
//! ## Agentic Contracts
//! - The validator is conservative: ambiguous transcript-shaped payloads are
//!   blocked instead of auto-extracted.
//! - Replayable payloads are returned as either the trimmed original string or
//!   an extracted patch-only body when leading commentary had to be stripped.
//!
//! ## Evals
//! - `classify_empty_payload`
//! - `classify_plain_response_body_as_replayable`
//! - `classify_single_exchange_patch_as_replayable`
//! - `classify_multiple_clean_patches_as_replayable`
//! - `classify_blocks_agent_component_dump`
//! - `classify_blocks_prompt_lines`
//! - `classify_allows_a_marker_named_inside_inline_backticks`
//! - `classify_still_blocks_a_standalone_marker_outside_code`
//! - `classify_allows_several_distinct_response_headings`
//! - `classify_blocks_a_heading_repeated_within_one_payload`
//! - `classify_blocks_a_heading_already_committed_in_the_document`
//! - `classify_allows_fresh_headings_against_a_document`
//! - `topic_normalization_splits_on_the_last_em_dash`
//! - `classify_blocks_patch_payload_with_unmatched_transcript`
//! - `classify_patch_payload_with_leading_guard_marker_as_replayable`
//! - `classify_patch_payload_with_safe_leading_commentary_extracts_patch_body`

use std::borrow::Cow;

#[derive(Debug)]
pub enum ReplayPayloadClassification<'a> {
    Empty,
    Replayable(Cow<'a, str>),
    Blocked(String),
}

fn is_safe_leading_patch_commentary(prefix: &str) -> bool {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return false;
    }

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let first = line.chars().next().unwrap_or_default();
        let starts_numbered_item =
            first.is_ascii_digit() && line[first.len_utf8()..].starts_with('.');
        if line.starts_with("<!--")
            || line.starts_with("```")
            || line.starts_with("~~~")
            || line.starts_with('#')
            || line.starts_with('-')
            || line.starts_with('*')
            || line.starts_with('>')
            || line.starts_with('|')
            || starts_numbered_item
        {
            return false;
        }
    }

    true
}

fn is_replay_guard_marker(line: &str) -> bool {
    matches!(
        line.trim(),
        "<!-- no-pending-capture -->" | "<!-- no-pending-done-guard -->"
    )
}

fn unmatched_is_only_replay_guard_markers(unmatched: &str) -> bool {
    let mut saw_marker = false;
    for line in unmatched.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !is_replay_guard_marker(line) {
            return false;
        }
        saw_marker = true;
    }
    saw_marker
}

/// GH 90 — the component-marker half of the replay guard, shared with the
/// write-time landability check so a payload is refused (or accepted) by the
/// same rule before and after durable capture.
///
/// This used to be a bare substring test: ANY `<!-- agent:` in the payload
/// blocked it as "a full document component dump", including a response that
/// merely named a marker inside inline backticks while explaining a repair. A
/// component dump carries markers as STRUCTURE — each on its own line, outside
/// code. So only a line-anchored marker outside code and quoted prose
/// ([`agent_doc_element::element::structural_marker_occurrences`]) blocks. An
/// inline, unquoted marker in prose is not a dump; the write path escapes it
/// (`sanitize::sanitize_component_tags`) before it can reach the document.
pub fn component_marker_dump_refusal(payload: &str) -> Option<String> {
    let anchored = agent_doc_element::element::structural_marker_occurrences(payload)
        .into_iter()
        .find(|occurrence| occurrence.line_anchored)?;
    Some(format!(
        "it contained agent component markers from a full document component dump (standalone marker outside code on payload line {}; quote marker text in backticks)",
        anchored.line
    ))
}

/// Classify a replay payload with no document to compare against.
///
/// Callers that HAVE the document should prefer
/// [`classify_replay_payload_against_document`]: without it the
/// already-committed-heading check cannot run, so a genuine replay is caught
/// only by its transcript artifacts and by repeating a heading within one
/// payload.
pub fn classify_replay_payload(message: &str) -> ReplayPayloadClassification<'_> {
    classify_replay_payload_against_document(message, None)
}

/// `#multiheadingreplay` — classify a replay payload against the document it
/// would be written into.
pub fn classify_replay_payload_against_document<'a>(
    message: &'a str,
    document: Option<&str>,
) -> ReplayPayloadClassification<'a> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return ReplayPayloadClassification::Empty;
    }

    if let Some(reason) = component_marker_dump_refusal(trimmed) {
        return ReplayPayloadClassification::Blocked(reason);
    }

    let prompt_lines = trimmed
        .lines()
        .filter(|line| line.trim_start().starts_with('❯'))
        .count();
    if prompt_lines > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained transcript prompt lines".to_string(),
        );
    }

    let user_headings = trimmed
        .lines()
        .filter(|line| {
            let line = line.trim();
            line == "## User" || line.starts_with("## User ")
        })
        .count();
    if user_headings > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained `## User` transcript headings".to_string(),
        );
    }

    let assistant_headings = trimmed
        .lines()
        .filter(|line| {
            let line = line.trim();
            line == "## Assistant" || line.starts_with("## Assistant ")
        })
        .count();
    if assistant_headings > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained `## Assistant` transcript headings".to_string(),
        );
    }

    // `#multiheadingreplay` — a heading COUNT cannot tell a replay from a turn
    // that answered several prompts.
    //
    // This used to block any payload with more than one `### Re:` heading, which
    // is exactly the shape `SKILL.md` prescribes: reconcile the changed exchange
    // tail oldest-first, answering each unresolved prompt. A six-prompt turn
    // therefore emits six headings, and `repair` refused the only recovery path
    // its own instructions produce — observed 2026-09-27, with a 12687-byte
    // durable capture that no path could replay.
    //
    // What the guard is really for is a TRANSCRIPT DUMP, and the four checks
    // above already catch every artifact one carries: component markers, `❯`
    // prompt lines, `## User`, and `## Assistant`. What is left that a dump has
    // and a fresh answer does not is a heading the document ALREADY holds, or
    // the same heading twice inside one payload. Both are replays by
    // construction; a set of distinct, new topics is one turn doing its job.
    let payload_topics = re_heading_topics(trimmed);
    if let Some(duplicate) = first_duplicate(&payload_topics) {
        return ReplayPayloadClassification::Blocked(format!(
            "it repeated the assistant response heading `{duplicate}` within one payload"
        ));
    }
    if let Some(document) = document {
        let committed = re_heading_topics(document);
        if let Some(already) = payload_topics
            .iter()
            .find(|topic| committed.contains(topic))
        {
            return ReplayPayloadClassification::Blocked(format!(
                "its assistant response heading `{already}` is already committed in the document"
            ));
        }
    }

    let has_patch_markers = trimmed.contains("<!-- patch:") || trimmed.contains("<!-- /patch:");
    if has_patch_markers {
        match crate::template::parse_patches(trimmed) {
            Ok((patches, unmatched)) => {
                if patches.is_empty() {
                    return ReplayPayloadClassification::Blocked(
                        "it contained malformed patch markers without a replayable patch block"
                            .to_string(),
                    );
                }
                if !patches.iter().any(|patch| patch.name == "exchange") {
                    return ReplayPayloadClassification::Blocked(
                        "it did not include a `patch:exchange` closeout".to_string(),
                    );
                }
                if !unmatched.trim().is_empty() {
                    if unmatched_is_only_replay_guard_markers(&unmatched) {
                        return ReplayPayloadClassification::Replayable(Cow::Borrowed(trimmed));
                    }
                    let first_patch = trimmed
                        .find("<!-- patch:")
                        .or_else(|| trimmed.find("<!-- replace:"));
                    if let Some(first_patch) = first_patch {
                        let prefix = &trimmed[..first_patch];
                        let patch_only = trimmed[first_patch..].trim();
                        if is_safe_leading_patch_commentary(prefix)
                            && let Ok((patches_only, unmatched_only)) =
                                crate::template::parse_patches(patch_only)
                            && !patches_only.is_empty()
                            && unmatched_only.trim().is_empty()
                            && patches_only.iter().any(|patch| patch.name == "exchange")
                        {
                            return ReplayPayloadClassification::Replayable(Cow::Owned(
                                patch_only.to_string(),
                            ));
                        }
                    }
                    if let Some(prefix) = first_patch.map(|idx| &trimmed[..idx])
                        && is_safe_leading_patch_commentary(prefix)
                    {
                        return ReplayPayloadClassification::Blocked(
                            "it contained non-patch trailing or interstitial content around patch blocks"
                                .to_string(),
                        );
                    }
                    return ReplayPayloadClassification::Blocked(
                        "it contained extra transcript content around patch blocks".to_string(),
                    );
                }
            }
            Err(err) => {
                return ReplayPayloadClassification::Blocked(format!(
                    "it contained malformed patch markers: {err}"
                ));
            }
        }
    }

    ReplayPayloadClassification::Replayable(Cow::Borrowed(trimmed))
}

/// The `### Re:` topics in `text`, normalized for comparison.
///
/// Attribution is stripped at the LAST spaced em dash, matching
/// `strip_re_heading_attribution`: the heading format is
/// `### Re: topic — model · timestamp`, and a topic may itself contain an em
/// dash. Splitting on the first one would truncate such a topic and make two
/// different headings compare equal.
fn re_heading_topics(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("### Re:")?;
            let topic = match rest.rfind(" — ") {
                Some(at) => &rest[..at],
                None => rest,
            };
            let topic = topic.trim();
            if topic.is_empty() {
                None
            } else {
                Some(topic.to_lowercase())
            }
        })
        .collect()
}

/// The first topic that appears more than once, if any.
fn first_duplicate(topics: &[String]) -> Option<&String> {
    topics
        .iter()
        .enumerate()
        .find(|(index, topic)| topics[..*index].contains(topic))
        .map(|(_, topic)| topic)
}

#[cfg(test)]
mod tests {
    use super::{
        ReplayPayloadClassification, classify_replay_payload,
        classify_replay_payload_against_document, first_duplicate, re_heading_topics,
    };

    fn assert_replayable(payload: &str) {
        match classify_replay_payload(payload) {
            ReplayPayloadClassification::Replayable(actual) => {
                assert_eq!(actual.as_ref(), payload.trim());
            }
            ReplayPayloadClassification::Empty => panic!("expected replayable payload, got empty"),
            ReplayPayloadClassification::Blocked(reason) => {
                panic!("expected replayable payload, got blocked: {reason}")
            }
        }
    }

    fn assert_blocked(payload: &str, needle: &str) {
        match classify_replay_payload(payload) {
            ReplayPayloadClassification::Blocked(reason) => {
                assert!(
                    reason.contains(needle),
                    "expected reason to contain `{needle}`, got `{reason}`"
                );
            }
            ReplayPayloadClassification::Empty => panic!("expected blocked payload, got empty"),
            ReplayPayloadClassification::Replayable(actual) => {
                panic!("expected blocked payload, got replayable: {actual}")
            }
        }
    }

    #[test]
    fn classify_empty_payload() {
        assert!(matches!(
            classify_replay_payload("   \n"),
            ReplayPayloadClassification::Empty
        ));
    }

    #[test]
    fn classify_plain_response_body_as_replayable() {
        assert_replayable("### Re: topic — gpt-5\n\nBody\n");
    }

    #[test]
    fn classify_single_exchange_patch_as_replayable() {
        assert_replayable(
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->\n",
        );
    }

    #[test]
    fn classify_multiple_clean_patches_as_replayable() {
        assert_replayable(
            "<!-- patch:status -->\nDone.\n<!-- /patch:status -->\n<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->\n",
        );
    }

    #[test]
    fn classify_blocks_agent_component_dump() {
        assert_blocked(
            "<!-- agent:exchange patch=append -->\n❯ hi\n### Re: topic — gpt-5\n<!-- /agent:exchange -->\n",
            "component",
        );
    }

    /// GH 90 — the 2552-byte laptop.md capture: one inline code span naming
    /// a marker, inside prose about repairing it. It is not a dump, and it
    /// must replay. The surrounding `<details>` line is a context in which the
    /// CommonMark AST does not parse inline code at all.
    #[test]
    fn classify_allows_a_marker_named_inside_inline_backticks() {
        assert_replayable(
            "### Re: queue marker — claude\n\nThe errant keystroke left\n`i<!-- /agent:queue -->`. Fixed: marker restored, zero stray characters left, and `tmp/caret-probe-client.md` is gone.\n",
        );
        assert_replayable(
            "### Re: queue marker — claude\n\n<details>\n`i<!-- /agent:queue -->`. Fixed.\n</details>\n",
        );
        assert_replayable(
            "### Re: queue marker — claude\n\n```markdown\n<!-- /agent:queue -->\n```\n",
        );
    }

    /// GH 90 — line-anchored markers outside code are still a dump.
    #[test]
    fn classify_still_blocks_a_standalone_marker_outside_code() {
        assert_blocked(
            "### Re: topic — claude\n\nBody `ok`\n  <!-- /agent:queue -->\n",
            "full document component dump",
        );
    }

    #[test]
    fn classify_blocks_prompt_lines() {
        assert_blocked("❯ do #task\n### Re: topic — gpt-5\n", "prompt lines");
    }

    /// `#multiheadingreplay` — several DISTINCT topics is a turn that answered
    /// several prompts, which is exactly what `SKILL.md` asks for. Blocking it
    /// left a durable capture with no replay path at all.
    #[test]
    fn classify_allows_several_distinct_response_headings() {
        match classify_replay_payload(
            "### Re: first — gpt-5\none\n### Re: second — gpt-5\ntwo\n",
        ) {
            ReplayPayloadClassification::Replayable(_) => {}
            other => panic!("expected replayable, got {other:?}"),
        }
    }

    /// The same heading twice in one payload is a replay by construction.
    #[test]
    fn classify_blocks_a_heading_repeated_within_one_payload() {
        assert_blocked(
            "### Re: same topic — gpt-5\none\n### Re: same topic — opus-5\ntwo\n",
            "repeated the assistant response heading `same topic`",
        );
    }

    /// With the document in hand, a heading it ALREADY holds is the real replay
    /// signal the count was standing in for.
    #[test]
    fn classify_blocks_a_heading_already_committed_in_the_document() {
        let document = "## Exchange\n### Re: do #alpha — opus-5 · 2026-09-27T10:00-04:00\nbody\n";
        match classify_replay_payload_against_document(
            "### Re: do #alpha — opus-5 · 2026-09-27T17:00-04:00\nagain\n",
            Some(document),
        ) {
            ReplayPayloadClassification::Blocked(reason) => assert!(
                reason.contains("already committed in the document"),
                "got `{reason}`"
            ),
            other => panic!("expected blocked, got {other:?}"),
        }
    }

    /// ...and fresh topics still pass against that same document.
    #[test]
    fn classify_allows_fresh_headings_against_a_document() {
        let document = "## Exchange\n### Re: do #alpha — opus-5 · 2026-09-27T10:00-04:00\nbody\n";
        match classify_replay_payload_against_document(
            "### Re: do #beta — opus-5\none\n### Re: do #gamma — opus-5\ntwo\n",
            Some(document),
        ) {
            ReplayPayloadClassification::Replayable(_) => {}
            other => panic!("expected replayable, got {other:?}"),
        }
    }

    /// Attribution is stripped at the LAST spaced em dash, so a topic that
    /// itself contains one is not truncated into a false match.
    #[test]
    fn topic_normalization_splits_on_the_last_em_dash() {
        assert_eq!(
            re_heading_topics("### Re: lazily.md — one crash behind it — opus-5 · 2026-09-27T17:44-04:00\n"),
            vec!["lazily.md — one crash behind it".to_string()]
        );
        // Without the last-em-dash rule both of these would normalize to
        // "lazily.md" and the second would read as a replay of the first.
        let topics = re_heading_topics(
            "### Re: lazily.md — one crash — opus-5\n### Re: lazily.md — two crashes — opus-5\n",
        );
        assert_eq!(topics.len(), 2);
        assert!(first_duplicate(&topics).is_none(), "{topics:?}");
    }

    #[test]
    fn classify_blocks_patch_payload_with_unmatched_transcript() {
        assert_blocked(
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\nBody\n<!-- /patch:exchange -->\nextra transcript",
            "extra transcript content",
        );
    }

    #[test]
    fn classify_patch_payload_with_leading_guard_marker_as_replayable() {
        assert_replayable(
            "<!-- no-pending-capture -->\n<!-- patch:exchange -->\n### Re: topic — gpt-5\nBody\n<!-- /patch:exchange -->\n",
        );
    }

    #[test]
    fn classify_patch_payload_with_safe_leading_commentary_extracts_patch_body() {
        let payload = concat!(
            "Reviewing the current plan and repo conventions so I can turn `#next-steps` into concrete backlog items in the session document.\n",
            "I have the plan context. Next I’m checking how this repo formats backlog items so the patch matches existing session-doc conventions instead of inventing a new shape.\n\n",
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#task] Follow-up\n",
            "<!-- /patch:backlog -->\n"
        );

        match classify_replay_payload(payload) {
            ReplayPayloadClassification::Replayable(actual) => assert_eq!(
                actual.as_ref(),
                concat!(
                    "<!-- patch:exchange -->\n",
                    "### Re: topic — gpt-5\n\n",
                    "Body\n",
                    "<!-- /patch:exchange -->\n",
                    "\n",
                    "<!-- patch:backlog -->\n",
                    "- [ ] [#task] Follow-up\n",
                    "<!-- /patch:backlog -->"
                )
            ),
            ReplayPayloadClassification::Empty => panic!("expected replayable payload, got empty"),
            ReplayPayloadClassification::Blocked(reason) => {
                panic!("expected replayable payload, got blocked: {reason}")
            }
        }
    }
}
