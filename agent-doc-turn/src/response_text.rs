//! Pure response text policy for turn closeout and write guards.

/// Strip leading `## Assistant` and trailing `## User` headings from append-mode
/// response text.
///
/// The append writer adds its own `## Assistant` prefix and `## User` suffix, so
/// echoed transcript headings are removed before the response is persisted.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

/// Separator between the model short name and the response timestamp inside a
/// `### Re:` heading's attribution segment (`#timestampresponseheader`).
///
/// Deliberately NOT another spaced em dash. Four separate parsers read this
/// heading, and they do not agree on which dash bounds the attribution: three
/// take the FIRST (`response_prompt_target_from_re_heading` here,
/// `agent_doc_queue::queue_response::response_heading_topic`, and
/// `agent_doc_turn::response_replay::normalize_replay_topic`) while
/// `agent_doc_document::transient_markers::strip_re_heading_attribution` takes the
/// LAST, via `rfind(" — ")`. A second em dash would therefore leave the model name
/// behind on the strip path, and since that strip backs
/// `normalize_post_commit_re_heading_drift` — the post-commit comparison that lets
/// an attributed heading match an unattributed one — the residue would read as
/// permanent heading drift on every committed response.
///
/// Keeping exactly one ` — ` in the heading preserves all four readings unchanged:
/// the topic is everything before it, the attribution everything after.
pub const RESPONSE_ATTRIBUTION_SEPARATOR: &str = " · ";

/// The spaced em dash that separates a response topic from its attribution.
pub const RESPONSE_TOPIC_ATTRIBUTION_SEPARATOR: &str = " \u{2014} ";

/// Build the attribution segment of a `### Re:` heading: model short name, then
/// the response timestamp.
///
/// An empty timestamp yields the model alone, which is the pre-timestamp shape —
/// so a harness that cannot resolve a clock degrades to the old heading instead of
/// emitting a dangling separator.
pub fn response_heading_attribution(model_short_name: &str, timestamp: &str) -> String {
    let model = model_short_name.trim();
    let timestamp = timestamp.trim();
    if timestamp.is_empty() {
        return model.to_string();
    }
    if model.is_empty() {
        return timestamp.to_string();
    }
    format!("{model}{RESPONSE_ATTRIBUTION_SEPARATOR}{timestamp}")
}

/// Build a complete `### Re:` heading line.
pub fn response_heading(topic: &str, model_short_name: &str, timestamp: &str) -> String {
    let attribution = response_heading_attribution(model_short_name, timestamp);
    if attribution.is_empty() {
        return format!("### Re: {}", topic.trim());
    }
    format!(
        "### Re: {}{RESPONSE_TOPIC_ATTRIBUTION_SEPARATOR}{attribution}",
        topic.trim()
    )
}

/// Split a `### Re:` heading's attribution segment into model and timestamp.
///
/// Returns `None` when the heading carries no attribution at all. The timestamp is
/// `None` for a pre-timestamp heading, which stays valid.
pub fn response_heading_model_and_timestamp(line: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let without_hashes = trimmed.trim_start_matches('#').trim_start();
    let rest = without_hashes.strip_prefix("Re:")?.trim();
    let (_, attribution) = rest.split_once(RESPONSE_TOPIC_ATTRIBUTION_SEPARATOR)?;
    let attribution = attribution.trim();
    match attribution.split_once(RESPONSE_ATTRIBUTION_SEPARATOR) {
        Some((model, timestamp)) => Some((model.trim(), Some(timestamp.trim()))),
        None => Some((attribution, None)),
    }
}

/// Whether a response timestamp matches the documented
/// `YYYY-MM-DDTHH:MM±HH:MM` shape (`#timestampresponseheader`).
///
/// A shape check, not a calendar check: it exists so a guard can tell a real
/// timestamp from prose that happened to land in the attribution slot, without
/// pulling a date library into a pure text module.
pub fn response_heading_timestamp_is_wellformed(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 22 {
        return false;
    }
    let digits_at = |positions: &[usize]| {
        positions
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
    };
    digits_at(&[0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21])
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && matches!(bytes[16], b'+' | b'-')
        && bytes[19] == b':'
}

