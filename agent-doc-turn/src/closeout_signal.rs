//! Pure closeout response and done-signal classification.
//!
//! Session-check owns file/cycle-state IO. This module owns the string-level
//! turn policy that decides whether response text completes a tracked-work id
//! and whether prompt text carries explicit done/resolved signals.

/// Tight list of "blocked / still needs future action" phrases that, combined
/// with a directed id gated this cycle, indicate a closeout reported more agent
/// execution is still needed. Kept narrow so ordinary review prose does not
/// trip closeout guards.
pub const BLOCKED_FUTURE_ACTION_PHRASES: &[&str] = &[
    "remains blocked",
    "still blocked",
    "is blocked",
    "are blocked",
    "blocked on",
    "blocked by",
    "blocked:",
    "blocked until",
    "next step to complete",
    "next steps to complete",
    "steps to complete",
    "cannot complete until",
    "can't complete until",
    "still needs to",
    "still need to",
    "must remove",
    "must delete",
    "must expire",
    "needs to be removed",
    "no live cutover",
    "waiting on approval",
    "awaiting approval",
    "needs approval before",
    "requires approval before",
    "deliberately delete",
    "get approval",
];

/// Explicit "no follow-up needed" justifications that satisfy a blocked-shape
/// closeout for a genuinely review-only gate.
pub const NO_FOLLOWUP_JUSTIFICATION_PHRASES: &[&str] = &[
    "no additional backlog follow-up",
    "no additional follow-up is needed",
    "no follow-up backlog",
    "no further backlog",
    "no actionable backlog follow-up",
    "no remaining backlog work",
];

pub fn text_has_blocked_future_action_signal(lower: &str) -> bool {
    BLOCKED_FUTURE_ACTION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn text_has_no_followup_justification(lower: &str) -> bool {
    NO_FOLLOWUP_JUSTIFICATION_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// True when a blocked / still-needed-work phrase co-occurs with `#id` inside
/// the same paragraph of the response. Paragraph scoping keeps incidental
/// blocked phrases about unrelated work from tying the signal to the directed id.
pub fn blocked_signal_tied_to_id(text: &str, id: &str) -> bool {
    let needle = format!("#{}", id.to_ascii_lowercase());
    text.split("\n\n").any(|paragraph| {
        let lower = paragraph.to_ascii_lowercase();
        lower.contains(&needle) && text_has_blocked_future_action_signal(&lower)
    })
}

/// True when a kept-open item body enumerates multiple gated/remaining phases:
/// the word "phase" appears, at least two short parenthesized phase markers
/// (`(1)`, `(2a)`, `(2b)`, `(3)`, ...) are present, and a gating/remaining
/// signal frames them as deferred work.
pub fn body_enumerates_multiple_gated_phases(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if !lower.contains("phase") {
        return false;
    }
    let gating = [
        "gated",
        "remaining",
        "live-verify",
        "live verify",
        "awaiting",
        "still needs",
        "not yet",
    ];
    if !gating.iter().any(|signal| lower.contains(signal)) {
        return false;
    }
    count_phase_markers(body) >= 2
}

/// Count distinct short parenthesized phase markers like `(1)`, `(2a)`, `(2b)`,
/// `(3)`. Requires 1-2 digits optionally followed by 1-2 ASCII lowercase letters
/// so dates and commit hashes (`(2026-05-31)`, `(submodule 407b0825)`) are not
/// mistaken for phase markers.
pub fn count_phase_markers(body: &str) -> usize {
    static MARKER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\((\d{1,2}[a-z]{0,2})\)").unwrap());
    let mut seen = std::collections::HashSet::new();
    for cap in MARKER.captures_iter(body) {
        seen.insert(cap[1].to_string());
    }
    seen.len()
}

/// True when the body already references at least two discrete child ids other
/// than its own (and other than the ubiquitous `#agent-doc-bug` preset tag).
pub fn body_already_split_into_child_ids(body: &str, own_id: &str) -> bool {
    static ID_REF: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"#([a-z0-9][a-z0-9-]*)").unwrap());
    let mut others = std::collections::HashSet::new();
    for cap in ID_REF.captures_iter(body) {
        let id = agent_doc_element_backlog::backlog::normalize_pending_id(&cap[1]);
        if !id.is_empty() && id != own_id && id != "agent-doc-bug" {
            others.insert(id);
        }
    }
    others.len() >= 2
}

