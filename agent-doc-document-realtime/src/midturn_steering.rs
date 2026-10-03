//! Mid-turn operator steering (`#midturn-steering`).
//!
//! A document is realtime: while a response turn runs (builds, releases,
//! waiting on background work), the operator keeps editing — adding
//! `agent:queue` items, editing the item being worked, typing new exchange
//! prompts. `session-check` only surfaces exchange steering at closeout, and
//! queue additions are not steering at all there (they belong to the normal
//! drain). This module is the deterministic core of the *mid-turn* surface:
//! it compares the live document against the active cycle's preflight
//! baseline plus a per-cycle watermark and returns only the **new, settled**
//! operator steering, each item classified with a typed dispatch intent.
//!
//! Routing (`SteeringDispatch`):
//! - an edit or deletion of the CURRENT queue item → `address_now`
//! - a new exchange prompt → `address_now`
//! - a new/edited queue item with subagent intent (`#subagents`, or a preset
//!   expanding to "run … in subagents", at item, queue, or cycle scope) →
//!   `subagent`
//! - any other new or edited queue item → `drain_after_current`: it runs in
//!   operator-authored queue order after the current item closes; the current
//!   item is never interrupted or interleaved.
//!
//! The watermark is the exactly-once fence. Queue steering is tracked as an
//! *acknowledged queue* (the queue as of the last surfacing), so a later edit
//! to an already-surfaced item aligns against what the agent was told and
//! surfaces once as `previous → verbatim`. Exchange steering is tracked by the
//! identity of every surfaced directive (`RealtimeSteering::identity`).
//!
//! Settle detection (debounce): the operator may be mid-keystroke when a hook
//! fires. An item surfaces only once it is quiet for `debounce_ms` (the
//! document has not changed for that long, or the item's own content has been
//! observed unchanged for that long) and does not look plainly incomplete
//! (`looks_incomplete`). Held items stay in `pending` and surface on a later
//! observation.
//!
//! Exchange extraction reuses `baseline_comparison::exchange_steering_set_between`
//! (the same `all_unstarted_prompt_bearing_changes_from_diff` path closeout
//! uses), so agent response checkpoints, `(HEAD)` boundary markers, recovery
//! artifacts, and replays of committed response body are excluded exactly as
//! they are at closeout.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::baseline_comparison::{RealtimeSteering, exchange_steering_set_between};

/// Default quiet period before an edited item counts as settled.
pub const DEFAULT_STEERING_DEBOUNCE_MS: u64 = 2500;

/// Typed dispatch intent for one steering item. The rendered instruction is
/// derived from this value, never the other way around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteeringDispatch {
    /// Address in the current turn (exchange prompts, current-item edits).
    AddressNow,
    /// Runs in queue order after the current item closes. Do not interrupt.
    DrainAfterCurrent,
    /// Dispatch now to a new background subagent (one per item).
    Subagent,
}

impl SteeringDispatch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddressNow => "address_now",
            Self::DrainAfterCurrent => "drain_after_current",
            Self::Subagent => "subagent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteeringSource {
    Exchange,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteeringChange {
    Added,
    Edited,
    Deleted,
}

/// Where a resolved preset applied from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetScope {
    /// Referenced on the queue line itself.
    Item,
    /// `agent:queue preset="…"` attribute or a `preset …` queue entry.
    Queue,
    /// Requested by this cycle's prompt (preflight `prompt_presets_requested`).
    Cycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetIntent {
    pub name: String,
    pub body: String,
    pub scope: PresetScope,
}

/// One settled operator steering item, ready to hand to the running agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteeringItem {
    pub source: SteeringSource,
    pub change: SteeringChange,
    pub dispatch: SteeringDispatch,
    /// True when the item is the work this turn is executing (the selected
    /// queue head, or the exchange prompt being answered).
    pub current_item: bool,
    /// The operator's text, verbatim (for a deletion: the removed text).
    pub verbatim: String,
    /// The text the agent last knew for this item (edits only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetIntent>,
}

/// A held (unsettled) candidate observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingObservation {
    pub content_hash: String,
    pub first_seen_ms: u64,
}

/// Per-cycle, per-consumer durable watermark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SteeringWatermark {
    pub cycle_id: String,
    /// The document as preflight admitted this cycle.
    pub baseline: String,
    /// Normalized text of the queue item this cycle is executing, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_item: Option<String>,
    /// Preset names this cycle's prompt requested (cycle-scoped presets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_presets: Vec<String>,
    /// Normalized queue prompts as of the last surfacing.
    #[serde(default)]
    pub acknowledged_queue: Vec<String>,
    /// Surfaced exchange directive identity → verbatim text.
    #[serde(default)]
    pub surfaced_exchange: BTreeMap<String, String>,
    /// Held candidates keyed by candidate key.
    #[serde(default)]
    pub pending: BTreeMap<String, PendingObservation>,
    /// Content hash of the last observed document (cheap no-change gate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_content_hash: Option<String>,
    /// File-stat fingerprint of the last observed document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_stat: Option<String>,
    /// The cycle closed (or was superseded); observations stay silent.
    #[serde(default)]
    pub closed: bool,
}

