//! `agent:recap`: this turn's recap, with the previous turn beside it until
//! the turn commits (`#agentrecap`, `#recappair`).
//!
//! # Spec
//! - While a turn is in progress, the previous turn's recap stays and this
//!   turn is placed under it. Until this turn's recap is written, the current
//!   section is the turn's status (`<!-- recap-status -->`). A closeout that
//!   carries a recap replaces only that current section. The previous one is
//!   the last settled body, not an earlier in-progress draft.
//! - When the turn commits, [`settle_recap_in_document`] removes the previous
//!   turn's recap. The completed document keeps only the current turn's recap,
//!   so the component never accumulates history across committed turns. A
//!   current section that is still only turn status is not a recap: commit
//!   restores the previous recap instead of keeping the status.
//! - A marker `patch=` value or project config cannot change that. The stored
//!   current body is the patch text trimmed of surrounding whitespace plus one
//!   trailing newline.
//! - It is agent-authored and informational. Its body never starts a turn and is
//!   never read as a prompt or a queue row.
//! - A document that predates the component gains it the first time a closeout
//!   carries a recap, at the canonical slot directly below `agent:status`.
//! - An agent-authored strict template closeout requires a non-blank recap and
//!   is refused before capture when the recap is missing or blank. Binary-owned
//!   replay, including harness Stop hooks, still lands without one and leaves
//!   the previous recap in place. A blank recap counts as no recap.
//! - [`recap_format_findings`] checks only what is cheap and deterministic, and
//!   its findings are advisory. The format itself lives in `runbooks/recap.md`.
//! - A recommended decision nests the `#recommendationinrow` row
//!   `- **Recommendation:** **<label>** — <reason>` under that numbered item.
//!   The row is display only: findings never turn the recap into choices
//!   (`#recaprecommendationrow`).
//!
//! # Agentic Contracts
//! - [`replace_recap_in_document`] changes nothing outside the recap section.
//! - [`project_in_progress_turn_status`] changes nothing outside the recap
//!   section. A document with no settled recap is unchanged, and a current
//!   section that is already this turn's recap stays.
//! - [`settle_recap_in_document`] changes nothing outside the recap section, and
//!   it is idempotent on a settled body. A current section that is still only
//!   turn status restores the previous recap.
//! - [`normalize_recap_body`] is idempotent, so replaying a closeout is a no-op.
//!
//! # Evals
//! - stage_keeps_the_previous_recap_until_settle
//! - in_progress_status_stays_with_the_previous_turn_until_commit
//! - replace_creates_the_section_below_status_and_nothing_else
//! - format_findings_accept_the_full_shape_and_any_omitted_section
//! - decision_recommendation_uses_the_rendered_marker

use std::fmt;

use anyhow::{Context, Result};

use crate::element;

/// Component name, without the `agent:` prefix.
pub const RECAP_COMPONENT: &str = "recap";

/// Section inserted into a document that has no `agent:recap` yet.
pub const RECAP_SECTION: &str =
    "\n## Recap <!-- agent:recap patch=replace -->\n<!-- /agent:recap -->\n";

/// Fixed section labels, in the order a recap uses them. Every one is optional.
pub const RECAP_SECTION_LABELS: [&str; 3] = [
    "Queue rows this cycle:",
    "Still can't close:",
    DECISIONS_LABEL,
];

const DECISIONS_LABEL: &str = "Your decisions, most urgent first:";

/// Trim a recap to the body a closeout stores. `None` means the text carries no
/// recap at all, which leaves the document's previous recap in place.
pub fn normalize_recap_body(raw: &str) -> Option<String> {
    let body = raw.trim();
    (!body.is_empty()).then(|| format!("{body}\n"))
}

/// Visible label above this turn's recap while the previous turn is still shown.
pub const CURRENT_TURN_LABEL: &str = "Current turn";

/// Visible label above the previous turn's recap while this turn is in progress.
pub const PREVIOUS_TURN_LABEL: &str = "Previous turn";

const CURRENT_OPEN: &str = "<!-- recap-current -->";
const CURRENT_CLOSE: &str = "<!-- /recap-current -->";
const PREVIOUS_OPEN: &str = "<!-- recap-previous -->";
const PREVIOUS_CLOSE: &str = "<!-- /recap-previous -->";
const STATUS_OPEN: &str = "<!-- recap-status -->";
const STATUS_CLOSE: &str = "<!-- /recap-status -->";

/// Status shown in the current section from admit until a later status write
/// or this turn's recap replaces it.
pub const IN_PROGRESS_TURN_STATUS: &str = "Turn in progress.";

struct InProgressRecap {
    current: String,
    previous: String,
}

/// The in-progress body: the previous turn's recap, then this turn's recap
/// under it.
///
/// `current` and `previous` are normalized bodies (trimmed, one trailing
/// newline). The markers are reserved; a recap that already contains one is
/// stored alone so the pair cannot be parsed back wrong.
pub fn render_in_progress_recap(current: &str, previous: &str) -> String {
    format!(
        "{PREVIOUS_TURN_LABEL}\n{PREVIOUS_OPEN}\n{previous}{PREVIOUS_CLOSE}\n\n{CURRENT_TURN_LABEL}\n{CURRENT_OPEN}\n{current}{CURRENT_CLOSE}\n"
    )
}