/// Substep-completion phrases that evidence partial progress in a queue audit.
pub const QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES: &[&str] = &[
    "is complete",
    "was complete",
    "are complete",
    "were complete",
    "is done",
    "was done",
    "was clean",
    "is clean",
    "is current",
    "are current",
    "passed",
    "verified clean",
    "already complete",
];

/// True when a queue-audit response collapses partial completion: it is about the
/// queue, makes a blanket none-complete claim, shows >=2 distinct substep
/// completions, and never frames anything as "partial."
pub fn queue_audit_collapses_partial_completion(lower: &str) -> bool {
    if !lower.contains("queue") {
        return false;
    }
    // Already broke it down — not a collapse.
    if lower.contains("partial") {
        return false;
    }
    if !queue_audit_has_none_complete_claim(lower) {
        return false;
    }
    let substep_completions = QUEUE_AUDIT_SUBSTEP_COMPLETE_PHRASES
        .iter()
        .filter(|phrase| lower.contains(*phrase))
        .count();
    substep_completions >= 2
}

/// A blanket "none / not ... complete" claim about the queue items.
pub fn queue_audit_has_none_complete_claim(lower: &str) -> bool {
    static NONE_COMPLETE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        // "none of the queue items is/are (fully) complete", "no items are
        // complete", "none are fully complete", etc. — a none/no quantifier
        // within a short span before a complete/completed token.
        regex::Regex::new(r"\b(none|no)\b[^.\n]{0,60}?\bcomplet(e|ed)\b").unwrap()
    });
    NONE_COMPLETE.is_match(lower)
}

/// Tight list of "deferred live work" phrases that, combined with a shipped
/// signal, indicate a `do [#id]` turn shipped a repo phase but left live
/// deploy / sync / verification / approval work for a later phase
/// (`#do-id-partial-closeout-state`). Kept narrow to avoid false positives on
/// ordinary closeout prose.
pub const PARTIAL_CLOSEOUT_REMAINING_PHRASES: &[&str] = &[
    "not deployed",
    "not yet deployed",
    "deploy remains",
    "deployment remains",
    "deploy/",
    "live verification",
    "live verify",
    "live-verify",
    "external validation remains",
    "awaiting approval",
    "awaiting user",
    "user approval",
    "sync remains",
    "feed sync",
    "merchant center",
    "live ads",
    "remains: deploy",
];

pub fn text_has_shipped_signal(lower: &str) -> bool {
    (lower.contains("committed")
        || lower.contains("commit + push")
        || lower.contains("commit and push"))
        && lower.contains("push")
}

pub fn text_has_partial_remaining_signal(lower: &str) -> bool {
    PARTIAL_CLOSEOUT_REMAINING_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn response_text_for_guards(response: &str) -> String {
    let Ok((patches, unmatched)) = agent_doc_template::parse_patches(response) else {
        return response.to_string();
    };

    let preferred: Vec<String> = patches
        .iter()
        .filter(|patch| matches!(patch.name.as_str(), "exchange" | "findings"))
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !preferred.is_empty() {
        return preferred.join("\n\n");
    }

    if !unmatched.trim().is_empty() {
        return unmatched.trim().to_string();
    }

    let fallback: Vec<String> = patches
        .iter()
        .filter(|patch| {
            !agent_doc_element::element::is_backlog_component(&patch.name)
                && !agent_doc_element::element::is_review_component(&patch.name)
        })
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !fallback.is_empty() {
        return fallback.join("\n\n");
    }

    response.to_string()
}

pub fn free_text_queue_marker_has_bare_heading_residue(content: &str) -> bool {
    content.contains("<!-- no-free-text-queue-head-guard -->")
        && content.lines().any(|line| line.trim() == "###")
}

pub fn response_head_plausibly_answers(content: &str, head: &str) -> bool {
    let head_words: Vec<&str> = head
        .split_whitespace()
        .filter(|w| {
            w.len() > 3
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "the"
                        | "this"
                        | "that"
                        | "with"
                        | "from"
                        | "also"
                        | "does"
                        | "what"
                        | "when"
                        | "how"
                )
        })
        .collect();
    if head_words.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    let mut matched = 0;
    for word in &head_words {
        if lower.contains(&word.to_ascii_lowercase()) {
            matched += 1;
        }
    }
    matched * 2 >= head_words.len()
}