impl SteeringWatermark {
    /// Seed a fresh watermark for a newly admitted cycle.
    pub fn seed(
        cycle_id: &str,
        baseline: &str,
        current_item: Option<&str>,
        session_presets: Vec<String>,
    ) -> Self {
        Self {
            cycle_id: cycle_id.to_string(),
            baseline: baseline.to_string(),
            current_item: current_item
                .map(normalize_queue_text)
                .filter(|text| !text.is_empty()),
            session_presets,
            acknowledged_queue: queue_items(baseline)
                .into_iter()
                .map(|item| item.norm)
                .collect(),
            surfaced_exchange: BTreeMap::new(),
            pending: BTreeMap::new(),
            last_observed_content_hash: Some(content_hash(baseline)),
            last_observed_stat: None,
            closed: false,
        }
    }
}

/// Inputs that are not part of the document text.
#[derive(Debug, Clone, Copy)]
pub struct ObserveContext<'a> {
    pub now_ms: u64,
    /// When the document last changed (file mtime / editor edit time).
    /// `None` means unknown: only per-item stability can settle an item.
    pub document_changed_ms: Option<u64>,
    pub debounce_ms: u64,
    /// `do [#id]` ids the binary itself mirrored/added this cycle; queue lines
    /// naming only these are binary bookkeeping, not operator steering.
    pub binary_owned_queue_ids: &'a BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub ready: Vec<SteeringItem>,
    /// Candidates still being typed (held by the debounce).
    pub pending: usize,
    pub next: SteeringWatermark,
}