fn reserved_recap_marker(text: &str) -> bool {
    text.contains(CURRENT_OPEN)
        || text.contains(CURRENT_CLOSE)
        || text.contains(PREVIOUS_OPEN)
        || text.contains(PREVIOUS_CLOSE)
}

fn status_marker(text: &str) -> bool {
    text.contains(STATUS_OPEN) || text.contains(STATUS_CLOSE)
}

/// The current section is turn status, not this turn's recap. Commit must not
/// keep it.
fn is_turn_status_body(body: &str) -> bool {
    let trimmed = body.trim();
    trimmed.starts_with(STATUS_OPEN)
        && trimmed.ends_with(STATUS_CLOSE)
        && trimmed.matches(STATUS_OPEN).count() == 1
        && trimmed.matches(STATUS_CLOSE).count() == 1
}

fn render_turn_status(status: &str) -> String {
    format!("{STATUS_OPEN}\n{status}{STATUS_CLOSE}\n")
}

const TURN_STATUS_EXCERPT_CHARS: usize = 400;

/// First prose paragraph of a response or plain status, capped for the current
/// section. Headings, fences, and HTML comments are not status. Empty input
/// becomes [`IN_PROGRESS_TURN_STATUS`].
pub fn excerpt_turn_status(text: &str) -> String {
    let paragraph = first_prose_paragraph(text);
    if paragraph.is_empty() {
        return IN_PROGRESS_TURN_STATUS.to_string();
    }
    truncate_status_excerpt(&paragraph, TURN_STATUS_EXCERPT_CHARS)
}

fn first_prose_paragraph(text: &str) -> String {
    let mut in_fence = false;
    let mut parts = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !parts.is_empty() {
                break;
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            if !parts.is_empty() {
                break;
            }
            continue;
        }
        if is_non_prose_status_line(trimmed) {
            if !parts.is_empty() {
                break;
            }
            continue;
        }
        parts.push(trimmed);
    }
    parts.join(" ")
}

fn is_non_prose_status_line(trimmed: &str) -> bool {
    trimmed.starts_with("<!--")
        || trimmed.starts_with("-->")
        || trimmed.starts_with("### Re:")
        || is_status_heading(trimmed)
}

fn is_status_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ')
}

fn truncate_status_excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    let cut = truncated
        .rfind(char::is_whitespace)
        .unwrap_or(truncated.len());
    let mut out = truncated[..cut].trim_end().to_string();
    if out.is_empty() {
        out = truncated;
    }
    out.push('…');
    out
}

/// Keep the previous turn and show `status` in the current section until this
/// turn's recap replaces it.
///
/// A blank status, a status that contains a marker, or a document with no
/// settled recap is unchanged. A current section that is already this turn's
/// recap stays. Replaying the same status is byte-identical.
pub fn project_in_progress_turn_status(doc: &str, status: &str) -> Result<String> {
    let Some(status) = normalize_recap_body(status) else {
        return Ok(doc.to_string());
    };
    if status_marker(&status) || reserved_recap_marker(&status) || status.contains("<!--") {
        return Ok(doc.to_string());
    }
    let components = element::parse(doc).context("failed to parse components")?;
    let Some(recap) = components
        .iter()
        .find(|component| component.name == RECAP_COMPONENT)
    else {
        return Ok(doc.to_string());
    };
    let existing = recap.content(doc);
    let status_body = render_turn_status(&status);
    let staged = if let Some(pair) = parse_in_progress_recap(existing) {
        if !is_turn_status_body(&pair.current) {
            return Ok(doc.to_string());
        }
        if reserved_recap_marker(&pair.previous) || is_turn_status_body(&pair.previous) {
            return Ok(doc.to_string());
        }
        if pair.current == status_body && current_is_under_previous(existing) {
            return Ok(doc.to_string());
        }
        render_in_progress_recap(&status_body, &pair.previous)
    } else if is_turn_status_body(existing) {
        if normalize_recap_body(existing).as_deref() == Some(status_body.as_str()) {
            return Ok(doc.to_string());
        }
        status_body
    } else {
        match normalize_recap_body(existing) {
            Some(previous)
                if !reserved_recap_marker(&previous) && !is_turn_status_body(&previous) =>
            {
                render_in_progress_recap(&status_body, &previous)
            }
            _ => return Ok(doc.to_string()),
        }
    };
    if staged == existing {
        return Ok(doc.to_string());
    }
    Ok(recap.replace_content(doc, &staged))
}

fn marked_section(body: &str, open: &str, close: &str) -> Option<String> {
    let open_at = body.find(open)?;
    let after_open = open_at + open.len();
    let close_at = body[after_open..].find(close)? + after_open;
    let inner = &body[after_open..close_at];
    if reserved_recap_marker(inner) {
        return None;
    }
    normalize_recap_body(inner)
}

/// `recap-current` follows `recap-previous`. A body that still has this turn
/// above the previous one is readable, but it is not the shape a closeout writes.
fn current_is_under_previous(body: &str) -> bool {
    match (body.find(PREVIOUS_OPEN), body.find(CURRENT_OPEN)) {
        (Some(previous), Some(current)) => previous < current,
        _ => false,
    }
}

