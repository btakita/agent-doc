//! Pure semantic memory ranking and result-shaping helpers for agent-doc.

use agent_doc_element_backlog::backlog::{self, PendingItem, PendingListMarker, PendingState};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use tsift_memory::{MemoryEvent, MemoryEventKind, MemoryInsertResult};

const TRACKED_WORK_IMPORT_SOURCE: &str = "agent-doc:tracked-work";
const EXCHANGE_IMPORT_SOURCE: &str = "agent-doc:exchange";
const MAX_RESPONSE_BODY_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
pub struct MemorySearchResult {
    pub score: f64,
    pub id: String,
    pub kind: String,
    pub source_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub source: String,
    pub source_ref: String,
    pub item_id: Option<String>,
    pub text: String,
    pub queue_index: Option<usize>,
    pub exclude_from_queue_strike: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticCompletionMatch {
    pub score: f64,
    pub candidate_source: String,
    pub candidate_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    pub candidate_text: String,
    pub matched_done_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_done_id: Option<String>,
    pub matched_done_text: String,
}

/// Which corpus a free-text queue head matched for auto-strike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStrikeMatchKind {
    Done,
    Backlog,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueStrikeMatch {
    pub score: f64,
    pub matched_kind: QueueStrikeMatchKind,
    pub candidate_index: usize,
    pub candidate_text: String,
    pub matched_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_id: Option<String>,
    pub matched_text: String,
}

/// Conservative auto-strike threshold for free-text queue heads.
pub const QUEUE_STRIKE_THRESHOLD: f64 = 1.6;

pub fn queue_prompt_target_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let tail = &text[marker + 1..];
    let id = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

pub fn format_semantic_completion_warning(candidate: &SemanticCompletionMatch) -> String {
    let candidate_id = candidate
        .candidate_id
        .as_deref()
        .map(|id| format!(" #{id}"))
        .unwrap_or_default();
    let done_id = candidate
        .matched_done_id
        .as_deref()
        .map(|id| format!(" #{id}"))
        .unwrap_or_else(|| " completed work".to_string());
    format!(
        "semantic completion candidate: {}{} may already be resolved by{} ({:.3}) at {}; candidate={:?}; match={:?}",
        candidate.candidate_source,
        candidate_id,
        done_id,
        candidate.score,
        candidate.matched_done_ref,
        candidate.candidate_text,
        candidate.matched_done_text
    )
}

pub fn semantic_completion_matches(
    candidates: &[CompletionCandidate],
    events: &[MemoryEvent],
    limit: usize,
) -> Vec<SemanticCompletionMatch> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let done_events = events
        .iter()
        .filter(|event| is_done_tracked_work_event(event))
        .cloned()
        .collect::<Vec<_>>();
    if done_events.is_empty() {
        return Vec::new();
    }

    let mut seen = BTreeSet::new();
    let mut matches = Vec::new();
    for candidate in candidates {
        let ranked = rank_events(&candidate.text, &done_events);
        for result in ranked {
            if result.score < 0.8 {
                break;
            }
            if candidate.item_id.is_some() && candidate.item_id == result.item_id {
                continue;
            }
            let key = (
                candidate.source_ref.clone(),
                result.source_ref.clone(),
                result.item_id.clone(),
            );
            if !seen.insert(key) {
                continue;
            }
            matches.push(SemanticCompletionMatch {
                score: result.score,
                candidate_source: candidate.source.clone(),
                candidate_ref: candidate.source_ref.clone(),
                candidate_id: candidate.item_id.clone(),
                candidate_text: trim_chars(&candidate.text, 320),
                matched_done_ref: result.source_ref,
                matched_done_id: result.item_id,
                matched_done_text: result.text,
            });
            break;
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.candidate_ref.cmp(&b.candidate_ref))
            .then_with(|| a.matched_done_ref.cmp(&b.matched_done_ref))
    });
    matches.truncate(limit.max(1));
    matches
}