/// Extract the session prompt target from the first `### Re:`-style response
/// heading, dropping the model suffix after a dash separator.
pub fn response_prompt_target_from_re_heading(response_body: &str) -> Option<String> {
    for line in response_body.lines() {
        let without_hashes = line.trim().trim_start_matches('#').trim_start();
        let Some(rest) = without_hashes.strip_prefix("Re:") else {
            continue;
        };
        let target = rest
            .split_once(" \u{2014} ")
            .map(|(target, _)| target)
            .or_else(|| rest.split_once(" - ").map(|(target, _)| target))
            .unwrap_or(rest)
            .trim();
        if !target.is_empty() {
            return Some(target.to_string());
        }
    }
    None
}

pub fn is_committed_prompt_diff_interruption(reason: &str) -> bool {
    reason.contains("is `committed`")
        && reason.contains("prompt_target:")
        && (reason.contains("unresolved prompt-bearing user changes")
            || reason.contains(
                "active harness session changed this document after the last committed closeout",
            ))
        && (reason.contains("no new agent-doc cycle started")
            || reason.contains("without reopening the binary-owned write/commit path"))
}

pub fn prompt_target_from_interruption_reason(reason: &str) -> Option<String> {
    let marker = "prompt_target:";
    let tail = reason.split_once(marker)?.1.trim();
    (!tail.is_empty()).then(|| tail.to_string())
}

pub fn first_nonempty_prompt_line(prompt: &str) -> String {
    prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(prompt)
        .trim()
        .to_string()
}

/// Summarize captured response text for external hook consumers. Agent-doc patch
/// comments are metadata, not useful response content, so they are removed
/// before the summary is bounded.
pub fn summarize_response_for_hook(response_body: &str) -> String {
    let lines = response_body
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "<!-- patch:exchange -->"
                && trimmed != "<!-- /patch:exchange -->"
                && !(trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate_response_summary(lines.trim(), 4000)
}

pub fn render_interleaved_thinking_response(thinking: &str, response: &str) -> String {
    if thinking.is_empty() {
        return response.to_string();
    }

    format!(
        "<details>\n<summary>Thinking</summary>\n\n{}\n</details>\n\n{}",
        thinking, response
    )
}

fn truncate_response_summary(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return input.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        output.push_str("\n[truncated]");
    }
    output
}

const IMPERATIVE_STATUS_ONLY_SIGNALS: &[&str] = &[
    "in progress",
    "continuing",
    "starting",
    "working on it",
    "still working",
    "next i'll",
    "next i will",
    "i'll update",
    "i will update",
    "i'm going to",
    "i am going to",
    "let me do that",
];

const IMPERATIVE_META_REFUSAL_SIGNALS: &[&str] = &[
    "because you asked me to run agent-doc",
    "treated that text as document content",
    "not to execute",
    "say do #",
    "repeat the instruction in chat",
    "i stayed on the first layer",
    "operate on the session document",
];

const IMPERATIVE_BLOCKER_SIGNALS: &[&str] = &[
    "blocked",
    "blocker",
    "failed",
    "error",
    "cannot",
    "can't",
    "unable",
    "missing",
    "permission denied",
    "requires approval",
    "needs approval",
    "lock file",
    "timed out",
];

const IMPERATIVE_EVIDENCE_LABELS: &[&str] = &[
    "what changed:",
    "verification:",
    "commit / push:",
    "outcome:",
    "root cause:",
    "blocked:",
    "blocker:",
];