/// Where a directive id's response heading materialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseSource {
    Exchange,
    Archive,
}

impl ResponseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseSource::Exchange => "exchange",
            ResponseSource::Archive => "archive",
        }
    }
}

/// Resolve where `id`'s `### Re:` response heading materialized, if anywhere.
///
/// Pure over already-resolved `content` (the live committed exchange) and
/// `archives` (HEAD compact-archive bodies). The caller owns all file/archive
/// IO and passes the string bodies here.
pub fn directive_response_source(
    content: &str,
    archives: &[String],
    id: &str,
) -> Option<ResponseSource> {
    if content_has_re_heading_for_id(content, id) {
        return Some(ResponseSource::Exchange);
    }
    if archives
        .iter()
        .any(|archive| content_has_re_heading_for_id(archive, id))
    {
        return Some(ResponseSource::Archive);
    }
    None
}

/// True when any `### Re:` heading line in `content` references `#id` / `[#id]`.
///
/// `do #id` responses always render under a `### Re: ... #id` heading, so a
/// heading-scoped match avoids false matches against queue-prompt echoes or
/// backlog lines that merely mention the id.
pub fn content_has_re_heading_for_id(content: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let needle = format!("#{}", id.to_ascii_lowercase());
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("### Re:") && !trimmed.starts_with("###Re") {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.find(&needle) {
            None => false,
            Some(pos) => {
                // Reject a longer-id prefix collision (`#ab` must not match `#abc`).
                let after = &lower[pos + needle.len()..];
                !after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
        }
    })
}

/// Pure inputs for per-id queue directive response-loss detection.
///
/// All ids are normalized (no leading `#`, lowercased) before they reach this
/// policy. The caller resolves `content` and `archives` up front so this logic
/// stays deterministic and IO-free.
#[derive(Clone, Debug)]
pub struct ReapedResponseLossInput<'a> {
    /// `do #id` directive target ids active this cycle.
    pub directive_ids: &'a [String],
    /// Pending ids reaped into `agent:done` this cycle.
    pub reaped_ids: &'a [String],
    /// Live committed exchange content.
    pub content: &'a str,
    /// HEAD-referenced compact-archive bodies (each searched like `content`).
    pub archives: &'a [String],
}

/// Reaped `do #id` directive ids whose `### Re: ... #id` response did not land.
///
/// Returns the directive ids whose response heading did not materialize in the
/// live exchange `content` or in any HEAD compact `archives` entry. Order follows
/// `directive_ids`; duplicates are collapsed.
pub fn reaped_directive_ids_without_response(input: &ReapedResponseLossInput<'_>) -> Vec<String> {
    let reaped: std::collections::HashSet<&str> =
        input.reaped_ids.iter().map(String::as_str).collect();
    let mut lost: Vec<String> = Vec::new();
    for id in input.directive_ids {
        if id.is_empty() || !reaped.contains(id.as_str()) {
            continue;
        }
        let materialized = content_has_re_heading_for_id(input.content, id)
            || input
                .archives
                .iter()
                .any(|archive| content_has_re_heading_for_id(archive, id));
        if materialized {
            continue;
        }
        if !lost.iter().any(|existing| existing == id) {
            lost.push(id.clone());
        }
    }
    lost
}

/// True when response text clearly completes `id`.
///
/// Completion is signalled by a response HEADING whose topic RESOLVES to exactly
/// this id, never by a bare prose citation of `#id` in the body
/// (#pending-done-guard-false-positive). A heading match plus a completion
/// marker still distinguishes a real completion from a halt/refusal response
/// that merely names the head (#queue-strike-on-halt).
pub fn response_clearly_completes_pending_id(response_text: &str, id: &str) -> bool {
    if !response_heading_resolves_to_pending_id(response_text, id) {
        return false;
    }
    contains_completion_marker(&response_text.to_ascii_lowercase())
}

