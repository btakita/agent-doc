//! Recommendation-detection heuristics for pending-capture enforcement.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatternKind {
    PriorityLabel,
    NumberedActionList,
    RecommendationHeader,
    ImperativeAfterRecommend,
    UnconditionalFollowUp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationSignal {
    pub estimated_count: usize,
    pub patterns_matched: Vec<PatternKind>,
    pub confidence: f32,
}

impl RecommendationSignal {
    fn none() -> Self {
        Self {
            estimated_count: 0,
            patterns_matched: Vec::new(),
            confidence: 0.0,
        }
    }
}

const ACTION_VERBS: &[&str] = &[
    "add",
    "audit",
    "cover",
    "create",
    "document",
    "fix",
    "implement",
    "land",
    "merge",
    "migrate",
    "refactor",
    "remove",
    "ship",
    "test",
    "update",
    "write",
];

const NO_FOLLOWUP_PHRASES: &[&str] = &[
    "no actionable follow-up",
    "no actionable follow up",
    "no follow-up items",
    "no follow up items",
    "no new backlog item came out of this change",
    "no new backlog items came out of this change",
    "nothing to add to the backlog",
    "nothing new to add to the backlog",
    "no backlog items to add",
    "no follow-up work to track",
    "no follow up work to track",
    "no new follow-up work",
    "no new follow up work",
];

pub fn response_explicitly_has_no_followups(response_text: &str) -> bool {
    let lower = response_text.to_ascii_lowercase();
    NO_FOLLOWUP_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn detect_uncaptured_recommendations(text: &str) -> RecommendationSignal {
    let mut priority_count = 0usize;
    let mut numbered_count = 0usize;
    let mut imperative_after_recommend = 0usize;
    let mut saw_header = false;
    let mut recommend_window = 0usize;
    let mut followup_count = 0usize;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            recommend_window = recommend_window.saturating_sub(1);
            continue;
        }

        if is_recommendation_header(trimmed) {
            saw_header = true;
            recommend_window = 5;
        } else if contains_recommend_keyword(trimmed) {
            recommend_window = 5;
        }

        if is_priority_label(trimmed) {
            priority_count += 1;
            recommend_window = 5;
        }

        if is_numbered_action(trimmed) {
            numbered_count += 1;
        } else if recommend_window > 0 && starts_with_action_verb(trimmed) {
            imperative_after_recommend += 1;
        }

        if has_unconditional_followup_signal(trimmed) {
            followup_count += 1;
        }

        recommend_window = recommend_window.saturating_sub(1);
    }

    if priority_count == 0
        && numbered_count == 0
        && imperative_after_recommend == 0
        && followup_count == 0
        && !saw_header
    {
        return RecommendationSignal::none();
    }

    let mut patterns = Vec::new();
    let mut confidence = 0.0f32;

    if priority_count > 0 {
        patterns.push(PatternKind::PriorityLabel);
        confidence = confidence.max(if priority_count >= 2 { 0.9 } else { 0.4 });
    }
    if numbered_count > 0 {
        patterns.push(PatternKind::NumberedActionList);
        confidence = confidence.max(if numbered_count >= 2 { 0.75 } else { 0.35 });
    }
    if saw_header {
        patterns.push(PatternKind::RecommendationHeader);
        confidence = confidence.max(0.2);
    }
    if imperative_after_recommend > 0 {
        patterns.push(PatternKind::ImperativeAfterRecommend);
        confidence = confidence.max(if imperative_after_recommend >= 2 {
            0.65
        } else {
            0.3
        });
    }
    if followup_count > 0 {
        patterns.push(PatternKind::UnconditionalFollowUp);
        confidence = confidence.max(if followup_count >= 2 { 0.85 } else { 0.7 });
    }

    if saw_header
        && (priority_count >= 2
            || numbered_count >= 2
            || imperative_after_recommend >= 2
            || followup_count >= 1)
    {
        confidence = confidence.max(0.85);
    }

    patterns.sort();
    patterns.dedup();

    RecommendationSignal {
        estimated_count: priority_count
            .max(numbered_count)
            .max(imperative_after_recommend)
            .max(followup_count),
        patterns_matched: patterns,
        confidence: confidence.min(1.0),
    }
}

fn has_unconditional_followup_signal(line: &str) -> bool {
    has_quantified_remaining_work(line) || has_unresolved_issue_followup(line)
}

fn has_quantified_remaining_work(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let keywords = [" remaining", " left to ", " outstanding", " unfinished"];
    for kw in keywords {
        if let Some(pos) = lower.find(kw) {
            // `pos` is a char boundary (keyword start), but `pos - 30` is a raw
            // byte offset that can land inside a multibyte char (e.g. an em-dash
            // in the response prose), which would panic the slice. Round the
            // window start down to the nearest char boundary first.
            let start = lower.floor_char_boundary(pos.saturating_sub(30));
            let prefix = &lower[start..pos];
            if prefix.chars().any(|c| c.is_ascii_digit() && c != '0') {
                return true;
            }
        }
    }
    false
}