fn parse_in_progress_recap(body: &str) -> Option<InProgressRecap> {
    if body.matches(CURRENT_OPEN).count() != 1
        || body.matches(CURRENT_CLOSE).count() != 1
        || body.matches(PREVIOUS_OPEN).count() != 1
        || body.matches(PREVIOUS_CLOSE).count() != 1
    {
        return None;
    }
    let current_open = body.find(CURRENT_OPEN)?;
    let current_close = body.find(CURRENT_CLOSE)?;
    let previous_open = body.find(PREVIOUS_OPEN)?;
    let previous_close = body.find(PREVIOUS_CLOSE)?;
    if current_close < current_open || previous_close < previous_open {
        return None;
    }
    let current_span = current_open..current_close + CURRENT_CLOSE.len();
    let previous_span = previous_open..previous_close + PREVIOUS_CLOSE.len();
    let overlaps = current_span.start < previous_span.end && previous_span.start < current_span.end;
    if overlaps {
        return None;
    }
    Some(InProgressRecap {
        current: marked_section(body, CURRENT_OPEN, CURRENT_CLOSE)?,
        previous: marked_section(body, PREVIOUS_OPEN, PREVIOUS_CLOSE)?,
    })
}

/// Body to store when `current` arrives and the turn is still in progress.
///
/// A settled body becomes the previous turn. An in-progress body keeps that
/// same previous turn and replaces only the current section. No previous body,
/// or a reserved marker that would make the pair unparsable, stores `current`
/// alone.
fn stage_recap_body(existing: &str, current: &str) -> String {
    if reserved_recap_marker(current) || status_marker(current) {
        return current.to_string();
    }
    // Turn status is not a previous recap. This turn's recap replaces it.
    if is_turn_status_body(existing) {
        return current.to_string();
    }
    if let Some(pair) = parse_in_progress_recap(existing) {
        if pair.current == current && current_is_under_previous(existing) {
            // Exact replay of the canonical order stays byte-identical.
            return existing.to_string();
        }
        if reserved_recap_marker(&pair.previous) || is_turn_status_body(&pair.previous) {
            return current.to_string();
        }
        return render_in_progress_recap(current, &pair.previous);
    }
    match normalize_recap_body(existing) {
        Some(previous)
            if previous != current
                && !reserved_recap_marker(&previous)
                && !is_turn_status_body(&previous) =>
        {
            render_in_progress_recap(current, &previous)
        }
        _ => current.to_string(),
    }
}

/// Show `raw` as this turn's recap, under the previous turn's recap, until
/// [`settle_recap_in_document`]. Creates the section when the document has
/// none. A blank `raw` returns the document unchanged. The previous recap
/// remains; `raw` does not replace it.
pub fn replace_recap_in_document(doc: &str, raw: &str) -> Result<String> {
    let Some(current) = normalize_recap_body(raw) else {
        return Ok(doc.to_string());
    };
    let doc = ensure_recap_component_in_document(doc)?;
    let components = element::parse(&doc).context("failed to parse components")?;
    let recap = components
        .iter()
        .find(|component| component.name == RECAP_COMPONENT)
        .context("document has no recap component after insertion")?;
    let body = stage_recap_body(recap.content(&doc), &current);
    if body == recap.content(&doc) {
        return Ok(doc);
    }
    Ok(recap.replace_content(&doc, &body))
}

/// Drop the previous turn's recap. A settled body, or a document with no recap,
/// is unchanged. A current section that is still only turn status is not kept:
/// the previous recap is restored, and a lone status body is cleared.
pub fn settle_recap_in_document(doc: &str) -> Result<String> {
    let components = element::parse(doc).context("failed to parse components")?;
    let Some(recap) = components
        .iter()
        .find(|component| component.name == RECAP_COMPONENT)
    else {
        return Ok(doc.to_string());
    };
    let content = recap.content(doc);
    if let Some(pair) = parse_in_progress_recap(content) {
        let settled = if is_turn_status_body(&pair.current) {
            pair.previous
        } else {
            pair.current
        };
        if settled == content {
            return Ok(doc.to_string());
        }
        return Ok(recap.replace_content(doc, &settled));
    }
    if is_turn_status_body(content) {
        return Ok(recap.replace_content(doc, ""));
    }
    Ok(doc.to_string())
}

/// Insert an empty `agent:recap` section when the document has none.
fn ensure_recap_component_in_document(doc: &str) -> Result<String> {
    let components = element::parse(doc).context("failed to parse components")?;
    if components
        .iter()
        .any(|component| component.name == RECAP_COMPONENT)
    {
        return Ok(doc.to_string());
    }
    let offset =
        element::canonical_managed_component_insertion_offset(doc, &components, RECAP_COMPONENT);
    let (before, after) = doc.split_at(offset);
    let mut out = String::with_capacity(doc.len() + RECAP_SECTION.len() + 2);
    out.push_str(before);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(RECAP_SECTION);
    if !after.is_empty() && !after.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(after);
    Ok(out)
}

