//! Agent-doc session memory indexing and retrieval.
//!
//! This is the first agent-doc consumer of the shared `tsift-memory` crate:
//! the CLI writes tracked session work into `.tsift/memory.db` and performs a
//! deterministic lexical ranking pass over current + persisted events. The
//! ranking is intentionally local and cheap; heavier codebase indexing stays
//! in the tsift CLI.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tsift_memory::{
    MEMORY_CONTRACT_VERSION, MemoryEvent, MemoryEventKind, MemoryInsertResult, MemoryStore,
    default_memory_db_path, read_memory_events,
};

use crate::component;
use crate::fs_util;
use crate::pending::{self, PendingItem, PendingListMarker, PendingState};
use crate::snapshot;

const TRACKED_WORK_IMPORT_SOURCE: &str = "agent-doc:tracked-work";
const EXCHANGE_IMPORT_SOURCE: &str = "agent-doc:exchange";
const MAX_EVENT_READ_LIMIT: usize = 20_000;
const MAX_RESPONSE_BODY_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
pub struct MemoryIndexReport {
    pub contract_version: String,
    pub session_path: String,
    pub memory_db_path: String,
    pub planned_events: usize,
    pub inserted_events: usize,
    pub already_present_events: usize,
    pub component_counts: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

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

#[derive(Debug, Clone, Serialize)]
pub struct MemorySearchReport {
    pub contract_version: String,
    pub session_path: String,
    pub memory_db_path: String,
    pub query: String,
    pub indexed_current_events: usize,
    pub searched_events: usize,
    pub results: Vec<MemorySearchResult>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Which corpus a free-text queue head matched against for the
/// `#qftbklgstrike` auto-strike: a completed `agent:done` item (`Done`) or an
/// active `agent:backlog` item (`Backlog`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueStrikeMatchKind {
    /// The head is already complete — a matching item exists in `agent:done`.
    Done,
    /// The head is tracked — a matching active item exists in `agent:backlog`.
    Backlog,
}

/// A free-text `agent:queue` head that the deterministic semantic scorer matched
/// to either a completed (`agent:done`) or active-backlog (`agent:backlog`)
/// tracked-work item above the conservative auto-strike threshold
/// (`#qftbklgstrike`). Reuses the same lexical [`rank_events`] scorer that backs
/// [`semantic_completion_matches`]; the only difference is the candidate corpus
/// (done + active backlog instead of done only) and a `matched_kind`
/// discriminator so the convergence strike can name the right reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueStrikeMatch {
    pub score: f64,
    pub matched_kind: QueueStrikeMatchKind,
    /// Zero-based queue entry index of the matched free-text head (the
    /// `source_ref` suffix from [`collect_completion_candidates`]). Informational
    /// only — convergence matches on normalized head text, not this index, since
    /// earlier maintenance phases can shift entry positions.
    pub candidate_index: usize,
    /// The FULL (untruncated) free-text head prose. Convergence keys the strike
    /// on this text, so it must not be display-truncated.
    pub candidate_text: String,
    pub matched_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_id: Option<String>,
    pub matched_text: String,
}

#[derive(Debug, Clone)]
struct SessionEvents {
    events: Vec<MemoryEvent>,
    component_counts: BTreeMap<String, usize>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct CompletionCandidate {
    source: String,
    source_ref: String,
    item_id: Option<String>,
    text: String,
}

pub fn run_index(file: &Path, db: Option<&Path>, json: bool) -> Result<()> {
    let db_path = resolve_memory_db_path(file, db)?;
    let report = index_session(file, &db_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Indexed {} event(s) into {} ({} inserted, {} already present)",
            report.planned_events,
            report.memory_db_path,
            report.inserted_events,
            report.already_present_events
        );
        for (component, count) in &report.component_counts {
            println!("- {component}: {count}");
        }
        for warning in &report.warnings {
            eprintln!("[memory] warning: {warning}");
        }
    }
    Ok(())
}

