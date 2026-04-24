//! Recommendation-detection heuristics for pending-capture enforcement.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatternKind {
    PriorityLabel,
    NumberedActionList,
    RecommendationHeader,
    ImperativeAfterRecommend,
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

pub fn detect_uncaptured_recommendations(text: &str) -> RecommendationSignal {
    let mut priority_count = 0usize;
    let mut numbered_count = 0usize;
    let mut imperative_after_recommend = 0usize;
    let mut saw_header = false;
    let mut recommend_window = 0usize;

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

        recommend_window = recommend_window.saturating_sub(1);
    }

    if priority_count == 0 && numbered_count == 0 && imperative_after_recommend == 0 && !saw_header
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

    if saw_header && (priority_count >= 2 || numbered_count >= 2 || imperative_after_recommend >= 2)
    {
        confidence = confidence.max(0.85);
    }

    patterns.sort();
    patterns.dedup();

    RecommendationSignal {
        estimated_count: priority_count
            .max(numbered_count)
            .max(imperative_after_recommend),
        patterns_matched: patterns,
        confidence: confidence.min(1.0),
    }
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
    fn no_recommendations_no_signal() {
        let signal = detect_uncaptured_recommendations("This is explanatory prose.\nNo follow-up.");
        assert_eq!(signal.estimated_count, 0);
        assert_eq!(signal.confidence, 0.0);
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
}