/// One deterministic departure from the shared recap format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecapFormatFinding {
    /// The recap must open with the outcome paragraph.
    MissingOutcomeParagraph,
    /// A recap uses plain section labels, not Markdown headings.
    Heading { line: String },
    /// Section labels must keep their fixed order.
    SectionOutOfOrder { label: &'static str },
    /// A section label must introduce a list.
    SectionWithoutList { label: &'static str },
    /// Operator decisions must be a numbered list.
    DecisionsNotNumbered,
    /// A decision recommends in prose instead of the rendered marker.
    DecisionRecommendsInProse,
    /// A `**Recommendation:**` line is not the rendered marker.
    DecisionRecommendationMalformed { line: String },
    /// A numbered decision names more than one recommendation.
    DecisionRecommendationRepeated,
    /// A recommendation's reason restates itself instead of a tradeoff.
    DecisionRecommendationTautology { label: String },
}

impl fmt::Display for RecapFormatFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOutcomeParagraph => {
                f.write_str("open with the outcome paragraph, before any label or list")
            }
            Self::Heading { line } => {
                write!(
                    f,
                    "use a plain section label instead of the heading `{line}`"
                )
            }
            Self::SectionOutOfOrder { label } => {
                write!(
                    f,
                    "`{label}` is out of order; the labels have a fixed order"
                )
            }
            Self::SectionWithoutList { label } => {
                write!(f, "`{label}` must introduce a list; omit an empty section")
            }
            Self::DecisionsNotNumbered => {
                write!(f, "number the items under `{DECISIONS_LABEL}`")
            }
            Self::DecisionRecommendsInProse => write!(
                f,
                "nest `- **Recommendation:** **<label>** — <reason>` under the numbered decision instead of `I recommend` or `My recommendation`"
            ),
            Self::DecisionRecommendationMalformed { line } => write!(
                f,
                "write `{line}` as `- **Recommendation:** **<label>** — <reason>` nested under its numbered decision"
            ),
            Self::DecisionRecommendationRepeated => write!(
                f,
                "a numbered decision has more than one `**Recommendation:**`; keep one"
            ),
            Self::DecisionRecommendationTautology { label } => write!(
                f,
                "recommendation `{label}` needs a concrete contextual reason, not a tautological recommendation"
            ),
        }
    }
}

/// Check a recap body against the deterministic part of the shared format.
/// Findings are advisory: a closeout never fails on them.
pub fn recap_format_findings(body: &str) -> Vec<RecapFormatFinding> {
    let lines = prose_lines(body);
    let mut findings = Vec::new();
    if lines.first().is_none_or(|line| !is_outcome_paragraph(line)) {
        findings.push(RecapFormatFinding::MissingOutcomeParagraph);
    }
    let mut last_rank = None;
    for (index, line) in lines.iter().enumerate() {
        if is_heading(line) {
            findings.push(RecapFormatFinding::Heading {
                line: line.trim().to_string(),
            });
        }
        let Some(rank) = label_rank(line) else {
            continue;
        };
        let label = RECAP_SECTION_LABELS[rank];
        if last_rank.is_some_and(|last| rank < last) {
            findings.push(RecapFormatFinding::SectionOutOfOrder { label });
        }
        last_rank = Some(rank);
        let items = section_items(&lines[index + 1..]);
        if items.is_empty() {
            findings.push(RecapFormatFinding::SectionWithoutList { label });
        } else if label == DECISIONS_LABEL && items.iter().any(|item| !is_numbered_item(item)) {
            findings.push(RecapFormatFinding::DecisionsNotNumbered);
        }
    }
    findings.extend(decision_recommendation_findings(&lines));
    findings
}

/// Recommendations under `Your decisions`, and only there (`#recaprecommendationrow`).
///
/// The marker is the row `#recommendationinrow` renders above `**Choices:**`.
/// Prose such as `I recommend hold` is a finding, not a guess of which choice
/// it meant, and a well-formed row is never turned into a choice.
fn decision_recommendation_findings(lines: &[&str]) -> Vec<RecapFormatFinding> {
    let Some(start) = lines
        .iter()
        .position(|line| label_rank(line) == Some(RECAP_SECTION_LABELS.len() - 1))
    else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    let mut index = start + 1;
    while index < lines.len() {
        let line = lines[index];
        if !line.starts_with(char::is_whitespace) && !is_list_item(line) {
            break;
        }
        if is_numbered_item(line) {
            let item_start = index;
            index += 1;
            while index < lines.len() && lines[index].starts_with(char::is_whitespace) {
                index += 1;
            }
            findings.extend(findings_for_decision(&lines[item_start..index]));
            continue;
        }
        findings.extend(findings_for_loose_recommendation(line));
        index += 1;
    }
    findings
}

fn findings_for_decision(lines: &[&str]) -> Vec<RecapFormatFinding> {
    let mut findings = Vec::new();
    if lines.iter().copied().any(contains_prose_recommendation) {
        findings.push(RecapFormatFinding::DecisionRecommendsInProse);
    }
    if lines[0].contains(RECOMMENDATION_ROW_PREFIX) {
        findings.push(RecapFormatFinding::DecisionRecommendationMalformed {
            line: lines[0].trim().to_string(),
        });
    }
    let mut markers = 0usize;
    for line in &lines[1..] {
        if !line.contains(RECOMMENDATION_ROW_PREFIX) {
            continue;
        }
        markers += 1;
        match parse_recommendation_marker(line) {
            Some((label, reason)) if rationale_states_a_reason(label, reason) => {}
            Some((label, _)) => {
                findings.push(RecapFormatFinding::DecisionRecommendationTautology {
                    label: label.to_string(),
                });
            }
            None => findings.push(RecapFormatFinding::DecisionRecommendationMalformed {
                line: line.trim().to_string(),
            }),
        }
    }
    if markers > 1 {
        findings.push(RecapFormatFinding::DecisionRecommendationRepeated);
    }
    findings
}