pub fn semantic_queue_strike_matches(
    candidates: &[CompletionCandidate],
    events: &[MemoryEvent],
    threshold: f64,
    limit: usize,
) -> Vec<QueueStrikeMatch> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let done_events = events
        .iter()
        .filter(|event| is_done_tracked_work_event(event))
        .cloned()
        .collect::<Vec<_>>();
    let backlog_events = events
        .iter()
        .filter(|event| is_active_backlog_work_event(event))
        .cloned()
        .collect::<Vec<_>>();
    if done_events.is_empty() && backlog_events.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for candidate in candidates {
        if candidate.source != "queue" || candidate.item_id.is_some() {
            continue;
        }
        let Some(candidate_index) = candidate.queue_index else {
            continue;
        };
        if candidate.exclude_from_queue_strike {
            continue;
        }

        let best_done = rank_events(&candidate.text, &done_events)
            .into_iter()
            .next();
        let best_backlog = rank_events(&candidate.text, &backlog_events)
            .into_iter()
            .next();
        let chosen = match (best_done, best_backlog) {
            (Some(d), Some(b)) => {
                if d.score >= b.score {
                    Some((QueueStrikeMatchKind::Done, d))
                } else {
                    Some((QueueStrikeMatchKind::Backlog, b))
                }
            }
            (Some(d), None) => Some((QueueStrikeMatchKind::Done, d)),
            (None, Some(b)) => Some((QueueStrikeMatchKind::Backlog, b)),
            (None, None) => None,
        };
        let Some((matched_kind, result)) = chosen else {
            continue;
        };
        if result.score < threshold {
            continue;
        }
        matches.push(QueueStrikeMatch {
            score: result.score,
            matched_kind,
            candidate_index,
            candidate_text: candidate.text.clone(),
            matched_ref: result.source_ref,
            matched_id: result.item_id,
            matched_text: result.text,
        });
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.candidate_index.cmp(&b.candidate_index))
    });
    matches.truncate(limit.max(1));
    matches
}

pub fn is_done_tracked_work_event(event: &MemoryEvent) -> bool {
    event
        .metadata
        .get("agent_doc_surface")
        .is_some_and(|surface| surface == "tracked_work")
        && event
            .metadata
            .get("state")
            .is_some_and(|state| state == "done")
}

/// An active (non-done) tracked-work event sourced from an `agent:backlog`
/// component. Review/icebox surfaces are intentionally excluded so only items the
/// operator is actively tracking in the backlog count as "addresses it".
pub fn is_active_backlog_work_event(event: &MemoryEvent) -> bool {
    event
        .metadata
        .get("agent_doc_surface")
        .is_some_and(|surface| surface == "tracked_work")
        && event
            .metadata
            .get("state")
            .is_none_or(|state| state != "done")
        && event.metadata.get("component").is_some_and(|component| {
            backlog::component_matches_tracked_surface(component, "backlog")
        })
}

pub fn count_insert_results(results: &[MemoryInsertResult]) -> (usize, usize) {
    let inserted = results.iter().filter(|result| result.inserted).count();
    (inserted, results.len().saturating_sub(inserted))
}

pub fn bump_component_count(counts: &mut BTreeMap<String, usize>, component: &str, amount: usize) {
    if amount > 0 {
        *counts.entry(component.to_string()).or_insert(0) += amount;
    }
}

pub fn display_path(path: &Path) -> String {
    path.display().to_string()
}

pub fn rank_events(query: &str, events: &[MemoryEvent]) -> Vec<MemorySearchResult> {
    let query_tokens = tokenize(query);
    let query_lower = query.trim().to_ascii_lowercase();
    let mut ranked = events
        .iter()
        .filter_map(|event| {
            let score = score_event(&query_tokens, &query_lower, event);
            (score > 0.0).then(|| search_result(score, event))
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.source_ref.cmp(&b.source_ref))
            .then_with(|| a.text.cmp(&b.text))
    });
    ranked
}

pub fn dedupe_events(events: Vec<MemoryEvent>) -> Vec<MemoryEvent> {
    let mut by_key = HashMap::new();
    for event in events {
        let key = (
            event.source_ref.clone(),
            event.text.clone(),
            event.metadata.get("item_id").cloned(),
        );
        by_key.entry(key).or_insert(event);
    }
    by_key.into_values().collect()
}