fn has_unresolved_issue_followup(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if !contains_issue_or_followup_noun(&lower) {
        return false;
    }
    if contains_resolved_marker(&lower) {
        return false;
    }
    contains_unresolved_marker(&lower)
}

fn contains_issue_or_followup_noun(line: &str) -> bool {
    const ISSUE_NOUNS: &[&str] = &[
        " bug",
        " issue",
        " regression",
        " failure",
        " problem",
        " gap",
        " follow-up",
        " follow up",
        " backlog item",
        " pending item",
        " fix",
    ];

    ISSUE_NOUNS.iter().any(|marker| line.contains(marker))
}

fn contains_resolved_marker(line: &str) -> bool {
    const RESOLVED_MARKERS: &[&str] = &[
        "already fixed",
        "was fixed",
        "is fixed",
        "fixed in ",
        "resolved in ",
        "no longer reproducible",
        "closed by ",
    ];

    RESOLVED_MARKERS.iter().any(|marker| line.contains(marker))
}

fn contains_unresolved_marker(line: &str) -> bool {
    const UNRESOLVED_MARKERS: &[&str] = &[
        "still ",
        " remains ",
        " remain ",
        " unresolved",
        " still open",
        " remains open",
        " outstanding",
        " needs ",
        " need to ",
        " should ",
        " must ",
        " meant to close",
        " not fixed",
        " not resolved",
        " missing",
    ];

    UNRESOLVED_MARKERS
        .iter()
        .any(|marker| line.contains(marker))
}

fn is_priority_label(line: &str) -> bool {
    let stripped = strip_leading_markers(line);
    let lower = stripped.to_ascii_lowercase();

    for prefix in ["p0:", "p1:", "p2:", "p3:"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return starts_with_action_verb(rest);
        }
    }

    for prefix in ["[p0]", "[p1]", "[p2]", "[p3]"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return starts_with_action_verb(rest);
        }
    }

    for prefix in ["priority 0", "priority 1", "priority 2", "priority 3"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return starts_with_action_verb(rest.trim_start_matches([':', '-', ' ']));
        }
    }

    false
}

fn is_numbered_action(line: &str) -> bool {
    let stripped = line.trim_start();
    let number_end = stripped
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(stripped.len());
    if number_end == 0 || number_end == stripped.len() {
        return false;
    }

    let rest = &stripped[number_end..];
    let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) else {
        return false;
    };
    starts_with_action_verb(rest)
}

fn is_recommendation_header(line: &str) -> bool {
    let normalized = normalize_header_text(line);
    matches!(
        normalized.as_str(),
        "recommendation"
            | "recommendations"
            | "next steps"
            | "action items"
            | "todo"
            | "follow-up"
            | "follow up"
    )
}

fn contains_recommend_keyword(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("recommend")
        || lower.contains("next steps")
        || lower.contains("action items")
        || lower.contains("follow-up")
        || lower.contains("follow up")
}

fn starts_with_action_verb(line: &str) -> bool {
    let stripped = strip_leading_markers(line).trim();
    let lower = stripped.to_ascii_lowercase();
    let first = lower
        .split(|c: char| c.is_whitespace() || c == ':' || c == ',' || c == ';')
        .find(|part| !part.is_empty())
        .unwrap_or("");
    ACTION_VERBS.contains(&first)
}

fn strip_leading_markers(line: &str) -> &str {
    let mut rest = line.trim_start();

    loop {
        let trimmed = rest.trim_start();
        if let Some(next) = trimmed.strip_prefix('>') {
            rest = next;
            continue;
        }
        if let Some(next) = trimmed.strip_prefix("- [ ]") {
            rest = next;
            continue;
        }
        if let Some(next) = trimmed.strip_prefix("- [/]") {
            rest = next;
            continue;
        }
        if let Some(next) = trimmed
            .strip_prefix("- [x]")
            .or_else(|| trimmed.strip_prefix("- [X]"))
        {
            rest = next;
            continue;
        }
        if let Some(next) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            rest = next;
            continue;
        }
        return trimmed
            .trim_start_matches('*')
            .trim_start_matches('#')
            .trim();
    }
}