fn findings_for_loose_recommendation(line: &str) -> Vec<RecapFormatFinding> {
    let mut findings = Vec::new();
    if contains_prose_recommendation(line) {
        findings.push(RecapFormatFinding::DecisionRecommendsInProse);
    }
    if line.contains(RECOMMENDATION_ROW_PREFIX) {
        findings.push(RecapFormatFinding::DecisionRecommendationMalformed {
            line: line.trim().to_string(),
        });
    }
    findings
}

const RECOMMENDATION_ROW_PREFIX: &str = "**Recommendation:**";

/// The rendered `#recommendationinrow` row: `- **Recommendation:** **<label>** — <reason>`.
fn parse_recommendation_marker(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim();
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))?
        .trim_start();
    let rest = rest
        .strip_prefix(RECOMMENDATION_ROW_PREFIX)?
        .trim_start()
        .strip_prefix("**")?;
    let (label, after) = rest.split_once("**")?;
    let label = label.trim();
    let reason = after.trim_start().strip_prefix('—')?.trim();
    if label.is_empty() || reason.is_empty() || label.contains('*') {
        return None;
    }
    Some((label, reason))
}

fn contains_prose_recommendation(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    contains_phrase(&lower, "i recommend") || contains_phrase(&lower, "my recommendation")
}

fn contains_phrase(haystack: &str, phrase: &str) -> bool {
    haystack.match_indices(phrase).any(|(index, _)| {
        let before = index == 0 || !haystack.as_bytes()[index - 1].is_ascii_alphanumeric();
        let after = index + phrase.len();
        let after = after >= haystack.len() || !haystack.as_bytes()[after].is_ascii_alphanumeric();
        before && after
    })
}

/// Same bar as a queue recommendation: at least two words that are neither the
/// choice label nor recommendation filler. `agent-doc-element` does not depend
/// on `agent-doc-queue`, so this stays beside `rationale_states_a_reason` there.
fn rationale_states_a_reason(label: &str, reason: &str) -> bool {
    let label = label.to_ascii_lowercase();
    let reason = reason.to_ascii_lowercase();
    let choice_words = label
        .trim_end_matches("(recommended)")
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<std::collections::HashSet<_>>();
    let substantive_words = reason
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .filter(|word| !choice_words.contains(word))
        .filter(|word| {
            !matches!(
                *word,
                "a" | "an"
                    | "and"
                    | "be"
                    | "best"
                    | "better"
                    | "because"
                    | "choice"
                    | "choose"
                    | "chosen"
                    | "it"
                    | "is"
                    | "option"
                    | "prefer"
                    | "preferred"
                    | "recommend"
                    | "recommendation"
                    | "recommended"
                    | "select"
                    | "selected"
                    | "should"
                    | "that"
                    | "the"
                    | "this"
                    | "to"
            )
        })
        .take(2)
        .count();
    substantive_words >= 2
}

/// Non-blank lines outside fenced code, so a quoted example is never checked.
fn prose_lines(body: &str) -> Vec<&str> {
    let mut in_fence = false;
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence && !line.trim().is_empty()
        })
        .collect()
}

fn label_rank(line: &str) -> Option<usize> {
    let line = line.trim().replace('\u{2019}', "'");
    RECAP_SECTION_LABELS
        .iter()
        .position(|label| *label == line.trim_matches('*'))
}

/// Top-level list items directly under a label, up to the next prose line.
fn section_items<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .take_while(|line| line.starts_with(char::is_whitespace) || is_list_item(line))
        .filter(|line| is_list_item(line))
        .copied()
        .collect()
}

fn is_outcome_paragraph(line: &str) -> bool {
    !is_heading(line)
        && !is_list_item(line)
        && !line.trim_start().starts_with('>')
        && label_rank(line).is_none()
}