pub fn run_search(
    file: &Path,
    query: &str,
    db: Option<&Path>,
    limit: usize,
    json: bool,
    rebuild: bool,
) -> Result<()> {
    let db_path = resolve_memory_db_path(file, db)?;
    let report = search_session_memory(file, &db_path, query, limit, rebuild)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Memory search for {:?} in {}",
            report.query, report.memory_db_path
        );
        if report.results.is_empty() {
            println!("No matches.");
        }
        for result in &report.results {
            let item = result
                .item_id
                .as_deref()
                .map(|id| format!(" #{id}"))
                .unwrap_or_default();
            let component = result
                .component
                .as_deref()
                .map(|c| format!(" [{c}]"))
                .unwrap_or_default();
            println!(
                "- {:.3}{}{} {}",
                result.score, component, item, result.source_ref
            );
            println!("  {}", result.text.replace('\n', " "));
        }
        for warning in &report.warnings {
            eprintln!("[memory] warning: {warning}");
        }
    }
    Ok(())
}

pub fn index_session(file: &Path, db_path: &Path) -> Result<MemoryIndexReport> {
    let session = collect_session_events(file)?;
    let mut store = MemoryStore::open_or_create(db_path)?;
    let results = store.insert_events(&session.events)?;
    let (inserted_events, already_present_events) = count_insert_results(&results);
    Ok(MemoryIndexReport {
        contract_version: MEMORY_CONTRACT_VERSION.to_string(),
        session_path: display_path(file),
        memory_db_path: display_path(db_path),
        planned_events: session.events.len(),
        inserted_events,
        already_present_events,
        component_counts: session.component_counts,
        warnings: session.warnings,
    })
}

pub fn search_session_memory(
    file: &Path,
    db_path: &Path,
    query: &str,
    limit: usize,
    rebuild: bool,
) -> Result<MemorySearchReport> {
    if query.trim().is_empty() {
        bail!("memory search query must not be empty");
    }
    if rebuild {
        let _ = index_session(file, db_path)?;
    }

    let session = collect_session_events(file)?;
    let mut events = read_memory_events(db_path, MAX_EVENT_READ_LIMIT)?;
    events.extend(session.events.clone());
    let events = dedupe_events(events);
    let mut results = rank_events(query, &events);
    results.truncate(limit.max(1));

    Ok(MemorySearchReport {
        contract_version: MEMORY_CONTRACT_VERSION.to_string(),
        session_path: display_path(file),
        memory_db_path: display_path(db_path),
        query: query.to_string(),
        indexed_current_events: session.events.len(),
        searched_events: events.len(),
        results,
        warnings: session.warnings,
    })
}