/// True when some `### Re:` response heading's topic resolves to `#id`.
///
/// A batch `do [#a] [#b] ...` directive heading resolves to every bracketed id;
/// a titled `#id descriptive text` heading resolves only to its LEADING id. A
/// heading that merely contains `#id` later in descriptive prose, and any `#id`
/// cited in the response body, never resolves to it.
pub fn response_heading_resolves_to_pending_id(response_text: &str, id: &str) -> bool {
    let id_lower = id.to_ascii_lowercase();
    for raw in response_text.lines() {
        let line = raw.trim().to_ascii_lowercase();
        let Some(after) = line.strip_prefix('#') else {
            continue;
        };
        let heading = after.trim_start_matches('#').trim_start();
        let Some(topic) = heading.strip_prefix("re:") else {
            continue;
        };
        let topic = topic.split(" — ").next().unwrap_or(topic).trim();
        if let Some(do_list) = topic.strip_prefix("do ") {
            let bracket_ids = extract_bracket_ids(do_list);
            if !bracket_ids.is_empty() {
                if bracket_ids.iter().any(|b| b == &id_lower) {
                    return true;
                }
                continue;
            }
            if leading_hash_id(do_list).as_deref() == Some(id_lower.as_str()) {
                return true;
            }
        } else if leading_hash_id(topic).as_deref() == Some(id_lower.as_str()) {
            return true;
        }
    }
    false
}

fn leading_hash_id(text: &str) -> Option<String> {
    let t = text.strip_prefix('[').unwrap_or(text);
    let rest = t.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    (!id.is_empty()).then_some(id)
}

fn extract_bracket_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("[#") {
        let after = &rest[pos + 2..];
        let id: String = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
        let consumed = id.len();
        if !id.is_empty() {
            out.push(id);
        }
        rest = &after[consumed..];
    }
    out
}