/// Decide whether a response satisfies an executable imperative directive.
///
/// This is the pure response half of the binary backstop. Diff extraction,
/// ops-log emission, and fail-closed error formatting stay in orchestration.
pub fn response_satisfies_imperative_contract(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    if contains_any_signal(&lower, IMPERATIVE_BLOCKER_SIGNALS) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_META_REFUSAL_SIGNALS) {
        return false;
    }
    if contains_execution_evidence(response, &lower) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_STATUS_ONLY_SIGNALS) {
        return false;
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImperativeResponseContractDecision {
    NoImperativeDirective,
    Satisfied,
    Rejected { trigger: String },
}

/// Decide whether an imperative document directive has been concretely handled.
///
/// The diff and response-text classification are pure turn policy. File/snapshot
/// loading, ops-log emission, and returning a process error stay in orchestration.
pub fn imperative_response_contract_decision(
    diff_text: &str,
    response: &str,
) -> ImperativeResponseContractDecision {
    if !agent_doc_diff::diff_contains_imperative_directive(diff_text) {
        return ImperativeResponseContractDecision::NoImperativeDirective;
    }
    if response_satisfies_imperative_contract(response) {
        return ImperativeResponseContractDecision::Satisfied;
    }
    let trigger = agent_doc_diff::extract_imperative_directives(diff_text)
        .into_iter()
        .next()
        .unwrap_or_else(|| "approval".to_string());
    ImperativeResponseContractDecision::Rejected { trigger }
}

pub fn truncate_imperative_trigger(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let mut cut = max;
        while cut > 0 && !value.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &value[..cut])
    }
}

fn contains_any_signal(haystack: &str, signals: &[&str]) -> bool {
    signals.iter().any(|signal| haystack.contains(signal))
}

fn contains_execution_evidence(response: &str, lower: &str) -> bool {
    if response.contains("```") || response.contains("~~~") {
        return true;
    }
    if IMPERATIVE_EVIDENCE_LABELS
        .iter()
        .any(|label| lower.contains(label))
    {
        return true;
    }
    if lower.contains("implemented and verified")
        || lower.contains("built and installed")
        || lower.contains("added regression coverage")
        || lower.contains("pushed to ")
    {
        return true;
    }
    response.lines().any(|line| {
        has_commandish_backticks(line)
            || has_code_path(line)
            || contains_commit_hash(line)
            || line.trim_start().starts_with("- `")
    })
}

fn has_commandish_backticks(line: &str) -> bool {
    if !line.contains('`') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("git ")
        || lower.contains("make ")
        || lower.contains("npm ")
        || lower.contains("pnpm ")
        || lower.contains("yarn ")
        || lower.contains("pytest")
        || lower.contains("uv run")
        || lower.contains("agent-doc ")
        || line.contains('/')
}

fn has_code_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("src/")
        || line.contains("tests/")
        || line.contains("specs/")
        || line.contains("runbooks/")
        || lower.contains(".rs")
        || lower.contains(".md")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".sh")
        || lower.contains(".kt")
        || lower.contains(".ts")
}