/// Compare `current` against the watermark and return settled new steering
/// plus the advanced watermark. Pure: callers persist `next`.
pub fn observe(
    watermark: &SteeringWatermark,
    current: &str,
    ctx: &ObserveContext<'_>,
) -> Observation {
    let mut candidates: Vec<Candidate> = Vec::new();
    candidates.extend(exchange_candidates(watermark, current));
    let queue = queue_alignment(watermark, current, ctx.binary_owned_queue_ids);
    candidates.extend(queue.candidates.iter().cloned());

    let doc_quiet = ctx
        .document_changed_ms
        .is_some_and(|changed| ctx.now_ms.saturating_sub(changed) >= ctx.debounce_ms);
    let mut ready_keys = BTreeSet::new();
    let mut next_pending = BTreeMap::new();
    for candidate in &candidates {
        let prior = watermark
            .pending
            .get(&candidate.key)
            .filter(|prior| prior.content_hash == candidate.content_hash);
        let stable = prior
            .is_some_and(|prior| ctx.now_ms.saturating_sub(prior.first_seen_ms) >= ctx.debounce_ms);
        let incomplete = candidate.item.change != SteeringChange::Deleted
            && looks_incomplete(&candidate.item.verbatim);
        if !incomplete && (doc_quiet || stable) {
            ready_keys.insert(candidate.key.clone());
        } else {
            next_pending.insert(
                candidate.key.clone(),
                PendingObservation {
                    content_hash: candidate.content_hash.clone(),
                    first_seen_ms: prior.map_or(ctx.now_ms, |prior| prior.first_seen_ms),
                },
            );
        }
    }

    let mut next = watermark.clone();
    next.pending = next_pending;
    next.last_observed_content_hash = Some(content_hash(current));
    let mut ready = Vec::new();
    for candidate in &candidates {
        if !ready_keys.contains(&candidate.key) {
            continue;
        }
        if let Some(identity) = &candidate.exchange_identity {
            next.surfaced_exchange
                .insert(identity.clone(), candidate.item.verbatim.clone());
        }
        ready.push(candidate.item.clone());
    }

    // Rebuild the acknowledged queue: settled changes become what the agent
    // knows; held changes keep the old view so they surface once settled.
    let mut acknowledged = Vec::new();
    for event in &queue.events {
        match event {
            QueueEvent::Keep(norm) => acknowledged.push(norm.clone()),
            QueueEvent::Insert { new, key } => {
                if ready_keys.contains(key) {
                    acknowledged.push(new.clone());
                }
            }
            QueueEvent::Edit {
                old,
                new,
                key,
                current,
            } => {
                if ready_keys.contains(key) {
                    acknowledged.push(new.clone());
                    if *current {
                        next.current_item = Some(new.clone());
                    }
                } else {
                    acknowledged.push(old.clone());
                }
            }
            QueueEvent::Delete { old, key } => match key {
                Some(key) if ready_keys.contains(key) => next.current_item = None,
                Some(_) => acknowledged.push(old.clone()),
                None => {}
            },
        }
    }
    next.acknowledged_queue = acknowledged;

    Observation {
        pending: next.pending.len(),
        ready,
        next,
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    key: String,
    content_hash: String,
    exchange_identity: Option<String>,
    item: SteeringItem,
}

fn exchange_candidates(watermark: &SteeringWatermark, current: &str) -> Vec<Candidate> {
    let without_agent_responses = strip_new_response_sections(&watermark.baseline, current);
    let set = exchange_steering_set_between(&watermark.baseline, &without_agent_responses);
    let mut out = Vec::new();
    for directive in set.directives() {
        let Some(identity) = directive.identity() else {
            continue;
        };
        if watermark.surfaced_exchange.contains_key(&identity) {
            continue;
        }
        let Some(verbatim) = directive.verbatim().map(str::to_string) else {
            continue;
        };
        let (change, current_item, previous) = match directive {
            RealtimeSteering::PromptTarget { .. } | RealtimeSteering::ContentEdit { .. } => {
                let previous = watermark
                    .surfaced_exchange
                    .values()
                    .find(|old| {
                        let old = collapse_ws(old);
                        !old.is_empty() && collapse_ws(&verbatim).contains(&old)
                    })
                    .cloned();
                let change = if previous.is_some() {
                    SteeringChange::Edited
                } else {
                    SteeringChange::Added
                };
                (change, false, previous)
            }
            RealtimeSteering::PromptDeleted { .. } => {
                // The agent's own response checkpoint resolves the prompt it
                // answers, which reads as "deleted" to the unresolved-prompt
                // probe. Only a prompt whose text is really gone counts.
                let still_present = verbatim
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .any(|line| current.contains(line));
                if still_present {
                    continue;
                }
                (SteeringChange::Deleted, true, None)
            }
            RealtimeSteering::PromptReduced { .. } => (SteeringChange::Edited, true, None),
            RealtimeSteering::None => continue,
        };
        out.push(Candidate {
            key: format!("exchange:{identity}"),
            content_hash: identity.clone(),
            exchange_identity: Some(identity),
            item: SteeringItem {
                source: SteeringSource::Exchange,
                change,
                dispatch: SteeringDispatch::AddressNow,
                current_item,
                verbatim,
                previous,
                presets: Vec::new(),
            },
        });
    }
    out
}

/// Remove response sections the agent added since the baseline.
///
/// At closeout the turn's prompt and its response are both new relative to
/// the committed snapshot, so the extractor sees an answered run. Mid-turn the
/// prompt is already in the preflight baseline and only the response
/// checkpoint is new, which the extractor would otherwise read as fresh
/// prose. A response section runs from a new `### Re:` heading to the next
/// boundary marker (the binary re-anchors a boundary after every response
/// checkpoint, and operator text typed afterwards lands below it), the next
/// response heading, or the end of the exchange component.
fn strip_new_response_sections(baseline: &str, current: &str) -> String {
    let baseline_headings: BTreeSet<&str> = baseline
        .lines()
        .map(str::trim)
        .filter(|line| crate::baseline_comparison::is_exchange_response_heading(line))
        .collect();
    let mut out = String::with_capacity(current.len());
    let mut skipping = false;
    for line in current.split_inclusive('\n') {
        let trimmed = line.trim();
        let heading = crate::baseline_comparison::is_exchange_response_heading(trimmed);
        if heading {
            skipping = !baseline_headings.contains(trimmed);
        } else if trimmed.starts_with("<!-- agent:boundary:") || trimmed.starts_with("<!-- /agent:")
        {
            skipping = false;
        }
        if !skipping {
            out.push_str(line);
        }
    }
    out
}

#[derive(Debug, Clone)]
struct QueueItem {
    raw: String,
    norm: String,
}

#[derive(Debug, Clone)]
enum QueueEvent {
    Keep(String),
    Insert {
        new: String,
        key: String,
    },
    Edit {
        old: String,
        new: String,
        key: String,
        current: bool,
    },
    /// `key` is `Some` only for the current item (the only deletion that is
    /// steering); other deletions are acknowledged silently.
    Delete {
        old: String,
        key: Option<String>,
    },
}

struct QueueAlignment {
    candidates: Vec<Candidate>,
    events: Vec<QueueEvent>,
}

/// Normalize a queue prompt for comparison: progress/pin/priority markers are
/// cosmetic and never read as an operator edit.
pub fn normalize_queue_text(text: &str) -> String {
    agent_doc_document::queue_projection::strip_priority_markers(
        &agent_doc_document::queue_projection::strip_in_progress_marker(text),
    )
    .replace("\r\n", "\n")
    .trim()
    .to_string()
}

fn queue_items(content: &str) -> Vec<QueueItem> {
    let body = agent_doc_queue::queue_prompt_drift::queue_component_text(content);
    let Ok(entries) = agent_doc_queue::document_queue::parse(&body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                let norm = normalize_queue_text(&prompt.text);
                (!norm.is_empty()).then(|| QueueItem {
                    raw: prompt.text.trim().to_string(),
                    norm,
                })
            }
            _ => None,
        })
        .collect()
}