pub fn trim_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

pub fn tracked_work_events(
    session_ref: &str,
    doc_hash: &str,
    component: &str,
    items: impl IntoIterator<Item = PendingItem>,
    state_override: Option<PendingState>,
) -> Vec<MemoryEvent> {
    items
        .into_iter()
        .filter(|item| !item.id.is_empty() || !item.text.trim().is_empty())
        .map(|item| tracked_work_event(session_ref, doc_hash, component, item, state_override))
        .collect()
}

pub fn tracked_work_event(
    session_ref: &str,
    doc_hash: &str,
    component: &str,
    item: PendingItem,
    state_override: Option<PendingState>,
) -> MemoryEvent {
    let state = state_override.unwrap_or(item.state);
    let state_str = pending_state_str(state);
    let mut text = format!("#{} {}", item.id, item.text.trim());
    if !item.continuation.trim().is_empty() {
        text.push('\n');
        text.push_str(item.continuation.trim());
    }
    let item_id = if item.id.is_empty() {
        format!("anon:{}", agent_doc_hash::content_hash(&text))
    } else {
        item.id.clone()
    };
    let text_hash = agent_doc_hash::content_hash(&text);
    let source_ref = format!("{session_ref}#{component}:{item_id}");
    MemoryEvent::new(MemoryEventKind::ImportedObservation, source_ref, text)
        .with_session_id(session_ref.to_string())
        .with_metadata("agent_doc_surface", "tracked_work")
        .with_metadata("component", component)
        .with_metadata("item_id", item_id.clone())
        .with_metadata("state", state_str)
        .with_import(
            TRACKED_WORK_IMPORT_SOURCE,
            format!("{doc_hash}:{component}:{item_id}:{text_hash}"),
        )
}

pub fn response_summary_events(session_ref: &str, doc_hash: &str, body: &str) -> Vec<MemoryEvent> {
    response_sections(body)
        .into_iter()
        .map(|(index, heading, text)| {
            let event_text = trim_chars(&format!("{heading}\n{text}"), MAX_RESPONSE_BODY_CHARS);
            let text_hash = agent_doc_hash::content_hash(&event_text);
            MemoryEvent::new(
                MemoryEventKind::ResponseSummary,
                format!("{session_ref}#exchange:{index}"),
                event_text,
            )
            .with_session_id(session_ref.to_string())
            .with_metadata("agent_doc_surface", "exchange")
            .with_metadata("component", "exchange")
            .with_metadata("heading", heading)
            .with_import(
                EXCHANGE_IMPORT_SOURCE,
                format!("{doc_hash}:exchange:{index}:{text_hash}"),
            )
        })
        .collect()
}

pub fn response_sections(body: &str) -> Vec<(usize, String, String)> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();

    for line in body.lines() {
        if line.starts_with("### Re:") {
            if let Some(heading) = current_heading.take() {
                let index = sections.len() + 1;
                sections.push((index, heading, current_body.trim().to_string()));
                current_body.clear();
            }
            current_heading = Some(line.trim_start_matches('#').trim().to_string());
        } else if current_heading.is_some() && !line.starts_with("<!-- agent:boundary:") {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(heading) = current_heading {
        let index = sections.len() + 1;
        sections.push((index, heading, current_body.trim().to_string()));
    }
    sections
}

pub fn parse_done_archive_items(body: &str) -> Vec<PendingItem> {
    let mut items = Vec::new();
    let mut current: Option<PendingItem> = None;

    for line in body.lines() {
        if let Some((date, id, text)) = parse_done_archive_line(line) {
            if let Some(item) = current.take() {
                items.push(item);
            }
            current = Some(PendingItem {
                marker: PendingListMarker::Bullet,
                id,
                state: PendingState::Done,
                gate_type: None,
                in_progress: false,
                text: format!("{date} {text}"),
                continuation: String::new(),
            });
        } else if let Some(item) = current.as_mut()
            && (line.starts_with(' ') || line.starts_with('\t'))
        {
            item.continuation.push_str(line);
            item.continuation.push('\n');
        }
    }

    if let Some(item) = current {
        items.push(item);
    }
    items
}

pub fn parse_done_archive_line(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix("- ")?;
    if !looks_like_iso_date_prefix(rest) {
        return None;
    }
    let date = rest[..10].to_string();
    let after_date = &rest[11..];
    let after_id = after_date.strip_prefix("[#")?;
    let end = after_id.find(']')?;
    let id = after_id[..end].trim();
    if id.is_empty() {
        return None;
    }
    let text = after_id[end + 1..].trim_start();
    Some((date, id.to_string(), text.to_string()))
}

pub fn looks_like_iso_date_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 11
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

pub fn pending_state_str(state: PendingState) -> &'static str {
    match state {
        PendingState::Open => "open",
        PendingState::Gated => "gated",
        PendingState::Done => "done",
    }
}