fn normalize_header_text(line: &str) -> String {
    let mut text = line.trim();
    while let Some(next) = text.strip_prefix('#') {
        text = next.trim_start();
    }
    text = text.trim_matches('*').trim();
    text.trim_end_matches(':').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantified_remaining_work_handles_multibyte_char_in_window() {
        // Regression: `pos - 30` is a raw byte offset that can land inside a
        // multibyte char (em-dash). Place the em-dash so `pos - 30` falls in its
        // middle byte, reproducing the panic that crashed session-check.
        // 33 ASCII bytes, then "—" (bytes 33..36), 28 bytes, then " remaining"
        // at byte 64 → pos-30 = 34, inside the em-dash.
        let line = format!("{}—{} remaining tail", "x".repeat(33), "y".repeat(28));
        // Must not panic; no qualifying non-zero digit precedes the keyword.
        assert!(!has_quantified_remaining_work(&line));
        // The true branch still works when a digit precedes the keyword, even
        // with a multibyte char in the window.
        let hit = format!("{}— 7 items remaining now", "x".repeat(33));
        assert!(has_quantified_remaining_work(&hit));
    }

    #[test]
    fn no_recommendations_no_signal() {
        let signal = detect_uncaptured_recommendations("This is explanatory prose.\nNo follow-up.");
        assert_eq!(signal.estimated_count, 0);
        assert_eq!(signal.confidence, 0.0);
    }

    #[test]
    fn no_followups_detection_accepts_explicit_proof_phrases() {
        assert!(response_explicitly_has_no_followups(
            "No new backlog item came out of this change."
        ));
        assert!(response_explicitly_has_no_followups(
            "There were no actionable follow-up items to capture."
        ));
    }

    #[test]
    fn no_followups_detection_ignores_unrelated_prose() {
        assert!(!response_explicitly_has_no_followups(
            "I did not find a third issue in this pass."
        ));
    }

    #[test]
    fn numbered_recommendations_detected() {
        let signal = detect_uncaptured_recommendations(
            "## Recommendations\n1. Add regression coverage\n2. Fix the stale state update\n3. Update the spec\n",
        );
        assert_eq!(signal.estimated_count, 3);
        assert!(signal.confidence >= 0.75);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::NumberedActionList)
        );
    }

    #[test]
    fn priority_labels_detected() {
        let signal = detect_uncaptured_recommendations(
            "P0: Fix scorer prompt\nP1: Add session-check coverage\n",
        );
        assert_eq!(signal.estimated_count, 2);
        assert!(signal.confidence >= 0.9);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::PriorityLabel)
        );
    }

    #[test]
    fn imperative_after_recommend_context_detected() {
        let signal = detect_uncaptured_recommendations(
            "What I'd recommend:\n- Add a regression test\n- Update the command spec\n",
        );
        assert_eq!(signal.estimated_count, 2);
        assert!(signal.confidence >= 0.65);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::ImperativeAfterRecommend)
        );
    }

    #[test]
    fn single_action_item_stays_low_confidence() {
        let signal = detect_uncaptured_recommendations("Recommendation:\n1. Fix it.\n");
        assert_eq!(signal.estimated_count, 1);
        assert!(signal.confidence < 0.75);
    }

    #[test]
    fn mutually_exclusive_options_do_not_look_like_recommendations() {
        let signal = detect_uncaptured_recommendations(
            "Options:\n1. Option A keeps the current behavior\n2. Option B defers the change\nChoose one.\n",
        );
        assert!(signal.estimated_count < 2 || signal.confidence < 0.5);
    }

    #[test]
    fn unconditional_followup_with_quantified_remaining_work() {
        let signal = detect_uncaptured_recommendations(
            "Completed 5 of 23 diagrams. 18 remaining to transfer.\n\nOptions to continue:\n1. Retry with rate limiting\n2. Use manual upload\n3. Wait for quota reset\n",
        );
        assert!(signal.estimated_count >= 1);
        assert!(signal.confidence >= 0.7);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn unconditional_followup_standalone_remaining() {
        let signal = detect_uncaptured_recommendations(
            "Transfer hit rate limit after 5 diagrams. 18 remaining items need processing.\n",
        );
        assert!(signal.estimated_count >= 1);
        assert!(signal.confidence >= 0.7);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn zero_remaining_does_not_trigger_followup() {
        let signal = detect_uncaptured_recommendations("All diagrams transferred. 0 remaining.\n");
        assert!(
            !signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn quantified_left_to_triggers_followup() {
        let signal = detect_uncaptured_recommendations(
            "Processed 10 pages. 15 left to migrate before cutover.\n",
        );
        assert!(signal.estimated_count >= 1);
        assert!(signal.confidence >= 0.7);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn options_without_remaining_work_no_followup() {
        let signal = detect_uncaptured_recommendations(
            "Here are your options:\n- Option A: keep current approach\n- Option B: switch to new API\n",
        );
        assert!(
            !signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn unresolved_bug_signal_triggers_followup() {
        let signal = detect_uncaptured_recommendations(
            "Because that session was still hitting the older tmux route/sync cleanup bug that #4qgx was meant to close.\n",
        );
        assert!(signal.estimated_count >= 1);
        assert!(signal.confidence >= 0.7);
        assert!(
            signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }

    #[test]
    fn resolved_bug_does_not_trigger_followup() {
        let signal = detect_uncaptured_recommendations(
            "The tmux cleanup bug was fixed in f4e646d and is no longer reproducible.\n",
        );
        assert!(
            !signal
                .patterns_matched
                .contains(&PatternKind::UnconditionalFollowUp)
        );
    }
}