fn queue_identity(norm: &str) -> String {
    format!(
        "{:?}",
        agent_doc_element_queue::QueueItemIdentity::from_prompt(norm)
    )
}

fn queue_alignment(
    watermark: &SteeringWatermark,
    current: &str,
    binary_owned_ids: &BTreeSet<String>,
) -> QueueAlignment {
    let old = &watermark.acknowledged_queue;
    let new = queue_items(current);
    let old_keys: Vec<String> = old.iter().map(|norm| queue_identity(norm)).collect();
    let new_keys: Vec<String> = new.iter().map(|item| queue_identity(&item.norm)).collect();
    let current_index = watermark.current_item.as_deref().and_then(|current_item| {
        let identity = queue_identity(current_item);
        old_keys
            .iter()
            .position(|key| key == &identity)
            .or_else(|| old.iter().position(|norm| norm == current_item))
    });
    let presets = PresetContext::new(current, &watermark.session_presets);
    let mut alignment = QueueAlignment {
        candidates: Vec::new(),
        events: Vec::new(),
    };

    let insert = |alignment: &mut QueueAlignment, idx: usize| {
        let item = &new[idx];
        if binary_owned_line(&item.norm, binary_owned_ids) {
            alignment.events.push(QueueEvent::Keep(item.norm.clone()));
            return;
        }
        let (dispatch, intents) = presets.classify(&item.raw);
        let key = format!("queue:add:{}", content_hash(&item.norm));
        alignment.candidates.push(Candidate {
            key: key.clone(),
            content_hash: content_hash(&item.norm),
            exchange_identity: None,
            item: SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Added,
                dispatch,
                current_item: false,
                verbatim: item.raw.clone(),
                previous: None,
                presets: intents,
            },
        });
        alignment.events.push(QueueEvent::Insert {
            new: item.norm.clone(),
            key,
        });
    };
    let edit = |alignment: &mut QueueAlignment, old_idx: usize, new_idx: usize| {
        let item = &new[new_idx];
        let previous = &old[old_idx];
        let is_current = current_index == Some(old_idx);
        let (dispatch, intents) = if is_current {
            (SteeringDispatch::AddressNow, Vec::new())
        } else {
            presets.classify(&item.raw)
        };
        let key = format!(
            "queue:edit:{}:{}",
            content_hash(previous),
            content_hash(&item.norm)
        );
        alignment.candidates.push(Candidate {
            key: key.clone(),
            content_hash: content_hash(&item.norm),
            exchange_identity: None,
            item: SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Edited,
                dispatch,
                current_item: is_current,
                verbatim: item.raw.clone(),
                previous: Some(previous.clone()),
                presets: intents,
            },
        });
        alignment.events.push(QueueEvent::Edit {
            old: previous.clone(),
            new: item.norm.clone(),
            key,
            current: is_current,
        });
    };
    let delete = |alignment: &mut QueueAlignment, old_idx: usize| {
        let previous = &old[old_idx];
        if current_index != Some(old_idx) {
            alignment.events.push(QueueEvent::Delete {
                old: previous.clone(),
                key: None,
            });
            return;
        }
        let key = format!("queue:delete:{}", content_hash(previous));
        alignment.candidates.push(Candidate {
            key: key.clone(),
            content_hash: content_hash(previous),
            exchange_identity: None,
            item: SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Deleted,
                dispatch: SteeringDispatch::AddressNow,
                current_item: true,
                verbatim: previous.clone(),
                previous: None,
                presets: Vec::new(),
            },
        });
        alignment.events.push(QueueEvent::Delete {
            old: previous.clone(),
            key: Some(key),
        });
    };

    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &old_keys, &new_keys) {
        match op {
            similar::DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for offset in 0..len {
                    let (o, n) = (old_index + offset, new_index + offset);
                    if old[o] == new[n].norm {
                        alignment.events.push(QueueEvent::Keep(old[o].clone()));
                    } else {
                        // Same id-backed identity, different text.
                        edit(&mut alignment, o, n);
                    }
                }
            }
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for o in old_index..old_index + old_len {
                    delete(&mut alignment, o);
                }
            }
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for n in new_index..new_index + new_len {
                    insert(&mut alignment, n);
                }
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let paired = old_len.min(new_len);
                for offset in 0..paired {
                    let (o, n) = (old_index + offset, new_index + offset);
                    if binary_owned_line(&new[n].norm, binary_owned_ids) {
                        delete(&mut alignment, o);
                        alignment.events.push(QueueEvent::Keep(new[n].norm.clone()));
                    } else {
                        edit(&mut alignment, o, n);
                    }
                }
                for o in old_index + paired..old_index + old_len {
                    delete(&mut alignment, o);
                }
                for n in new_index + paired..new_index + new_len {
                    insert(&mut alignment, n);
                }
            }
        }
    }
    alignment
}