fn score_event(query_tokens: &BTreeSet<String>, query_lower: &str, event: &MemoryEvent) -> f64 {
    let event_text = format!(
        "{} {} {}",
        event.source_ref,
        event.text,
        event
            .metadata
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    );
    let event_lower = event_text.to_ascii_lowercase();
    let event_tokens = tokenize(&event_text);
    let overlap = query_tokens.intersection(&event_tokens).count();
    let mut score = if query_tokens.is_empty() {
        0.0
    } else {
        overlap as f64 / query_tokens.len() as f64
    };

    if !query_lower.is_empty() && event_lower.contains(query_lower) {
        score += 1.0;
    }
    if let Some(item_id) = event.metadata.get("item_id")
        && query_tokens.contains(&item_id.to_ascii_lowercase())
    {
        score += 1.5;
    }
    if event
        .metadata
        .get("agent_doc_surface")
        .is_some_and(|surface| surface == "tracked_work")
    {
        score += 0.05;
    }
    score
}

fn search_result(score: f64, event: &MemoryEvent) -> MemorySearchResult {
    MemorySearchResult {
        score,
        id: event.stable_id(),
        kind: event.kind.as_str().to_string(),
        source_ref: event.source_ref.clone(),
        component: event.metadata.get("component").cloned(),
        item_id: event.metadata.get("item_id").cloned(),
        state: event.metadata.get("state").cloned(),
        text: trim_chars(&event.text, 320),
    }
}

fn tokenize(text: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else {
            push_token(&mut tokens, &mut current);
        }
    }
    push_token(&mut tokens, &mut current);
    tokens
}