fn is_heading(line: &str) -> bool {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

fn is_list_item(line: &str) -> bool {
    ["- ", "* ", "+ "]
        .iter()
        .any(|marker| line.starts_with(marker))
        || is_numbered_item(line)
}

fn is_numbered_item(line: &str) -> bool {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_RECAP: &str = concat!(
        "The cycle is committed at abc123, and no workers are running. Nothing you need to read changed.\n\n",
        "The release is waiting on approval. The review is requested; I did not check the mirror.\n\n",
        "Queue rows this cycle:\n",
        "- Closed: the docs row.\n",
        "- Waiting on you (🙋): the release row.\n\n",
        "Still can't close:\n",
        "- do [#example]: the merge is still blocked.\n\n",
        "Your decisions, most urgent first:\n",
        "1. Release: ship or hold.\n",
        "   - **Recommendation:** **hold** — the mirror is unchecked.\n",
        "2. Docs: publish or hold.\n",
    );

    fn doc() -> String {
        concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status <!-- agent:status patch=replace -->\nReady.\n<!-- /agent:status -->\n\n",
            "## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
            "## Exchange <!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .to_string()
    }

    fn recap_body(doc: &str) -> String {
        element::parse(doc)
            .unwrap()
            .iter()
            .find(|component| component.name == RECAP_COMPONENT)
            .unwrap()
            .content(doc)
            .to_string()
    }

    #[test]
    fn replace_creates_the_section_below_status_and_nothing_else() {
        let before = doc();
        let after = replace_recap_in_document(&before, "\n\nDone.\n\n").unwrap();

        assert_eq!(recap_body(&after), "Done.\n");
        let status = after.find("<!-- /agent:status -->").unwrap();
        let recap = after.find("<!-- agent:recap patch=replace -->").unwrap();
        let active = after.find("<!-- agent:active priority -->").unwrap();
        assert!(status < recap && recap < active, "{after}");
        let without_body = after.replacen("Done.\n", "", 1);
        assert_eq!(without_body.replacen(RECAP_SECTION, "", 1), before);
    }

    #[test]
    fn stage_keeps_the_previous_recap_until_settle() {
        let first = replace_recap_in_document(&doc(), "First turn.").unwrap();
        let second = replace_recap_in_document(&first, "Second turn.").unwrap();

        assert_eq!(
            recap_body(&second),
            render_in_progress_recap("Second turn.\n", "First turn.\n")
        );
        assert!(second.contains("Current turn"));
        assert!(second.contains("Previous turn"));
        let current = second.find("Second turn.").unwrap();
        let previous = second.find("First turn.").unwrap();
        assert!(
            previous < current,
            "recap-current must be under recap-previous and must not replace it:\n{second}"
        );
        assert!(second.find(PREVIOUS_OPEN).unwrap() < second.find(CURRENT_OPEN).unwrap());
        assert_eq!(second.matches("<!-- agent:recap").count(), 1);

        let third = replace_recap_in_document(&second, "Third turn.").unwrap();
        assert_eq!(
            recap_body(&third),
            render_in_progress_recap("Third turn.\n", "First turn.\n")
        );
        assert!(!third.contains("Second turn."));

        let settled = settle_recap_in_document(&third).unwrap();
        assert_eq!(recap_body(&settled), "Third turn.\n");
        assert!(!settled.contains("First turn."));
        assert!(!settled.contains("Previous turn"));
        assert_eq!(settle_recap_in_document(&settled).unwrap(), settled);
        assert_eq!(
            replace_recap_in_document(&settled, "Third turn.\n").unwrap(),
            settled
        );
    }

    /// Until this turn's recap is written, the previous turn stays and the
    /// current section is turn status. Commit drops that pair. A status-only
    /// current section is not the completed recap: commit restores the previous
    /// turn.
    #[test]
    fn in_progress_status_stays_with_the_previous_turn_until_commit() {
        let settled = replace_recap_in_document(&doc(), "First turn.").unwrap();
        assert_eq!(recap_body(&settled), "First turn.\n");

        let shown = project_in_progress_turn_status(&settled, IN_PROGRESS_TURN_STATUS).unwrap();
        let status_body = format!("{STATUS_OPEN}\n{IN_PROGRESS_TURN_STATUS}\n{STATUS_CLOSE}\n");
        assert_eq!(
            recap_body(&shown),
            render_in_progress_recap(&status_body, "First turn.\n")
        );
        let previous = shown.find("First turn.").unwrap();
        let status = shown.find(IN_PROGRESS_TURN_STATUS).unwrap();
        assert!(previous < status, "{shown}");
        assert!(shown.find(PREVIOUS_OPEN).unwrap() < shown.find(CURRENT_OPEN).unwrap());
        assert_eq!(
            project_in_progress_turn_status(&shown, IN_PROGRESS_TURN_STATUS).unwrap(),
            shown
        );

        let updated = project_in_progress_turn_status(&shown, "Still reading the queue.").unwrap();
        assert!(updated.contains("Still reading the queue."));
        assert!(!updated.contains(IN_PROGRESS_TURN_STATUS));
        assert!(updated.contains("First turn."));
        assert_eq!(
            project_in_progress_turn_status(&updated, "Still reading the queue.").unwrap(),
            updated
        );

        let with_recap = replace_recap_in_document(&updated, "Second turn.").unwrap();
        assert_eq!(
            recap_body(&with_recap),
            render_in_progress_recap("Second turn.\n", "First turn.\n")
        );
        assert!(!with_recap.contains(STATUS_OPEN));
        assert_eq!(
            project_in_progress_turn_status(&with_recap, "A later status.").unwrap(),
            with_recap,
            "status must not replace a recap already written for this turn"
        );

        let committed = settle_recap_in_document(&with_recap).unwrap();
        assert_eq!(recap_body(&committed), "Second turn.\n");
        assert!(!committed.contains("First turn."));
        assert!(!committed.contains("Previous turn"));

        let again = project_in_progress_turn_status(&committed, "Checking the mirror.").unwrap();
        assert!(again.contains("Second turn."));
        assert!(again.contains("Checking the mirror."));
        assert!(again.find("Second turn.").unwrap() < again.find("Checking the mirror.").unwrap());
        let restored = settle_recap_in_document(&again).unwrap();
        assert_eq!(recap_body(&restored), "Second turn.\n");
        assert!(!restored.contains("Checking the mirror."));
        assert!(!restored.contains(STATUS_OPEN));

        let bare = doc();
        assert_eq!(
            project_in_progress_turn_status(&bare, IN_PROGRESS_TURN_STATUS).unwrap(),
            bare
        );
        assert_eq!(
            project_in_progress_turn_status(&settled, " \n").unwrap(),
            settled
        );
        assert_eq!(
            project_in_progress_turn_status(&settled, "see <!-- recap-current -->").unwrap(),
            settled
        );
        assert_eq!(
            project_in_progress_turn_status(&settled, "<!-- status:sync -->\nready\n").unwrap(),
            settled
        );

        let empty = ensure_recap_component_in_document(&doc()).unwrap();
        assert_eq!(
            project_in_progress_turn_status(&empty, IN_PROGRESS_TURN_STATUS).unwrap(),
            empty
        );

        let lone = settled.replacen(
            "First turn.\n",
            &format!("{STATUS_OPEN}\nWorking.\n{STATUS_CLOSE}\n"),
            1,
        );
        assert_eq!(recap_body(&settle_recap_in_document(&lone).unwrap()), "");
        assert_eq!(
            excerpt_turn_status("### Re: topic\n\nThe mirror is unchecked.\n\nQueue rows:"),
            "The mirror is unchecked."
        );
    }

    /// A document that still has this turn above the previous one keeps that
    /// previous body. The next closeout rewrites it so recap-current is under
    /// recap-previous instead of replacing the previous recap.
    #[test]
    fn legacy_current_first_recap_is_kept_under_previous() {
        let legacy = format!(
            "{CURRENT_TURN_LABEL}\n{CURRENT_OPEN}\nSecond turn.\n{CURRENT_CLOSE}\n\n{PREVIOUS_TURN_LABEL}\n{PREVIOUS_OPEN}\nFirst turn.\n{PREVIOUS_CLOSE}\n"
        );
        let document =
            format!("<!-- agent:recap patch=replace -->\n{legacy}<!-- /agent:recap -->\n");
        let rewritten = replace_recap_in_document(&document, "Second turn.").unwrap();
        assert_eq!(
            recap_body(&rewritten),
            render_in_progress_recap("Second turn.\n", "First turn.\n")
        );
        let next = replace_recap_in_document(&document, "Third turn.").unwrap();
        let body = recap_body(&next);
        assert_eq!(
            body,
            render_in_progress_recap("Third turn.\n", "First turn.\n")
        );
        assert!(body.contains("First turn."));
        assert!(!body.contains("Second turn."));
        assert!(body.find(PREVIOUS_OPEN).unwrap() < body.find(CURRENT_OPEN).unwrap());
    }

    #[test]
    fn replay_of_the_same_recap_is_a_no_op() {
        let first = replace_recap_in_document(&doc(), "Same.\n").unwrap();
        assert_eq!(replace_recap_in_document(&first, "Same.\n").unwrap(), first);
    }

    #[test]
    fn blank_recap_keeps_the_previous_one_and_creates_nothing() {
        let without = doc();
        assert_eq!(
            replace_recap_in_document(&without, " \n\n").unwrap(),
            without
        );

        let with = replace_recap_in_document(&without, "Kept.").unwrap();
        assert_eq!(replace_recap_in_document(&with, "\n").unwrap(), with);
    }

    /// The opener may share a heading's line, and the first canonical successor
    /// may be the only anchor a document has. The section must land on its own
    /// lines above that heading instead of splitting it from its opener.
    #[test]
    fn replace_goes_above_a_successor_that_owns_a_heading() {
        let same_line = "## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let after = replace_recap_in_document(same_line, "Done.").unwrap();
        assert!(
            after.contains("## Exchange <!-- agent:exchange -->\n"),
            "{after}"
        );
        assert!(
            after.find("<!-- /agent:recap -->").unwrap() < after.find("## Exchange").unwrap(),
            "{after}"
        );

        let own_line = "## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let after = replace_recap_in_document(own_line, "Done.").unwrap();
        assert!(
            after.contains("## Exchange\n\n<!-- agent:exchange -->\n"),
            "{after}"
        );
        assert!(
            after.find("<!-- /agent:recap -->").unwrap() < after.find("## Exchange").unwrap(),
            "{after}"
        );
    }

    #[test]
    fn format_findings_accept_the_full_shape_and_any_omitted_section() {
        assert_eq!(recap_format_findings(FULL_RECAP), []);
        assert_eq!(recap_format_findings("One outcome paragraph.\n"), []);
        assert_eq!(
            recap_format_findings("Outcome.\n\nYour decisions, most urgent first:\n1. Ship.\n"),
            []
        );
    }

    #[test]
    fn format_findings_name_each_deterministic_departure() {
        assert_eq!(
            recap_format_findings(""),
            [RecapFormatFinding::MissingOutcomeParagraph]
        );
        assert_eq!(
            recap_format_findings("- a list first\n"),
            [RecapFormatFinding::MissingOutcomeParagraph]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\n### Decisions\n"),
            [RecapFormatFinding::Heading {
                line: "### Decisions".to_string()
            }]
        );
        assert_eq!(
            recap_format_findings(concat!(
                "Outcome.\n\n",
                "Your decisions, most urgent first:\n1. Ship.\n\n",
                "Still can't close:\n- do [#a]: blocked.\n",
            )),
            [RecapFormatFinding::SectionOutOfOrder {
                label: "Still can't close:"
            }]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\nStill can't close:\n\nNothing, really.\n"),
            [RecapFormatFinding::SectionWithoutList {
                label: "Still can't close:"
            }]
        );
        assert_eq!(
            recap_format_findings("Outcome.\n\nYour decisions, most urgent first:\n- Ship.\n"),
            [RecapFormatFinding::DecisionsNotNumbered]
        );
    }

    #[test]
    fn format_findings_read_nested_lists_and_ignore_fenced_examples() {
        assert_eq!(
            recap_format_findings(concat!(
                "Outcome.\n\n",
                "Queue rows this cycle:\n- Closed:\n  - one\n  - two\n\n",
                "```text\n# not a heading\nStill can't close:\n```\n",
            )),
            []
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Ship.\n   - detail\n"
            ),
            []
        );
    }

    #[test]
    fn format_labels_tolerate_a_typographic_apostrophe() {
        assert_eq!(
            recap_format_findings("Outcome.\n\nStill can\u{2019}t close:\n- do [#a]: blocked.\n"),
            []
        );
    }

    #[test]
    fn every_finding_renders_guidance() {
        let findings = [
            RecapFormatFinding::MissingOutcomeParagraph,
            RecapFormatFinding::Heading {
                line: "# x".to_string(),
            },
            RecapFormatFinding::SectionOutOfOrder { label: "a" },
            RecapFormatFinding::SectionWithoutList { label: "a" },
            RecapFormatFinding::DecisionsNotNumbered,
            RecapFormatFinding::DecisionRecommendsInProse,
            RecapFormatFinding::DecisionRecommendationMalformed {
                line: "- **Recommendation:** hold".to_string(),
            },
            RecapFormatFinding::DecisionRecommendationRepeated,
            RecapFormatFinding::DecisionRecommendationTautology {
                label: "hold".to_string(),
            },
        ];
        for finding in findings {
            assert!(!finding.to_string().is_empty());
        }
    }

    #[test]
    fn decision_recommendation_uses_the_rendered_marker() {
        let marked = concat!(
            "Outcome.\n\n",
            "Your decisions, most urgent first:\n",
            "1. Release: ship it or hold it.\n",
            "   - **Recommendation:** **hold** — the mirror is unchecked.\n",
            "2. Docs: publish them now or with the release.\n",
        );
        assert_eq!(recap_format_findings(marked), []);

        let prose_outside = concat!(
            "I recommend reading the notes.\n\n",
            "Your decisions, most urgent first:\n",
            "1. Ship.\n",
        );
        assert_eq!(recap_format_findings(prose_outside), []);

        let fenced = concat!(
            "Outcome.\n\n",
            "```\n",
            "Your decisions, most urgent first:\n",
            "1. I recommend hold.\n",
            "```\n",
        );
        assert_eq!(recap_format_findings(fenced), []);

        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Release: ship or hold. I recommend hold.\n"
            ),
            [RecapFormatFinding::DecisionRecommendsInProse]
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Release: ship or hold. My recommendation: hold.\n"
            ),
            [RecapFormatFinding::DecisionRecommendsInProse]
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Release: ship or hold.\n   - **Recommendation:** **hold** - the mirror is unchecked.\n"
            ),
            [RecapFormatFinding::DecisionRecommendationMalformed {
                line: "- **Recommendation:** **hold** - the mirror is unchecked.".to_string()
            }]
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. **Recommendation:** **hold** — the mirror is unchecked.\n"
            ),
            [RecapFormatFinding::DecisionRecommendationMalformed {
                line: "1. **Recommendation:** **hold** — the mirror is unchecked.".to_string()
            }]
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n1. Release: ship or hold.\n   - **Recommendation:** **hold** — it is recommended.\n"
            ),
            [RecapFormatFinding::DecisionRecommendationTautology {
                label: "hold".to_string()
            }]
        );
        assert_eq!(
            recap_format_findings(concat!(
                "Outcome.\n\nYour decisions, most urgent first:\n",
                "1. Release: ship or hold.\n",
                "   - **Recommendation:** **hold** — the mirror is unchecked.\n",
                "   - **Recommendation:** **ship** — the review is requested.\n",
            )),
            [RecapFormatFinding::DecisionRecommendationRepeated]
        );
        assert_eq!(
            recap_format_findings(
                "Outcome.\n\nYour decisions, most urgent first:\n- **Recommendation:** **hold** — the mirror is unchecked.\n"
            ),
            [
                RecapFormatFinding::DecisionsNotNumbered,
                RecapFormatFinding::DecisionRecommendationMalformed {
                    line: "- **Recommendation:** **hold** — the mirror is unchecked.".to_string()
                }
            ]
        );
    }
}