/// A queue line whose only directive targets are ids the binary itself added
/// or mirrored this cycle (`#backlogqueuepopulation`) is bookkeeping.
fn binary_owned_line(norm: &str, owned: &BTreeSet<String>) -> bool {
    if owned.is_empty() {
        return false;
    }
    match agent_doc_element_queue::QueueItemIdentity::from_prompt(norm) {
        agent_doc_element_queue::QueueItemIdentity::Id(id) => {
            owned.contains(&id.to_ascii_lowercase())
        }
        agent_doc_element_queue::QueueItemIdentity::FreeText(_) => false,
    }
}

struct PresetContext<'a> {
    current: &'a str,
    inherited: Vec<PresetIntent>,
}

impl<'a> PresetContext<'a> {
    fn new(current: &'a str, session_presets: &[String]) -> Self {
        let mut inherited = Vec::new();
        let mut queue_names = Vec::new();
        if let Ok(components) = agent_doc_element::element::parse(current)
            && let Some(queue) = components.iter().find(|c| c.name == "queue")
        {
            if let Some(value) = queue.attrs.get("preset") {
                queue_names.push(value.clone());
            }
            if let Ok(entries) = agent_doc_queue::document_queue::parse(queue.content(current)) {
                for entry in entries {
                    if let agent_doc_queue::document_queue::QueueEntry::Preset(name) = entry {
                        queue_names.push(name);
                    }
                }
            }
        }
        for (names, scope) in [
            (queue_names.as_slice(), PresetScope::Queue),
            (session_presets, PresetScope::Cycle),
        ] {
            for name in names {
                let resolved =
                    agent_doc_queue::queue_response::queue_prompt_preset_expansions(current, name);
                if resolved.is_empty() {
                    // Unregistered names still carry literal intent.
                    inherited.push(PresetIntent {
                        name: name.trim().to_string(),
                        body: String::new(),
                        scope,
                    });
                }
                for (name, body) in resolved {
                    inherited.push(PresetIntent { name, body, scope });
                }
            }
        }
        Self { current, inherited }
    }

    /// Resolve presets for one queue line through the same resolver closeout
    /// uses (`queue_prompt_preset_expansions`) and classify its dispatch.
    fn classify(&self, raw: &str) -> (SteeringDispatch, Vec<PresetIntent>) {
        let mut intents: Vec<PresetIntent> =
            agent_doc_queue::queue_response::queue_prompt_preset_expansions(self.current, raw)
                .into_iter()
                .map(|(name, body)| PresetIntent {
                    name,
                    body,
                    scope: PresetScope::Item,
                })
                .collect();
        let literal_subagent_tag = raw
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
            .filter(|token| token.starts_with('#'))
            .any(|token| preset_requests_subagents(token, ""));
        intents.extend(self.inherited.iter().cloned());
        let subagent = literal_subagent_tag
            || intents
                .iter()
                .any(|intent| preset_requests_subagents(&intent.name, &intent.body));
        let dispatch = if subagent {
            SteeringDispatch::Subagent
        } else {
            SteeringDispatch::DrainAfterCurrent
        };
        (dispatch, intents)
    }
}

/// True when a preset (by name or expansion body) asks for subagent dispatch.
pub fn preset_requests_subagents(name: &str, body: &str) -> bool {
    let name = name.trim().trim_start_matches('#').to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "subagents" | "subagent" | "sub-agents" | "sub-agent"
    ) {
        return true;
    }
    let body = body.to_ascii_lowercase();
    ["subagent", "sub-agent", "sub agent"]
        .iter()
        .any(|needle| body.contains(needle))
}

/// Conservative "the operator is plainly still typing this" shapes: an
/// unbalanced code fence, a trailing `#` (or an unclosed `[#…`) with no id yet,
/// or an empty trailing bullet.
pub fn looks_incomplete(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.trim().is_empty() {
        return true;
    }
    let fences = trimmed
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("```") || line.starts_with("~~~"))
        .count();
    if fences % 2 == 1 {
        return true;
    }
    let last_line = trimmed.lines().last().unwrap_or("").trim();
    if matches!(last_line, "-" | "*" | "+") {
        return true;
    }
    if trimmed.ends_with('#') {
        return true;
    }
    let last_token = trimmed.split_whitespace().last().unwrap_or("");
    last_token.starts_with("[#") && !last_token.contains(']')
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn content_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

/// Header line every rendered steering payload starts with.
pub const STEERING_MARKER: &str = "[agent-doc] operator steering arrived mid-turn";