pub fn semantic_completion_matches(
    file: &Path,
    db: Option<&Path>,
    limit: usize,
) -> Result<Vec<SemanticCompletionMatch>> {
    let candidates = collect_completion_candidates(file)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let db_path = resolve_memory_db_path(file, db)?;
    let session = collect_session_events(file)?;
    let mut events = read_memory_events(&db_path, MAX_EVENT_READ_LIMIT)?;
    events.extend(session.events.clone());
    let done_events = dedupe_events(events)
        .into_iter()
        .filter(is_done_tracked_work_event)
        .collect::<Vec<_>>();
    if done_events.is_empty() {
        return Ok(Vec::new());
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
    Ok(matches)
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

/// Conservative auto-strike threshold for `#qftbklgstrike`.
///
/// The [`rank_events`] scorer awards `+1.0` when the queue head is a full
/// substring of the matched tracked-work item, plus the token-overlap fraction
/// (`0.0..=1.0`), plus `+0.05` for the tracked-work surface. A genuine
/// near-restatement of a done/backlog item therefore lands at ~1.7–1.9 (the
/// range the existing `semantic_completion_match` *warning* fires in the wild),
/// while an unrelated operator prompt that merely shares a stray common word
/// scores only its small overlap fraction (well under `1.0`, since it does not
/// contain the item as a substring).
///
/// We set the strike threshold at `1.6` — strictly **above** the `0.8` floor the
/// warning path uses and high enough that the `+1.0` substring-contains bonus is
/// effectively required (a head can only clear `1.6` without that bonus by
/// matching essentially every token, which is itself a restatement). This is the
/// false-strike safety margin: an unanswered operator prompt that is not a near
/// restatement of a tracked item can never reach `1.6`, so it is never silently
/// buried. Raising the corpus to include active backlog does not lower this bar —
/// both corpora are scored by the identical deterministic scorer against the same
/// threshold.
pub const QUEUE_STRIKE_THRESHOLD: f64 = 1.6;

/// Deterministically score every LIVE free-text `agent:queue` head against BOTH
/// the completed `agent:done` archive (case **a**: already complete) AND the
/// active `agent:backlog` items (case **b**: tracked by a backlog item), and
/// return the best match per head that clears `threshold` (`#qftbklgstrike`).
///
/// This is the auto-strike sibling of [`semantic_completion_matches`] (which only
/// scores against done and only emits a *warning*). Both reuse the same
/// [`rank_events`] lexical scorer, so the behavior stays deterministic (no LLM).
/// Id-backed heads are skipped here (they have their own done-strike path); only
/// free-text heads (no `#id`) are candidates.
pub fn semantic_queue_strike_matches(
    file: &Path,
    db: Option<&Path>,
    threshold: f64,
    limit: usize,
) -> Result<Vec<QueueStrikeMatch>> {
    let candidates: Vec<CompletionCandidate> = collect_completion_candidates(file)?
        .into_iter()
        .filter(|c| c.source == "queue" && c.item_id.is_none())
        .collect();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let db_path = resolve_memory_db_path(file, db)?;
    let session = collect_session_events(file)?;
    let mut events = read_memory_events(&db_path, MAX_EVENT_READ_LIMIT)?;
    events.extend(session.events.clone());
    let events = dedupe_events(events);

    // Two disjoint corpora, both scored by the identical deterministic scorer.
    let done_events: Vec<MemoryEvent> = events
        .iter()
        .filter(|e| is_done_tracked_work_event(e))
        .cloned()
        .collect();
    let backlog_events: Vec<MemoryEvent> = events
        .iter()
        .filter(|e| is_active_backlog_work_event(e))
        .cloned()
        .collect();
    if done_events.is_empty() && backlog_events.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for candidate in candidates {
        let candidate_index = candidate
            .source_ref
            .rsplit_once(':')
            .and_then(|(_, idx)| idx.parse::<usize>().ok());
        let Some(candidate_index) = candidate_index else {
            continue;
        };
        // `#qimpstrike`: a recurring imperative command head (`deploy`, `commit`,
        // `push`, `commit + push`, the `#spec-test-commit-push` preset, …) is an
        // executable directive, not a restatement of tracked work — it is valid
        // every time it is queued. Never strike it as "already done"/backlog,
        // even when a common verb is lexically close to many prior done items.
        // It stays drainable and is retired only once actually dispatched this
        // cycle (the answered-this-turn consume path), not by fuzzy matching.
        if crate::queue_continuation::is_recurring_imperative_head(&candidate.text) {
            continue;
        }
        // Score against both corpora; keep the single best match across both,
        // preferring a completed (done) match on a tie since "already complete"
        // is the stronger statement than "tracked".
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
    Ok(matches)
}

/// An active (non-done) tracked-work event sourced from an `agent:backlog`
/// component. Review/icebox surfaces are intentionally excluded so only items the
/// operator is actively tracking in the backlog count as "addresses it".
fn is_active_backlog_work_event(event: &MemoryEvent) -> bool {
    event
        .metadata
        .get("agent_doc_surface")
        .is_some_and(|surface| surface == "tracked_work")
        && event
            .metadata
            .get("state")
            .is_none_or(|state| state != "done")
        && event
            .metadata
            .get("component")
            .is_some_and(|component| component::is_backlog_component(component))
}

fn collect_completion_candidates(file: &Path) -> Result<Vec<CompletionCandidate>> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read session document {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let session_ref = display_path(&canonical);
    let components = component::parse(&content).context("failed to parse session components")?;
    let mut candidates = Vec::new();

    for comp in &components {
        let body = comp.content(&content);
        if component::is_backlog_component(&comp.name) || component::is_review_component(&comp.name)
        {
            let (_, items, _) = pending::parse_items(body);
            for item in items {
                if item.state == PendingState::Done || item.text.trim().is_empty() {
                    continue;
                }
                let item_id = (!item.id.is_empty()).then_some(item.id.clone());
                let id_ref = item_id.clone().unwrap_or_else(|| {
                    format!("anon:{}", snapshot::doc_hash_from_str(item.text.trim()))
                });
                let mut text = item.text.trim().to_string();
                if !item.continuation.trim().is_empty() {
                    text.push('\n');
                    text.push_str(item.continuation.trim());
                }
                candidates.push(CompletionCandidate {
                    source: comp.name.clone(),
                    source_ref: format!("{session_ref}#{}:{id_ref}", comp.name),
                    item_id,
                    text,
                });
            }
        } else if comp.name == "queue" {
            let Ok(entries) = crate::queue::parse(body) else {
                continue;
            };
            for (index, entry) in entries.iter().enumerate() {
                let crate::queue::QueueEntry::Prompt(prompt) = entry else {
                    continue;
                };
                if queue_prompt_target_id(&prompt.text).is_some() || prompt.text.trim().is_empty() {
                    continue;
                }
                // #qftstuck: the cosmetic `🚧` in-progress marker is not part of
                // the head's prose identity. Strip it so the deterministic strike
                // scorer (and the downstream `gate_norm` identity match in
                // `run_queue_maintenance`) sees the same clean prose whether or not
                // the head is currently the in-progress head — otherwise an
                // in-progress head can never be matched/struck and the marker
                // lingers on a now-redundant head forever.
                candidates.push(CompletionCandidate {
                    source: "queue".to_string(),
                    source_ref: format!("{session_ref}#queue:{index}"),
                    item_id: None,
                    text: crate::queue::strip_in_progress_marker(prompt.text.trim()),
                });
            }
        }
    }

    Ok(candidates)
}

fn is_done_tracked_work_event(event: &MemoryEvent) -> bool {
    event
        .metadata
        .get("agent_doc_surface")
        .is_some_and(|surface| surface == "tracked_work")
        && event
            .metadata
            .get("state")
            .is_some_and(|state| state == "done")
}

fn queue_prompt_target_id(text: &str) -> Option<String> {
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

fn count_insert_results(results: &[MemoryInsertResult]) -> (usize, usize) {
    let inserted = results.iter().filter(|result| result.inserted).count();
    (inserted, results.len().saturating_sub(inserted))
}

fn collect_session_events(file: &Path) -> Result<SessionEvents> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read session document {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let doc_hash = snapshot::doc_hash(&canonical)
        .unwrap_or_else(|_| snapshot::doc_hash_from_str(&canonical.to_string_lossy()));
    let session_ref = display_path(&canonical);
    let components = component::parse(&content).context("failed to parse session components")?;
    let mut events = Vec::new();
    let mut component_counts = BTreeMap::new();
    let mut warnings = Vec::new();

    for comp in &components {
        let body = comp.content(&content);
        if component::is_backlog_component(&comp.name)
            || component::is_review_component(&comp.name)
            || component::is_icebox_component(&comp.name)
        {
            let state_override = None;
            let new_events =
                tracked_work_events(&session_ref, &doc_hash, &comp.name, body, state_override);
            bump_count(&mut component_counts, &comp.name, new_events.len());
            events.extend(new_events);
        } else if component::is_backlog_done_component(&comp.name) {
            let new_events = tracked_work_events(
                &session_ref,
                &doc_hash,
                &comp.name,
                body,
                Some(PendingState::Done),
            );
            bump_count(&mut component_counts, &comp.name, new_events.len());
            events.extend(new_events);
            if let Some(archive) = comp.attrs.get("archive") {
                match read_done_archive(file, archive) {
                    Ok(Some((archive_path, archive_content))) => {
                        let source = format!("{}", archive_path.display());
                        let archive_events =
                            done_archive_events(&source, &doc_hash, &comp.name, &archive_content);
                        bump_count(
                            &mut component_counts,
                            &format!("{}:archive", comp.name),
                            archive_events.len(),
                        );
                        events.extend(archive_events);
                    }
                    Ok(None) => warnings.push(format!("done archive not found: {archive}")),
                    Err(err) => warnings.push(err.to_string()),
                }
            }
        } else if comp.name == "exchange" {
            let new_events = response_summary_events(&session_ref, &doc_hash, body);
            bump_count(&mut component_counts, &comp.name, new_events.len());
            events.extend(new_events);
        }
    }

    Ok(SessionEvents {
        events,
        component_counts,
        warnings,
    })
}

fn tracked_work_events(
    session_ref: &str,
    doc_hash: &str,
    component: &str,
    body: &str,
    state_override: Option<PendingState>,
) -> Vec<MemoryEvent> {
    let (_, items, _) = pending::parse_items(body);
    items
        .into_iter()
        .filter(|item| !item.id.is_empty() || !item.text.trim().is_empty())
        .map(|item| tracked_work_event(session_ref, doc_hash, component, item, state_override))
        .collect()
}

fn done_archive_events(
    session_ref: &str,
    doc_hash: &str,
    component: &str,
    body: &str,
) -> Vec<MemoryEvent> {
    let archive_items = parse_done_archive_items(body);
    if archive_items.is_empty() {
        return tracked_work_events(
            session_ref,
            doc_hash,
            component,
            body,
            Some(PendingState::Done),
        );
    }
    archive_items
        .into_iter()
        .map(|item| {
            tracked_work_event(
                session_ref,
                doc_hash,
                component,
                item,
                Some(PendingState::Done),
            )
        })
        .collect()
}

fn parse_done_archive_items(body: &str) -> Vec<PendingItem> {
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

fn parse_done_archive_line(line: &str) -> Option<(String, String, String)> {
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

fn looks_like_iso_date_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 11
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn tracked_work_event(
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
        format!("anon:{}", snapshot::doc_hash_from_str(&text))
    } else {
        item.id.clone()
    };
    let text_hash = snapshot::doc_hash_from_str(&text);
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

fn response_summary_events(session_ref: &str, doc_hash: &str, body: &str) -> Vec<MemoryEvent> {
    response_sections(body)
        .into_iter()
        .map(|(index, heading, text)| {
            let event_text = trim_chars(&format!("{heading}\n{text}"), MAX_RESPONSE_BODY_CHARS);
            let text_hash = snapshot::doc_hash_from_str(&event_text);
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

fn response_sections(body: &str) -> Vec<(usize, String, String)> {
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

fn read_done_archive(file: &Path, archive: &str) -> Result<Option<(PathBuf, String)>> {
    if archive.trim().is_empty() {
        bail!("agent:done archive= must not be empty");
    }
    if !archive.ends_with(".done.md") {
        bail!("agent:done archive={archive} must point to a .done.md file");
    }
    let relative = Path::new(archive);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("agent:done archive={archive} must be repo-relative");
    }
    let root = fs_util::find_project_root_canonical(file)
        .with_context(|| format!("failed to find project root for {}", file.display()))?;
    let target = root.join(relative);
    if let Ok(canonical_target) = target.canonicalize()
        && !canonical_target.starts_with(&root)
    {
        bail!("agent:done archive={archive} resolves outside the project root");
    }
    match std::fs::read_to_string(&target) {
        Ok(content) => Ok(Some((target, content))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", target.display())),
    }
}

fn resolve_memory_db_path(file: &Path, db: Option<&Path>) -> Result<PathBuf> {
    if let Some(db) = db {
        return Ok(db.to_path_buf());
    }
    let root = fs_util::find_project_root_canonical(file)
        .or_else(|| std::env::current_dir().ok())
        .with_context(|| format!("failed to resolve memory DB root for {}", file.display()))?;
    Ok(default_memory_db_path(&root))
}

fn rank_events(query: &str, events: &[MemoryEvent]) -> Vec<MemorySearchResult> {
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

fn dedupe_events(events: Vec<MemoryEvent>) -> Vec<MemoryEvent> {
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

fn pending_state_str(state: PendingState) -> &'static str {
    match state {
        PendingState::Open => "open",
        PendingState::Gated => "gated",
        PendingState::Done => "done",
    }
}

fn bump_count(counts: &mut BTreeMap<String, usize>, key: &str, amount: usize) {
    if amount > 0 {
        *counts.entry(key.to_string()).or_insert(0) += amount;
    }
}

fn trim_chars(text: &str, max_chars: usize) -> String {
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

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_doc(root: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc_dir = root.join("tasks");
        std::fs::create_dir_all(&doc_dir).unwrap();
        let doc = doc_dir.join("session.md");
        std::fs::write(&doc, body).unwrap();
        doc
    }

    #[test]
    fn collect_session_events_reads_backlog_review_done_archive_and_exchange() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#oldfix] historical cache fix\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"---
agent_doc_session: test
---

## Exchange
<!-- agent:exchange -->
### Re: cache fix

Shipped cache repair.
<!-- /agent:exchange -->

## Backlog
<!-- agent:backlog -->
- [ ] [#cachefix] Repair cache duplication
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
- [/] [#shipcheck] Await release verification
<!-- /agent:review -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );

        let session = collect_session_events(&doc).unwrap();
        assert!(
            session
                .events
                .iter()
                .any(|event| event.metadata.get("item_id") == Some(&"cachefix".to_string()))
        );
        assert!(
            session
                .events
                .iter()
                .any(|event| event.metadata.get("item_id") == Some(&"shipcheck".to_string()))
        );
        assert!(
            session
                .events
                .iter()
                .any(|event| event.metadata.get("item_id") == Some(&"oldfix".to_string()))
        );
        assert!(
            session
                .events
                .iter()
                .any(|event| event.kind == MemoryEventKind::ResponseSummary)
        );
    }

    #[test]
    fn search_ranks_current_tracked_work_by_query_terms() {
        let tmp = tempdir().unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:backlog -->
- [ ] [#memrag] Add semantic retrieval over backlog history
- [ ] [#routes] Fix tmux route timeout
<!-- /agent:backlog -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let report = search_session_memory(&doc, &db, "semantic backlog retrieval", 5, true)
            .expect("search should succeed");
        assert_eq!(
            report.results.first().and_then(|r| r.item_id.as_deref()),
            Some("memrag")
        );
    }

    #[test]
    fn semantic_completion_matches_done_archive_for_free_text_queue_prompt() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- Repair cache duplication
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );

        let db = tmp.path().join(".tsift/memory.db");
        let matches = semantic_completion_matches(&doc, Some(&db), 5).unwrap();
        let first = matches.first().expect("expected semantic match");
        assert_eq!(first.candidate_source, "queue");
        assert_eq!(first.candidate_id, None);
        assert_eq!(first.matched_done_id.as_deref(), Some("cachefix"));
        assert!(first.score >= 0.8, "{first:?}");
        assert!(
            format_semantic_completion_warning(first).contains("semantic completion candidate")
        );
    }

    #[test]
    fn queue_strike_matches_done_archive_above_threshold() {
        // #qftbklgstrike case (a): a free-text queue head that restates a
        // completed `agent:done` archive item matches with kind=Done above the
        // conservative strike threshold.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication on save\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- Repair cache duplication on save
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        let first = matches.first().expect("expected a done strike match");
        assert_eq!(first.matched_kind, QueueStrikeMatchKind::Done);
        assert_eq!(first.matched_id.as_deref(), Some("cachefix"));
        assert!(
            first.score >= QUEUE_STRIKE_THRESHOLD,
            "score {} must clear threshold {QUEUE_STRIKE_THRESHOLD}: {first:?}",
            first.score
        );
    }

    #[test]
    fn queue_strike_matches_active_backlog_above_threshold() {
        // #qftbklgstrike case (b): a free-text queue head that restates an active
        // `agent:backlog` item matches with kind=Backlog above threshold.
        let tmp = tempdir().unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- Opencode responses are reverse ordering numeric lists
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#revlist] Opencode responses are reverse ordering numeric lists
<!-- /agent:backlog -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        let first = matches.first().expect("expected a backlog strike match");
        assert_eq!(first.matched_kind, QueueStrikeMatchKind::Backlog);
        assert_eq!(first.matched_id.as_deref(), Some("revlist"));
        assert!(
            first.score >= QUEUE_STRIKE_THRESHOLD,
            "score {} must clear threshold: {first:?}",
            first.score
        );
    }

    #[test]
    fn queue_strike_does_not_match_unrelated_operator_prompt() {
        // #qftbklgstrike false-strike safety: an unrelated operator prompt that
        // is NOT a restatement of any done/backlog item never reaches the
        // conservative threshold, so it is never returned for a strike.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication on save\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- Please add a dark mode toggle to the settings panel
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#revlist] Opencode responses are reverse ordering numeric lists
<!-- /agent:backlog -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        assert!(
            matches.is_empty(),
            "unrelated operator prompt must NOT be struck: {matches:?}"
        );
    }

    #[test]
    fn queue_strike_skips_id_backed_heads() {
        // #qftbklgstrike: id-backed heads have their own done-strike path and must
        // never be returned by the free-text strike scorer.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication on save\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- do [#cachefix] Repair cache duplication on save
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        assert!(
            matches.is_empty(),
            "id-backed head must be skipped by the free-text strike scorer: {matches:?}"
        );
    }

    #[test]
    fn queue_strike_exempts_recurring_imperative_deploy_head() {
        // #qimpstrike: a recurring imperative command head (`deploy`) is an
        // executable directive, NOT a restatement of tracked work — it must not be
        // auto-struck even when a lexically-close done item containing the word
        // "deploy" exists above the strike threshold.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#do-id-closeout-open-backlog] commit + push + deploy the worker\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- deploy
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        assert!(
            matches.is_empty(),
            "recurring-imperative `deploy` head must NOT be struck as done: {matches:?}"
        );
    }

    #[test]
    fn queue_strike_exempts_recurring_command_preset_token_head() {
        // #qimpstrike: a recurring-command preset token (`#spec-test-commit-push`)
        // is an executable directive and must not be struck by a lexically-close
        // done item.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#shiplast] spec test commit push the release\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- #spec-test-commit-push
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        assert!(
            matches.is_empty(),
            "recurring-command preset head must NOT be struck as done: {matches:?}"
        );
    }

    #[test]
    fn queue_strike_still_strikes_legitimate_restatement_with_deploy_word() {
        // #qimpstrike non-regression: a multi-word free-text head that genuinely
        // restates a specific completed tracked task STILL auto-strikes, even when
        // it happens to contain a recurring-command verb. The exemption is for the
        // recurring-imperative subclass (short, verb-led), not for any prose that
        // mentions `deploy`/`commit`/`push`.
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tasks.done.md"),
            "- 2026-06-07 [#deployretry] fix the deploy script so it retries on a transient 500\n",
        )
        .unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:queue auto -->
- fix the deploy script so it retries on a transient 500
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let matches =
            semantic_queue_strike_matches(&doc, Some(&db), QUEUE_STRIKE_THRESHOLD, 5).unwrap();
        let first = matches
            .first()
            .expect("legitimate multi-word restatement must still be struck");
        assert_eq!(first.matched_kind, QueueStrikeMatchKind::Done);
        assert_eq!(first.matched_id.as_deref(), Some("deployretry"));
    }

    #[test]
    fn index_session_writes_tsift_memory_events() {
        let tmp = tempdir().unwrap();
        let doc = write_doc(
            tmp.path(),
            r#"
<!-- agent:backlog -->
- [ ] [#dedupe] Detect duplicate backlog item
<!-- /agent:backlog -->
"#,
        );
        let db = tmp.path().join(".tsift/memory.db");
        let report = index_session(&doc, &db).unwrap();
        assert_eq!(report.inserted_events, 1);
        let events = read_memory_events(&db, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].metadata.get("agent_doc_surface"),
            Some(&"tracked_work".to_string())
        );
    }
}