fn push_token(tokens: &mut BTreeSet<String>, current: &mut String) {
    if current.len() >= 2 {
        tokens.insert(std::mem::take(current));
    } else {
        current.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsift_memory::MemoryEventKind;

    fn tracked_event_with_state(
        source_ref: &str,
        item_id: &str,
        component: &str,
        state: &str,
        text: &str,
    ) -> MemoryEvent {
        MemoryEvent::new(
            MemoryEventKind::ImportedObservation,
            source_ref.to_string(),
            text.to_string(),
        )
        .with_metadata("agent_doc_surface", "tracked_work")
        .with_metadata("component", component)
        .with_metadata("item_id", item_id)
        .with_metadata("state", state)
    }

    fn tracked_event(source_ref: &str, item_id: &str, text: &str) -> MemoryEvent {
        tracked_event_with_state(source_ref, item_id, "backlog", "open", text)
    }

    fn done_event(source_ref: &str, item_id: &str, text: &str) -> MemoryEvent {
        tracked_event_with_state(source_ref, item_id, "done", "done", text)
    }

    fn completion_candidate(
        source: &str,
        source_ref: &str,
        item_id: Option<&str>,
        text: &str,
    ) -> CompletionCandidate {
        CompletionCandidate {
            source: source.to_string(),
            source_ref: source_ref.to_string(),
            item_id: item_id.map(str::to_string),
            text: text.to_string(),
            queue_index: None,
            exclude_from_queue_strike: false,
        }
    }

    fn queue_candidate(index: usize, text: &str, exclude: bool) -> CompletionCandidate {
        CompletionCandidate {
            source: "queue".to_string(),
            source_ref: format!("doc#queue:{index}"),
            item_id: None,
            text: text.to_string(),
            queue_index: Some(index),
            exclude_from_queue_strike: exclude,
        }
    }

    #[test]
    fn ranks_by_overlap_substring_and_item_id() {
        let events = vec![
            tracked_event(
                "doc#backlog:routes",
                "routes",
                "#routes Fix tmux route timeout",
            ),
            tracked_event(
                "doc#backlog:memrag",
                "memrag",
                "#memrag Add semantic retrieval over backlog history",
            ),
        ];

        let ranked = rank_events("semantic backlog retrieval", &events);

        assert_eq!(
            ranked.first().and_then(|result| result.item_id.as_deref()),
            Some("memrag")
        );
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn trims_by_character_boundary() {
        assert_eq!(trim_chars("abcd", 3), "abc...");
        assert_eq!(trim_chars("åßcd", 2), "åß...");
        assert_eq!(trim_chars("abcd", 4), "abcd");
    }

    #[test]
    fn dedupes_by_source_text_and_item_id() {
        let first = tracked_event("doc#backlog:memrag", "memrag", "Add semantic retrieval");
        let duplicate = tracked_event("doc#backlog:memrag", "memrag", "Add semantic retrieval");
        let other = tracked_event("doc#backlog:routes", "routes", "Fix route timeout");

        let deduped = dedupe_events(vec![first, duplicate, other]);

        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn bump_component_count_skips_zero_and_accumulates_positive_counts() {
        let mut counts = BTreeMap::new();

        bump_component_count(&mut counts, "backlog", 0);
        bump_component_count(&mut counts, "backlog", 2);
        bump_component_count(&mut counts, "backlog", 3);

        assert_eq!(counts.len(), 1);
        assert_eq!(counts.get("backlog"), Some(&5));
    }

    #[test]
    fn display_path_uses_standard_path_display() {
        assert_eq!(
            display_path(Path::new("tasks/session.md")),
            "tasks/session.md"
        );
    }

    #[test]
    fn semantic_completion_matches_done_events_and_skips_same_item_id() {
        let candidates = vec![
            completion_candidate(
                "backlog",
                "doc#backlog:cachefix",
                Some("cachefix"),
                "Repair cache duplication on save",
            ),
            queue_candidate(0, "Repair cache duplication on save", false),
        ];
        let events = vec![done_event(
            "doc#done:cachefix",
            "cachefix",
            "#cachefix Repair cache duplication on save",
        )];

        let matches = semantic_completion_matches(&candidates, &events, 5);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].candidate_source, "queue");
        assert_eq!(matches[0].matched_done_id.as_deref(), Some("cachefix"));
    }

    #[test]
    fn semantic_queue_strike_prefers_done_over_equal_backlog_match() {
        let candidates = vec![queue_candidate(
            3,
            "Repair cache duplication on save",
            false,
        )];
        let events = vec![
            done_event(
                "doc#done:cachefix",
                "cachefix",
                "#cachefix Repair cache duplication on save",
            ),
            tracked_event(
                "doc#backlog:cachefix",
                "cachefix",
                "#cachefix Repair cache duplication on save",
            ),
        ];

        let matches =
            semantic_queue_strike_matches(&candidates, &events, QUEUE_STRIKE_THRESHOLD, 5);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_kind, QueueStrikeMatchKind::Done);
        assert_eq!(matches[0].candidate_index, 3);
        assert_eq!(matches[0].matched_id.as_deref(), Some("cachefix"));
    }

    #[test]
    fn semantic_queue_strike_skips_excluded_candidate() {
        let candidates = vec![queue_candidate(0, "Repair cache duplication on save", true)];
        let events = vec![done_event(
            "doc#done:cachefix",
            "cachefix",
            "#cachefix Repair cache duplication on save",
        )];

        let matches =
            semantic_queue_strike_matches(&candidates, &events, QUEUE_STRIKE_THRESHOLD, 5);

        assert!(matches.is_empty());
    }

    #[test]
    fn semantic_event_classifiers_distinguish_done_and_active_backlog() {
        let done = done_event("doc#done:cachefix", "cachefix", "Repair cache duplication");
        let active_pending = tracked_event_with_state(
            "doc#pending:cachefix",
            "cachefix",
            "pending",
            "open",
            "Repair cache duplication",
        );
        let review = tracked_event_with_state(
            "doc#review:cachefix",
            "cachefix",
            "review",
            "open",
            "Repair cache duplication",
        );

        assert!(is_done_tracked_work_event(&done));
        assert!(!is_active_backlog_work_event(&done));
        assert!(is_active_backlog_work_event(&active_pending));
        assert!(!is_active_backlog_work_event(&review));
    }

    #[test]
    fn queue_prompt_target_id_parses_bracketed_id_like_prose() {
        assert_eq!(
            queue_prompt_target_id("[ ] #Cache-Fix_7 repair duplicate cache writes").as_deref(),
            Some("cache-fix_7")
        );
    }

    #[test]
    fn queue_prompt_target_id_returns_none_when_missing_id() {
        assert_eq!(
            queue_prompt_target_id("repair duplicate cache writes"),
            None
        );
    }

    #[test]
    fn queue_prompt_target_id_returns_none_for_empty_hash() {
        assert_eq!(
            queue_prompt_target_id("# repair duplicate cache writes"),
            None
        );
    }

    #[test]
    fn queue_prompt_target_id_normalizes_to_lowercase() {
        assert_eq!(
            queue_prompt_target_id("#ABC_Def-12").as_deref(),
            Some("abc_def-12")
        );
    }

    #[test]
    fn parse_done_archive_items_reads_archive_lines_and_continuations() {
        let items = parse_done_archive_items(
            "- 2026-06-07 [#cachefix] Repair cache duplication\n  proof: shipped\n",
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "cachefix");
        assert_eq!(items[0].state, PendingState::Done);
        assert_eq!(items[0].text, "2026-06-07 Repair cache duplication");
        assert_eq!(items[0].continuation, "  proof: shipped\n");
    }

    #[test]
    fn tracked_work_event_shapes_stable_import_metadata() {
        let event = tracked_work_event(
            "tasks/doc.md",
            "doc-hash",
            "backlog",
            PendingItem {
                marker: PendingListMarker::Bullet,
                id: "cachefix".to_string(),
                state: PendingState::Gated,
                gate_type: None,
                in_progress: false,
                text: "Repair cache duplication".to_string(),
                continuation: "  proof: pending\n".to_string(),
            },
            None,
        );

        assert_eq!(event.kind, MemoryEventKind::ImportedObservation);
        assert_eq!(event.source_ref, "tasks/doc.md#backlog:cachefix");
        assert_eq!(
            event.metadata.get("state").map(String::as_str),
            Some("gated")
        );
        assert_eq!(
            event.metadata.get("component").map(String::as_str),
            Some("backlog")
        );
        assert!(event.text.contains("proof: pending"));
    }

    #[test]
    fn response_summary_events_split_replies_and_skip_boundaries() {
        let events = response_summary_events(
            "tasks/doc.md",
            "doc-hash",
            "### Re: first\nbody\n<!-- agent:boundary:x -->\n### Re: second\nnext\n",
        );

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, MemoryEventKind::ResponseSummary);
        assert_eq!(events[0].source_ref, "tasks/doc.md#exchange:1");
        assert_eq!(
            events[0].metadata.get("heading").map(String::as_str),
            Some("Re: first")
        );
        assert_eq!(events[0].text, "Re: first\nbody");
        assert_eq!(events[1].source_ref, "tasks/doc.md#exchange:2");
    }

    #[test]
    fn formats_semantic_completion_warning() {
        let warning = format_semantic_completion_warning(&SemanticCompletionMatch {
            score: 1.75,
            candidate_source: "queue".to_string(),
            candidate_ref: "doc#queue:0".to_string(),
            candidate_id: None,
            candidate_text: "Repair cache duplication".to_string(),
            matched_done_ref: "doc#done:cachefix".to_string(),
            matched_done_id: Some("cachefix".to_string()),
            matched_done_text: "#cachefix Repair cache duplication".to_string(),
        });

        assert!(warning.contains("semantic completion candidate"));
        assert!(warning.contains("#cachefix"));
        assert!(warning.contains("1.750"));
    }
}