/// Render settled steering as agent-facing context. `None` when nothing is
/// ready (pending-only observations stay silent).
pub fn render_steering_context(
    document: &str,
    ready: &[SteeringItem],
    pending: usize,
) -> Option<String> {
    if ready.is_empty() {
        return None;
    }
    let mut out = format!(
        "{STEERING_MARKER} ({} item(s)) in {document}. The operator edited the session \
         document while this turn was running; every item below is verbatim and must be \
         handled per its `dispatch`. Do not re-answer prompts already committed, do not run \
         `agent-doc preflight`, and do not `--force-disk`.",
        ready.len()
    );
    for (idx, item) in ready.iter().enumerate() {
        out.push_str(&format!(
            "\n\n[steering {}/{}] dispatch={} source={} change={}{}",
            idx + 1,
            ready.len(),
            item.dispatch.as_str(),
            match item.source {
                SteeringSource::Exchange => "exchange",
                SteeringSource::Queue => "queue",
            },
            match item.change {
                SteeringChange::Added => "added",
                SteeringChange::Edited => "edited",
                SteeringChange::Deleted => "deleted",
            },
            if item.current_item {
                " current_item=true"
            } else {
                ""
            }
        ));
        if let Some(previous) = &item.previous {
            out.push_str(&format!("\nprevious: {previous}"));
        }
        let label = if item.change == SteeringChange::Deleted {
            "removed"
        } else {
            "verbatim"
        };
        out.push_str(&format!("\n{label}: {}", item.verbatim));
        if !item.presets.is_empty() {
            let names = item
                .presets
                .iter()
                .map(|preset| {
                    if preset.body.is_empty() {
                        preset.name.clone()
                    } else {
                        format!("{} = {:?}", preset.name, preset.body)
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            out.push_str(&format!("\npresets: {names}"));
        }
        out.push_str(&format!("\naction: {}", instruction_for(item)));
    }
    if pending > 0 {
        out.push_str(&format!(
            "\n\nThe operator is still typing {pending} more item(s); they will be surfaced \
             once settled."
        ));
    }
    Some(out)
}

/// Instruction text derived from the typed dispatch intent.
pub fn instruction_for(item: &SteeringItem) -> &'static str {
    match (item.dispatch, item.source, item.change, item.current_item) {
        (SteeringDispatch::AddressNow, SteeringSource::Queue, SteeringChange::Deleted, _) => {
            "the operator REMOVED the queue item this turn is executing. Stop or wrap up the \
             in-progress work per their intent; do not keep executing the removed instruction."
        }
        (SteeringDispatch::AddressNow, SteeringSource::Queue, _, _) => {
            "the operator EDITED the queue item this turn is executing. Adjust the in-progress \
             work to the edited instruction in THIS turn."
        }
        (SteeringDispatch::AddressNow, SteeringSource::Exchange, SteeringChange::Deleted, _) => {
            "the operator removed the prompt this turn is answering. Stop or wrap up per their \
             intent."
        }
        (SteeringDispatch::AddressNow, SteeringSource::Exchange, _, true) => {
            "the operator edited the prompt this turn is answering. Address the edited prompt \
             in THIS turn."
        }
        (SteeringDispatch::AddressNow, SteeringSource::Exchange, _, false) => {
            "a new operator prompt aimed at the current turn. Address it in THIS turn, together \
             with the current work."
        }
        (SteeringDispatch::Subagent, _, _, _) => {
            "subagent intent: dispatch this item NOW to a NEW background subagent (one \
             subagent per item). If it touches a repository, give that subagent its own git \
             worktree outside the IDE-watched project; never run two subagents against one \
             checkout. Before dispatching, claim it with `agent-doc queue claim <FILE> --item \
             <id-or-line> --owner subagent:<label>` so the loop and Stop hook do not re-enter for \
             it; run `agent-doc queue release` when the subagent reports back. Keep working the \
             current item yourself; record the item as dispatched in your response."
        }
        (SteeringDispatch::DrainAfterCurrent, _, _, _) => {
            "queued in operator order: it runs AFTER the current item closes, through the normal \
             queue drain. Do NOT interrupt, interleave, or start it now; acknowledge it and \
             finish the current item first."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FM: &str = "---\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n  '#gh-fix': 'fix the github issue'\n---\n";

    fn doc(queue: &str, exchange: &str) -> String {
        format!(
            "{FM}# Session\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n"
        )
    }

    fn quiet_ctx(owned: &BTreeSet<String>) -> ObserveContext<'_> {
        ObserveContext {
            now_ms: 100_000,
            document_changed_ms: Some(0),
            debounce_ms: DEFAULT_STEERING_DEBOUNCE_MS,
            binary_owned_queue_ids: owned,
        }
    }

    fn seeded(baseline: &str, current_item: Option<&str>) -> SteeringWatermark {
        SteeringWatermark::seed("cycle-1", baseline, current_item, Vec::new())
    }

    const EX: &str =
        "Earlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n\nwork on the current item\n";

    #[test]
    fn queue_addition_surfaces_verbatim_as_drain_after_current() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc("- current task\n- also update the README table\n", EX);
        let wm = seeded(&baseline, Some("current task"));
        let obs = observe(&wm, &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.source, SteeringSource::Queue);
        assert_eq!(item.change, SteeringChange::Added);
        assert_eq!(item.dispatch, SteeringDispatch::DrainAfterCurrent);
        assert_eq!(item.verbatim, "also update the README table");
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(rendered.contains("dispatch=drain_after_current"));
        assert!(rendered.contains("AFTER the current item closes"));
        assert!(rendered.contains("Do NOT interrupt"));
    }

    #[test]
    fn subagent_tagged_queue_addition_dispatches_subagent() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc(
            "- current task\n- #subagents #gh-fix https://github.com/btakita/agent-doc/issues/111\n",
            EX,
        );
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert_eq!(obs.ready.len(), 1);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::Subagent);
        assert!(
            obs.ready[0]
                .presets
                .iter()
                .any(|p| p.name == "#subagents" && p.scope == PresetScope::Item)
        );
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(rendered.contains("dispatch=subagent"));
        assert!(rendered.contains("NEW background subagent"));
    }

    #[test]
    fn cycle_and_queue_scoped_subagent_presets_apply_to_untagged_additions() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc("- current task\n- untagged follow-up\n", EX);
        let wm = SteeringWatermark::seed(
            "cycle-1",
            &baseline,
            Some("current task"),
            vec!["#subagents".to_string()],
        );
        let obs = observe(&wm, &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::Subagent);

        let queue_scoped = current.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        let baseline_scoped = baseline.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        let obs = observe(
            &seeded(&baseline_scoped, Some("current task")),
            &queue_scoped,
            &quiet_ctx(&owned),
        );
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::Subagent);
        assert_eq!(obs.ready[0].presets[0].scope, PresetScope::Queue);
    }

    #[test]
    fn exchange_prompt_surfaces_address_now() {
        let owned = BTreeSet::new();
        let baseline = doc("", EX);
        let current = doc("", &format!("{EX}\nalso check the CI status please\n"));
        let obs = observe(&seeded(&baseline, None), &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].source, SteeringSource::Exchange);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::AddressNow);
        assert!(
            obs.ready[0]
                .verbatim
                .contains("also check the CI status please")
        );
    }

    #[test]
    fn agent_response_checkpoint_does_not_surface() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc(
            "- current task\n",
            &format!(
                "{EX}\n### Re: work on the current item\n\nProgress so far: built and tested.\n"
            ),
        );
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(obs.pending, 0);
    }

    #[test]
    fn operator_prompt_after_a_response_checkpoint_still_surfaces() {
        let owned = BTreeSet::new();
        let baseline = doc("", EX);
        let current = doc(
            "",
            &format!(
                "{EX}\n### Re: work on the current item\n\nPartial progress.\n\n<!-- agent:boundary:abc -->\nplease also bump the version\n"
            ),
        );
        let obs = observe(&seeded(&baseline, None), &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert!(
            obs.ready[0]
                .verbatim
                .contains("please also bump the version")
        );
        assert!(!obs.ready[0].verbatim.contains("Partial progress"));
    }

    #[test]
    fn binary_queue_bookkeeping_does_not_surface() {
        let owned = BTreeSet::from(["mirrored1".to_string()]);
        let baseline = doc("- 🚧 current task\n- do [#gone1]\n", EX);
        // Marker churn + the binary mirroring its own backlog add + a
        // consumed non-current line: none of it is operator steering.
        let current = doc("- current task\n- do [#mirrored1]\n", EX);
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(
            obs.next.acknowledged_queue,
            vec!["current task".to_string(), "do [#mirrored1]".to_string()]
        );
    }

    #[test]
    fn watermark_prevents_reinjection() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc(
            "- current task\n- new item\n",
            &format!("{EX}\nnew exchange prompt here\n"),
        );
        let first = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert_eq!(first.ready.len(), 2, "{:?}", first.ready);
        let second = observe(&first.next, &current, &quiet_ctx(&owned));
        assert!(second.ready.is_empty(), "{:?}", second.ready);
    }

    #[test]
    fn concurrent_edits_aggregate_in_one_observation() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n- pending one\n", EX);
        let current = doc(
            "- current task edited\n- pending one\n- second addition\n- #subagents third\n",
            &format!("{EX}\nexchange steering text\n"),
        );
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        let dispatches: Vec<_> = obs.ready.iter().map(|i| i.dispatch).collect();
        assert_eq!(obs.ready.len(), 4, "{:?}", obs.ready);
        assert_eq!(
            dispatches,
            vec![
                SteeringDispatch::AddressNow,
                SteeringDispatch::AddressNow,
                SteeringDispatch::DrainAfterCurrent,
                SteeringDispatch::Subagent,
            ]
        );
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(rendered.contains("[steering 4/4]"));
    }

    #[test]
    fn editing_the_current_item_is_address_now_with_old_and_new() {
        let owned = BTreeSet::new();
        let baseline = doc("- fix the login bug\n- later work\n", EX);
        let current = doc(
            "- fix the login bug and add a regression test\n- later work\n",
            EX,
        );
        let obs = observe(
            &seeded(&baseline, Some("🚧 fix the login bug")),
            &current,
            &quiet_ctx(&owned),
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.change, SteeringChange::Edited);
        assert_eq!(item.dispatch, SteeringDispatch::AddressNow);
        assert!(item.current_item);
        assert_eq!(item.previous.as_deref(), Some("fix the login bug"));
        assert_eq!(item.verbatim, "fix the login bug and add a regression test");
        assert_eq!(
            obs.next.current_item.as_deref(),
            Some("fix the login bug and add a regression test")
        );
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(rendered.contains("previous: fix the login bug"));
        assert!(rendered.contains("EDITED the queue item this turn is executing"));
    }

    #[test]
    fn deleting_the_current_item_is_address_now() {
        let owned = BTreeSet::new();
        let baseline = doc("- fix the login bug\n- later work\n", EX);
        let current = doc("- later work\n", EX);
        let obs = observe(
            &seeded(&baseline, Some("fix the login bug")),
            &current,
            &quiet_ctx(&owned),
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].change, SteeringChange::Deleted);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::AddressNow);
        assert!(obs.ready[0].current_item);
        assert_eq!(obs.next.current_item, None);
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(rendered.contains("REMOVED the queue item"));
    }

    #[test]
    fn editing_a_non_current_item_is_drain_after_current() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n- later work\n", EX);
        let current = doc("- current task\n- later work, but also docs\n", EX);
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].change, SteeringChange::Edited);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::DrainAfterCurrent);
        assert!(!obs.ready[0].current_item);
        assert_eq!(obs.ready[0].previous.as_deref(), Some("later work"));
    }

    #[test]
    fn in_progress_edit_is_held_until_settled() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc("- current task\n- add retries to the uploader\n", EX);
        let wm = seeded(&baseline, Some("current task"));
        let typing = ObserveContext {
            now_ms: 10_000,
            document_changed_ms: Some(9_500),
            debounce_ms: 2_000,
            binary_owned_queue_ids: &owned,
        };
        let held = observe(&wm, &current, &typing);
        assert!(held.ready.is_empty());
        assert_eq!(held.pending, 1);
        assert_eq!(
            held.next.acknowledged_queue,
            vec!["current task".to_string()]
        );
        // Item unchanged for the window → settled even if the doc mtime moved.
        let later = ObserveContext {
            now_ms: 12_100,
            document_changed_ms: Some(12_000),
            ..typing
        };
        let settled = observe(&held.next, &current, &later);
        assert_eq!(settled.ready.len(), 1);
        assert_eq!(settled.pending, 0);
        // And it never surfaces again.
        assert!(observe(&settled.next, &current, &later).ready.is_empty());
    }

    #[test]
    fn incomplete_shapes_are_held_even_after_the_window() {
        for text in ["do #", "do [#fi", "-", "```\nunterminated fence", "   "] {
            assert!(looks_incomplete(text), "{text:?}");
        }
        for text in ["do #fix1", "do [#fix1]", "plain sentence", "```\nok\n```"] {
            assert!(!looks_incomplete(text), "{text:?}");
        }
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc("- current task\n- do [#fi\n", EX);
        let obs = observe(
            &seeded(&baseline, Some("current task")),
            &current,
            &quiet_ctx(&owned),
        );
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(obs.pending, 1);
    }

    #[test]
    fn reedit_of_surfaced_item_surfaces_old_to_new_once() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let v1 = doc("- current task\n- add retries\n", EX);
        let v2 = doc("- current task\n- add retries with backoff\n", EX);
        let first = observe(
            &seeded(&baseline, Some("current task")),
            &v1,
            &quiet_ctx(&owned),
        );
        assert_eq!(first.ready[0].change, SteeringChange::Added);
        let second = observe(&first.next, &v2, &quiet_ctx(&owned));
        assert_eq!(second.ready.len(), 1, "{:?}", second.ready);
        assert_eq!(second.ready[0].change, SteeringChange::Edited);
        assert_eq!(second.ready[0].previous.as_deref(), Some("add retries"));
        assert_eq!(
            second.ready[0].dispatch,
            SteeringDispatch::DrainAfterCurrent
        );
        assert!(
            observe(&second.next, &v2, &quiet_ctx(&owned))
                .ready
                .is_empty()
        );
    }

    #[test]
    fn debounce_window_is_honoured() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = doc("- current task\n- something new\n", EX);
        let wm = seeded(&baseline, Some("current task"));
        let ctx = |debounce_ms| ObserveContext {
            now_ms: 5_000,
            document_changed_ms: Some(1_000),
            debounce_ms,
            binary_owned_queue_ids: &owned,
        };
        assert_eq!(observe(&wm, &current, &ctx(3_000)).ready.len(), 1);
        assert!(observe(&wm, &current, &ctx(10_000)).ready.is_empty());
    }

    #[test]
    fn nothing_new_renders_nothing() {
        assert_eq!(render_steering_context("plan.md", &[], 3), None);
    }
}