fn contains_completion_marker(text: &str) -> bool {
    [
        "implemented",
        "fixed",
        "done.",
        "done ",
        "completed",
        "updated",
        "verification:",
        "verified",
        "pushed",
        "commit:",
        "outcome:",
        "what changed:",
        "landed",
        "shipped",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

pub fn explicit_done_signal_ids(text: &str) -> Vec<String> {
    let normalized = normalize_done_signal_text(text);
    if normalized.is_empty() {
        return Vec::new();
    }

    let lower = normalized.to_ascii_lowercase();
    let is_done_signal = lower.contains(" done")
        || lower.ends_with(" done")
        || lower.starts_with("done ")
        || lower.contains(" complete")
        || lower.ends_with(" complete")
        || lower.starts_with("complete ")
        || lower.contains(" completed")
        || lower.ends_with(" completed")
        || lower.starts_with("completed ")
        || lower.contains(" resolved")
        || lower.ends_with(" resolved")
        || lower.starts_with("resolved ");
    if !is_done_signal {
        return Vec::new();
    }

    agent_doc_element_backlog::backlog::extract_pending_hash_ids(&normalized)
}

pub fn plain_done_signal(text: &str) -> bool {
    let normalized = normalize_done_signal_text(text);
    let lower = normalized.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "done"
            | "done."
            | "complete"
            | "complete."
            | "completed"
            | "completed."
            | "resolved"
            | "resolved."
    )
}

fn normalize_done_signal_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('❯')
        .trim()
        .trim_start_matches("- ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loss_input<'a>(
        directive_ids: &'a [String],
        reaped_ids: &'a [String],
        content: &'a str,
        archives: &'a [String],
    ) -> ReapedResponseLossInput<'a> {
        ReapedResponseLossInput {
            directive_ids,
            reaped_ids,
            content,
            archives,
        }
    }

    #[test]
    fn reaped_directive_response_loss_flags_reap_only_loss() {
        let directive = vec!["lostresp".to_string()];
        let reaped = vec!["lostresp".to_string()];
        let content = "### Re: prior — gpt-5\n\nAnswered something else.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lostresp".to_string()],
        );
    }

    #[test]
    fn reaped_directive_response_loss_flags_captured_but_id_lost() {
        let directive = vec!["kept".to_string(), "lost".to_string()];
        let reaped = vec!["kept".to_string(), "lost".to_string()];
        let content = "### Re: do #kept — opus-4-8\n\nShipped the kept fix.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            )),
            vec!["lost".to_string()],
        );
    }

    #[test]
    fn reaped_directive_response_loss_accepts_archive_materialization() {
        let directive = vec!["archived".to_string()];
        let reaped = vec!["archived".to_string()];
        let content = "### Re: prior — gpt-5\n\nUnrelated live response.\n";
        let archives = vec!["### Re: do #archived — opus-4-8\n\nShipped earlier.\n".to_string()];
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "a reaped id materialized in a HEAD compact archive is not a loss"
        );
    }

    #[test]
    fn directive_response_source_resolves_found_and_source() {
        let archives = vec!["### Re: do #archived — opus-4-8\n\nShipped earlier.\n".to_string()];

        let exchange = "### Re: do #live — opus-4-8\n\nShipped the live fix.\n";
        assert_eq!(
            directive_response_source(exchange, &archives, "live"),
            Some(ResponseSource::Exchange)
        );

        let unrelated = "### Re: prior — gpt-5\n\nUnrelated live response.\n";
        assert_eq!(
            directive_response_source(unrelated, &archives, "archived"),
            Some(ResponseSource::Archive)
        );
        assert!(directive_response_source(unrelated, &archives, "lost").is_none());
    }

    #[test]
    fn blocked_followup_policy_ties_future_action_to_directed_id() {
        assert!(text_has_blocked_future_action_signal(
            "merchant center is blocked on approval"
        ));
        assert!(text_has_no_followup_justification(
            "no additional backlog follow-up is needed because rollout is review-only"
        ));
        assert!(blocked_signal_tied_to_id(
            "### Re: do #abc\n\n#abc remains blocked on approval.",
            "abc"
        ));
        assert!(!blocked_signal_tied_to_id(
            "### Re: do #abc\n\n#other remains blocked on approval.",
            "abc"
        ));
    }

    #[test]
    fn gated_phase_split_policy_detects_multi_phase_unsplit_body() {
        let body =
            "Remaining gated phases: phase (2b) live-verify the pane, phase (3) ship the rollout.";
        assert!(body_enumerates_multiple_gated_phases(body));
        assert_eq!(count_phase_markers(body), 2);
        assert!(!body_already_split_into_child_ids(body, "parentfix"));
    }

    #[test]
    fn gated_phase_split_policy_accepts_child_id_breakout() {
        let body = "Remaining gated phases tracked as children: phase (2b) -> #childb, phase (3) -> #childc. Plan: tasks/x.md";
        assert!(body_enumerates_multiple_gated_phases(body));
        assert!(body_already_split_into_child_ids(body, "parentfix"));
    }

    #[test]
    fn queue_audit_policy_flags_none_complete_collapse_with_substep_progress() {
        let lower = "none of the six queue items are complete. same-day qa is complete and the url validate-only check was clean, but each row still has at least one remaining action.";
        assert!(queue_audit_collapses_partial_completion(lower));
        assert!(queue_audit_has_none_complete_claim(lower));
    }

    #[test]
    fn queue_audit_policy_ignores_already_partial_or_unrelated_reports() {
        assert!(!queue_audit_collapses_partial_completion(
            "none of the queue items are fully complete, but several are partially complete: qa is complete and validate-only was clean."
        ));
        assert!(!queue_audit_collapses_partial_completion(
            "none of the migration steps are complete. schema dump is complete and backup was clean."
        ));
        assert!(!queue_audit_collapses_partial_completion(
            "none of the queue items are complete yet; every row is still blocked on input."
        ));
    }

    #[test]
    fn partial_closeout_policy_detects_shipped_with_remaining_live_work() {
        let lower = "committed and pushed the repo plus tests. live deploy and live verification remain; not deployed yet.";
        assert!(text_has_shipped_signal(lower));
        assert!(text_has_partial_remaining_signal(lower));

        assert!(text_has_shipped_signal("commit + push completed"));
        assert!(text_has_shipped_signal("commit and push completed"));
        assert!(!text_has_shipped_signal("committed locally only"));
        assert!(!text_has_partial_remaining_signal(
            "completed the full task with no external validation left"
        ));
    }

    #[test]
    fn response_text_for_guards_prefers_exchange_and_findings_patches() {
        let response = concat!(
            "<!-- replace:backlog -->\n- [ ] [#later] Follow-up\n<!-- /replace:backlog -->\n\n",
            "<!-- patch:exchange -->\n  Exchange closeout body.  \n<!-- /patch:exchange -->\n\n",
            "<!-- patch:findings -->\nFinding body.\n<!-- /patch:findings -->\n",
        );

        assert_eq!(
            response_text_for_guards(response),
            "Exchange closeout body.\n\nFinding body."
        );
    }

    #[test]
    fn response_text_for_guards_falls_back_to_unmatched_or_non_tracking_patch() {
        let unmatched = "Plain final answer.\n\n<!-- replace:backlog -->\n- [ ] [#later] Follow-up\n<!-- /replace:backlog -->\n";
        assert_eq!(response_text_for_guards(unmatched), "Plain final answer.");

        let status_only = concat!(
            "<!-- replace:backlog -->\n- [ ] [#later] Follow-up\n<!-- /replace:backlog -->\n\n",
            "<!-- patch:status -->\n  Shipped status text.  \n<!-- /patch:status -->\n",
        );
        assert_eq!(
            response_text_for_guards(status_only),
            "Shipped status text."
        );
    }

    #[test]
    fn free_text_queue_marker_residue_requires_marker_and_bare_heading() {
        assert!(free_text_queue_marker_has_bare_heading_residue(
            "<!-- no-free-text-queue-head-guard -->\n\n###\n"
        ));
        assert!(!free_text_queue_marker_has_bare_heading_residue(
            "<!-- no-free-text-queue-head-guard -->\n\n### Re: answered\n"
        ));
        assert!(!free_text_queue_marker_has_bare_heading_residue(
            "###\n\nResponse text without the suppression marker.\n"
        ));
    }

    #[test]
    fn response_head_plausibility_requires_half_of_meaningful_words() {
        assert!(response_head_plausibly_answers(
            "The churn comes from stale queue convergence.",
            "Please explain the queue churn"
        ));
        assert!(response_head_plausibly_answers(
            "Stale convergence explains the queue behavior.",
            "Please explain stale queue convergence"
        ));
        assert!(!response_head_plausibly_answers(
            "A short acknowledgement.",
            "Please explain stale queue convergence"
        ));
        assert!(!response_head_plausibly_answers(
            "Done.",
            "how does this work"
        ));
    }

    #[test]
    fn reaped_directive_response_loss_ignores_unreaped_directive() {
        let directive = vec!["pending".to_string()];
        let reaped: Vec<String> = Vec::new();
        let content = "### Re: prior — gpt-5\n\nAnswered.\n";
        let archives: Vec<String> = Vec::new();
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive, &reaped, content, &archives
            ))
            .is_empty(),
            "an unreaped directive id is not a loss"
        );
    }

    #[test]
    fn reaped_directive_response_loss_pins_multi_directive_single_heading_shape() {
        let directive = vec!["a".to_string(), "b".to_string()];
        let reaped = vec!["a".to_string(), "b".to_string()];
        let single_heading = "### Re: do #a — opus-4-8\n\nFixed #a; also addressed #b inline.\n";
        let archives: Vec<String> = Vec::new();
        assert_eq!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                single_heading,
                &archives
            )),
            vec!["b".to_string()],
            "documents the multi-directive-single-heading false positive"
        );

        let grouped_heading = "### Re: do #a, #b — opus-4-8\n\nFixed both.\n";
        assert!(
            reaped_directive_ids_without_response(&loss_input(
                &directive,
                &reaped,
                grouped_heading,
                &archives
            ))
            .is_empty(),
            "a grouped heading naming both ids is not a loss"
        );
    }

    #[test]
    fn response_completion_requires_matching_heading_and_completion_marker() {
        let response = "### Re: do [#alpha] — gpt-5\n\nImplemented the fix.\n\nRelated to #beta.";
        assert!(response_clearly_completes_pending_id(response, "alpha"));
        assert!(!response_clearly_completes_pending_id(response, "beta"));
        assert!(!response_clearly_completes_pending_id(
            "### Re: do [#alpha]\n\nBlocked on CI.",
            "alpha"
        ));
    }

    #[test]
    fn response_heading_resolution_uses_heading_topic_not_body_mentions() {
        assert!(response_heading_resolves_to_pending_id(
            "### Re: do [#a] [#b]\n\nDone.",
            "b"
        ));
        assert!(response_heading_resolves_to_pending_id(
            "### Re: #lead descriptive text\n\nDone.",
            "lead"
        ));
        assert!(!response_heading_resolves_to_pending_id(
            "### Re: unrelated #body\n\nDone #body.",
            "body"
        ));
    }

    #[test]
    fn done_signals_extract_explicit_ids_and_plain_single_review_signals() {
        assert_eq!(explicit_done_signal_ids("❯ #abc done"), vec!["abc"]);
        assert_eq!(
            explicit_done_signal_ids("- complete #abc and #def"),
            vec!["abc", "def"]
        );
        assert!(plain_done_signal("❯ done."));
        assert!(!plain_done_signal("done #abc"));
        assert!(explicit_done_signal_ids("look at #abc").is_empty());
    }
}