fn contains_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    /// `#timestampresponseheader`: the whole reason the timestamp joins the model with
    /// `·` instead of a second spaced em dash. Four parsers read this heading and they
    /// disagree about which dash bounds the attribution — three take the first, and
    /// `strip_re_heading_attribution` takes the LAST. This drives all four off one
    /// timestamped heading, so a future format change cannot satisfy one and break the
    /// others silently.
    #[test]
    fn a_timestamped_heading_reads_the_same_through_every_parser() {
        let heading = super::response_heading("do [#fix1]", "opus-5", "2026-09-11T23:45-04:00");
        assert_eq!(
            heading,
            "### Re: do [#fix1] \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00"
        );
        assert_eq!(
            heading.matches(" \u{2014} ").count(),
            1,
            "exactly one spaced em dash must separate topic from attribution: {heading}"
        );

        // 1. This module's own parser (first dash).
        assert_eq!(
            super::response_prompt_target_from_re_heading(&heading).as_deref(),
            Some("do [#fix1]")
        );
        // 2. The queue-closeout parser (first dash).
        assert_eq!(
            agent_doc_queue::queue_response::response_heading_topic(&heading),
            Some("do [#fix1]")
        );
        // 3. The compact digest's topic summarizer.
        assert_eq!(
            agent_doc_topic::summarize_compacted_exchange(&format!("{heading}\nbody\n")),
            vec!["Archived 1 response topic(s): do [#fix1]".to_string()]
        );
        // 4. The post-commit drift strip (LAST dash) must remove model AND timestamp.
        assert_eq!(
            agent_doc_document::transient_markers::strip_re_heading_attribution(&format!(
                "{heading}\n"
            )),
            "### Re: do [#fix1]\n",
            "the attribution strip must not leave the model name behind"
        );
    }

    /// The pre-timestamp heading stays valid — a harness with no clock must degrade to
    /// it rather than emit a dangling separator.
    #[test]
    fn an_untimestamped_heading_still_round_trips() {
        let heading = super::response_heading("do [#fix1]", "gpt-5", "");
        assert_eq!(heading, "### Re: do [#fix1] \u{2014} gpt-5");
        assert_eq!(
            super::response_heading_model_and_timestamp(&heading),
            Some(("gpt-5", None))
        );
        assert_eq!(
            agent_doc_document::transient_markers::strip_re_heading_attribution(&format!(
                "{heading}\n"
            )),
            "### Re: do [#fix1]\n"
        );
    }

    #[test]
    fn heading_attribution_splits_into_model_and_timestamp() {
        let heading = super::response_heading("topic", "opus-5", "2026-09-11T23:45-04:00");
        assert_eq!(
            super::response_heading_model_and_timestamp(&heading),
            Some(("opus-5", Some("2026-09-11T23:45-04:00")))
        );
        // No attribution at all.
        assert_eq!(
            super::response_heading_model_and_timestamp("### Re: topic"),
            None
        );
        // The active-prompt marker and heading level must not defeat the split.
        assert_eq!(
            super::response_heading_model_and_timestamp(
                "\u{276F} #### Re: topic \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00"
            ),
            Some(("opus-5", Some("2026-09-11T23:45-04:00")))
        );
    }

    /// A shape check, so a guard can tell a timestamp from prose that landed in the
    /// attribution slot. The offset sign sits at index 16 and the offset colon at 19 —
    /// transposing them accepts `...23:45:04-00` and rejects the real format.
    #[test]
    fn timestamp_shape_check_pins_each_separator_position() {
        assert!(super::response_heading_timestamp_is_wellformed(
            "2026-09-11T23:45-04:00"
        ));
        assert!(super::response_heading_timestamp_is_wellformed(
            "2026-09-11T23:45+05:30"
        ));
        for bad in [
            "2026-09-11T23:45:04-00",  // sign/colon transposed
            "2026-09-11T23:45",        // no offset
            "2026-09-11 23:45-04:00",  // space instead of T
            "2026-9-11T23:45-04:00",   // unpadded month
            "2026-09-11T23:45-04:0",   // too short
            "2026-09-11T23:45-04:000", // too long
            "opus-5",
            "",
        ] {
            assert!(
                !super::response_heading_timestamp_is_wellformed(bad),
                "must reject {bad:?}"
            );
        }
    }

    use super::*;

    #[test]
    fn strips_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_leading_space_before_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("\n\n## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_trailing_user_heading_and_keeps_newline() {
        assert_eq!(
            strip_assistant_heading("Done.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn strips_both_echoed_headings() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn leaves_plain_response_unchanged() {
        let response = "Done.\n\nDetails.";
        assert_eq!(strip_assistant_heading(response), response.to_string());
    }

    #[test]
    fn response_prompt_target_uses_re_heading_without_model_suffix() {
        let response = "<!-- patch:exchange -->\n### Re: do [#tsiftmemhooks] \u{2014} gpt-5\nDone.\n<!-- /patch:exchange -->\n";
        assert_eq!(
            response_prompt_target_from_re_heading(response).as_deref(),
            Some("do [#tsiftmemhooks]")
        );
    }

    #[test]
    fn committed_prompt_diff_interruption_detects_unresolved_prompt_after_commit() {
        let reason = "session-check: cycle is `committed`; unresolved prompt-bearing user changes found; no new agent-doc cycle started; prompt_target: do #deploy";

        assert!(is_committed_prompt_diff_interruption(reason));
    }

    #[test]
    fn committed_prompt_diff_interruption_detects_active_session_drift_after_commit() {
        let reason = "session-check: cycle is `committed`; active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path; prompt_target: do #repair";

        assert!(is_committed_prompt_diff_interruption(reason));
    }

    #[test]
    fn committed_prompt_diff_interruption_rejects_open_cycle_reason() {
        let reason = "session-check: cycle is `preflight_started`; unresolved prompt-bearing user changes found; no new agent-doc cycle started; prompt_target: do #deploy";

        assert!(!is_committed_prompt_diff_interruption(reason));
    }

    #[test]
    fn prompt_target_from_interruption_reason_returns_trimmed_tail() {
        let reason =
            "session-check: committed prompt diff; prompt_target: \n\n do [#seopdp] deploy page ";

        assert_eq!(
            prompt_target_from_interruption_reason(reason).as_deref(),
            Some("do [#seopdp] deploy page")
        );
    }

    #[test]
    fn prompt_target_from_interruption_reason_ignores_missing_or_empty_target() {
        assert_eq!(prompt_target_from_interruption_reason("no target"), None);
        assert_eq!(
            prompt_target_from_interruption_reason("prompt_target:   "),
            None
        );
    }

    #[test]
    fn first_nonempty_prompt_line_skips_blank_lines_and_trims() {
        assert_eq!(
            first_nonempty_prompt_line("\n\n  do [#seopdp] deploy page  \nmore"),
            "do [#seopdp] deploy page"
        );
    }

    #[test]
    fn response_summary_removes_patch_markers_and_truncates() {
        let body = format!(
            "<!-- patch:exchange -->\n### Re: x\n{}\n<!-- /patch:exchange -->\n",
            "a".repeat(4100)
        );
        let summary = summarize_response_for_hook(&body);
        assert!(!summary.contains("patch:exchange"));
        assert!(summary.contains("[truncated]"));
        assert!(summary.starts_with("### Re: x"));
    }

    #[test]
    fn interleaved_thinking_response_wraps_thinking_in_details() {
        let rendered = render_interleaved_thinking_response("Reasoning here.", "Visible answer.");

        assert!(rendered.starts_with("<details>\n<summary>Thinking</summary>"));
        assert!(rendered.contains("Reasoning here."));
        assert!(rendered.ends_with("\n\nVisible answer."));
    }

    #[test]
    fn interleaved_thinking_response_leaves_response_when_thinking_empty() {
        assert_eq!(
            render_interleaved_thinking_response("", "Visible answer."),
            "Visible answer."
        );
    }

    #[test]
    fn imperative_contract_rejects_status_only_response() {
        assert!(!response_satisfies_imperative_contract(
            "### Re: task - gpt-5\nIn progress. Continuing now."
        ));
    }

    #[test]
    fn imperative_contract_rejects_meta_refusal() {
        assert!(!response_satisfies_imperative_contract(
            "I treated that text as document content and not to execute it."
        ));
    }

    #[test]
    fn imperative_contract_allows_concrete_blocker() {
        assert!(response_satisfies_imperative_contract(
            "### Re: blocked - gpt-5\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed."
        ));
    }

    #[test]
    fn imperative_contract_allows_execution_evidence() {
        assert!(response_satisfies_imperative_contract(
            "### Re: done - gpt-5\nVerification:\n- `cargo test --manifest-path src/agent-doc/Cargo.toml`\nCommit / push:\n- `abc1234`\n"
        ));
    }

    #[test]
    fn imperative_contract_allows_code_path_or_commit_hash_evidence() {
        assert!(response_satisfies_imperative_contract(
            "Updated agent-doc-turn/src/response_text.rs and pushed 1b215b7."
        ));
    }

    #[test]
    fn imperative_contract_decision_skips_non_imperative_diff() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+notes only\n";

        assert_eq!(
            imperative_response_contract_decision(diff, "In progress."),
            ImperativeResponseContractDecision::NoImperativeDirective
        );
    }

    #[test]
    fn imperative_contract_decision_rejects_with_first_trigger() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+do #abc. run tests and commit + push\n";

        assert_eq!(
            imperative_response_contract_decision(diff, "In progress. Continuing now."),
            ImperativeResponseContractDecision::Rejected {
                trigger: "do #abc. run tests and commit + push".to_string()
            }
        );
    }

    #[test]
    fn imperative_contract_decision_accepts_evidence() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+go\n";

        assert_eq!(
            imperative_response_contract_decision(
                diff,
                "Verification:\n- `cargo test -p agent-doc-turn`"
            ),
            ImperativeResponseContractDecision::Satisfied
        );
    }

    #[test]
    fn imperative_trigger_truncation_preserves_char_boundaries() {
        assert_eq!(truncate_imperative_trigger("abcdef", 4), "abcd...");
        assert_eq!(truncate_imperative_trigger("ééé", 3), "é...");
    }
}
