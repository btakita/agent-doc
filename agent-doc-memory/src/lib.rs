//! Pure semantic memory ranking and result-shaping helpers for agent-doc.

use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use tsift_memory::{MemoryEvent, MemoryInsertResult};

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

pub fn count_insert_results(results: &[MemoryInsertResult]) -> (usize, usize) {
    let inserted = results.iter().filter(|result| result.inserted).count();
    (inserted, results.len().saturating_sub(inserted))
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

    fn tracked_event(source_ref: &str, item_id: &str, text: &str) -> MemoryEvent {
        MemoryEvent::new(
            MemoryEventKind::ImportedObservation,
            source_ref.to_string(),
            text.to_string(),
        )
        .with_metadata("agent_doc_surface", "tracked_work")
        .with_metadata("component", "backlog")
        .with_metadata("item_id", item_id)
        .with_metadata("state", "open")
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
}
