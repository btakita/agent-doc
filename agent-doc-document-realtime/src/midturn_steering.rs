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
//!   item is never interrupted or interleaved. An explicit `Run Agent Doc`
//!   send upgrades this last case to `address_now`: pressing send is an
//!   authoritative request for the owning turn to handle the item now.
//! - deletion of a claimed queue item → `cancel_claimed`: removal is an
//!   authoritative cancellation, so the harness interrupts that exact worker
//!   attempt and retires its generation-fenced claim. A different row added
//!   in its place is a deletion plus new work, not an edit (`#krwr`).
//!
//! The watermark is the exactly-once fence. Queue steering is tracked as an
//! *acknowledged queue* (the queue as of the last surfacing), so a later edit
//! to an already-surfaced item aligns against what the agent was told and
//! surfaces once as `previous → verbatim`. Exchange steering is tracked by the
//! identity of every surfaced directive (`RealtimeSteering::identity`).
//!
//! Settle detection (`#steeringtypinggate`): the operator may be
//! mid-keystroke when a hook fires. Each candidate goes through the shared
//! operator-edit gate, `agent_doc_debounce::edit_settle::settle_decision`, the
//! same decision preflight admission reads: structural completion
//! (`completion_signal`: a trailing article or function word, an unbalanced
//! delimiter, or a dangling connector is unfinished; terminal punctuation, a
//! closed `[#id]`, or a URL is finished), adaptive quiescence (finished text
//! settles after half the debounce window, inconclusive text after the full
//! window, unfinished text never on quiescence alone), and a hard max-hold
//! after which the item is delivered anyway, flagged `possibly_partial`. Held
//! items stay in `pending` (with the deadline at which the decision can next
//! change) and surface on a later observation; a re-edit before delivery
//! supersedes the held version.
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
    /// Legacy routing retained for decoding steering envelopes emitted before
    /// Reported handbacks stopped holding unrelated dispatch.
    AwaitReportedCloseout,
    /// The item is already claimed by a worker (`agent-doc queue claim`):
    /// forward the operator's change to that owner, never dispatch a new
    /// subagent (`#claimedsteerwake`). [`SteeringItem::owner`] names it.
    ForwardToOwner,
    /// The operator removed an item owned by an active worker attempt. The
    /// harness must interrupt that exact worker and retire its claim; it may
    /// not continue based on sunk-cost judgment (`#queue-removal-cancels`).
    CancelClaimed,
    /// A deleted struck row still has durable live claim/report/outbox
    /// evidence. Markdown presentation cannot override that conflict; the
    /// harness preserves the attempt and reports degraded state.
    DegradedClaimMismatch,
    /// The item's worker already reported and its claim was released with a
    /// receipt (`#reportedrelease`): the coordinator persists that result in
    /// the current turn, with the closeout that strikes the line. Never a new
    /// subagent dispatch. [`SteeringItem::owner`] names the reported worker.
    PersistReported,
    /// The operator changed a queue marker's subagent mode mid-turn
    /// (`#queuemodesteer`): adding or removing `subagents` / `fan-out` / `=N`,
    /// or a queue preset that resolves to subagents. [`SteeringItem::queue_mode`]
    /// names the queue and the new mode; `verbatim` is the marker line.
    QueueModeChanged,
}

impl SteeringDispatch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddressNow => "address_now",
            Self::DrainAfterCurrent => "drain_after_current",
            Self::Subagent => "subagent",
            Self::AwaitReportedCloseout => "await_reported_closeout",
            Self::ForwardToOwner => "forward_to_owner",
            Self::CancelClaimed => "cancel_claimed",
            Self::DegradedClaimMismatch => "degraded_claim_mismatch",
            Self::PersistReported => "persist_reported",
            Self::QueueModeChanged => "queue_mode_changed",
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
    /// `agent:exchange preset="…"` attribute.
    Exchange,
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
    /// Delivered by the max-hold, not because it settled: the operator may
    /// still have been typing it (`#steeringtypinggate`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub possibly_partial: bool,
    /// The claim owner (`subagent:<label>`) for a
    /// [`SteeringDispatch::ForwardToOwner`] item (`#claimedsteerwake`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Generation fence for an active claimed attempt. Present for claim-owned
    /// routing so cancellation cannot release a newer replacement worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// Concrete harness worker identity recorded by `queue start`, when that
    /// acknowledgement has happened. Harnesses use it for direct interrupt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// The operator explicitly sent this item (`Run Agent Doc` / `agent-doc
    /// route`, `#claimedsteerwake`): it bypassed the typing gate and is final,
    /// never partial. Rendered as `sent=explicit`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub explicit: bool,
    /// The full nest vector (`parent: child`) of a nested queue leaf, when it
    /// differs from the visible row. A short child such as `#review` under two
    /// parents is otherwise ambiguous to the agent and to `queue claim --item`,
    /// which binds the first unclaimed row with that leaf (`#nestlistlineage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<String>,
    /// The queue and its new subagent mode for a
    /// [`SteeringDispatch::QueueModeChanged`] item (`#queuemodesteer`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_mode: Option<QueueModeChange>,
}

impl SteeringItem {
    /// The text that names this item's queue work unambiguously: its nest
    /// vector for a nested leaf, else the verbatim row. Every queue command a
    /// rendering hands the agent (`claim`, `start`, `brief`, `release`) uses it.
    pub fn claim_text(&self) -> &str {
        self.vector.as_deref().unwrap_or(&self.verbatim)
    }
}

/// The nest vector of a nested queue row, `None` for a top-level row whose
/// lineage is its own (normalized) text.
fn nested_vector(item: &QueueItem) -> Option<String> {
    (normalize_queue_text(&item.raw) != item.norm).then(|| item.norm.clone())
}

/// One queue's subagent mode as the operator just set it (`#queuemodesteer`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueModeChange {
    /// The queue block id (`id=` or its derived name).
    pub queue: String,
    /// True when the queue now runs its work in subagents.
    pub subagents: bool,
}

/// A held (unsettled) candidate observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingObservation {
    pub content_hash: String,
    pub first_seen_ms: u64,
    /// When the item was first observed in any version (`#steergatelog`):
    /// a re-edit before delivery carries this forward from the superseded
    /// version, so "time since the item started" survives keystrokes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_started_ms: Option<u64>,
    /// The item's length in characters at `item_started_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_started_chars: Option<u64>,
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
    /// Queue view of the PREVIOUS cycle's seed, kept when preflight re-seeds
    /// the same cycle, so "new since the last cycle" (`queue_subagent_dispatch`)
    /// stays stable across a re-entrant preflight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_cycle_queue: Option<Vec<String>>,
    /// Struck-row deletions this consumer already reported as
    /// [`SteeringDispatch::DegradedClaimMismatch`], keyed by
    /// [`struck_removal_report_key`] (`#deletedstruckmismatch`). The struck
    /// row is diffed against the cycle `baseline`, which never advances, so
    /// without this receipt every later observation of a changed document
    /// re-derived and re-reported the same deletion.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub surfaced_struck_removals: BTreeSet<String>,
    /// For a committed-baseline fallback watermark: the identity
    /// ([`SteeringWatermark::identity`]) of the stale seeded base it stood in
    /// for. Every consumer keeps this watermark's progress until that base is
    /// re-seeded, instead of bouncing between the stale base and the fallback
    /// and re-delivering everything the fallback already surfaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_of: Option<String>,
    /// Queue id -> whether it runs in subagents, as last surfaced
    /// (`#queuemodesteer`). `None` until the first observation derives it
    /// from `baseline`, so a watermark written before this field existed
    /// compares against the admitted document, never against nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_queue_modes: Option<BTreeMap<String, bool>>,
}

impl SteeringWatermark {
    /// The seeded cycle and baseline this watermark describes.
    pub fn identity(&self) -> String {
        format!("{}:{}", self.cycle_id, content_hash(&self.baseline))
    }

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
            // Steering follows the same executable-head view as queue claim
            // and dispatch. Binary-owned context parents and completed
            // response metadata can appear or disappear during closeout, but
            // they are presentation, never newly runnable work.
            acknowledged_queue: executable_queue_items(baseline)
                .into_iter()
                .map(|item| item.norm)
                .collect(),
            surfaced_exchange: BTreeMap::new(),
            pending: BTreeMap::new(),
            last_observed_content_hash: Some(content_hash(baseline)),
            last_observed_stat: None,
            closed: false,
            prior_cycle_queue: None,
            surfaced_struck_removals: BTreeSet::new(),
            fallback_of: None,
            acknowledged_queue_modes: None,
        }
    }
}

/// Inputs that are not part of the document text.
#[derive(Debug, Clone, Copy)]
pub struct ObserveContext<'a> {
    /// Hard max-hold: past this, an unsettled item is delivered anyway,
    /// flagged `possibly_partial` (`#steeringtypinggate`).
    pub max_hold_ms: u64,
    /// Optional cached completion verdicts; `DeterministicOnly` by default.
    pub classifier: &'a dyn agent_doc_debounce::edit_settle::CompletionClassifier,
    /// The operator's rolling median pause between document edits
    /// (`#steergatelog`), when the decision log has observed enough edits.
    pub median_pause_ms: Option<u64>,
    pub now_ms: u64,
    /// When the document last changed (file mtime / editor edit time).
    /// `None` means unknown: only per-item stability can settle an item.
    pub document_changed_ms: Option<u64>,
    pub debounce_ms: u64,
    /// `do [#id]` ids the binary itself mirrored/added this cycle; queue lines
    /// naming only these are binary bookkeeping, not operator steering.
    pub binary_owned_queue_ids: &'a BTreeSet<String>,
    /// The operator explicitly sent the document (`#claimedsteerwake`): every
    /// candidate is final, so the typing gate is bypassed and each ready item
    /// is marked [`SteeringItem::explicit`].
    pub explicit_send: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub ready: Vec<SteeringItem>,
    /// Candidates still being typed (held by the debounce).
    pub pending: usize,
    /// When the earliest held candidate's decision can next change, in ms
    /// from `now_ms`. Callers schedule one re-observation instead of polling.
    pub recheck_after_ms: Option<u64>,
    /// One settle decision per candidate, with its features (`#steergatelog`).
    pub decisions: Vec<GateDecision>,
    pub next: SteeringWatermark,
}

/// One settle decision for one candidate, as the decision log records it
/// (`#steergatelog`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    /// The candidate key (changes with every keystroke for additions).
    pub key: String,
    /// Hash of the normalized item text: the identity of this version.
    pub text_hash: String,
    /// For an edit: hash of the normalized text the agent last knew.
    pub previous_text_hash: Option<String>,
    /// The end of the item's text as decided on, bounded to
    /// [`GATE_TEXT_TAIL_CHARS`] (`#steergatenamo`): what a text classifier
    /// would have seen at this pause. The hash alone cannot be scored.
    pub text_tail: String,
    pub source: SteeringSource,
    pub change: SteeringChange,
    pub decision: agent_doc_debounce::edit_settle::SettleDecision,
    /// The deterministic tier, regardless of any classifier verdict.
    pub tier: agent_doc_debounce::edit_settle::SettleTier,
    pub features: agent_doc_debounce::edit_settle::GateFeatures,
}

/// Identity of an item version for the decision log: the hash of its
/// normalized text, the same for exchange and queue items.
pub fn gate_text_hash(text: &str) -> String {
    content_hash(&normalize_queue_text(text))
}

/// Characters of item text the decision log keeps per row (`#steergatenamo`).
/// End-of-turn evidence lives at the end of the text, so keep the tail.
pub const GATE_TEXT_TAIL_CHARS: usize = 280;

/// The last [`GATE_TEXT_TAIL_CHARS`] characters of `text`, on a char boundary.
pub fn gate_text_tail(text: &str) -> String {
    let trimmed = text.trim_end();
    let skip = trimmed.chars().count().saturating_sub(GATE_TEXT_TAIL_CHARS);
    trimmed.chars().skip(skip).collect()
}

/// The candidate-key family: versions of one item being typed share it.
fn candidate_family(key: &str) -> &str {
    match key.rsplit_once(':') {
        Some((family, _)) if key.starts_with("queue:") => family,
        _ => "exchange",
    }
}

/// Whether the observation happens inside the running turn or at/after its
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveMode {
    /// Mid-turn: the cycle is open and executing its current item.
    InTurn,
    /// Turn boundary (`respond` / `write --commit` terminal report, the
    /// post-commit `session-check`, or a `steering` poll after close): the
    /// cycle's current item is finished (consumed or answered), so its
    /// removal or the answered prompt's resolution is closeout bookkeeping,
    /// not steering, and every other unsurfaced change is reported in queue
    /// terms for the NEXT cycle (`#closeout-steering`).
    Boundary,
}

/// Compare `current` against the watermark and return settled new steering
/// plus the advanced watermark. Pure: callers persist `next`.
pub fn observe(
    watermark: &SteeringWatermark,
    current: &str,
    ctx: &ObserveContext<'_>,
) -> Observation {
    observe_with_mode(watermark, current, ctx, ObserveMode::InTurn)
}

/// [`observe`] with an explicit [`ObserveMode`].
pub fn observe_with_mode(
    watermark: &SteeringWatermark,
    current: &str,
    ctx: &ObserveContext<'_>,
    mode: ObserveMode,
) -> Observation {
    let presets = agent_doc_frontmatter::frontmatter::parse(current)
        .map(|(fm, _)| fm.prompt_presets)
        .unwrap_or_default();
    observe_with_mode_and_presets(watermark, current, ctx, mode, &presets)
}

/// [`observe_with_mode`] with caller-resolved effective presets.
pub fn observe_with_mode_and_presets(
    watermark: &SteeringWatermark,
    current: &str,
    ctx: &ObserveContext<'_>,
    mode: ObserveMode,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Observation {
    let boundary = mode == ObserveMode::Boundary;
    let mut candidates: Vec<Candidate> = Vec::new();
    candidates.extend(exchange_candidates(
        watermark,
        current,
        boundary,
        prompt_presets,
    ));
    let queue = queue_alignment(
        watermark,
        current,
        ctx.binary_owned_queue_ids,
        boundary,
        prompt_presets,
    );
    candidates.extend(queue.candidates.iter().cloned());
    let queue_modes = queue_mode_candidates(watermark, current, prompt_presets);
    candidates.extend(queue_modes.candidates.iter().cloned());

    use agent_doc_debounce::edit_settle::{
        CompletionSignal, GateComponent, GateFeatures, SettleDecision, SettleInputs,
        closed_list_item, completion_signal, deterministic_tier, has_unbalanced_delimiters,
        settle_decision, trailing_token, typing_chars_per_min,
    };
    // `#steergatelog`: a version that vanished from the candidates was
    // superseded by a re-edit; its successor in the same family inherits the
    // item's start.
    let live_keys: BTreeSet<&str> = candidates.iter().map(|c| c.key.as_str()).collect();
    let mut superseded_starts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for (key, pending) in &watermark.pending {
        if live_keys.contains(key.as_str()) {
            continue;
        }
        let start = (
            pending.item_started_ms.unwrap_or(pending.first_seen_ms),
            pending.item_started_chars.unwrap_or(0),
        );
        superseded_starts
            .entry(candidate_family(key).to_string())
            .and_modify(|existing| {
                if start.0 < existing.0 {
                    *existing = start;
                }
            })
            .or_insert(start);
    }
    let mut decisions = Vec::new();
    let quiet_for_ms = ctx
        .document_changed_ms
        .map(|changed| ctx.now_ms.saturating_sub(changed));
    let mut ready_keys = BTreeSet::new();
    let mut partial_keys = BTreeSet::new();
    let mut next_pending = BTreeMap::new();
    let mut recheck_after_ms: Option<u64> = None;
    for candidate in &candidates {
        let prior = watermark
            .pending
            .get(&candidate.key)
            .filter(|prior| prior.content_hash == candidate.content_hash);
        let stable_for_ms = prior.map(|prior| ctx.now_ms.saturating_sub(prior.first_seen_ms));
        let signal = if candidate.item.change == SteeringChange::Deleted {
            // A removal has no text left to finish.
            CompletionSignal::Complete
        } else {
            completion_signal(&candidate.item.verbatim)
        };
        let chars = candidate.item.verbatim.chars().count() as u64;
        let (item_started_ms, item_started_chars) = match prior {
            Some(prior) => (
                prior.item_started_ms.unwrap_or(prior.first_seen_ms),
                prior.item_started_chars.unwrap_or(chars),
            ),
            None => superseded_starts
                .get(candidate_family(&candidate.key))
                .copied()
                .unwrap_or((ctx.now_ms, chars)),
        };
        let last_edit_ms = ctx.document_changed_ms.unwrap_or(ctx.now_ms);
        let unbalanced_delimiters = candidate.item.change != SteeringChange::Deleted
            && has_unbalanced_delimiters(&candidate.item.verbatim);
        let features = GateFeatures {
            trailing_token: trailing_token(&candidate.item.verbatim),
            signal,
            unbalanced_delimiters,
            closed_list_item: candidate.item.change != SteeringChange::Deleted
                && closed_list_item(current, &candidate.item.verbatim),
            quiet_ms: quiet_for_ms.max(stable_for_ms).unwrap_or(0),
            median_pause_ms: ctx.median_pause_ms,
            typing_chars_per_min: typing_chars_per_min(
                chars.saturating_sub(item_started_chars),
                last_edit_ms.saturating_sub(item_started_ms),
            ),
            item_age_ms: ctx.now_ms.saturating_sub(item_started_ms),
            component: match candidate.item.source {
                SteeringSource::Queue => GateComponent::Queue,
                SteeringSource::Exchange => GateComponent::Exchange,
            },
            debounce_ms: ctx.debounce_ms,
            max_hold_ms: ctx.max_hold_ms,
        };
        // `#steergateperceptron`: the classifier sees the same features the
        // decision log records; the deterministic floors stay inside
        // `settle_decision`.
        let settle_inputs = SettleInputs {
            quiet_for_ms,
            stable_for_ms,
            held_for_ms: stable_for_ms.unwrap_or(0),
            debounce_ms: ctx.debounce_ms,
            max_hold_ms: ctx.max_hold_ms,
            signal,
            unbalanced_delimiters,
            verdict: ctx.classifier.assess(&features, &candidate.content_hash),
        };
        // An explicit send is the operator saying "this is final": no gate.
        let decision = if ctx.explicit_send {
            SettleDecision::Settled
        } else {
            settle_decision(settle_inputs)
        };
        decisions.push(GateDecision {
            key: candidate.key.clone(),
            text_hash: gate_text_hash(&candidate.item.verbatim),
            previous_text_hash: candidate.item.previous.as_deref().map(gate_text_hash),
            text_tail: gate_text_tail(&candidate.item.verbatim),
            source: candidate.item.source,
            change: candidate.item.change,
            decision,
            tier: deterministic_tier(settle_inputs),
            features,
        });
        if decision.deliver() {
            ready_keys.insert(candidate.key.clone());
            if decision == SettleDecision::MaxHoldExpired {
                partial_keys.insert(candidate.key.clone());
            }
        } else {
            if let SettleDecision::Held {
                recheck_after_ms: after,
            } = decision
            {
                recheck_after_ms = Some(recheck_after_ms.map_or(after, |prev| prev.min(after)));
            }
            next_pending.insert(
                candidate.key.clone(),
                PendingObservation {
                    content_hash: candidate.content_hash.clone(),
                    first_seen_ms: prior.map_or(ctx.now_ms, |prior| prior.first_seen_ms),
                    item_started_ms: Some(item_started_ms),
                    item_started_chars: Some(item_started_chars),
                },
            );
        }
    }

    let mut next = watermark.clone();
    next.pending = next_pending;
    next.last_observed_content_hash = Some(content_hash(current));
    let mut acknowledged_modes = queue_modes.acknowledged.clone();
    for (key, queue, subagents) in &queue_modes.changes {
        if ready_keys.contains(key) {
            acknowledged_modes.insert(queue.clone(), *subagents);
        }
    }
    next.acknowledged_queue_modes = Some(acknowledged_modes);
    let mut ready = Vec::new();
    for candidate in &candidates {
        if !ready_keys.contains(&candidate.key) {
            continue;
        }
        if let Some(identity) = &candidate.exchange_identity {
            next.surfaced_exchange
                .insert(identity.clone(), candidate.item.verbatim.clone());
        }
        let mut item = candidate.item.clone();
        // `#explicitqueueaddress`: passive queue additions stay ordered behind
        // the active head, but Run Agent Doc is an explicit send to the owning
        // turn. Preserve stronger subagent/claim routing; only upgrade the
        // otherwise-deferred queue case.
        if ctx.explicit_send && item.dispatch == SteeringDispatch::DrainAfterCurrent {
            item.dispatch = SteeringDispatch::AddressNow;
        }
        item.possibly_partial = partial_keys.contains(&candidate.key);
        item.explicit = ctx.explicit_send;
        ready.push(item);
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
        recheck_after_ms,
        decisions,
        ready,
        next,
    }
}

/// `#queuemodesteer`: queue marker mode flips since the last surfacing.
struct QueueModeCandidates {
    candidates: Vec<Candidate>,
    /// The acknowledged map before this observation's deliveries, with any
    /// queue first seen now recorded at its current mode.
    acknowledged: BTreeMap<String, bool>,
    /// (candidate key, queue id, new mode) per flip.
    changes: Vec<(String, String, bool)>,
}

/// Each queue block's id, marker line, and whether it runs in subagents
/// ([`queue_component_runs_in_subagents`]).
fn queue_modes(
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> BTreeMap<String, (String, bool)> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return BTreeMap::new();
    };
    let Ok(graph) = agent_doc_queue::queue_set::QueueSet::parse(content, &components) else {
        return BTreeMap::new();
    };
    graph
        .blocks()
        .iter()
        .map(|block| {
            let component = block.component;
            let line_start = content[..component.open_start]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            let marker = content[line_start..component.open_end].trim_end().to_string();
            (
                block.id.to_string(),
                (
                    marker,
                    queue_component_runs_in_subagents(content, component, prompt_presets),
                ),
            )
        })
        .collect()
}

/// `#queuemodesteer`: the operator turning subagent dispatch on or off for a
/// queue mid-turn is steering, never a binary-owned marker change. One
/// candidate per queue whose mode differs from the acknowledged mode.
fn queue_mode_candidates(
    watermark: &SteeringWatermark,
    current: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> QueueModeCandidates {
    let baseline_modes = queue_modes(&watermark.baseline, prompt_presets);
    let mut acknowledged = watermark.acknowledged_queue_modes.clone().unwrap_or_else(|| {
        baseline_modes
            .iter()
            .map(|(queue, (_, subagents))| (queue.clone(), *subagents))
            .collect()
    });
    let mut candidates = Vec::new();
    let mut changes = Vec::new();
    for (queue, (marker, subagents)) in queue_modes(current, prompt_presets) {
        let Some(&known) = acknowledged.get(&queue) else {
            acknowledged.insert(queue, subagents);
            continue;
        };
        if known == subagents {
            continue;
        }
        let key = format!(
            "queue_mode:{queue}:{}",
            if subagents { "on" } else { "off" }
        );
        candidates.push(Candidate {
            key: key.clone(),
            content_hash: content_hash(&marker),
            exchange_identity: None,
            item: SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Edited,
                dispatch: SteeringDispatch::QueueModeChanged,
                current_item: false,
                previous: baseline_modes
                    .get(&queue)
                    .map(|(marker, _)| marker.clone())
                    .filter(|previous| previous != &marker),
                verbatim: marker,
                presets: Vec::new(),
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: None,
                queue_mode: Some(QueueModeChange {
                    queue: queue.clone(),
                    subagents,
                }),
            },
        });
        changes.push((key, queue, subagents));
    }
    QueueModeCandidates {
        candidates,
        acknowledged,
        changes,
    }
}

/// `#queuemodesteer`: what the coordinator does when a queue's subagent mode
/// changes mid-turn. The dispatch list is a function of the current document
/// (`agent-doc plan` re-derives it), so re-planning picks the new mode up.
pub fn queue_mode_change_instruction(document: &str, change: &QueueModeChange) -> String {
    if change.subagents {
        format!(
            "the operator turned subagent dispatch ON for queue `{queue}`. Run `agent-doc plan \
             {document}` now: every unstarted head of that queue is in its \
             `queue_subagent_dispatch`; claim, spawn, and start them all in parallel now, each in \
             its own worktree. The head you are executing inline (if it is in that queue) hands \
             off only if it has produced no outward effect yet (no commit, push, file edit outside \
             scratch, message, API call, or document write): then stop it and dispatch it with the \
             rest. Otherwise finish it inline and skip its dispatch entry. Never run one head both \
             inline and in a subagent, and never redo work a head already did.",
            queue = change.queue,
        )
    } else {
        format!(
            "the operator turned subagent dispatch OFF for queue `{queue}`. Stop claiming or \
             spawning new heads from it. Workers already running continue to completion and are \
             closed out as usual; the remaining unstarted heads drain inline in operator order \
             after the current item. `agent-doc plan {document}` no longer lists them in \
             `queue_subagent_dispatch`.",
            queue = change.queue,
        )
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    key: String,
    content_hash: String,
    exchange_identity: Option<String>,
    item: SteeringItem,
}

fn exchange_candidates(
    watermark: &SteeringWatermark,
    current: &str,
    boundary: bool,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Vec<Candidate> {
    let presets =
        PresetContext::new_with_presets(current, &watermark.session_presets, prompt_presets);
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
            // At the boundary the answered prompt resolving is closeout, not
            // steering.
            RealtimeSteering::PromptDeleted { .. } | RealtimeSteering::PromptReduced { .. }
                if boundary =>
            {
                continue;
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
        let (dispatch, preset_intents) = if current_item || change == SteeringChange::Deleted {
            (SteeringDispatch::AddressNow, Vec::new())
        } else {
            presets.classify(&verbatim, SteeringSource::Exchange)
        };
        out.push(Candidate {
            key: format!("exchange:{identity}"),
            content_hash: identity.clone(),
            exchange_identity: Some(identity),
            item: SteeringItem {
                source: SteeringSource::Exchange,
                change,
                dispatch,
                current_item,
                verbatim,
                previous,
                presets: preset_intents,
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: None,
                queue_mode: None,
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
/// response heading, or the end of the exchange component. Other component
/// close markers may be examples inside the response and are not boundaries.
fn strip_new_response_sections(baseline: &str, current: &str) -> String {
    let baseline_headings: BTreeSet<&str> = baseline
        .lines()
        .map(str::trim)
        .filter(|line| crate::baseline_comparison::is_exchange_response_heading(line))
        .collect();
    let baseline_lines: BTreeSet<&str> = baseline.lines().map(str::trim).collect();
    let mut out = String::with_capacity(current.len());
    let mut skipping = false;
    // `#echopreludesteer`: held `> **Queue prompt:**` prelude awaiting proof
    // that a new response heading follows it.
    let mut prelude: Option<String> = None;
    // `#chatprompt`: an agent-written record of a prompt that arrived in the
    // harness chat (`> **Chat prompt (#chatprompt):** …`, plus its `>`
    // continuation lines) is agent bookkeeping, never operator steering.
    let mut in_chat_record = false;
    for line in current.split_inclusive('\n') {
        let trimmed = line.trim();
        // Once a newly added record starts, every contiguous blockquote line
        // belongs to that record. Do not consult `baseline_lines` here: a
        // quoted blank separator is just `>`, which commonly already exists in
        // an older record. Treating that repeated line as old content would end
        // the record early and reclassify the following paragraph as steering.
        if in_chat_record && trimmed.starts_with('>') {
            continue;
        }
        in_chat_record = false;
        if agent_doc_prompt_lines::is_chat_prompt_record_line(trimmed)
            && !baseline_lines.contains(trimmed)
        {
            in_chat_record = true;
            continue;
        }
        let heading = crate::baseline_comparison::is_exchange_response_heading(trimmed);
        // `#echopreludesteer`: a response opens with its `> **Queue prompt:**`
        // echo, often followed by a prose paragraph (a `**Deferred:**` outcome
        // the finalize gate demands), ABOVE its `### Re:` heading. Skipping
        // began only at the heading, so that agent-authored prelude came back
        // as `source=exchange change=added dispatch=subagent` operator prompts
        // (2026-11-03, agent-doc-bugs.md, commit b3bd1a09f7). A new echo line
        // is agent-authored by construction, but it is only a prelude when a
        // NEW response heading follows it: hold the echo and the lines after
        // it, drop them at that heading, and release them unchanged at a
        // boundary, the component end, or EOF.
        //
        // `#committedresponseown`: whether the echo text already occurs in
        // the baseline proves nothing. The Waiting-on-you echo is
        // byte-identical in every response that reports the group, so from
        // the second such response on its echo "was in the baseline" and was
        // never held: after every closeout the hook delivered it as
        // `source=exchange change=added dispatch=subagent`. The NEW heading
        // that must follow is the proof; a baseline echo above its own
        // baseline heading is released unchanged.
        if !skipping && !heading && prelude.is_none() && is_queue_prompt_echo_line(trimmed) {
            prelude = Some(String::new());
        }
        if heading {
            skipping = !baseline_headings.contains(trimmed);
            if let Some(held) = prelude.take()
                && !skipping
            {
                out.push_str(&held);
            }
        } else if trimmed.starts_with("<!-- agent:boundary:")
            || trimmed == "<!-- /agent:exchange -->"
        {
            skipping = false;
            if let Some(held) = prelude.take() {
                out.push_str(&held);
            }
        }
        if let Some(held) = prelude.as_mut() {
            held.push_str(line);
        } else if !skipping {
            out.push_str(line);
        }
    }
    if let Some(held) = prelude {
        out.push_str(&held);
    }
    out
}

/// A `> **Queue prompt:** …` response echo line.
fn is_queue_prompt_echo_line(trimmed: &str) -> bool {
    trimmed
        .strip_prefix('>')
        .map(str::trim_start)
        .is_some_and(|rest| rest.starts_with("**Queue prompt:**"))
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
    agent_doc_queue::queue_head_alignment::normalize_queue_head_text(text)
}

/// Live, executable queue heads (group-parent labels excluded) in queue
/// order: exactly the set `agent-doc queue claim` resolves `--item` against
/// (`live_queue_head_texts`), so dispatch can never offer a head the claim
/// path refuses as "no live queue head" (GH #124 ask 5).
fn executable_queue_items(content: &str) -> Vec<QueueItem> {
    agent_doc_queue::queue_continuation::live_executable_rows(content)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(raw, lineage)| {
            let norm = normalize_queue_text(&lineage);
            (!norm.is_empty()).then_some(QueueItem {
                // Steering remains operator-facing and therefore preserves the
                // visible row. Identity is the exact claim lineage above.
                raw,
                norm,
            })
        })
        .collect()
}

/// Whether a queue head waits on the operator (`#queueawaitop`, `⏳`): its
/// tracked item is `[operator-verify]` or a gated `agent:review` row and the
/// head carries no inline operator verdict. Such a head has no agent work, so
/// it is never subagent dispatch work (`#regressionawaitingoperat`): before
/// this, a finished `[operator-verify]` head whose worker released its claim
/// was re-listed under `queue_subagent_dispatch` every preflight.
fn head_awaits_operator(norm: &str, awaiting_operator: &std::collections::HashSet<String>) -> bool {
    !awaiting_operator.is_empty()
        && agent_doc_queue::queue_continuation::extract_head_id(norm)
            .is_some_and(|id| awaiting_operator.contains(&id.to_ascii_lowercase()))
}

/// The shared queue-head work identity (`#claimdispatchidentity`):
/// alignment, dispatch newness and claims all key on
/// [`agent_doc_element_queue::queue_head_identity`].
fn queue_identity(norm: &str) -> String {
    format!("{:?}", agent_doc_element_queue::queue_head_identity(norm))
}

/// Whether `new` is a newly executable descendant of `old`, rather than an
/// edit of the same executable row.
///
/// Nested free-text leaves key claims by their full lineage (`parent: leaf`).
/// When a formerly executable parent gains its first child, the generic text
/// matcher sees the old parent as a substring of that lineage and would pair
/// the child as an edit. The visible row proves the structural boundary: the
/// suffix added to the old lineage is exactly the new visible leaf. A real
/// row edit has the edited text as its visible row instead.
fn is_lineage_descendant(old: &str, new: &QueueItem) -> bool {
    new.norm
        .strip_prefix(old)
        .and_then(|suffix| suffix.strip_prefix(": "))
        .is_some_and(|suffix| normalize_queue_text(suffix) == normalize_queue_text(&new.raw))
}

/// Whether the queue item selected for this cycle is still present in the
/// live document. Its continued presence is the queue lifecycle's typed proof
/// that the active head has not closed, even if stale-lock repair prematurely
/// marked the cycle record terminal.
pub fn current_item_is_present(watermark: &SteeringWatermark, current: &str) -> bool {
    let Some(selected) = watermark.current_item.as_deref() else {
        return false;
    };
    let selected_identity = queue_identity(selected);
    executable_queue_items(current)
        .iter()
        .any(|item| item.norm == selected || queue_identity(&item.norm) == selected_identity)
}

fn queue_alignment(
    watermark: &SteeringWatermark,
    current: &str,
    binary_owned_ids: &BTreeSet<String>,
    boundary: bool,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> QueueAlignment {
    let old = &watermark.acknowledged_queue;
    // `#steeringclaimparity`: only claimable queue heads can become steering
    // work. In particular, the binary-owned Waiting-on-you mirror contains
    // context parents and completed response metadata; aligning the raw tree
    // offered those rows as fresh subagent dispatch even though `queue claim`
    // correctly refused every one of them.
    let new = executable_queue_items(current);
    let old_keys: Vec<String> = old.iter().map(|norm| queue_identity(norm)).collect();
    // At the boundary the current item is finished: its consumption is not
    // an operator deletion and an edit to it is ordinary queue work.
    let current_item = if boundary {
        None
    } else {
        watermark.current_item.as_deref()
    };
    let current_index = current_item.and_then(|current_item| {
        let identity = queue_identity(current_item);
        old_keys
            .iter()
            .position(|key| key == &identity)
            .or_else(|| old.iter().position(|norm| norm == current_item))
    });
    let presets =
        PresetContext::new_with_presets(current, &watermark.session_presets, prompt_presets);
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
        let (dispatch, intents) = presets.classify(&item.raw, SteeringSource::Queue);
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
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: nested_vector(item),
                queue_mode: None,
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
            presets.classify(&item.raw, SteeringSource::Queue)
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
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: nested_vector(item),
                queue_mode: None,
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
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: None,
                queue_mode: None,
            },
        });
        alignment.events.push(QueueEvent::Delete {
            old: previous.clone(),
            key: Some(key),
        });
    };

    // `#steeringeditmisalign`: pair by item identity first. Myers over the
    // identity keys aligns the untouched run; an item whose identity survives
    // outside that run (a moved line) still pairs with itself. Only lines
    // with no identity partner left are offered to the similarity pairing,
    // so deleting struck lines above claimed items can never turn a removed
    // `do [#a]` into the "previous" text of an untouched `do [#b]`. The rule
    // is shared with the queue-claim ledger (`#claimfollowedit`), so a claim
    // follows exactly the edits steering reports.
    let new_norms: Vec<String> = new.iter().map(|item| item.norm.clone()).collect();
    let agent_doc_queue::queue_head_alignment::QueueHeadPairing {
        mut pair_of_new,
        mut old_paired,
        ops,
    } = agent_doc_queue::queue_head_alignment::pair_queue_heads(old, &new_norms);

    // `#nestedclaimsteer`: lineage is claim identity, not editable prose. A
    // parent becoming context and exposing a child is a parent deletion plus
    // child insertion even though `parent: child` contains `parent`. Detach
    // only similarity pairs; exact work-identity pairs still denote the same
    // row. Claim following independently applies the same context-parent rule.
    for n in 0..new.len() {
        let Some((o, true)) = pair_of_new[n] else {
            continue;
        };
        if is_lineage_descendant(&old[o], &new[n]) {
            pair_of_new[n] = None;
            old_paired[o] = false;
        }
    }

    let emit_new = |alignment: &mut QueueAlignment, n: usize| match pair_of_new[n] {
        Some((o, true)) if binary_owned_line(&new[n].norm, binary_owned_ids) => {
            delete(alignment, o);
            alignment.events.push(QueueEvent::Keep(new[n].norm.clone()));
        }
        Some((o, _)) if old[o] == new[n].norm => {
            alignment.events.push(QueueEvent::Keep(old[o].clone()));
        }
        // Same identity with different text, or a replace-hunk row mutation.
        Some((o, _)) => edit(alignment, o, n),
        None => insert(alignment, n),
    };
    for op in ops {
        let (old_range, new_range) = match op {
            similar::DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => (old_index..old_index, new_index..new_index + len),
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => (old_index..old_index + old_len, 0..0),
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => (0..0, new_index..new_index + new_len),
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
            ),
        };
        for o in old_range {
            if !old_paired[o] {
                delete(&mut alignment, o);
            }
        }
        for n in new_range {
            emit_new(&mut alignment, n);
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

struct PresetContext {
    prompt_presets: agent_doc_frontmatter::frontmatter::PromptPresets,
    cycle: Vec<PresetIntent>,
    queue: Vec<PresetIntent>,
    exchange: Vec<PresetIntent>,
    /// `<!-- agent:queue subagents -->` (alias `fan-out`): every queue line
    /// without an opt-out tag carries subagent intent.
    queue_subagents: bool,
    /// `<!-- agent:exchange subagents -->` (alias `fan-out`): each new
    /// exchange prompt is delegated while the parent retains cycle ownership.
    exchange_subagents: bool,
}

impl PresetContext {
    fn new_with_presets(
        current: &str,
        session_presets: &[String],
        prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
    ) -> Self {
        if let Ok(components) = agent_doc_element::element::parse(current)
            && let Ok(Some(queue)) =
                agent_doc_queue::queue_set::selected_component(current, &components)
        {
            return Self::from_queue_component(
                current,
                session_presets,
                prompt_presets,
                Some(queue),
            );
        }
        Self::from_queue_component(current, session_presets, prompt_presets, None)
    }

    /// Presets and the `subagents` flag of one queue block, not the claim-blind
    /// selected queue. Exchange defaults still come from the document.
    fn for_queue_component(
        current: &str,
        component: &agent_doc_element::element::Component,
        session_presets: &[String],
        prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
    ) -> Self {
        Self::from_queue_component(current, session_presets, prompt_presets, Some(component))
    }

    fn from_queue_component(
        current: &str,
        session_presets: &[String],
        prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
        queue: Option<&agent_doc_element::element::Component>,
    ) -> Self {
        let mut cycle: Vec<PresetIntent> = Vec::new();
        let mut queue_intents: Vec<PresetIntent> = Vec::new();
        let mut exchange_intents: Vec<PresetIntent> = Vec::new();
        let mut queue_attrs =
            agent_doc_queue::prompt_component_attrs::PromptComponentAttrs::default();
        let mut queue_names = Vec::new();
        if let Some(queue) = queue {
            queue_attrs =
                agent_doc_queue::prompt_component_attrs::prompt_component_attrs_for(queue, "queue");
            queue_names.extend(queue_attrs.preset_names().map(str::to_string));
            if let Ok(entries) = agent_doc_queue::document_queue::parse(queue.content(current)) {
                for entry in entries {
                    if let agent_doc_queue::document_queue::QueueEntry::Preset(name) = entry {
                        queue_names.push(name);
                    }
                }
            }
        }
        let exchange_attrs =
            agent_doc_queue::prompt_component_attrs::prompt_component_attrs(current, "exchange");
        let exchange_names: Vec<String> =
            exchange_attrs.preset_names().map(str::to_string).collect();
        // `#runpresetunresolved`: an inline `preset="#name: body"` definition
        // resolves exactly like a frontmatter entry, within its own component.
        let queue_presets = queue_attrs.overlay_inline_presets(prompt_presets);
        let exchange_presets = exchange_attrs.overlay_inline_presets(prompt_presets);
        for (names, scope, target, prompt_presets) in [
            (
                queue_names.as_slice(),
                PresetScope::Queue,
                &mut queue_intents,
                &queue_presets,
            ),
            (
                exchange_names.as_slice(),
                PresetScope::Exchange,
                &mut exchange_intents,
                &exchange_presets,
            ),
            (
                session_presets,
                PresetScope::Cycle,
                &mut cycle,
                prompt_presets,
            ),
        ] {
            for name in names {
                let requested = name.trim();
                let resolved = agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(
                    prompt_presets,
                    requested,
                );
                if let Some(name) = resolved {
                    if !target
                        .iter()
                        .any(|intent| intent.name.eq_ignore_ascii_case(&name))
                    {
                        target.push(PresetIntent {
                            body: prompt_presets.get(&name).cloned().unwrap_or_default(),
                            name,
                            scope,
                        });
                    }
                } else if !target
                    .iter()
                    .any(|intent| intent.name.eq_ignore_ascii_case(requested))
                {
                    // Unregistered names still carry literal intent.
                    target.push(PresetIntent {
                        name: requested.to_string(),
                        body: String::new(),
                        scope,
                    });
                }
            }
        }
        Self {
            prompt_presets: prompt_presets.clone(),
            cycle,
            queue: queue_intents,
            exchange: exchange_intents,
            queue_subagents: queue_attrs.subagents,
            exchange_subagents: exchange_attrs.subagents,
        }
    }

    /// Resolve presets for one queue line through the same resolver closeout
    /// uses (`queue_prompt_preset_expansions`) and classify its dispatch.
    fn classify(&self, raw: &str, source: SteeringSource) -> (SteeringDispatch, Vec<PresetIntent>) {
        let mut intents: Vec<PresetIntent> =
            agent_doc_queue::queue_response::queue_prompt_preset_expansions_with_presets(
                raw,
                &self.prompt_presets,
            )
            .into_iter()
            .map(|(name, body)| PresetIntent {
                name,
                body,
                scope: PresetScope::Item,
            })
            .collect();
        let literal_subagent_tag =
            agent_doc_queue::subagent_intent::carries_subagent_intent_tag(raw);
        let scoped = match source {
            SteeringSource::Queue => &self.queue,
            SteeringSource::Exchange => &self.exchange,
        };
        intents.extend(scoped.iter().cloned());
        intents.extend(self.cycle.iter().cloned());
        let component_attr_subagent = match source {
            SteeringSource::Queue => {
                self.queue_subagents
                    && !agent_doc_queue::subagent_intent::opts_out_of_queue_subagents(raw)
            }
            SteeringSource::Exchange => self.exchange_subagents,
        };
        let subagent = literal_subagent_tag
            || component_attr_subagent
            || intents
                .iter()
                .any(|intent| preset_requests_subagents(&intent.name, &intent.body));
        let dispatch = if subagent {
            SteeringDispatch::Subagent
        } else {
            match source {
                SteeringSource::Queue => SteeringDispatch::DrainAfterCurrent,
                SteeringSource::Exchange => SteeringDispatch::AddressNow,
            }
        };
        (dispatch, intents)
    }
}

/// `#subagentsalwaysparallel`: whether `component`'s queue work runs in
/// subagents at queue scope: its `subagents` / `fan-out` attribute, or a
/// queue-scoped preset that expands to subagent dispatch. Item-level intent
/// (`#subagents` on one line) is resolved per row by the dispatch list. Uses
/// the same [`PresetContext`] resolution as [`queue_component_subagent_heads`].
pub fn queue_component_runs_in_subagents(
    content: &str,
    component: &agent_doc_element::element::Component,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> bool {
    let presets = PresetContext::for_queue_component(content, component, &[], prompt_presets);
    presets.queue_subagents
        || presets
            .queue
            .iter()
            .chain(presets.cycle.iter())
            .any(|intent| preset_requests_subagents(&intent.name, &intent.body))
}

/// True when a preset (by name or expansion body) asks for subagent dispatch.
/// The vocabulary is shared with queue drainability
/// (`agent_doc_queue::subagent_intent`, `#subagentintent`).
pub fn preset_requests_subagents(name: &str, body: &str) -> bool {
    agent_doc_queue::subagent_intent::is_subagent_intent_tag(name)
        || agent_doc_queue::subagent_intent::text_requests_subagents(body)
}

/// "The operator is plainly still typing this": the shared gate's structural
/// signal (`#steeringtypinggate`) or one of the original conservative shapes.
pub fn looks_incomplete(text: &str) -> bool {
    agent_doc_debounce::edit_settle::completion_signal(text)
        == agent_doc_debounce::edit_settle::CompletionSignal::Incomplete
        || looks_incomplete_legacy(text)
}

fn looks_incomplete_legacy(text: &str) -> bool {
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

/// Harness-neutral steering instructions for an explicit Run Agent Doc on a
/// running turn (`#routesyncsteer`): the unified diff since the last send,
/// plus any classified items passive delivery would have injected. `None`
/// when both are empty.
pub fn render_route_sync_steering(
    document: &str,
    diff: Option<&str>,
    items: &[SteeringItem],
) -> Option<String> {
    let diff = diff.map(str::trim).filter(|diff| !diff.is_empty());
    let items_text = render_steering_context(document, items, 0);
    match (diff, items_text) {
        (None, None) => None,
        (Some(diff), items_text) => {
            let mut out = format!(
                "{STEERING_MARKER} in {document}. The operator sent the session document \
                 again while this turn was running. Here is the diff since the last send. \
                 Handle it as operator steering now; do not start a second turn, do not run \
                 `agent-doc preflight`, and do not `--force-disk`.\n\n{diff}"
            );
            if let Some(items_text) = items_text {
                out.push_str("\n\n");
                out.push_str(&items_text);
            }
            Some(out)
        }
        (None, Some(items_text)) => Some(items_text),
    }
}

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
            "\n\n[steering {}/{}] dispatch={} source={} change={}{}{}",
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
            },
            item_header_suffix(item),
        ));
        if let Some(vector) = &item.vector {
            out.push_str(&format!("\nvector: {vector}"));
        }
        if let Some(previous) = &item.previous {
            out.push_str(&format!("\nprevious: {previous}"));
        }
        let label = if item.change == SteeringChange::Deleted {
            "removed"
        } else {
            "verbatim"
        };
        out.push_str(&format!("\n{label}: {}", item.verbatim));
        if item.possibly_partial {
            out.push_str(
                "\npossibly_partial: true (delivered by the max-hold, not because it settled; \
                 the operator may still be typing it. Act on what is unambiguous and re-read it \
                 with `agent-doc steering --peek` before relying on its ending.)",
            );
        }
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
        out.push_str(&format!("\naction: {}", action_for(document, item)));
    }
    if pending > 0 {
        out.push_str(&format!(
            "\n\nThe operator is still typing {pending} more item(s); they will be surfaced \
             once settled."
        ));
    }
    Some(out)
}

/// How a dispatched queue item closes once its worker reports
/// (`#reportedrelease`): the closeout that strikes the line, not a bare
/// `queue release`, which leaves the line queued and answered nowhere.
pub const REPORTED_WORKER_CLOSEOUT: &str = "When the subagent reports back, persist its result in \
     THIS turn. First compare the handback with the item and resolved preset actions: execute and \
     verify every authorized current-item or returned-subagent outward effect before closeout; a \
     prepared artifact does not defer a post/publish/push/message/API effect to auto-clear \
     (`#effectbeforecloseout`). Then use the closeout that strikes the item: `agent-doc finalize` with the verbatim \
     `> **Queue prompt:** <line>` echo for a free-text line, or `--done <id>` for an id head. \
     That closeout also ends the claim. Only when integration must finish first, run \
     `agent-doc queue release` (it records that the worker reported, so the line is never \
     re-dispatched) and persist the result before you end the turn; `queue release --requeue` \
     abandons the work for a new worker.";

const FORWARD_EDITED: &str = "the operator EDITED a queue item a worker already claimed (`owner` \
     above; the claim followed the edit). FORWARD the change (`previous` -> `verbatim`) to that \
     owner now (for example SendMessage to the running subagent) so the work in flight picks it \
     up. If `verbatim` adds or keeps a nested line that says `push` or `publish`, tell the owner \
     that line is operator authorization to perform that action now (`#nestedoutwardauth`): it is \
     not a question, not a chat reply, and not blocked by a preset or skill that says to wait \
     until asked. A trailing `⟦effect:N⟧` marker does not remove it. `publish` on a review posts \
     the review as a plain comment unless the line says approve. Do NOT dispatch a new subagent \
     and do NOT re-claim the item.";
const FORWARD_ADDED: &str = "already claimed by the worker named in `owner` (in flight there). \
     Nothing to dispatch; do NOT start a second subagent for it.";

/// ` owner=<label>` / ` sent=explicit` header fields (`#claimedsteerwake`).
fn item_header_suffix(item: &SteeringItem) -> String {
    let mut out = String::new();
    if let Some(owner) = &item.owner {
        out.push_str(&format!(" owner={owner}"));
    }
    if let Some(generation) = item.generation {
        out.push_str(&format!(" generation={generation}"));
    }
    if let Some(worker_id) = &item.worker_id {
        out.push_str(&format!(" worker={worker_id}"));
    }
    if item.explicit {
        out.push_str(" sent=explicit");
    }
    if let Some(change) = &item.queue_mode {
        out.push_str(&format!(
            " queue={} subagents={}",
            change.queue,
            if change.subagents { "on" } else { "off" }
        ));
    }
    out
}

/// Re-route queue items a worker already claimed (`#claimedsteerwake`).
///
/// `owner_of` answers the claim owner for a queue line's text (the caller's
/// claim ledger, which follows operator annotations, `#claimfollowsedit`).
/// A claimed item becomes [`SteeringDispatch::ForwardToOwner`] with its owner:
/// an edit is forwarded to the running worker instead of offered as a NEW
/// subagent dispatch. A main-agent current item stays local unless its live
/// claim belongs to a `subagent:*` worker; a delegated row may remain the
/// watermark's selected item after claim/spawn/start and must still forward.
/// The edit's previous text is consulted too, so a legacy claim without an
/// anchor still routes conservatively to its owner.
pub fn apply_claim_owners(items: &mut [SteeringItem], owner_of: impl Fn(&str) -> Option<String>) {
    for item in items.iter_mut() {
        if item.source != SteeringSource::Queue {
            continue;
        }
        let owner = item
            .vector
            .as_deref()
            .and_then(&owner_of)
            .or_else(|| owner_of(&item.verbatim))
            .or_else(|| item.previous.as_deref().and_then(&owner_of));
        if let Some(owner) = owner {
            if item.current_item && !owner.starts_with("subagent:") {
                continue;
            }
            item.dispatch = SteeringDispatch::ForwardToOwner;
            item.presets.clear();
            item.owner = Some(owner);
        }
    }
}

/// Route live claimed items with their generation-fenced worker attempt.
///
/// This is the typed form used by the harness-facing IO layer. The older
/// [`apply_claim_owners`] remains for compatibility callers that only know an
/// owner label.
pub fn apply_claim_attempts(
    items: &mut [SteeringItem],
    claim_of: impl Fn(&str) -> Option<agent_doc_queue::queue_claim::QueueClaim>,
) {
    for item in items.iter_mut() {
        if item.source != SteeringSource::Queue {
            continue;
        }
        let claim = item
            .vector
            .as_deref()
            .and_then(&claim_of)
            .or_else(|| claim_of(&item.verbatim))
            .or_else(|| item.previous.as_deref().and_then(&claim_of));
        if let Some(claim) = claim {
            if item.current_item && !claim.owner.starts_with("subagent:") {
                continue;
            }
            item.dispatch = SteeringDispatch::ForwardToOwner;
            item.presets.clear();
            item.owner = Some(claim.owner);
            item.generation = Some(claim.generation);
            item.worker_id = claim.worker.map(|worker| worker.worker_id);
        }
    }
}

/// The receipt key of one degraded struck-row deletion report
/// (`#deletedstruckmismatch`): the work identity plus the exact owning
/// attempt. A later attempt on the same text (a new generation or owner) is a
/// new mismatch and reports again; the same attempt reports once.
pub fn struck_removal_report_key(
    identity: &agent_doc_element_queue::QueueItemIdentity,
    owner: Option<&str>,
    generation: Option<u64>,
) -> String {
    let identity = match identity {
        agent_doc_element_queue::QueueItemIdentity::Id(id) => format!("id:{id}"),
        agent_doc_element_queue::QueueItemIdentity::FreeText(text) => format!("ft:{text}"),
    };
    format!(
        "{identity}|{}|{}",
        owner.unwrap_or("-"),
        generation.map_or_else(|| "-".to_string(), |generation| generation.to_string())
    )
}

/// Add authoritative cancellation steering for claimed queue rows the
/// operator removed (`#queue-removal-cancels`).
///
/// Ordinary deletion of future queue work remains silent. A deletion becomes
/// harness work only when the durable claim ledger still names an active
/// attempt for the removed identity. Identity pairing is the same pairing used
/// for edits, and cancellation is withheld while the same identity remains
/// live (for example one of two identical rows was removed). A different row
/// added in the removed row's slot does not keep it alive (`#krwr`): the
/// alignment pairs only same-identity and similar rows, so a replacement is a
/// removal here and its owner is cancelled. Only an edit of the same row
/// steers the owner instead.
pub fn append_claimed_queue_removals(
    items: &mut Vec<SteeringItem>,
    watermark: &SteeringWatermark,
    current: &str,
    claim_of: impl Fn(&str) -> Option<agent_doc_queue::queue_claim::QueueClaim>,
) {
    let current_identities: Vec<_> = executable_queue_items(current)
        .into_iter()
        .map(|item| agent_doc_queue::queue_claim::claim_identity(&item.norm))
        .collect();
    let context_identities: std::collections::HashSet<_> =
        agent_doc_queue::queue_claim::ClaimQueueView::from_content(current)
            .into_iter()
            .flat_map(|view| view.queues)
            .flat_map(|queue| queue.context_claim_texts)
            .map(|text| agent_doc_queue::queue_claim::claim_identity(&text))
            .collect();
    let alignment = queue_alignment(
        watermark,
        current,
        &BTreeSet::new(),
        false,
        &agent_doc_frontmatter::frontmatter::PromptPresets::default(),
    );
    for event in alignment.events {
        let QueueEvent::Delete { old, .. } = event else {
            continue;
        };
        let identity = agent_doc_queue::queue_claim::claim_identity(&old);
        if current_identities
            .iter()
            .any(|current| current == &identity)
        {
            continue;
        }
        // The row still exists as structural context after gaining live
        // children. It ceased to be executable, but the operator did not
        // delete it, so cancelling its worker would be false removal
        // steering. The children are independently added above.
        if context_identities.contains(&identity) {
            continue;
        }
        let Some(claim) = claim_of(&old) else {
            continue;
        };
        if let Some(item) = items.iter_mut().find(|item| {
            item.source == SteeringSource::Queue
                && item.change == SteeringChange::Deleted
                && agent_doc_queue::queue_claim::claim_identity(&item.verbatim) == identity
        }) {
            item.dispatch = SteeringDispatch::CancelClaimed;
            item.presets.clear();
            item.owner = Some(claim.owner);
            item.generation = Some(claim.generation);
            item.worker_id = claim.worker.map(|worker| worker.worker_id);
            continue;
        }
        items.push(SteeringItem {
            source: SteeringSource::Queue,
            change: SteeringChange::Deleted,
            dispatch: SteeringDispatch::CancelClaimed,
            current_item: watermark.current_item.as_deref().is_some_and(|selected| {
                agent_doc_queue::queue_claim::claim_identity(selected) == identity
            }),
            verbatim: old,
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: Some(claim.owner),
            generation: Some(claim.generation),
            worker_id: claim.worker.map(|worker| worker.worker_id),
            explicit: false,
            vector: None,
            queue_mode: None,
        });
    }
}

/// Re-route queue items whose worker already reported (`#reportedrelease`).
///
/// `reported_owner` answers the released owner and its exact attempt
/// generation for a queue line that carries a live release receipt (the
/// caller's receipt ledger; a live claim already supersedes a receipt there).
/// The generation fences the strike the item's action names
/// (`#closeoutguidanceagree`). Such an item becomes
/// [`SteeringDispatch::PersistReported`]: its answer exists in the
/// coordinator's context, so the action is to persist it now, never to offer
/// it to a NEW subagent. Runs after [`apply_claim_owners`]; a claimed item
/// keeps its forward routing.
pub fn apply_release_receipts(
    items: &mut [SteeringItem],
    reported_owner: impl Fn(&str) -> Option<(String, u64)>,
) {
    for item in items.iter_mut() {
        if item.source != SteeringSource::Queue
            || item.current_item
            || item.change == SteeringChange::Deleted
            || item.dispatch == SteeringDispatch::ForwardToOwner
        {
            continue;
        }
        if let Some((owner, generation)) = reported_owner(&item.verbatim) {
            item.dispatch = SteeringDispatch::PersistReported;
            item.presets.clear();
            item.owner = Some(owner);
            item.generation = (generation > 0).then_some(generation);
        }
    }
}

/// Legacy compatibility transform for callers replaying the former global
/// Reported-writeback admission barrier. Current routing must keep a Reported
/// row excluded from redispatch without applying this to independent work.
pub fn hold_subagent_dispatch_for_reported(items: &mut [SteeringItem]) {
    for item in items.iter_mut() {
        if item.source == SteeringSource::Queue && item.dispatch == SteeringDispatch::Subagent {
            item.dispatch = SteeringDispatch::AwaitReportedCloseout;
        }
    }
}

/// The document-specific action for `item`: [`instruction_for`], except a
/// reported line names its exact closeout (`#reportedrelease`).
fn action_for(document: &str, item: &SteeringItem) -> String {
    match item.dispatch {
        SteeringDispatch::CancelClaimed => cancel_claimed_instruction(document, item),
        SteeringDispatch::DegradedClaimMismatch => {
            "the deleted row looked struck/terminal in Markdown, but durable claim, report, or completion-outbox evidence still owns that exact work. Treat this as a degraded state mismatch: preserve and fence the claim/outbox and recoverable artifacts; do NOT interrupt or release the worker, integrate/apply its result, or infer terminality from strikethrough. Report the mismatch to the operator and wait for an explicit authoritative resolution.".to_string()
        }
        SteeringDispatch::PersistReported => {
            agent_doc_queue::claim_release_receipt::persist_reported_instruction(
                document,
                &item.verbatim,
                item.owner.as_deref().unwrap_or("its worker"),
                item.generation,
            )
        }
        SteeringDispatch::ForwardToOwner if item.change != SteeringChange::Deleted => {
            forward_to_owner_instruction(document, item)
        }
        SteeringDispatch::QueueModeChanged => match &item.queue_mode {
            Some(change) => queue_mode_change_instruction(document, change),
            None => instruction_for(item).to_string(),
        },
        _ => instruction_for(item).to_string(),
    }
}

/// Render the worker handoff together with its generation-fenced unavailable-owner
/// recovery (`#staleownersteer`).
///
/// Agent-doc can name the bound harness worker but cannot observe that external
/// worker's lifecycle. The harness handoff result is therefore the boundary fact:
/// a successful handoff keeps the attempt, while an explicit unavailable/exited
/// result abandons exactly this generation and leaves the operator's edited row in
/// place for a fresh dispatch generation.
fn forward_to_owner_instruction(document: &str, item: &SteeringItem) -> String {
    let generation = item
        .generation
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "<generation-above>".to_string());
    let requeue = format!(
        "agent-doc queue release {} --item {} --generation {} --requeue",
        shell_single_quote(document),
        shell_single_quote(item.claim_text()),
        generation,
    );
    format!(
        "{} If the harness reports that the exact worker is unavailable or has exited, the \
         handoff did NOT happen: do not drop this operator answer/edit, claim it was forwarded, \
         or keep routing only to the unreachable owner. Preserve `verbatim` in the queue and run \
         `{requeue}`. That generation-fenced transition abandons only the stale attempt and makes \
         the edited row redispatchable; let the normal queue dispatcher start its replacement \
         under a fresh generation.",
        instruction_for(item),
    )
}

fn cancel_claimed_instruction(document: &str, item: &SteeringItem) -> String {
    let owner = item.owner.as_deref().unwrap_or("the claimed worker");
    let worker = item.worker_id.as_deref().unwrap_or(owner);
    let generation = item
        .generation
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "<generation-above>".to_string());
    let release = format!(
        "agent-doc queue release {} --item {} --generation {}",
        shell_single_quote(document),
        shell_single_quote(item.claim_text()),
        generation,
    );
    format!(
        "the operator explicitly REMOVED work claimed by `{owner}` (worker `{worker}`). This is \
         authoritative cancellation, never best-effort or sunk-cost continuation. If that exact \
         harness worker is still alive, interrupt it now at the nearest safe consistency boundary; best judgment applies \
         only to shutdown mechanics, never to whether work continues. Preserve recoverable \
         branch/worktree/commit artifacts, but stop further implementation, tests, integration, \
         publication, release, and external issue closure. The binary already tombstoned this exact \
         claim generation and retired any pending completion delivery so replay cannot resurrect it; \
         `{release}` is now only an idempotent generation-fenced cleanup if still needed. The binary \
         cannot directly kill a harness subagent. Report any irreversible \
         effect already crossed instead of trying to undo it. Do not close out or requeue the \
         removed line."
    )
}

/// Keep unsurfaced edits of CLAIMED queue items pending across a new cycle's
/// seed (`#claimedsteerwake`).
///
/// A fresh seed acknowledges the whole admitted queue, so an edit made while
/// the coordinator was idle (the edit that woke it) would be absorbed into the
/// new baseline and never reach the agent: the claimed head is not drainable,
/// so the woken cycle only saw an unchanged-looking queue. For each claimed
/// line of `seed.acknowledged_queue` that `prior_acknowledged` (what the agent
/// channel last surfaced) knew under different text, the seed keeps the
/// prior text, so the cycle's first hook (the preflight command's own
/// PostToolUse) surfaces it as `previous` -> `verbatim` for the owner.
pub fn carry_unsurfaced_claimed_edits(
    seed: &mut SteeringWatermark,
    prior_acknowledged: &[String],
    claimed: impl Fn(&str) -> bool,
) -> usize {
    let prior = SteeringWatermark {
        acknowledged_queue: prior_acknowledged.to_vec(),
        baseline: seed.baseline.clone(),
        ..SteeringWatermark::default()
    };
    let prompt_presets = agent_doc_frontmatter::frontmatter::parse(&seed.baseline)
        .map(|(fm, _)| fm.prompt_presets)
        .unwrap_or_default();
    let alignment = queue_alignment(
        &prior,
        &seed.baseline,
        &BTreeSet::new(),
        true,
        &prompt_presets,
    );
    let mut carried = 0;
    for event in &alignment.events {
        if let QueueEvent::Edit { old, new, .. } = event
            && claimed(new)
            && let Some(slot) = seed.acknowledged_queue.iter_mut().find(|norm| *norm == new)
        {
            *slot = old.clone();
            carried += 1;
        }
    }
    if carried > 0 {
        // The seed's own content gate must not report "unchanged".
        seed.last_observed_content_hash = None;
    }
    carried
}

/// Instruction text derived from the typed dispatch intent.
pub fn instruction_for(item: &SteeringItem) -> &'static str {
    if item.explicit
        && item.dispatch == SteeringDispatch::AddressNow
        && item.source == SteeringSource::Queue
        && !item.current_item
    {
        return "the operator explicitly sent this queue item with Run Agent Doc. Address it in \
                THIS turn together with the current work; do not defer it to a later queue cycle.";
    }
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
        (SteeringDispatch::Subagent, SteeringSource::Queue, _, _) => {
            "subagent intent: dispatch this item NOW to a NEW background subagent (one \
             subagent per item). If it touches a repository, give that subagent its own git \
             worktree outside the IDE-watched project; never run two subagents against one \
             checkout. Before dispatching, claim it with `agent-doc queue claim <FILE> --item \
             <id-or-line> --owner subagent:<label>` so the loop and Stop hook do not re-enter for \
             it; then paste the output of `agent-doc queue brief <FILE> --item <id-or-line>` \
             verbatim at the top of the subagent prompt (the operator's authorization for the \
             item, `#waypostauthorization`). When the subagent reports back, persist its result \
             in THIS turn, but first execute and verify every authorized current-item or \
             returned-subagent outward effect before closeout; \
             a prepared artifact does not defer a post/publish/push/message/API effect to \
             auto-clear (`#effectbeforecloseout`). Use the closeout that strikes the item \
             (`agent-doc finalize` with the \
             verbatim `> **Queue prompt:** <line>` echo, or `--done <id>`); that also ends the \
             claim. Keep working the current item yourself; record the item as dispatched in \
             your response."
        }
        (SteeringDispatch::Subagent, SteeringSource::Exchange, _, _) => {
            concat!(
                "subagent intent: dispatch this exchange prompt NOW to a NEW background subagent \
                 (one subagent per prompt). Exchange prompts have no queue claim/release lifecycle. \
                 If it touches a repository, give that subagent its own git worktree outside the \
                 IDE-watched project; never run two subagents against one checkout. Keep working the \
                 current item yourself; review the worker result and answer the exchange prompt in \
                 this response cycle, then perform the normal closeout. Put these rules in the \
                 subagent prompt: ",
                agent_doc_queue::subagent_scratch_isolation_rule!(),
                " ",
                agent_doc_queue::subagent_tmux_isolation_rule!()
            )
        }
        (SteeringDispatch::AwaitReportedCloseout, _, _, _) => {
            "subagent intent recorded behind Reported coordinator writeback. Do NOT dispatch or \
             drain it inline now; first integrate, verify, and persist every available reported \
             row. Fresh dispatch resumes from the queue after that closeout."
        }
        (SteeringDispatch::CancelClaimed, _, _, _) => {
            "the operator removed claimed work. Interrupt its exact worker attempt and retire the \
             generation-fenced claim; do not continue based on sunk cost."
        }
        (SteeringDispatch::DegradedClaimMismatch, _, _, _) => {
            "a struck-row deletion conflicts with durable work evidence. Preserve/fence it and report the mismatch; do not interrupt, release, or integrate automatically."
        }
        (SteeringDispatch::ForwardToOwner, _, SteeringChange::Deleted, _) => {
            "the operator removed claimed work; treat it as cancellation and do not continue it."
        }
        (SteeringDispatch::ForwardToOwner, _, SteeringChange::Added, _) => FORWARD_ADDED,
        (SteeringDispatch::ForwardToOwner, _, SteeringChange::Edited, _) => FORWARD_EDITED,
        (SteeringDispatch::QueueModeChanged, _, _, _) => {
            "the operator changed this queue's subagent mode: re-run `agent-doc plan` and follow \
             its `queue_subagent_dispatch` (on: claim, spawn, and start every unstarted head in \
             parallel; off: stop new dispatch, let running workers finish, drain the rest inline)."
        }
        (SteeringDispatch::PersistReported, _, _, _) => {
            "this queue item's worker already reported and its claim was released: persist that \
             result in THIS turn with the closeout that strikes the item (`agent-doc finalize` \
             with the verbatim `> **Queue prompt:** <line>` echo, or `--done <id>`). Do NOT \
             dispatch a new subagent for it and do NOT re-claim it."
        }
        (SteeringDispatch::DrainAfterCurrent, _, _, _) => {
            "queued in operator order: it runs AFTER the current item closes, through the normal \
             queue drain. Do NOT interrupt, interleave, or start it now; acknowledge it and \
             finish the current item first."
        }
    }
}

/// Header line every closeout (turn-boundary) steering payload starts with.
pub const CLOSEOUT_STEERING_MARKER: &str =
    "[agent-doc] operator steering arrived during this turn and was not yet surfaced";

/// The `--item` handle to claim a queue line. Only a structurally id-backed
/// head collapses to `#id`; a later `[#id]` mention remains verbatim (GH #182).
pub fn claim_item_handle(verbatim: &str) -> String {
    agent_doc_queue::queue_claim::claim_item_handle(verbatim)
}

/// Tracked ids a queue line references (`[#id]` / `do #id`).
pub fn referenced_queue_ids(text: &str) -> Vec<String> {
    agent_doc_queue::queue_claim::referenced_queue_ids(text)
}

fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The exact claim command for a subagent dispatch of `verbatim` in `document`.
pub fn claim_command_for(document: &str, verbatim: &str) -> String {
    format!(
        "agent-doc queue claim {} --item {} --owner subagent:<label>",
        shell_single_quote(document),
        shell_single_quote(&claim_item_handle(verbatim))
    )
}

/// The exact post-dispatch acknowledgement command. The coordinator fills in
/// the generation printed by [`claim_command_for`] and the identity returned
/// by the successful harness spawn.
pub fn start_command_for(document: &str, verbatim: &str) -> String {
    format!(
        "agent-doc queue start {} --item {} --owner subagent:<label> --generation <generation> --worker-id <worker-id>",
        shell_single_quote(document),
        shell_single_quote(&claim_item_handle(verbatim))
    )
}

/// The command that prints the operator-authorization preamble for a
/// subagent dispatch of `verbatim` in `document` (`#waypostauthorization`).
pub fn brief_command_for(document: &str, verbatim: &str) -> String {
    format!(
        "agent-doc queue brief {} --item {}",
        shell_single_quote(document),
        shell_single_quote(&claim_item_handle(verbatim))
    )
}

/// Render unsurfaced steering for the turn-boundary report (`respond` /
/// `write --commit` terminal output, post-commit `session-check`, or a
/// `steering` poll after close). `None` when nothing is ready.
///
/// Unlike [`render_steering_context`] the current item is already closed, so
/// queue work is framed for the NEXT cycle, and a `subagent` item is an
/// explicit dispatch-now directive. Queue-sourced items carry their claim
/// command; exchange-sourced prompts explicitly have no queue claim/release
/// lifecycle. A harness whose PostToolUse hook never ran still gets the
/// source-appropriate directive here, before the loop schedules its next
/// re-entry (`#closeout-steering`).
pub fn render_closeout_steering_context(
    document: &str,
    ready: &[SteeringItem],
    pending: usize,
) -> Option<String> {
    if ready.is_empty() {
        return None;
    }
    let mut out = format!(
        "{CLOSEOUT_STEERING_MARKER} ({} item(s)) in {document}. The operator edited the session \
         document while the turn ran and no mid-turn hook delivered it; every item below is \
         verbatim and must be handled per its `dispatch` BEFORE you end the turn or schedule the \
         next loop re-entry. Your response is already committed: do not re-run `respond`, do \
         not `--force-disk`.",
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
            item_header_suffix(item),
        ));
        if let Some(vector) = &item.vector {
            out.push_str(&format!("\nvector: {vector}"));
        }
        if let Some(previous) = &item.previous {
            out.push_str(&format!("\nprevious: {previous}"));
        }
        let label = if item.change == SteeringChange::Deleted {
            "removed"
        } else {
            "verbatim"
        };
        out.push_str(&format!("\n{label}: {}", item.verbatim));
        if item.possibly_partial {
            out.push_str(
                "\npossibly_partial: true (delivered by the max-hold, not because it settled; \
                 the operator may still be typing it. Act on what is unambiguous and re-read it \
                 with `agent-doc steering --peek` before relying on its ending.)",
            );
        }
        let action = match (item.dispatch, item.source) {
            (
                SteeringDispatch::ForwardToOwner
                | SteeringDispatch::PersistReported
                | SteeringDispatch::QueueModeChanged,
                _,
            ) => action_for(document, item),
            (SteeringDispatch::Subagent, SteeringSource::Queue) => format!(
                "DISPATCH NOW to a NEW background subagent (one per item). Claim it first with \
                 `{}` so the loop and Stop hook do not drain it inline, then dispatch. Only \
                 after the harness returns a worker identity, acknowledge that exact generation \
                 with `{}`; this changes 🟡 dispatching to 🚧 running and is not optional. If it \
                 touches a repository, give the subagent its own git worktree outside the \
                 IDE-watched project. Paste the output of `{}` verbatim at the top of the \
                 subagent prompt: it carries the operator's authorization for the item \
                 (`#waypostauthorization`). {}",
                claim_command_for(document, item.claim_text()),
                start_command_for(document, item.claim_text()),
                brief_command_for(document, item.claim_text()),
                REPORTED_WORKER_CLOSEOUT,
            ),
            (SteeringDispatch::Subagent, SteeringSource::Exchange) => {
                concat!(
                    "DISPATCH NOW to a NEW background subagent (one per prompt). Exchange prompts \
                     have no queue claim/release lifecycle. If it touches a repository, give the \
                     subagent its own git worktree outside the IDE-watched project. When the worker \
                     reports back, carry its result into the next response cycle; the current \
                     response is already committed. Put these rules in the subagent prompt: ",
                    agent_doc_queue::subagent_scratch_isolation_rule!(),
                    " ",
                    agent_doc_queue::subagent_tmux_isolation_rule!()
                )
                .to_string()
            }
            (SteeringDispatch::AwaitReportedCloseout, SteeringSource::Queue) => {
                "HOLD this subagent intent behind Reported coordinator writeback. Do not claim or \
                 dispatch it and do not drain it inline; integrate, verify, and persist every \
                 available reported row first. The normal queue dispatch resumes after that \
                 closeout."
                    .to_string()
            }
            (SteeringDispatch::AddressNow, SteeringSource::Exchange) => format!(
                "a new operator prompt: answer it in the next cycle (`agent-doc {document}`); do \
                 not re-answer prompts already committed."
            ),
            _ => "queued in operator order: the next cycle drains it through the normal queue; \
                  acknowledge it, nothing to start now."
                .to_string(),
        };
        out.push_str(&format!("\naction: {action}"));
    }
    if pending > 0 {
        out.push_str(&format!(
            "\n\nThe operator is still typing {pending} more item(s); `agent-doc steering \
             {document}` or the next preflight will surface them once settled."
        ));
    }
    Some(out)
}

/// Live executable heads of one queue block, in queue order. Group-parent
/// labels are excluded, matching `agent-doc queue claim`.
pub fn queue_component_live_heads(
    content: &str,
    component: &agent_doc_element::element::Component,
) -> Vec<String> {
    queue_component_items(content, component)
        .into_iter()
        .map(|item| item.norm)
        .collect()
}

fn queue_component_items(
    content: &str,
    component: &agent_doc_element::element::Component,
) -> Vec<QueueItem> {
    let Ok(entries) = agent_doc_queue::document_queue::parse(component.content(content)) else {
        return Vec::new();
    };
    let entries = agent_doc_queue::nested_claim_steer::expand_if_installed(entries);
    agent_doc_queue::document_queue::prompts(&entries)
        .into_iter()
        .filter_map(|prompt| {
            let raw =
                agent_doc_document::queue_projection::strip_in_progress_marker(prompt.text.trim());
            let norm = normalize_queue_text(&raw);
            (!norm.is_empty()).then_some(QueueItem { raw, norm })
        })
        .collect()
}

struct QueueDispatchRow {
    raw: String,
    norm: String,
    /// Claim key. A nested non-tracked leaf is its nest vector when that
    /// identity differs from the visible line (`#nestlistlineage`).
    head: String,
}

/// Executable rows of one queue block with the head `queue claim` resolves.
fn queue_component_dispatch_rows(
    content: &str,
    component: &agent_doc_element::element::Component,
) -> Vec<QueueDispatchRow> {
    let Ok(entries) = agent_doc_queue::document_queue::parse(component.content(content)) else {
        return Vec::new();
    };
    let entries = agent_doc_queue::nested_claim_steer::expand_if_installed(entries);
    agent_doc_queue::document_queue::executable_prompt_indexes(&entries)
        .into_iter()
        .filter_map(|index| {
            let agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) = &entries[index]
            else {
                return None;
            };
            let raw =
                agent_doc_document::queue_projection::strip_in_progress_marker(prompt.text.trim());
            let norm = normalize_queue_text(&raw);
            if norm.is_empty() {
                return None;
            }
            let lineage = agent_doc_queue::document_queue::lineage_claim_text(&entries, index)
                .unwrap_or_else(|| norm.clone());
            let head = if agent_doc_queue::queue_claim::claim_identity(&lineage)
                == agent_doc_queue::queue_claim::claim_identity(&norm)
            {
                norm.clone()
            } else {
                lineage
            };
            Some(QueueDispatchRow { raw, norm, head })
        })
        .collect()
}

/// The queue-level subagents attribute applied to one queue block:
/// `(mode, eligible heads, live heads)`, or `None` when that block's marker
/// has no valid `subagents` / `fan-out` attribute. Eligible heads are every
/// live line classified `subagent` against that block (no "new since seed"
/// filter), in queue order. A shared nested leaf is the nest vector, so two
/// `#rebase` rows stay two heads.
pub fn queue_component_subagent_heads(
    content: &str,
    component: &agent_doc_element::element::Component,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Option<(
    agent_doc_queue::subagent_intent::QueueSubagentsMode,
    Vec<String>,
    Vec<String>,
)> {
    agent_doc_queue::subagent_intent::queue_subagents_mode(&component.attrs)?;
    queue_component_subagent_heads_with_facts(
        content,
        component,
        prompt_presets,
        &QueueDispatchDocumentFacts::from_content(content),
    )
}

/// `#preflightqueuescale`: the document-wide inputs of
/// [`queue_component_subagent_heads`]. They depend only on the document text,
/// not on the queue block, so a caller planning every queue block of one
/// document derives them once instead of once per block (each is a full
/// document derivation).
#[derive(Debug, Clone)]
pub struct QueueDispatchDocumentFacts {
    awaiting_operator: std::collections::HashSet<String>,
    blockers: agent_doc_queue::queue_blocked::QueueBlockers,
    answer_rows: agent_doc_queue::queue_continuation::AnswerRowRouting,
}

thread_local! {
    static QUEUE_DISPATCH_FACT_DERIVATIONS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

impl QueueDispatchDocumentFacts {
    pub fn from_content(content: &str) -> Self {
        QUEUE_DISPATCH_FACT_DERIVATIONS.with(|count| count.set(count.get() + 1));
        Self {
            awaiting_operator: agent_doc_queue::queue_continuation::awaiting_operator_head_ids(
                content,
            ),
            // `#queueblocked`: a row whose tracked item waits on open blockers
            // is not dispatchable, so it never fills a subagent slot.
            blockers: agent_doc_queue::queue_blocked::queue_blockers(content),
            answer_rows: agent_doc_queue::queue_continuation::AnswerRowRouting::for_content(
                content,
            ),
        }
    }
}

impl QueueDispatchDocumentFacts {
    /// How many times this thread derived the facts. Bounded-work regression
    /// tests read the delta across one plan: it must not grow with the number
    /// of queue blocks.
    pub fn derivations() -> u64 {
        QUEUE_DISPATCH_FACT_DERIVATIONS.with(std::cell::Cell::get)
    }
}

/// [`queue_component_subagent_heads`] with document facts the caller already
/// derived from the same `content`.
pub fn queue_component_subagent_heads_with_facts(
    content: &str,
    component: &agent_doc_element::element::Component,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
    facts: &QueueDispatchDocumentFacts,
) -> Option<(
    agent_doc_queue::subagent_intent::QueueSubagentsMode,
    Vec<String>,
    Vec<String>,
)> {
    let mode = agent_doc_queue::subagent_intent::queue_subagents_mode(&component.attrs)?;
    let presets = PresetContext::for_queue_component(content, component, &[], prompt_presets);
    let items = queue_component_dispatch_rows(content, component);
    let eligible = items
        .iter()
        .filter(|item| {
            presets.classify(&item.raw, SteeringSource::Queue).0 == SteeringDispatch::Subagent
        })
        .filter(|item| !head_awaits_operator(&item.norm, &facts.awaiting_operator))
        .filter(|item| {
            // `🙋` plus a trailing `[#id]` is operator-gated, not a worker head.
            agent_doc_document::queue_projection::operator_action_ids(&item.raw).is_empty()
        })
        .filter(|item| !facts.blockers.blocks_row(&item.norm))
        // `#queueoperatoranswer`: an operator answer row goes to the worker that asked.
        .filter(|item| !facts.answer_rows.holds_from_dispatch(&item.raw))
        .map(|item| item.head.clone())
        .collect();
    let live = items.into_iter().map(|item| item.head).collect();
    Some((mode, eligible, live))
}

/// `#queueskipsubagents`: lowercased `#id`s of every live queue row, in any
/// queue or run block, that is routed to subagent dispatch rather than to the
/// inline turn: its block runs in subagents (`subagents` / `fan-out`, or a
/// queue-scoped preset asking for it) and the row does not opt out, or the row
/// itself carries subagent intent (`#subagents`).
///
/// Such a row is never run by an inline chat turn, so an inline turn that left
/// it unconsumed is not evidence that it was dispatched and "came back"
/// (`#queueskip`). Unlike [`queue_component_subagent_heads`], no
/// awaiting-operator, blocked or answer-row filter applies: the question here is
/// only *who* runs the row, not whether it is dispatchable now.
pub fn subagent_routed_queue_head_ids(
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> std::collections::HashSet<String> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return std::collections::HashSet::new();
    };
    let Ok(queues) = agent_doc_queue::queue_set::QueueSet::parse(content, &components) else {
        return std::collections::HashSet::new();
    };
    queues
        .blocks()
        .iter()
        .flat_map(|block| {
            let presets =
                PresetContext::for_queue_component(content, block.component, &[], prompt_presets);
            queue_component_dispatch_rows(content, block.component)
                .into_iter()
                .filter(move |item| {
                    presets.classify(&item.raw, SteeringSource::Queue).0
                        == SteeringDispatch::Subagent
                })
                .filter_map(|item| {
                    agent_doc_queue::queue_continuation::extract_head_id(&item.norm)
                        .map(|id| id.to_ascii_lowercase())
                })
        })
        .collect()
}

/// The queue-level subagents attribute applied to the claim-blind selected
/// queue: `(mode, eligible heads, live heads)`, or `None` when that queue's
/// marker has no valid `subagents` / `fan-out` attribute.
pub fn queue_attr_subagent_heads(
    content: &str,
) -> Option<(
    agent_doc_queue::subagent_intent::QueueSubagentsMode,
    Vec<String>,
    Vec<String>,
)> {
    let prompt_presets = agent_doc_frontmatter::frontmatter::parse(content)
        .map(|(fm, _)| fm.prompt_presets)
        .unwrap_or_default();
    queue_attr_subagent_heads_with_presets(content, &prompt_presets)
}

pub fn queue_attr_subagent_heads_with_presets(
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Option<(
    agent_doc_queue::subagent_intent::QueueSubagentsMode,
    Vec<String>,
    Vec<String>,
)> {
    queue_attr_subagent_heads_for_owner(
        content,
        prompt_presets,
        &agent_doc_queue::queue_claim::ClaimedQueueItems::none(),
    )
}

/// [`queue_attr_subagent_heads_with_presets`] for the queue `claimed` leaves
/// runnable. No `selected()` fallback: an earlier `subagents` queue whose
/// heads are all claimed or operator-gated must not return an empty plan and
/// hide a later queue.
pub fn queue_attr_subagent_heads_for_owner(
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
    claimed: &agent_doc_queue::queue_claim::ClaimedQueueItems,
) -> Option<(
    agent_doc_queue::subagent_intent::QueueSubagentsMode,
    Vec<String>,
    Vec<String>,
)> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let queues = agent_doc_queue::queue_set::QueueSet::parse(content, &components).ok()?;
    // No `selected()` fallback: an earlier `subagents` queue whose heads are
    // all claimed or operator-gated must not return an empty plan and hide a
    // later queue. Callers that pass an empty claim set still see `selected()`.
    let block = queues.selected_excluding_claimed(claimed)?;
    queue_component_subagent_heads(content, block.component, prompt_presets)
}

/// Live queue heads with item- or queue-scoped subagent intent that are NEW
/// dispatch work relative to `reference_queue` (normalized queue texts as of
/// the previous cycle's seed). `None` means no reference exists, so every
/// subagent-intent head counts as new. Returns the raw (marker-stripped) line
/// texts in queue order. Claims are the caller's concern.
///
/// "New dispatch work" is judged on the shared work identity
/// ([`agent_doc_element_queue::queue_head_identity`], `#claimdispatchidentity`):
/// a head is new when its identity was absent from the reference, or present
/// only WITHOUT subagent intent (the operator escalated `do [#a]` to
/// `#subagents do [#a]`). A tag-only edit of a line that already carried
/// subagent intent (`#subagents: <url>` -> `#subagents: #gh-fix <url>`) is the
/// same dispatch, not a new one.
pub fn subagent_dispatch_heads(content: &str, reference_queue: Option<&[String]>) -> Vec<String> {
    let prompt_presets = agent_doc_frontmatter::frontmatter::parse(content)
        .map(|(fm, _)| fm.prompt_presets)
        .unwrap_or_default();
    subagent_dispatch_heads_with_presets(content, reference_queue, &prompt_presets)
}

pub fn subagent_dispatch_heads_with_presets(
    content: &str,
    reference_queue: Option<&[String]>,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Vec<String> {
    let presets = PresetContext::new_with_presets(content, &[], prompt_presets);
    let reference: Option<(BTreeSet<&str>, BTreeMap<String, bool>)> =
        reference_queue.map(|queue| {
            let mut identities: BTreeMap<String, bool> = BTreeMap::new();
            for norm in queue {
                let subagent =
                    presets.classify(norm, SteeringSource::Queue).0 == SteeringDispatch::Subagent;
                *identities.entry(queue_identity(norm)).or_default() |= subagent;
            }
            (queue.iter().map(String::as_str).collect(), identities)
        });
    let awaiting_operator =
        agent_doc_queue::queue_continuation::awaiting_operator_head_ids(content);
    let blockers = agent_doc_queue::queue_blocked::queue_blockers(content);
    let answer_rows = agent_doc_queue::queue_continuation::AnswerRowRouting::for_content(content);
    executable_queue_items(content)
        .into_iter()
        .filter(|item| {
            presets.classify(&item.raw, SteeringSource::Queue).0 == SteeringDispatch::Subagent
        })
        .filter(|item| !head_awaits_operator(&item.norm, &awaiting_operator))
        .filter(|item| !blockers.blocks_row(&item.norm))
        .filter(|item| !answer_rows.holds_from_dispatch(&item.raw))
        .filter(|item| {
            reference.as_ref().is_none_or(|(norms, identities)| {
                !norms.contains(item.norm.as_str())
                    && identities.get(&queue_identity(&item.norm)) != Some(&true)
            })
        })
        .map(|item| item.norm)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_expansion_is_a_new_descendant_not_a_parent_edit() {
        let parent = "Sync PR and Jira state into this document.";
        let leaf = "First steer child stays with the parent.";
        let descendant = QueueItem {
            raw: leaf.to_string(),
            norm: format!("{parent}: {leaf}"),
        };
        assert!(is_lineage_descendant(parent, &descendant));

        let edited = QueueItem {
            raw: format!("{parent}: now include Jira comments"),
            norm: format!("{parent}: now include Jira comments"),
        };
        assert!(!is_lineage_descendant(parent, &edited));

        let grandparent = "Release: Validate";
        let grandchild = QueueItem {
            raw: "Publish".to_string(),
            norm: "Release: Validate: Publish".to_string(),
        };
        assert!(is_lineage_descendant(grandparent, &grandchild));
    }

    #[test]
    fn route_sync_steering_carries_the_diff_since_the_last_send() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1 @@\n-old\n+new queue head\n";
        let text = render_route_sync_steering("tasks/bugs.md", Some(diff), &[]).unwrap();
        assert!(text.contains(STEERING_MARKER), "{text}");
        assert!(text.contains("diff since the last send"), "{text}");
        assert!(text.contains("+new queue head"), "{text}");
        assert!(text.contains("do not start a second turn"), "{text}");
        assert!(render_route_sync_steering("tasks/bugs.md", None, &[]).is_none());
        assert!(render_route_sync_steering("tasks/bugs.md", Some("  \n"), &[]).is_none());
    }

    /// `#steergatenamo`: the decision log keeps the END of the item, where
    /// end-of-turn evidence is, bounded and cut on a char boundary.
    #[test]
    fn gate_text_tail_keeps_the_bounded_end_of_the_item() {
        assert_eq!(gate_text_tail("fix the bug.  \n"), "fix the bug.");
        let long = format!("{}é tail end", "x".repeat(GATE_TEXT_TAIL_CHARS * 2));
        let tail = gate_text_tail(&long);
        assert_eq!(tail.chars().count(), GATE_TEXT_TAIL_CHARS);
        assert!(tail.ends_with("é tail end"));
    }

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
            explicit_send: false,
            max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
            classifier: &agent_doc_debounce::edit_settle::DeterministicOnly,
            median_pause_ms: None,
        }
    }

    fn seeded(baseline: &str, current_item: Option<&str>) -> SteeringWatermark {
        SteeringWatermark::seed("cycle-1", baseline, current_item, Vec::new())
    }

    const EX: &str =
        "Earlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n\nwork on the current item\n";

    /// `#queuemodesteer` (operator, 2026-11-04): "If the queue starts without
    /// `subagents`, then the operator adds `subagents` mid-turn, agent-doc
    /// should move the work to subagents." The marker edit is one typed
    /// steering item, surfaced once; the reverse turns dispatch back off.
    #[test]
    fn adding_and_removing_subagents_on_a_queue_marker_is_one_mode_change_item() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n- next task\n", EX);
        let on = baseline.replace("<!-- agent:queue -->", "<!-- agent:queue subagents -->");
        let wm = seeded(&baseline, Some("current task"));

        let obs = observe(&wm, &on, &quiet_ctx(&owned));

        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.dispatch, SteeringDispatch::QueueModeChanged);
        assert_eq!(item.verbatim, "<!-- agent:queue subagents -->");
        assert_eq!(item.previous.as_deref(), Some("<!-- agent:queue -->"));
        let change = item.queue_mode.as_ref().unwrap();
        assert!(change.subagents);
        let rendered = render_steering_context("plan.md", &obs.ready, 0).unwrap();
        assert!(
            rendered.contains(&format!(
                "dispatch=queue_mode_changed source=queue change=edited queue={} subagents=on",
                change.queue
            )),
            "{rendered}"
        );
        assert!(rendered.contains("agent-doc plan plan.md"), "{rendered}");
        assert!(rendered.contains("in parallel now"), "{rendered}");
        assert!(rendered.contains("no outward effect"), "{rendered}");
        assert!(rendered.contains("Never run one head both"), "{rendered}");

        // Surfaced once: the same document observes nothing new.
        let again = observe(&obs.next, &on, &quiet_ctx(&owned));
        assert!(again.ready.is_empty(), "{:?}", again.ready);

        // The reverse direction.
        let off = observe(&again.next, &baseline, &quiet_ctx(&owned));
        assert_eq!(off.ready.len(), 1, "{:?}", off.ready);
        let item = &off.ready[0];
        assert_eq!(item.dispatch, SteeringDispatch::QueueModeChanged);
        assert!(!item.queue_mode.as_ref().unwrap().subagents);
        let rendered = render_closeout_steering_context("plan.md", &off.ready, 0).unwrap();
        assert!(rendered.contains("subagents=off"), "{rendered}");
        assert!(rendered.contains("Stop claiming or spawning new heads"), "{rendered}");
        assert!(rendered.contains("continue to completion"), "{rendered}");
        assert!(rendered.contains("drain inline"), "{rendered}");
    }

    /// `#queuemodesteer`: a queue preset that resolves to subagents is the
    /// same mode change as the bare attribute.
    #[test]
    fn a_subagent_queue_preset_added_mid_turn_is_a_mode_change() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let current = baseline.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        let obs = observe(&seeded(&baseline, Some("current task")), &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::QueueModeChanged);
        assert!(obs.ready[0].queue_mode.as_ref().unwrap().subagents);
    }

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

    /// `#runpresetunresolved`: an inline `preset="#name: body"` on the run
    /// marker is one queue intent carrying the operator's body, not one empty
    /// intent per word token.
    #[test]
    fn inline_run_preset_definition_is_one_resolved_queue_intent() {
        let current = concat!(
            "<!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push: ",
            "approve all commit + integration + push: approve all worktree removals\" go -->\n",
            "- do it\n<!-- /agent:run -->\n",
        );
        let presets = PresetContext::new_with_presets(current, &[], &Default::default());
        assert_eq!(presets.queue.len(), 1, "{:?}", presets.queue);
        assert_eq!(
            presets.queue[0].name,
            "#spec-test-build-install-commit-push"
        );
        assert_eq!(
            presets.queue[0].body,
            "approve all commit + integration + push: approve all worktree removals"
        );
        assert_eq!(presets.queue[0].scope, PresetScope::Queue);
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

    /// `#preflightsteeradmit` SimWorld: the operator types into the queue as a
    /// turn starts (tasks/sdk.md, 2026-10-03). The relay hub already holds the
    /// edit in its canonical text while a second replica has not acknowledged it,
    /// so delivery is pending. Admission must proceed on that canonical text, and
    /// the operator's NEXT queue and exchange edits — landing after the seeded
    /// baseline — must reach the running turn as mid-turn steering.
    #[test]
    fn operator_edit_in_flight_at_turn_start_is_admitted_and_later_edits_steer() {
        use crate::crdt_relay::RelayHub;
        use agent_doc_debounce::{CurrentAuthorityAdmission, current_authority_admission};
        use agent_doc_merge::crdt_sync::ReplicaState;

        fn drain(hub: &mut RelayHub, client: u64) {
            for update in hub.pending_updates(client).unwrap() {
                hub.ack_delivery(client, &update.patch_id, update.generation)
                    .unwrap();
            }
        }
        fn operator_insert(hub: &mut RelayHub, editor: &ReplicaState, after: &str, text: &str) {
            let current = editor.text();
            let offset = current.find(after).expect("anchor") + after.len();
            editor.apply_local_edit(offset as u32, 0, text);
            let update = editor.diff(&ReplicaState::new(99).state_vector()).unwrap();
            hub.relay_update(2, &update).unwrap();
        }

        let owned = BTreeSet::new();
        let committed = doc("- current task\n", EX);
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap(); // the operator's editor
        hub.register(3).unwrap(); // a second live replica
        hub.apply_canonical_replace("", &committed).unwrap();
        drain(&mut hub, 2);
        drain(&mut hub, 3);
        assert!(hub.delivery_converged());
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        assert_eq!(editor.text(), committed);

        // The operator's edit is in the canonical authority, but replica 3 has
        // not acknowledged it: exactly the `delivery_pending` refusal shape.
        operator_insert(&mut hub, &editor, "- current task\n", "- in-flight item\n");
        assert!(
            !hub.delivery_converged(),
            "the fixture must hold delivery pending"
        );
        let admitted = hub.canonical_text();
        assert!(admitted.contains("- in-flight item\n"));
        assert_eq!(
            current_authority_admission(true, hub.delivery_converged()),
            CurrentAuthorityAdmission::AdmitWhileDeliveryPending,
            "an operator edit still being delivered must not refuse the turn",
        );
        assert!(current_authority_admission(true, hub.delivery_converged()).admits());

        // Preflight seeds the steering watermark from the admitted authority:
        // the in-flight edit is turn input, not steering.
        let watermark = seeded(&admitted, Some("current task"));
        let unchanged = observe(&watermark, &admitted, &quiet_ctx(&owned));
        assert!(unchanged.ready.is_empty(), "{:?}", unchanged.ready);

        // Edits that land after the baseline flow through mid-turn steering.
        operator_insert(
            &mut hub,
            &editor,
            "- in-flight item\n",
            "- also update the README table\n",
        );
        operator_insert(
            &mut hub,
            &editor,
            "work on the current item\n",
            "\nalso check the CI status please\n",
        );
        let later = hub.canonical_text();
        let steering = observe(&watermark, &later, &quiet_ctx(&owned));
        let queue = steering
            .ready
            .iter()
            .find(|item| item.source == SteeringSource::Queue)
            .unwrap_or_else(|| panic!("queue steering missing: {:?}", steering.ready));
        assert_eq!(queue.verbatim, "also update the README table");
        assert_eq!(queue.dispatch, SteeringDispatch::DrainAfterCurrent);
        let exchange = steering
            .ready
            .iter()
            .find(|item| item.source == SteeringSource::Exchange)
            .unwrap_or_else(|| panic!("exchange steering missing: {:?}", steering.ready));
        assert!(
            exchange
                .verbatim
                .contains("also check the CI status please")
        );
        assert_eq!(exchange.dispatch, SteeringDispatch::AddressNow);

        // A state with no authoritative text still waits.
        assert_eq!(
            current_authority_admission(false, false),
            CurrentAuthorityAdmission::WaitForAuthority
        );
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
    fn recovered_response_prose_is_not_redispatched_as_exchange_prompts() {
        let owned = BTreeSet::new();
        let marker = "<!-- agent:exchange subagents -->";
        let resolved_exchange = "Earlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n";
        let baseline = doc("", resolved_exchange).replace("<!-- agent:exchange -->", marker);
        let response = concat!(
            "\n### Re: recovered response\n\n",
            "Implemented, integrated, pushed, and installed.\n\n",
            "Focused tests pass. The full suite encountered one unrelated, pre-existing nested-prompt test failure, reproduced on the baseline commit.\n\n",
            "The document response was committed. Its final session check found a binary closeout bookkeeping mismatch and explicitly directed the final-message hook to recover it; I did not duplicate the response or modify the document manually.\n",
        );
        let current = doc("", &format!("{resolved_exchange}{response}"))
            .replace("<!-- agent:exchange -->", marker);

        let obs = observe_with_mode(
            &seeded(&baseline, None),
            &current,
            &quiet_ctx(&owned),
            ObserveMode::Boundary,
        );
        assert!(
            obs.ready.is_empty() && obs.pending == 0,
            "recovered assistant prose was redispatched: {:#?}",
            obs.ready
        );
        assert!(
            !crate::baseline_comparison::realtime_steering_all_between(&baseline, &current)
                .is_present(),
            "closed-cycle steering redispatched recovered assistant prose"
        );
    }

    #[test]
    fn explicit_operator_prompt_after_recovered_response_still_dispatches() {
        let resolved_exchange = "Earlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n";
        let baseline = doc("", resolved_exchange);
        let current = doc(
            "",
            &format!(
                "{resolved_exchange}\n### Re: recovered response\n\nImplemented, integrated, pushed, and installed.\n\n❯ Audit the remaining install state.\n"
            ),
        );

        let steering =
            crate::baseline_comparison::realtime_steering_all_between(&baseline, &current);
        assert_eq!(steering.len(), 1, "{steering:#?}");
        assert!(
            steering.directives()[0]
                .verbatim()
                .is_some_and(|text| text.contains("Audit the remaining install state."))
        );
    }

    #[test]
    fn exchange_component_attrs_dispatch_subagent_with_scoped_preset() {
        let owned = BTreeSet::new();
        let baseline = doc("", EX).replace(
            "<!-- agent:exchange -->",
            "<!-- agent:exchange subagents preset=\"#gh-fix\" -->",
        );
        let current = doc("", &format!("{EX}\nfix the newly reported issue\n")).replace(
            "<!-- agent:exchange -->",
            "<!-- agent:exchange subagents preset=\"#gh-fix\" -->",
        );
        let obs = observe(&seeded(&baseline, None), &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.source, SteeringSource::Exchange);
        assert_eq!(item.dispatch, SteeringDispatch::Subagent);
        assert!(item.presets.iter().any(|preset| {
            preset.name == "#gh-fix"
                && preset.body == "fix the github issue"
                && preset.scope == PresetScope::Exchange
        }));
    }

    #[test]
    fn exchange_component_attr_applies_multiple_presets_once_in_source_order() {
        let owned = BTreeSet::new();
        let marker = "<!-- agent:exchange subagents preset=\"#gh-fix subagents #gh-fix\" -->";
        let baseline = doc("", EX).replace("<!-- agent:exchange -->", marker);
        let current = doc("", &format!("{EX}\nfix the newly reported issue\n"))
            .replace("<!-- agent:exchange -->", marker);
        let obs = observe(&seeded(&baseline, None), &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(
            obs.ready[0]
                .presets
                .iter()
                .map(|preset| preset.name.as_str())
                .collect::<Vec<_>>(),
            vec!["#gh-fix", "#subagents"]
        );
    }

    /// `#steerbacklogsource` (agent-doc-bugs.md, 2026-10-04): the operator
    /// rewrote the backlog stub `- [ ] [#gvqv] Add a ``` into the full
    /// Dashboard request while a turn ran, and every steering report surfaced it
    /// as `source=exchange change=added dispatch=address_now`. Edits inside
    /// managed tracked-work components are component state, never exchange
    /// prompts — including free text that is not item-shaped — while a real
    /// exchange prompt typed in the same edit still surfaces.
    #[test]
    fn managed_component_edits_never_surface_as_exchange_prompts() {
        fn tracked(exchange: &str, backlog: &str, review: &str, done: &str) -> String {
            format!(
                "{FM}# Session\n\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n## Backlog\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n\n## Review\n\n<!-- agent:review -->\n{review}<!-- /agent:review -->\n\n## Done\n\n<!-- agent:done -->\n{done}<!-- /agent:done -->\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
            )
        }
        let owned = BTreeSet::new();
        let baseline = tracked(
            EX,
            "- [ ] [#gvqv] Add a ``\n- [ ] [#other] keep me\n",
            "",
            "",
        );
        let backlog_only = tracked(
            EX,
            "- [ ] [#gvqv] Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard. Can we also support a dashboard md file that live updates?\n- [ ] [#other] keep me\n",
            "Reviewer note: the dashboard needs a refresh button\n",
            "- 2026-10-04 [#shipped] closed last cycle\n",
        );
        let obs = observe(&seeded(&baseline, None), &backlog_only, &quiet_ctx(&owned));
        assert!(
            obs.ready.is_empty() && obs.pending == 0,
            "managed-component edits surfaced as steering: {:?}",
            obs.ready
        );
        // The shared closed-cycle comparison agrees: no steering at all.
        assert!(
            !crate::baseline_comparison::realtime_steering_all_between(&baseline, &backlog_only)
                .is_present()
        );

        // A real exchange prompt in the same edit is still surfaced, alone.
        let with_prompt = backlog_only.replacen(
            "work on the current item\n",
            "work on the current item\n\nalso check the CI status please\n",
            1,
        );
        let obs = observe(&seeded(&baseline, None), &with_prompt, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].source, SteeringSource::Exchange);
        assert_eq!(obs.ready[0].dispatch, SteeringDispatch::AddressNow);
        assert_eq!(obs.ready[0].verbatim, "also check the CI status please");
    }

    /// Closeout response prose can document component markup in a fenced code
    /// block.  A nested-looking close marker is response content, not the end
    /// of the exchange.  Stopping there leaves the closing fence and following
    /// queue in the exchange diff, which previously resurfaced the entire
    /// rendered queue snapshot after every lifecycle-marker projection.
    #[test]
    fn closeout_response_component_example_does_not_project_queue_as_exchange() {
        fn fixture(queue_marker: &str, response: &str) -> String {
            format!(
                concat!(
                    "{}# Session\n\n",
                    "<!-- agent:exchange -->\n",
                    "Earlier exchange history.\n",
                    "{}",
                    "<!-- agent:boundary:stable -->\n",
                    "<!-- /agent:exchange -->\n\n",
                    "## Queue: Main\n",
                    "<!-- agent:queue id=\"main\" -->\n",
                    "- do [#post535release]\n",
                    "---\n",
                    "{} sdk-demo.md queue items were duplicated\n",
                    "```\n",
                    "- first duplicate\n",
                    "- second duplicate\n",
                    "```\n",
                    "---\n",
                    "- ~~completed work~~\n",
                    "<!-- /agent:queue -->\n"
                ),
                FM, response, queue_marker,
            )
        }

        let baseline = fixture("⚠️", "");
        let response = concat!(
            "\n### Re: completed queue work\n\n",
            "Use this managed hook:\n\n",
            "```markdown\n",
            "<!-- agent:status patch=replace -->\n",
            "managed status\n",
            "<!-- /agent:status -->\n",
            "```\n"
        );
        let owned = BTreeSet::new();
        for marker in ["🟡", "🚧"] {
            let current = fixture(marker, response);
            let obs = observe_with_mode(
                &seeded(&baseline, None),
                &current,
                &quiet_ctx(&owned),
                ObserveMode::Boundary,
            );
            assert!(
                obs.ready
                    .iter()
                    .all(|item| item.source != SteeringSource::Exchange),
                "queue projection surfaced as exchange steering for {marker}: {:#?}",
                obs.ready
            );
        }
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

    /// The live mirror shape (tasks/agent-doc/agent-doc-bugs.md, 2026-11-04):
    /// a backlog→queue mirror writes `- ### 📥 do [#id]` heading rows, not
    /// bare `- do [#id]`. A row the binary mirrored this cycle is bookkeeping
    /// in that shape too; an operator-added row in the same shape is not.
    #[test]
    fn binary_mirrored_heading_rows_do_not_surface() {
        let owned = BTreeSet::from(["mirrored1".to_string(), "mirrored2".to_string()]);
        let baseline = doc("- ### 🚧 do [#current]\n", EX);
        let current = doc(
            "- ### 🚧 do [#current]\n- ### 📥 do [#mirrored1]\n- ### 📥 do [#mirrored2]\n",
            EX,
        );
        for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
            let obs = observe_with_mode(
                &seeded(&baseline, Some("do [#current]")),
                &current,
                &quiet_ctx(&owned),
                mode,
            );
            assert!(obs.ready.is_empty(), "{mode:?}: {:?}", obs.ready);
        }
        let operator = doc(
            "- ### 🚧 do [#current]\n- ### 📥 do [#mirrored1]\n- ### do [#operatoradded]\n",
            EX,
        );
        let obs = observe_with_mode(
            &seeded(&baseline, Some("do [#current]")),
            &operator,
            &quiet_ctx(&owned),
            ObserveMode::Boundary,
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert!(
            obs.ready[0].verbatim.contains("operatoradded"),
            "{:?}",
            obs.ready
        );
    }

    /// `#mirrorbinaryownedids` (2026-11-04, tasks/agent-doc/agent-doc-bugs.md):
    /// the hook delivered `source=queue change=added owner=subagent:… verbatim:
    /// 🚧 [#subagent:summarypick#1] do [#responsesummarypick]` and later the
    /// same row as `☑️ … ⟦local⟧`. The claim/report marker transitions are NOT
    /// the cause: once a row is acknowledged, every binary lifecycle projection
    /// of it (📥→🟡→🚧→☑️, the `[#owner#N]` prefix, the `⟦local⟧` suffix, the
    /// strike) is bookkeeping. The cause was the insertion itself: a mirrored
    /// row the watermark never acknowledged, carrying whatever marker it had
    /// reached by the time steering looked. Recording the mirrored id silences
    /// that insertion in every marker state, and an operator's real edit of
    /// the same row still surfaces.
    #[test]
    fn mirrored_row_marker_transitions_are_bookkeeping_and_operator_edits_surface() {
        let fm = "---\n---\n";
        let doc = |row: &str| {
            format!(
                "{fm}# S\n\n<!-- agent:queue -->\n- ### 🚧 do [#cur]\n{row}<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n{EX}<!-- /agent:exchange -->\n"
            )
        };
        let lifecycle = [
            "- ### 📥 do [#mirrored]\n",
            "- ### 🟡 [#subagent:pick#1] do [#mirrored]\n",
            "- ### 🚧 [#subagent:pick#1] do [#mirrored]\n",
            "- ### ☑️ [#subagent:pick#1] do [#mirrored] ⟦local⟧\n",
            "- ### ☑️ [#subagent:pick#1] do [#mirrored]\n",
            "- ### ~~do [#mirrored] ⟦local⟧~~\n",
        ];
        let none = BTreeSet::new();
        for pair in lifecycle.windows(2) {
            let obs = observe(
                &seeded(&doc(pair[0]), Some("do [#cur]")),
                &doc(pair[1]),
                &quiet_ctx(&none),
            );
            assert!(
                obs.ready.is_empty(),
                "{:?} -> {:?}: {:?}",
                pair[0],
                pair[1],
                obs.ready
            );
        }

        // The live shape: the mirrored row is new to the watermark and has
        // already been claimed (or reported) when steering first sees it.
        let baseline = doc("");
        let owned = BTreeSet::from(["mirrored".to_string()]);
        for row in &lifecycle[..5] {
            let unrecorded = observe(
                &seeded(&baseline, Some("do [#cur]")),
                &doc(row),
                &quiet_ctx(&none),
            );
            assert_eq!(unrecorded.ready.len(), 1, "{row:?}: {:?}", unrecorded.ready);
            assert_eq!(unrecorded.ready[0].change, SteeringChange::Added);
            for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
                let recorded = observe_with_mode(
                    &seeded(&baseline, Some("do [#cur]")),
                    &doc(row),
                    &quiet_ctx(&owned),
                    mode,
                );
                assert!(
                    recorded.ready.is_empty(),
                    "{row:?} {mode:?}: {:?}",
                    recorded.ready
                );
            }
        }

        // An operator's real edit of the same (binary-owned) row surfaces.
        let acknowledged = doc("- ### 🚧 [#subagent:pick#1] do [#mirrored]\n");
        let edited =
            doc("- ### 🚧 [#subagent:pick#1] do [#mirrored] and also cover the reply marker\n");
        for owned in [&none, &owned] {
            let obs = observe(
                &seeded(&acknowledged, Some("do [#cur]")),
                &edited,
                &quiet_ctx(owned),
            );
            assert_eq!(obs.ready.len(), 1, "{owned:?}: {:?}", obs.ready);
            assert!(
                obs.ready[0]
                    .verbatim
                    .contains("also cover the reply marker"),
                "{:?}",
                obs.ready
            );
        }
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
            explicit_send: false,
            max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
            classifier: &agent_doc_debounce::edit_settle::DeterministicOnly,
            median_pause_ms: None,
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
        // Quiet well past the debounce window, but inside the max-hold.
        let quiet = ObserveContext {
            now_ms: 30_000,
            ..quiet_ctx(&owned)
        };
        let obs = observe(&seeded(&baseline, Some("current task")), &current, &quiet);
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(obs.pending, 1);
        assert!(obs.recheck_after_ms.is_some());
    }

    /// One delivery observed while replaying a recorded edit stream.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Delivered {
        at_ms: u64,
        verbatim: String,
        possibly_partial: bool,
    }

    /// Replay a recorded edit stream (`(at_ms, queue lines)`) against a fake
    /// clock that ticks every 250ms, observing at every tick the way the
    /// hook does after each tool call.
    fn replay(
        stream: &[(u64, &str)],
        until_ms: u64,
        classifier: &dyn agent_doc_debounce::edit_settle::CompletionClassifier,
    ) -> Vec<Delivered> {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let mut wm = seeded(&baseline, Some("current task"));
        let mut delivered = Vec::new();
        let mut now = 0;
        while now <= until_ms {
            let latest = stream.iter().rev().find(|(at, _)| *at <= now);
            let (changed, current) = match latest {
                Some((at, lines)) => (*at, doc(&format!("- current task\n{lines}"), EX)),
                None => (0, baseline.clone()),
            };
            let ctx = ObserveContext {
                now_ms: now,
                document_changed_ms: Some(changed),
                debounce_ms: 2_000,
                binary_owned_queue_ids: &owned,
                explicit_send: false,
                max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
                classifier,
                median_pause_ms: Some(900),
            };
            let obs = observe(&wm, &current, &ctx);
            delivered.extend(obs.ready.iter().map(|item| Delivered {
                at_ms: now,
                verbatim: item.verbatim.clone(),
                possibly_partial: item.possibly_partial,
            }));
            wm = obs.next;
            now += 250;
        }
        delivered
    }

    /// `#steergateperceptron`: fake clock + recorded edit stream. The seeded
    /// learned gate makes exactly the deterministic gate's deliveries: the
    /// tsift fragment is held (until max-hold delivers it flagged), a bare
    /// `do [#id]` is delivered half a window after its last keystroke, and an
    /// issue URL is delivered the same way.
    #[test]
    fn recorded_edit_stream_seeded_learned_gate_matches_deterministic() {
        let stream: &[(u64, &str)] = &[
            (0, "- Should we\n"),
            (400, "- Should we release\n"),
            (900, "- Should we release + publish the\n"),
            // Abandoned mid-sentence; 10s later two more items arrive.
            (10_000, "- Should we release + publish the\n- do\n"),
            (10_200, "- Should we release + publish the\n- do [#\n"),
            (10_400, "- Should we release + publish the\n- do [#abc]\n"),
            (
                20_000,
                "- Should we release + publish the\n- do [#abc]\n- #subagent: https://github.com/btakita/agent-doc/issues/118\n",
            ),
        ];
        let learned = agent_doc_debounce::learned_gate::LearnedGate::new(
            agent_doc_debounce::learned_gate::GateWeights::seeded(),
        );
        let deterministic = replay(
            stream,
            50_000,
            &agent_doc_debounce::edit_settle::DeterministicOnly,
        );
        assert_eq!(replay(stream, 50_000, &learned), deterministic);

        let find = |text: &str| {
            deterministic
                .iter()
                .find(|d| d.verbatim == text)
                .unwrap_or_else(|| panic!("{text:?} never delivered: {deterministic:?}"))
        };
        let id = find("do [#abc]");
        assert!(!id.possibly_partial);
        assert!((11_400..=11_750).contains(&id.at_ms), "{id:?}");
        let url = find("#subagent: https://github.com/btakita/agent-doc/issues/118");
        assert!((21_000..=21_250).contains(&url.at_ms), "{url:?}");
        // The fragment is never delivered as settled: only the max-hold
        // (45s after it was first seen in this version) releases it, flagged.
        let fragment = find("Should we release + publish the");
        assert!(fragment.possibly_partial, "{fragment:?}");
        assert!(fragment.at_ms >= 45_000, "{fragment:?}");
        assert_eq!(deterministic.len(), 3, "{deterministic:?}");
    }

    /// `#steeringtypinggate` real case (tasks/software/tsift.md, 2026-10-03):
    /// the deterministic gate must not deliver the fragment as settled, the
    /// max-hold must still deliver it (flagged), and the finished line
    /// supersedes the held fragment.
    #[test]
    fn tsift_fragment_is_held_then_max_hold_delivers_it_flagged() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        let fragment = doc("- current task\n- Should we release + publish the\n", EX);
        let wm = seeded(&baseline, Some("current task"));
        let quiet_10s = ObserveContext {
            now_ms: 10_000,
            document_changed_ms: Some(0),
            ..quiet_ctx(&owned)
        };
        let held = observe(&wm, &fragment, &quiet_10s);
        assert!(held.ready.is_empty(), "{:?}", held.ready);
        assert_eq!(held.pending, 1);

        // Finishing the line supersedes the held fragment and settles it.
        let finished = doc(
            "- current task\n- Should we release + publish the C++ bindings?\n",
            EX,
        );
        let settled = observe(&held.next, &finished, &quiet_10s);
        assert_eq!(settled.ready.len(), 1, "{:?}", settled.ready);
        assert_eq!(
            settled.ready[0].verbatim,
            "Should we release + publish the C++ bindings?"
        );
        assert!(!settled.ready[0].possibly_partial);

        // Abandoned mid-word: the max-hold delivers it anyway, flagged.
        let past_max_hold = ObserveContext {
            now_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS + 1,
            document_changed_ms: Some(0),
            ..quiet_ctx(&owned)
        };
        let partial = observe(&held.next, &fragment, &past_max_hold);
        assert_eq!(partial.ready.len(), 1, "{:?}", partial.ready);
        assert!(partial.ready[0].possibly_partial);
        let rendered = render_steering_context("plan.md", &partial.ready, 0).unwrap();
        assert!(rendered.contains("possibly_partial: true"), "{rendered}");
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

    /// `#steeringeditmisalign`, replayed from the 2026-10-12
    /// `agent-doc-bugs.md` session: the acknowledged queue still held the
    /// eight claimed items the agent had since struck, the operator added the
    /// `ridernavp6*` items, and every hook paired a removed `do [#a]` with an
    /// untouched `do [#b]` by text similarity (`previous: do [#ridernavp4]`
    /// / `verbatim: do [#ridernavp6route]`). Distinct tracked ids are distinct
    /// items: the removals stay silent and the new ids are additions.
    #[test]
    fn distinct_tracked_ids_never_pair_as_an_edit() {
        let owned = BTreeSet::new();
        let claimed = "- 🚧 do [#bgclearpolicy]: Do you have enough direction for this? The hourglass is still visible in the queue.\n\
                       - 🚧 do [#ridernavp3]\n- 🚧 do [#ridernavp4]\n- 🚧 do [#ridernavp5]\n\
                       - 🚧 do [#stopreopenflake]\n- 🚧 do [#multilinequeuehead]\n\
                       - 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/242\n- 🚧 release + publish\n";
        let struck = "- ~~do [#bgclearpolicy]: Do you have enough direction for this? The hourglass is still visible in the queue.~~\n\
                      - ~~do [#ridernavp3]~~\n- ~~do [#ridernavp4]~~\n- ~~do [#ridernavp5]~~\n\
                      - ~~do [#stopreopenflake]~~\n- ~~do [#multilinequeuehead]~~\n\
                      - ~~#gh-fix https://github.com/btakita/agent-doc/issues/242~~\n- ~~release + publish~~\n";
        let added = "- do [#ridernavp6route]\n- do [#ridernavp6republish]\n- do [#ridernavp6status]\n\
                     - do [#ridernavp6gc]\n- do [#ridernavp6switchback]\n";
        let wm = seeded(&doc(claimed, EX), None);
        let current = doc(&format!("{struck}{added}"), EX);
        let obs = observe(&wm, &current, &quiet_ctx(&owned));
        let changes: Vec<_> = obs
            .ready
            .iter()
            .map(|item| {
                (
                    item.change,
                    item.verbatim.as_str(),
                    item.previous.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            changes,
            vec![
                (SteeringChange::Added, "do [#ridernavp6route]", None),
                (SteeringChange::Added, "do [#ridernavp6republish]", None),
                (SteeringChange::Added, "do [#ridernavp6status]", None),
                (SteeringChange::Added, "do [#ridernavp6gc]", None),
                (SteeringChange::Added, "do [#ridernavp6switchback]", None),
            ],
        );
        assert_eq!(obs.pending, 0);
    }

    /// `#steeringeditmisalign`: the operator deleting struck lines above
    /// claimed, untouched items is no steering at all, and a claim marker
    /// (`🚧`) on those items is not an edit either.
    #[test]
    fn deleting_struck_lines_above_claimed_items_is_not_steering() {
        let owned = BTreeSet::new();
        let before = doc(
            "- do [#infraclearsession]\n\
             - ~~do [#bgclearpolicy]: Do you have enough direction for this?~~\n\
             - ~~do [#ridernavp3]~~\n- ~~do [#ridernavp4]~~\n- ~~release + publish~~\n\
             - 🚧 do [#ridernavp6route]\n- 🚧 do [#ridernavp6republish]\n\
             - 🚧 do [#ridernavp6status]\n- 🚧 do [#ridernavp6gc]\n",
            EX,
        );
        let after = doc(
            "- 🚧 do [#infraclearsession]\n- 🚧 do [#ridernavp6route]\n\
             - 🚧 do [#ridernavp6republish]\n- 🚧 do [#ridernavp6status]\n\
             - 🚧 do [#ridernavp6gc]\n",
            EX,
        );
        let wm = seeded(&before, None);
        let obs = observe(&wm, &after, &quiet_ctx(&owned));
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(obs.pending, 0);
        // The same deletion while a removed item is the claimed one is still
        // a removal of THAT item, never an edit of a neighbour.
        let wm = seeded(
            &doc("- do [#ridernavp4]\n- do [#ridernavp6route]\n", EX),
            Some("do [#ridernavp4]"),
        );
        let obs = observe(
            &wm,
            &doc("- 🚧 do [#ridernavp6route]\n", EX),
            &quiet_ctx(&owned),
        );
        let changes: Vec<_> = obs
            .ready
            .iter()
            .map(|item| (item.change, item.verbatim.as_str(), item.current_item))
            .collect();
        assert_eq!(
            changes,
            vec![(SteeringChange::Deleted, "do [#ridernavp4]", true)]
        );
    }

    /// `#steeringeditmisalign`: the typing-gate count ("The operator is still
    /// typing N more item(s)") was these misaligned pairs. Each new line or
    /// claim re-paired the block, the candidate key changed, and the settle
    /// gate restarted, so nothing settled while the document kept changing.
    /// Identity-keyed candidates keep their key across claims and unrelated
    /// insertions and settle on their own stability.
    #[test]
    fn identity_keyed_candidates_settle_while_the_document_keeps_changing() {
        let owned = BTreeSet::new();
        let ctx = |now_ms| ObserveContext {
            now_ms,
            // The document changed at this very instant every time.
            document_changed_ms: Some(now_ms),
            ..quiet_ctx(&owned)
        };
        let wm = seeded(
            &doc(
                "- 🚧 do [#ridernavp3]\n- 🚧 do [#ridernavp4]\n- 🚧 release + publish\n",
                EX,
            ),
            None,
        );
        let v1 = doc(
            "- ~~do [#ridernavp3]~~\n- ~~do [#ridernavp4]~~\n- ~~release + publish~~\n\
             - do [#ridernavp6route]\n- do [#ridernavp6gc]\n",
            EX,
        );
        let first = observe(&wm, &v1, &ctx(10_000));
        assert!(first.ready.is_empty(), "{:?}", first.ready);
        assert_eq!(first.pending, 2);
        // The workers claim both items and the operator adds one above them.
        let v2 = doc(
            "- do [#infraclearsession]\n- ~~do [#ridernavp3]~~\n- ~~do [#ridernavp4]~~\n\
             - ~~release + publish~~\n- 🚧 do [#ridernavp6route]\n- 🚧 do [#ridernavp6gc]\n",
            EX,
        );
        let second = observe(&first.next, &v2, &ctx(12_000));
        let ready: Vec<_> = second
            .ready
            .iter()
            .map(|item| (item.change, item.verbatim.as_str()))
            .collect();
        assert_eq!(
            ready,
            vec![
                (SteeringChange::Added, "🚧 do [#ridernavp6route]"),
                (SteeringChange::Added, "🚧 do [#ridernavp6gc]"),
            ]
        );
        // Only the genuinely new line is still held.
        assert_eq!(second.pending, 1);
        let third = observe(&second.next, &v2, &ctx(14_000));
        assert_eq!(third.ready.len(), 1, "{:?}", third.ready);
        assert_eq!(third.ready[0].verbatim, "do [#infraclearsession]");
        assert_eq!(third.pending, 0);
    }

    /// `#steeringeditmisalign`: real edits still surface with the right
    /// `previous`, even next to a removal, and a moved item is untouched.
    #[test]
    fn identity_pairing_keeps_real_edits_and_moves() {
        let owned = BTreeSet::new();
        // Same tracked id, new text: an edit of that item.
        let wm = seeded(&doc("- do [#alpha]\n- do [#beta]\n", EX), None);
        let obs = observe(
            &wm,
            &doc("- do [#beta]\n- do [#alpha] and add docs\n", EX),
            &quiet_ctx(&owned),
        );
        let changes: Vec<_> = obs
            .ready
            .iter()
            .map(|item| {
                (
                    item.change,
                    item.verbatim.as_str(),
                    item.previous.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            changes,
            vec![(
                SteeringChange::Edited,
                "do [#alpha] and add docs",
                Some("do [#alpha]")
            )]
        );
        // Free-text edit beside a removed line: the edit keeps its own
        // previous text; the removal stays silent.
        let wm = seeded(&doc("- do [#gone]\n- later work\n- other work\n", EX), None);
        let obs = observe(
            &wm,
            &doc("- later work, but also docs\n- other work\n", EX),
            &quiet_ctx(&owned),
        );
        let changes: Vec<_> = obs
            .ready
            .iter()
            .map(|item| {
                (
                    item.change,
                    item.verbatim.as_str(),
                    item.previous.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            changes,
            vec![(
                SteeringChange::Edited,
                "later work, but also docs",
                Some("later work")
            )]
        );
        // Reordering untouched items is not steering.
        let wm = seeded(
            &doc("- do [#alpha]\n- free text item\n- do [#beta]\n", EX),
            None,
        );
        let obs = observe(
            &wm,
            &doc("- 🚧 do [#beta]\n- do [#alpha]\n- free text item\n", EX),
            &quiet_ctx(&owned),
        );
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);
        assert_eq!(
            obs.next.acknowledged_queue,
            vec![
                "do [#beta]".to_string(),
                "do [#alpha]".to_string(),
                "free text item".to_string()
            ]
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
            explicit_send: false,
            max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
            classifier: &agent_doc_debounce::edit_settle::DeterministicOnly,
            median_pause_ms: None,
        };
        assert_eq!(observe(&wm, &current, &ctx(3_000)).ready.len(), 1);
        assert!(observe(&wm, &current, &ctx(10_000)).ready.is_empty());
    }

    #[test]
    fn boundary_mode_treats_head_consumption_as_bookkeeping() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n- later work\n", EX);
        // Closeout consumed the current head; the operator added a line and
        // edited a later one.
        let current = doc(
            "- later work, plus docs\n- #subagents do [#preflightdeadline]\n",
            EX,
        );
        let wm = seeded(&baseline, Some("current task"));
        let obs = observe_with_mode(&wm, &current, &quiet_ctx(&owned), ObserveMode::Boundary);
        let changes: Vec<_> = obs
            .ready
            .iter()
            .map(|item| (item.change, item.dispatch, item.current_item))
            .collect();
        assert_eq!(
            changes,
            vec![
                (
                    SteeringChange::Edited,
                    SteeringDispatch::DrainAfterCurrent,
                    false
                ),
                (SteeringChange::Added, SteeringDispatch::Subagent, false),
            ],
            "{:?}",
            obs.ready
        );
        // In-turn, the same removal is an address-now deletion.
        let in_turn = observe(&wm, &current, &quiet_ctx(&owned));
        assert!(
            in_turn
                .ready
                .iter()
                .any(|item| item.change == SteeringChange::Deleted && item.current_item)
        );
    }

    #[test]
    fn subagent_instruction_is_source_aware_about_queue_lifecycle() {
        let item = |source| SteeringItem {
            source,
            change: SteeringChange::Added,
            dispatch: SteeringDispatch::Subagent,
            current_item: false,
            verbatim: "fix the parser".to_string(),
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: None,
            generation: None,
            worker_id: None,
            explicit: false,
            vector: None,
            queue_mode: None,
        };

        let queue =
            render_steering_context("tasks/bugs.md", &[item(SteeringSource::Queue)], 0).unwrap();
        assert!(queue.contains("agent-doc queue claim"), "{queue}");
        // `#reportedrelease`: the queue lifecycle ends in the closeout that
        // strikes the line, not in a bare release.
        assert!(
            queue.contains("When the subagent reports back, persist its result"),
            "{queue}"
        );
        assert!(queue.contains("> **Queue prompt:** <line>"), "{queue}");
        assert!(
            queue.contains("authorized current-item or returned-subagent outward effect")
                && queue.contains("#effectbeforecloseout"),
            "queue dispatch guidance must keep returned effects ahead of closeout: {queue}"
        );
        let closeout =
            render_closeout_steering_context("tasks/bugs.md", &[item(SteeringSource::Queue)], 0)
                .unwrap();
        assert!(closeout.contains(REPORTED_WORKER_CLOSEOUT), "{closeout}");
        assert!(closeout.contains("queue release --requeue"), "{closeout}");

        let exchange =
            render_steering_context("tasks/bugs.md", &[item(SteeringSource::Exchange)], 0).unwrap();
        assert!(
            exchange.contains("no queue claim/release lifecycle"),
            "{exchange}"
        );
        assert!(!exchange.contains("agent-doc queue claim"), "{exchange}");
        assert!(!exchange.contains("agent-doc queue release"), "{exchange}");
    }

    /// `#reportedrelease`: a line whose worker reported is routed to the
    /// coordinator's persist-now closeout in both renders, never to a NEW
    /// subagent dispatch; a claimed (forwarded) or deleted line is untouched.
    #[test]
    fn reported_release_routes_to_persist_reported_never_a_new_dispatch() {
        let line = "Does the page status enum have an archived value?";
        let item = |change, dispatch| SteeringItem {
            source: SteeringSource::Queue,
            change,
            dispatch,
            current_item: false,
            verbatim: line.to_string(),
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: None,
            generation: None,
            worker_id: None,
            explicit: false,
            vector: None,
            queue_mode: None,
        };
        let mut items = vec![
            item(SteeringChange::Added, SteeringDispatch::Subagent),
            item(SteeringChange::Added, SteeringDispatch::ForwardToOwner),
            item(SteeringChange::Deleted, SteeringDispatch::Subagent),
        ];
        apply_release_receipts(&mut items, |_| {
            Some(("subagent:page-status-enum".to_string(), 0))
        });
        assert_eq!(items[0].dispatch, SteeringDispatch::PersistReported);
        assert_eq!(items[0].owner.as_deref(), Some("subagent:page-status-enum"));
        assert_eq!(items[1].dispatch, SteeringDispatch::ForwardToOwner);
        assert_eq!(items[2].dispatch, SteeringDispatch::Subagent);

        for rendered in [
            render_steering_context("tasks/docs.md", &items[..1], 0).unwrap(),
            render_closeout_steering_context("tasks/docs.md", &items[..1], 0).unwrap(),
        ] {
            assert!(rendered.contains("dispatch=persist_reported"), "{rendered}");
            assert!(
                rendered.contains("owner=subagent:page-status-enum"),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("`> **Queue prompt:** {line}`")),
                "{rendered}"
            );
            assert!(
                rendered.contains("agent-doc finalize tasks/docs.md"),
                "{rendered}"
            );
            assert!(!rendered.contains("DISPATCH NOW"), "{rendered}");
        }
    }

    #[test]
    fn closeout_exchange_subagent_has_no_queue_lifecycle() {
        let item = SteeringItem {
            source: SteeringSource::Exchange,
            change: SteeringChange::Added,
            dispatch: SteeringDispatch::Subagent,
            current_item: false,
            verbatim: "fix the parser".to_string(),
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: None,
            generation: None,
            worker_id: None,
            explicit: false,
            vector: None,
            queue_mode: None,
        };

        let item_clone = item.clone();
        let text = render_closeout_steering_context("tasks/bugs.md", &[item], 0).unwrap();
        assert!(text.contains("DISPATCH NOW"), "{text}");
        assert!(text.contains("no queue claim/release lifecycle"), "{text}");
        assert!(!text.contains("agent-doc queue claim"), "{text}");
        assert!(!text.contains("agent-doc queue release"), "{text}");
        // Exchange subagents get no `queue brief` preamble, so both render
        // paths carry the shared scratch-isolation rule themselves.
        let scratch = agent_doc_queue::subagent_brief::SUBAGENT_SCRATCH_ISOLATION_RULE;
        let tmux = agent_doc_queue::subagent_brief::SUBAGENT_TMUX_ISOLATION_RULE;
        assert!(text.contains(scratch), "{text}");
        assert!(text.contains(tmux), "{text}");
        let instruction = instruction_for(&item_clone);
        assert!(instruction.contains(scratch), "{instruction}");
        assert!(instruction.ends_with(tmux), "{instruction}");
    }

    #[test]
    fn closeout_render_is_an_explicit_dispatch_directive() {
        let item = SteeringItem {
            source: SteeringSource::Queue,
            change: SteeringChange::Added,
            dispatch: SteeringDispatch::Subagent,
            current_item: false,
            verbatim: "#subagents do [#preflightdeadline]".to_string(),
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: None,
            generation: None,
            worker_id: None,
            explicit: false,
            vector: None,
            queue_mode: None,
        };
        let text = render_closeout_steering_context("tasks/bugs.md", &[item], 0).unwrap();
        assert!(text.starts_with(CLOSEOUT_STEERING_MARKER), "{text}");
        assert!(text.contains("DISPATCH NOW"), "{text}");
        assert!(
            text.contains(
                "`agent-doc queue claim 'tasks/bugs.md' --item '#preflightdeadline' --owner subagent:<label>`"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "`agent-doc queue start 'tasks/bugs.md' --item '#preflightdeadline' --owner subagent:<label> --generation <generation> --worker-id <worker-id>`"
            ),
            "the dispatch directive names the worker acknowledgement: {text}"
        );
        assert!(
            text.contains("`agent-doc queue brief 'tasks/bugs.md' --item '#preflightdeadline'`"),
            "the dispatch directive names the authorization brief: {text}"
        );
        assert_eq!(
            claim_item_handle("#gh-fix https://x/issues/1"),
            "#gh-fix https://x/issues/1"
        );
        assert_eq!(
            claim_item_handle("verify [#existing] yourself"),
            "verify [#existing] yourself"
        );
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn queue_subagents_attr_makes_every_line_subagent_intent_except_opt_outs() {
        for marker in ["subagents", "fan-out", "subagents=2 preset=\"#gh-fix\""] {
            let content = format!(
                "{FM}# Session\n\n<!-- agent:queue {marker} go -->\n- do [#a]\n- do [#b] [inline]\n- [operator-verify] check the pane\n- fix the bug\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
            );
            assert_eq!(
                subagent_dispatch_heads(&content, None),
                vec!["do [#a]".to_string(), "fix the bug".to_string()],
                "{marker}"
            );
        }
        let invalid = format!(
            "{FM}# Session\n\n<!-- agent:queue subagents=0 -->\n- do [#a]\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        );
        assert!(subagent_dispatch_heads(&invalid, None).is_empty());
    }

    #[test]
    fn queue_subagents_attr_is_scoped_to_selected_identified_queue() {
        let content = format!(
            "{FM}# Session\n\n<!-- agent:queue id=release-a -->\n~~- shipped A~~\n<!-- /agent:queue -->\n\n<!-- agent:queue id=release-b after=release-a subagents -->\n- organize B\n- publish B [inline]\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        );
        assert_eq!(
            subagent_dispatch_heads(&content, None),
            vec!["organize B".to_string()]
        );
        let (mode, eligible, live) = queue_attr_subagent_heads(&content).unwrap();
        assert_eq!(
            mode.capacity,
            agent_doc_queue::subagent_intent::QueueSubagentsCapacity::AllEligible
        );
        assert_eq!(eligible, vec!["organize B".to_string()]);
        assert_eq!(
            live,
            vec!["organize B".to_string(), "publish B [inline]".to_string()]
        );
    }

    /// A shared `#rebase` leaf is dispatched as each parent's nest vector.
    /// One parent still names that row; the visible line stays the leaf.
    #[test]
    fn shared_rebase_heads_keep_each_parent_lineage() {
        let api = "https://github.com/haiven-dev/haiven-api/pull/767: #rebase";
        let contracts = "https://github.com/haiven-dev/haiven-contracts/pull/162: #rebase";
        let content = format!(
            "{FM}# Session\n\n<!-- agent:queue subagents -->\n- https://github.com/haiven-dev/haiven-api/pull/767\n  - #rebase\n- https://github.com/haiven-dev/haiven-contracts/pull/162\n  - #rebase\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        );
        let (_, eligible, live) = queue_attr_subagent_heads(&content).unwrap();
        assert_eq!(eligible, vec![api.to_string(), contracts.to_string()]);
        assert_eq!(live, eligible);
        let unique = format!(
            "{FM}# Session\n\n<!-- agent:queue subagents -->\n- https://github.com/haiven-dev/haiven-api/pull/767\n  - #rebase\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        );
        let (_, eligible, _) = queue_attr_subagent_heads(&unique).unwrap();
        assert_eq!(eligible, vec![api.to_string()]);
    }

    /// An earlier `subagents` run whose only head is operator-gated must not
    /// hide the next run's unclaimed work, and must not name the run after that.
    #[test]
    fn operator_gated_run_yields_the_next_subagents_queue() {
        let content = format!(
            "{FM}# Session\n\n<!-- agent:queue id=main subagents go -->\n- 🙋 do [#blocked]:\n  - 🙋 merge the pull request [#subagent-operator-request-blocked]\n<!-- /agent:queue -->\n\n<!-- agent:queue id=api-prs subagents -->\n- https://example.com/pull/759\n<!-- /agent:queue -->\n\n<!-- agent:queue id=contracts-prs subagents -->\n- https://example.com/pull/162\n<!-- /agent:queue -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        );
        let (mode, eligible, live) = queue_attr_subagent_heads(&content).unwrap();
        assert_eq!(
            mode.capacity,
            agent_doc_queue::subagent_intent::QueueSubagentsCapacity::AllEligible
        );
        assert_eq!(eligible, vec!["https://example.com/pull/759".to_string()]);
        assert_eq!(live, vec!["https://example.com/pull/759".to_string()]);
    }

    /// `#struckparentkids`: a row the operator writes under a closed
    /// (struck) parent is new work, not an inert edit of a completed entry:
    /// steering surfaces the reopened block once, verbatim with its parent,
    /// and it is subagent dispatch work under a `subagents` queue.
    #[test]
    fn a_row_added_under_a_struck_parent_surfaces_as_new_work() {
        let owned = BTreeSet::new();
        let closed =
            "- current task\n- ~~#req-integration-id~~\n  - ~~Why do we need a SBX/STG id?~~\n";
        let reopened = format!("{closed}  - We moved onto project templates.\n");
        let baseline = doc(closed, EX);
        let current = doc(&reopened, EX);
        let wm = seeded(&baseline, Some("current task"));
        let obs = observe(&wm, &current, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.source, SteeringSource::Queue);
        assert_eq!(item.change, SteeringChange::Added);
        assert_eq!(item.dispatch, SteeringDispatch::DrainAfterCurrent);
        assert_eq!(item.verbatim, "We moved onto project templates.");
        let scoped = current.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        let reference = vec!["current task".to_string()];
        assert_eq!(
            subagent_dispatch_heads(&scoped, Some(&reference)),
            vec!["#req-integration-id: We moved onto project templates."]
        );
        // The closed context and its closed child alone are never work.
        let closed_scoped = baseline.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        assert!(subagent_dispatch_heads(&closed_scoped, Some(&reference)).is_empty());
    }

    #[test]
    fn subagent_dispatch_heads_are_new_subagent_intent_lines() {
        let content = doc(
            "- current task\n- #subagents do [#old]\n- #subagents do [#new1]\n- plain new\n",
            EX,
        );
        let reference = vec![
            "current task".to_string(),
            "#subagents do [#old]".to_string(),
        ];
        assert_eq!(
            subagent_dispatch_heads(&content, Some(&reference)),
            vec!["#subagents do [#new1]".to_string()]
        );
        assert_eq!(
            subagent_dispatch_heads(&content, None),
            vec![
                "#subagents do [#old]".to_string(),
                "#subagents do [#new1]".to_string()
            ]
        );
        let scoped = content.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue preset=\"#subagents\" -->",
        );
        assert_eq!(
            subagent_dispatch_heads(&scoped, Some(&reference)),
            vec!["#subagents do [#new1]".to_string(), "plain new".to_string()]
        );
    }

    #[test]
    fn todo_tracking_and_waiting_response_edges_preserve_claim_dispatch_parity() {
        let todo_only = concat!(
            "<!-- agent:run id=main subagents -->\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#yeja] Choose the project-root policy.\n",
            "<!-- /agent:todo -->\n",
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_queue_head_texts(todo_only)
                .unwrap_or_default(),
            Vec::<String>::new(),
            "Todo tracking alone is never run selection"
        );
        assert!(subagent_dispatch_heads(todo_only, None).is_empty());

        let answered = concat!(
            "<!-- agent:run id=main subagents -->\n",
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - [#yeja] Choose the project-root policy. <!-- agent-doc:waiting-item -->\n",
            "    - ~~**Response:** Choose policy. <!-- agent-doc:waiting-metadata -->~~\n",
            "      - ~~**Next actions:** <!-- agent-doc:waiting-metadata -->~~\n",
            "        - ~~**Action:** Choose policy. <!-- agent-doc:waiting-metadata -->~~\n",
            "          - pick: a + b\n",
            "      - ~~**Next actions:** <!-- agent-doc:waiting-metadata -->~~\n",
            "        - ~~**Action:** Clean the live buffer. <!-- agent-doc:waiting-metadata -->~~\n",
            "          - ~/.agent-doc/live-buffer: delete\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#yeja] Choose the project-root policy.\n",
            "<!-- /agent:todo -->\n",
        );
        let claim_heads =
            agent_doc_queue::queue_continuation::live_queue_head_texts(answered).unwrap();
        let (_, eligible, dispatch_live) = queue_attr_subagent_heads(answered)
            .expect("the subagents run exposes one selected work node");
        assert_eq!(claim_heads, dispatch_live, "claim and dispatch share one live set");
        assert_eq!(eligible, dispatch_live);
        assert_eq!(dispatch_live.len(), 1, "{dispatch_live:#?}");
        assert!(dispatch_live[0].contains("[#yeja]"), "{dispatch_live:#?}");
        assert!(!dispatch_live[0].contains("pick: a + b"));
        assert!(!dispatch_live[0].contains("live-buffer"));
    }

    #[test]
    fn waiting_answer_edit_steers_one_parent_selection_not_response_leaves() {
        let owned = BTreeSet::new();
        let baseline_queue = concat!(
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - [#yeja] Choose the project-root policy. <!-- agent-doc:waiting-item -->\n",
            "    - ~~**Response:** Choose policy. <!-- agent-doc:waiting-metadata -->~~\n",
            "      - ~~**Next actions:** <!-- agent-doc:waiting-metadata -->~~\n",
            "        - ~~**Action:** Choose policy. <!-- agent-doc:waiting-metadata -->~~\n",
        );
        let answered_queue = format!(
            "{baseline_queue}          - pick: a + b\n          - ~/.agent-doc/live-buffer: delete\n"
        );
        let baseline = doc(baseline_queue, EX);
        let current = doc(&answered_queue, EX);
        let obs = observe(&seeded(&baseline, None), &current, &quiet_ctx(&owned));

        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        let item = &obs.ready[0];
        assert_eq!(item.source, SteeringSource::Queue);
        assert_eq!(item.change, SteeringChange::Added);
        assert!(item.verbatim.contains("[#yeja]"), "{item:?}");
        assert!(!item.verbatim.contains("pick: a + b"), "{item:?}");
        assert!(!item.verbatim.contains("live-buffer"), "{item:?}");
    }

    /// `#regressionawaitingoperat`: a head whose tracked item waits on the
    /// operator (`[operator-verify]`, projected `⏳`) is never dispatch work,
    /// through either the item tag or the queue-level attribute, until the
    /// operator answers it inline.
    #[test]
    fn awaiting_operator_heads_are_never_dispatch_heads() {
        let backlog = "\n<!-- agent:backlog -->\n- [ ] [#verify] [operator-verify] live proof\n- [ ] [#work] fix it\n<!-- /agent:backlog -->\n";
        let tagged = format!(
            "{}{backlog}",
            doc(
                "- ⏳ #subagents do [#verify]\n- #subagents do [#work]\n",
                EX
            )
        );
        assert_eq!(
            subagent_dispatch_heads(&tagged, None),
            vec!["#subagents do [#work]".to_string()]
        );
        let attr = format!("{}{backlog}", doc("- ⏳ do [#verify]\n- do [#work]\n", EX))
            .replace("<!-- agent:queue -->", "<!-- agent:queue subagents -->");
        let (_, eligible, live) = queue_attr_subagent_heads(&attr).unwrap();
        assert_eq!(eligible, vec!["do [#work]".to_string()]);
        assert_eq!(live.len(), 2, "{live:?}");
        // An inline operator verdict makes the head agent work again.
        let answered = attr.replace("- ⏳ do [#verify]", "- do [#verify]: verified, ship it");
        let (_, eligible, _) = queue_attr_subagent_heads(&answered).unwrap();
        assert_eq!(eligible.len(), 2, "{eligible:?}");
    }

    /// `#queueblocked`: a head whose tracked item waits on an open blocker is
    /// never dispatch work through either the item tag or the queue-level
    /// attribute, and returns once the blocker closes.
    #[test]
    fn blocked_heads_are_never_dispatch_heads() {
        let backlog = "\n<!-- agent:backlog -->\n- [ ] [#pre] prerequisite\n- [ ] [#dep] waits (stays open: blocked behind #pre)\n<!-- /agent:backlog -->\n";
        let tagged = format!(
            "{}{backlog}",
            doc("- #subagents do [#dep]\n- #subagents do [#pre]\n", EX)
        );
        assert_eq!(
            subagent_dispatch_heads(&tagged, None),
            vec!["#subagents do [#pre]".to_string()]
        );
        let attr = format!(
            "{}{backlog}",
            doc("- ⛔ ⟦blocked by pre⟧ do [#dep]\n- do [#pre]\n", EX)
        )
        .replace("<!-- agent:queue -->", "<!-- agent:queue subagents -->");
        let (_, eligible, live) = queue_attr_subagent_heads(&attr).unwrap();
        assert_eq!(eligible, vec!["do [#pre]".to_string()]);
        assert_eq!(live.len(), 2, "{live:?}");
        let closed = attr.replace("- [ ] [#pre]", "- [x] [#pre]");
        let (_, eligible, _) = queue_attr_subagent_heads(&closed).unwrap();
        assert_eq!(
            eligible,
            vec!["do [#dep]".to_string(), "do [#pre]".to_string()]
        );
    }

    /// `#claimdispatchidentity` (live 2026-10-03): a tag-only edit of a line
    /// that already carried subagent intent is the same dispatch, not new
    /// work; escalating a plain line to subagent intent and retargeting to a
    /// different URL are new. A nesting parent (a group label) is never a
    /// dispatch head because `queue claim` cannot claim it (GH #124 ask 5).
    #[test]
    fn dispatch_newness_keys_on_work_identity() {
        let url = "https://github.com/btakita/agent-doc/issues/120";
        let reference = vec![format!("#subagents: {url}"), "do [#a]".to_string()];
        let edited = doc(
            &format!(
                "- #subagents: #gh-fix {url}
- do [#a]
"
            ),
            EX,
        );
        assert!(
            subagent_dispatch_heads(&edited, Some(&reference)).is_empty(),
            "a tag-only edit of a dispatched line is not new dispatch work"
        );
        let escalated = doc(
            &format!(
                "- #subagents: {url}
- #subagents do [#a]
"
            ),
            EX,
        );
        assert_eq!(
            subagent_dispatch_heads(&escalated, Some(&reference)),
            vec!["#subagents do [#a]".to_string()],
            "adding subagent intent to a plain line still dispatches it"
        );
        let retarget = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/121";
        let retargeted = doc(
            &format!(
                "- {retarget}
- do [#a]
"
            ),
            EX,
        );
        assert_eq!(
            subagent_dispatch_heads(&retargeted, Some(&reference)),
            vec![retarget.to_string()],
            "a different issue is new work"
        );
        let nested = doc(
            "- #subagents: epic label
  - do [#child]
",
            EX,
        );
        assert!(
            subagent_dispatch_heads(&nested, None).is_empty(),
            "a group parent is not a claimable head"
        );
    }

    #[test]
    fn nothing_new_renders_nothing() {
        assert_eq!(render_steering_context("plan.md", &[], 3), None);
    }

    /// `#steerworks` item 2: the coordinator recorded a chat-originated prompt
    /// as `> **Chat prompt (#chatprompt):** …` just above its `### Re:`
    /// heading (agent-doc-bugs.md, 2026-10-04). That is agent bookkeeping and
    /// must never come back as `address_now` operator steering.
    #[test]
    fn agent_recorded_chat_prompt_is_never_operator_steering() {
        let owned = BTreeSet::new();
        // The pre-existing quote makes the blank blockquote separator (`>`)
        // non-unique relative to the baseline. It must still remain part of a
        // newly recorded multi-paragraph chat prompt.
        let baseline_exchange = format!(
            "{EX}> **Chat prompt (#chatprompt):** older prompt\n>\n> older second paragraph\n\n"
        );
        let baseline = doc("", &baseline_exchange);
        let record = "> **Chat prompt (#chatprompt):** Did you get my steering change when I edited the queue item?";
        let current = doc(
            "",
            &format!(
                "{baseline_exchange}{record}\n> first paragraph continuation\n>\n> second paragraph of the same chat prompt\n\n### Re: steering edit on the #126 queue head\n\nAnswer.\n"
            ),
        );
        let wm = seeded(&baseline, None);
        for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
            let obs = observe_with_mode(&wm, &current, &quiet_ctx(&owned), mode);
            assert!(obs.ready.is_empty(), "{mode:?}: {:?}", obs.ready);
            assert_eq!(obs.pending, 0, "{mode:?}");
        }
        // An operator prompt typed after the record still steers.
        let with_operator = current.replace(
            "Answer.\n",
            "Answer.\n\n<!-- agent:boundary:abc -->\nplease also bump the version\n",
        );
        let obs = observe(&wm, &with_operator, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert!(
            obs.ready[0]
                .verbatim
                .contains("please also bump the version")
        );
        assert!(!obs.ready[0].verbatim.contains("#chatprompt"));
    }

    /// `#echopreludesteer`: the response's own `> **Queue prompt:**` echo and
    /// the `**Deferred:**` paragraph under it sit above its `### Re:` heading.
    /// Neither is operator steering; an operator prompt after the response
    /// still is.
    #[test]
    fn response_echo_prelude_above_heading_is_never_operator_steering() {
        let owned = BTreeSet::new();
        let baseline = doc("", EX);
        let prelude = concat!(
            "> **Queue prompt:** Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n\n",
            "**Deferred:** this group waits on two things: the fix install and the operator answers.\n\n",
        );
        let current = doc(
            "",
            &format!(
                "{EX}{prelude}### Re: Queue status while the fix runs\n\nNo new operator edits this cycle.\n"
            ),
        );
        let wm = seeded(&baseline, None);
        for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
            let obs = observe_with_mode(&wm, &current, &quiet_ctx(&owned), mode);
            assert!(obs.ready.is_empty(), "{mode:?}: {:?}", obs.ready);
            assert_eq!(obs.pending, 0, "{mode:?}");
        }
        let with_operator = current.replace(
            "No new operator edits this cycle.\n",
            "No new operator edits this cycle.\n\n<!-- agent:boundary:abc -->\nplease also bump the version\n",
        );
        let obs = observe(&wm, &with_operator, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert!(
            obs.ready[0]
                .verbatim
                .contains("please also bump the version")
        );
        assert!(!obs.ready[0].verbatim.contains("Deferred"));
    }

    /// `#committedresponseown`: the Waiting-on-you echo is byte-identical in
    /// every response that reports the group, so a new response's echo line
    /// already exists in the baseline (an earlier response carried it). That
    /// repeated echo is still the new response's own prelude: it is never
    /// operator steering, in turn or at the boundary after closeout. An
    /// operator prompt typed after the new response still is.
    #[test]
    fn repeated_response_echo_prelude_is_never_operator_steering() {
        let owned = BTreeSet::new();
        let echo = "> **Queue prompt:** Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n";
        let earlier = format!(
            "{EX}{echo}\n**Deferred:** this group waits on the operator answers.\n\n### Re: Queue status — first\n\nNothing new.\n"
        );
        let baseline = doc("", &earlier);
        let wm = seeded(&baseline, None);
        for later in [
            // Echo directly above the heading.
            format!("{echo}\n### Re: Queue status — second\n\nStill nothing new.\n"),
            // Echo, a Deferred paragraph, then the heading.
            format!(
                "{echo}\n**Deferred:** the group still waits on the operator answers.\n\n### Re: Queue status — second\n\nStill nothing new.\n"
            ),
        ] {
            let current = doc("", &format!("{earlier}{later}"));
            for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
                let obs = observe_with_mode(&wm, &current, &quiet_ctx(&owned), mode);
                assert!(obs.ready.is_empty(), "{mode:?}: {:?}", obs.ready);
                assert_eq!(obs.pending, 0, "{mode:?}");
            }
            let with_operator = format!(
                "{}<!-- agent:boundary:abc -->\nplease also bump the version\n",
                current.trim_end_matches("<!-- /agent:exchange -->\n")
            ) + "<!-- /agent:exchange -->\n";
            for mode in [ObserveMode::InTurn, ObserveMode::Boundary] {
                let obs = observe_with_mode(&wm, &with_operator, &quiet_ctx(&owned), mode);
                assert_eq!(obs.ready.len(), 1, "{mode:?}: {:?}", obs.ready);
                assert!(
                    obs.ready[0]
                        .verbatim
                        .contains("please also bump the version"),
                    "{mode:?}: {:?}",
                    obs.ready
                );
                assert!(!obs.ready[0].verbatim.contains("Queue prompt"), "{mode:?}");
            }
        }
    }

    /// `#claimedsteerwake`: an explicit send bypasses the typing gate and
    /// marks every item final.
    #[test]
    fn explicit_send_flushes_held_items_as_final() {
        let owned = BTreeSet::new();
        let baseline = doc("- current task\n", EX);
        // Unfinished shape: the gate would hold it, even past the window.
        let current = doc("- current task\n- publish the\n", EX);
        let wm = seeded(&baseline, Some("current task"));
        let typing = ObserveContext {
            now_ms: 10_000,
            document_changed_ms: Some(9_900),
            ..quiet_ctx(&owned)
        };
        assert!(observe(&wm, &current, &typing).ready.is_empty());
        let sent = observe(
            &wm,
            &current,
            &ObserveContext {
                explicit_send: true,
                ..typing
            },
        );
        assert_eq!(sent.ready.len(), 1, "{:?}", sent.ready);
        assert!(sent.ready[0].explicit);
        assert_eq!(sent.ready[0].dispatch, SteeringDispatch::AddressNow);
        assert!(!sent.ready[0].possibly_partial);
        assert_eq!(sent.pending, 0);
        let text = render_steering_context("plan.md", &sent.ready, 0).unwrap();
        assert!(text.contains("sent=explicit"), "{text}");
        assert!(text.contains("Address it in THIS turn"), "{text}");
    }

    #[test]
    fn selected_queue_head_presence_survives_cosmetic_marker_changes() {
        let baseline = doc("- current task\n", EX);
        let watermark = seeded(&baseline, Some("current task"));
        let still_open = doc("- 🚧 current task\n- newly sent work\n", EX);
        assert!(current_item_is_present(&watermark, &still_open));
        let closed = doc("- newly sent work\n", EX);
        assert!(!current_item_is_present(&watermark, &closed));
    }

    /// `#managedlistnodrift` regression: a waiting-item head whose only change
    /// is the binary-owned roll-up token (`#waitingitemstatus`) and its
    /// binary-owned `Next:` row is not operator steering. The hook reported
    /// both as `change=edited/added` and forwarded one to a running subagent.
    #[test]
    fn waiting_roll_up_and_next_row_are_not_steering() {
        let owned = BTreeSet::new();
        let root = "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n";
        let baseline = doc(
            &format!(
                "{root}  - [#subagent-operator-request-sample-workspace] Provide a sample workspace <!-- agent-doc:waiting-item -->\n    - **Response:** Provide a sample workspace. ▫️ <!-- agent-doc:reply -->\n  - 🚧 [#subagent:smoke#1] Run the sample smoke test <!-- agent-doc:waiting-item --> [#samplesmoke]\n    - **Response:** Run it. ▫️ <!-- agent-doc:reply -->\n"
            ),
            EX,
        );
        let current = doc(
            &format!(
                "{root}  - ⟦you: 1 · agent: 0 · done: 0⟧ [#subagent-operator-request-sample-workspace] Provide a sample workspace <!-- agent-doc:waiting-item -->\n    - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet) ▫️ <!-- agent-doc:reply -->\n    - **Response:** Provide a sample workspace. ▫️ <!-- agent-doc:reply -->\n  - 🚧 [#subagent:smoke#1] ⟦🚧 you: 0 · agent: 1 · done: 0⟧ Run the sample smoke test <!-- agent-doc:waiting-item --> [#samplesmoke]\n    - **Next:** agent — finish the running attempt (evidence: 🚧 `subagent:smoke#1`) ▫️ <!-- agent-doc:reply -->\n    - **Response:** Run it. ▫️ <!-- agent-doc:reply -->\n"
            ),
            EX,
        );
        // Also the legacy unmarked head shape, whose sentinel is hidden.
        let unmarked = |doc: &str| doc.replace(" <!-- agent-doc:waiting-item -->", "");
        for (baseline, current) in [
            (baseline.clone(), current.clone()),
            (unmarked(&baseline), unmarked(&current)),
        ] {
            for mode in [ObserveMode::Boundary, ObserveMode::InTurn] {
                let obs = observe_with_mode(&seeded(&baseline, None), &current, &quiet_ctx(&owned), mode);
                assert!(obs.ready.is_empty(), "{mode:?}: {:?}", obs.ready);
            }
        }
    }

    /// `#steeringposttool`: the live waiting-group shapes from
    /// tasks/agent-doc/agent-doc-bugs.md (2026-11-04), in an `agent:run …
    /// subagents` component. A waiting item is executable once the operator
    /// has replied under it, so each head here carries a standing operator
    /// reply and stays the same selectable row on both sides. Every transition
    /// the closeout projection makes on such a head (the `🙋` gate appearing
    /// or clearing with the roll-up, the state moving into the token, the
    /// counts changing, a `**Next:**` row being added or rewritten) is
    /// binary-owned and never steering. A new operator reply still is.
    #[test]
    fn live_waiting_head_projection_transitions_are_not_steering() {
        let owned = BTreeSet::new();
        let run_doc = |rows: &str| {
            format!(
                "{FM}# Session\n\n## Run <!-- agent:run id=\"main\" subagents priority go -->\n- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n{rows}<!-- /agent:run -->\n\n<!-- agent:exchange -->\n{EX}<!-- /agent:exchange -->\n"
            )
        };
        let sentinel = " <!-- agent-doc:waiting-item -->";
        let free = "Run the sample smoke on a real workspace, then update the status cell.";
        let tracked = "[#subagent-operator-request-sample-workspace] Provide a real workspace context for #samplesmoke";
        let response =
            "    - **Response:** Provide a real workspace. ▫️ <!-- agent-doc:reply -->\n";
        let reply = "    - moot or still needed?\n";
        let next_you = "    - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet) ▫️ <!-- agent-doc:reply -->\n";
        let next_agent = "    - **Next:** agent — pick up your reply “moot or still needed?” and attempt `#subagent-operator-request-sample-workspace` (evidence: your reply) ▫️ <!-- agent-doc:reply -->\n";
        let head = |prefix: &str, body: &str| format!("  - {prefix}{body}{sentinel}\n");
        let item = |prefix: &str, body: &str, next: &str| {
            format!("{}{next}{response}{reply}", head(prefix, body))
        };
        let cases: Vec<(&str, String, String)> = vec![
            (
                "free-text head gains gate, token and Next row",
                item("🙋 ", free, ""),
                item("🙋 ⟦you: 1 · agent: 0 · done: 0⟧ ", free, next_you),
            ),
            (
                "free-text head: gate clears, state moves into the token",
                item("🙋 ⟦you: 1 · agent: 0 · done: 0⟧ ", free, next_you),
                item("⟦📥 you: 0 · agent: 1 · done: 1⟧ ", free, next_agent),
            ),
            (
                "free-text head: bare head gains a stateful token",
                item("", free, ""),
                item("⟦☑️ you: 0 · agent: 1 · done: 1⟧ ", free, next_agent),
            ),
            (
                "tracked head gains a stateful token and Next row",
                item("", tracked, ""),
                item("⟦📥 you: 0 · agent: 1 · done: 1⟧ ", tracked, next_agent),
            ),
            (
                "tracked head: counts change and the Next row is rewritten",
                item("⟦📥 you: 1 · agent: 0 · done: 0⟧ ", tracked, next_you),
                item("⟦📥 you: 0 · agent: 1 · done: 1⟧ ", tracked, next_agent),
            ),
        ];
        for (label, before, after) in &cases {
            let baseline = run_doc(before);
            let current = run_doc(after);
            let watermark = seeded(&baseline, None);
            // Non-vacuity: the head is a live executable row on both sides,
            // so the projection change really reaches queue alignment.
            assert_eq!(watermark.acknowledged_queue.len(), 1, "{label}");
            assert_eq!(executable_queue_items(&current).len(), 1, "{label}");
            for mode in [ObserveMode::Boundary, ObserveMode::InTurn] {
                let obs = observe_with_mode(&watermark, &current, &quiet_ctx(&owned), mode);
                assert!(obs.ready.is_empty(), "{label} / {mode:?}: {:?}", obs.ready);
                assert_eq!(obs.pending, 0, "{label} / {mode:?}");
            }
        }
        // Positive control: the operator's first reply under a projected head
        // selects it (`#waitingselectboundary`).
        let unanswered = format!(
            "{}{next_you}{response}",
            head("🙋 ⟦you: 1 · agent: 0 · done: 0⟧ ", free)
        );
        let baseline = run_doc(&unanswered);
        let replied = run_doc(&format!("{unanswered}{reply}"));
        let obs = observe_with_mode(
            &seeded(&baseline, None),
            &replied,
            &quiet_ctx(&owned),
            ObserveMode::Boundary,
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].source, SteeringSource::Queue);
        assert!(obs.ready[0].verbatim.contains(free), "{:?}", obs.ready);
    }

    /// `#steerworks` item 3 / `#claimedsteerwake`: an edit of a claimed head is
    /// forwarded to its owner, never offered as a NEW subagent dispatch.
    #[test]
    fn edited_claimed_head_forwards_to_its_owner() {
        let owned = BTreeSet::new();
        let url = "https://github.com/btakita/agent-doc/issues/126";
        let baseline = doc(&format!("- #gh-fix {url}\n"), EX);
        let current = doc(
            &format!("- #gh-fix {url}: note this is in a Coder environment\n"),
            EX,
        );
        let mut obs = observe_with_mode(
            &seeded(&baseline, None),
            &current,
            &quiet_ctx(&owned),
            ObserveMode::Boundary,
        );
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].change, SteeringChange::Edited);
        apply_claim_owners(&mut obs.ready, |text| {
            text.contains("issues/126")
                .then(|| "subagent:ghfix126".to_string())
        });
        let item = &obs.ready[0];
        assert_eq!(item.dispatch, SteeringDispatch::ForwardToOwner);
        assert_eq!(item.owner.as_deref(), Some("subagent:ghfix126"));
        for text in [
            render_steering_context("bugs.md", &obs.ready, 0).unwrap(),
            render_closeout_steering_context("bugs.md", &obs.ready, 0).unwrap(),
        ] {
            assert!(
                text.contains(
                    "dispatch=forward_to_owner source=queue change=edited owner=subagent:ghfix126"
                ),
                "{text}"
            );
            assert!(text.contains("FORWARD the change"), "{text}");
            assert!(text.contains("#nestedoutwardauth"), "{text}");
            assert!(text.contains("not a chat reply"), "{text}");
            assert!(!text.contains("DISPATCH NOW"), "{text}");
            assert!(!text.contains("dispatch this item NOW"), "{text}");
        }
    }

    /// A delegated row can remain the selected/current watermark item after
    /// claim → spawn → start. Lifecycle/attempt projection and repeated edits
    /// must continue to route to that live worker, never become fresh dispatch.
    #[test]
    fn projected_current_item_claimed_by_subagent_forwards_repeated_edits() {
        let original = "do [#artifact_response_refs]: include any snippets";
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim(original, "subagent:artifact_response_refs", 100, 3600);
        let claim = ledger.claims[0].clone();
        let mut items = [
            SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Edited,
                dispatch: SteeringDispatch::Subagent,
                current_item: true,
                verbatim: "🚧 [#subagent:artifact_response_refs#1] do [#artifact_response_refs]: include any salient snippets".into(),
                previous: Some(original.into()),
                presets: Vec::new(),
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: None,
                queue_mode: None,
            },
            SteeringItem {
                source: SteeringSource::Queue,
                change: SteeringChange::Edited,
                dispatch: SteeringDispatch::Subagent,
                current_item: true,
                verbatim: "🚧 [#subagent:artifact_response_refs#1] do [#artifact_response_refs]: include any especially salient snippets".into(),
                previous: Some(
                    "🚧 [#subagent:artifact_response_refs#1] do [#artifact_response_refs]: include any salient snippets".into(),
                ),
                presets: Vec::new(),
                possibly_partial: false,
                owner: None,
                generation: None,
                worker_id: None,
                explicit: false,
                vector: None,
                queue_mode: None,
            },
        ];
        apply_claim_attempts(&mut items, |text| {
            (agent_doc_queue::queue_claim::claim_identity(text) == claim.identity)
                .then(|| claim.clone())
        });

        for item in &items {
            assert_eq!(item.dispatch, SteeringDispatch::ForwardToOwner);
            assert_eq!(
                item.owner.as_deref(),
                Some("subagent:artifact_response_refs")
            );
        }
    }

    /// `#staleownersteer`: the leaf-only operator answer remains durable when
    /// its bound worker already exited. The coordinator gets an exact
    /// generation-fenced requeue transition instead of a dead-end handoff.
    #[test]
    fn unavailable_owner_recovery_preserves_leafcontext_leaf_only_answer() {
        let item = SteeringItem {
            source: SteeringSource::Queue,
            change: SteeringChange::Edited,
            dispatch: SteeringDispatch::ForwardToOwner,
            verbatim: "Leaf-only answer".to_string(),
            previous: Some("Leaf-only question".to_string()),
            owner: Some("subagent:leafcontext".to_string()),
            generation: Some(7),
            worker_id: Some("/root/leafcontext".to_string()),
            current_item: false,
            possibly_partial: false,
            explicit: false,
            vector: None,
            presets: Vec::new(),
            queue_mode: None,
        };

        let rendered = render_steering_context("tasks/leaf context.md", &[item], 0).unwrap();
        assert!(rendered.contains("worker=/root/leafcontext"), "{rendered}");
        assert!(
            rendered.contains("the handoff did NOT happen"),
            "{rendered}"
        );
        assert!(
            rendered.contains("do not drop this operator answer/edit"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "agent-doc queue release 'tasks/leaf context.md' --item 'Leaf-only answer' --generation 7 --requeue"
            ),
            "{rendered}"
        );
        assert!(rendered.contains("fresh generation"), "{rendered}");
    }

    /// `#krwr`: a different row added in a claimed row's slot is a deletion
    /// plus new work. The owner is cancelled (`cancel_claimed`) and the new row
    /// keeps its own routing; it never inherits the claim as an edit. A same-id
    /// prose rewrite and a free-text text edit still forward to the owner.
    #[test]
    fn a_same_slot_replacement_cancels_the_owner_instead_of_forwarding() {
        let owned = BTreeSet::new();
        let cases = [
            ("do [#a]", "do [#b]", false),
            (
                "rewrite the deployment guide",
                "replace this with a wholly different request",
                false,
            ),
            (
                "do [#a]",
                "do [#a]: actually ship it as a migration instead",
                true,
            ),
            (
                "fix the login redirect bug",
                "fix the login redirect loop bug on Safari",
                true,
            ),
        ];
        for (before, after, edit) in cases {
            let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
            ledger.claim(before, "subagent:owner", 100, 3600);
            let claim = ledger.claims[0].clone();
            let baseline = doc(&format!("- current task\n- {before}\n- later work\n"), EX);
            let current = doc(&format!("- current task\n- {after}\n- later work\n"), EX);
            let watermark = seeded(&baseline, Some("current task"));
            let mut obs = observe_with_mode(
                &watermark,
                &current,
                &quiet_ctx(&owned),
                ObserveMode::Boundary,
            );
            // The ledger answers by claim identity; a replacement is never
            // followed, so only the old row's identity is claimed.
            let claim_of = |text: &str| {
                let identity = agent_doc_queue::queue_claim::claim_identity(text);
                let followed = if edit {
                    agent_doc_queue::queue_claim::claim_identity(after)
                } else {
                    claim.identity.clone()
                };
                (identity == claim.identity || identity == followed).then(|| claim.clone())
            };
            apply_claim_attempts(&mut obs.ready, claim_of);
            append_claimed_queue_removals(&mut obs.ready, &watermark, &current, claim_of);
            let summary: Vec<_> = obs
                .ready
                .iter()
                .map(|item| (item.change, item.dispatch, item.verbatim.as_str()))
                .collect();
            if edit {
                assert_eq!(
                    summary,
                    vec![(
                        SteeringChange::Edited,
                        SteeringDispatch::ForwardToOwner,
                        after
                    )],
                    "{before} -> {after}"
                );
                assert_eq!(obs.ready[0].previous.as_deref(), Some(before));
                assert_eq!(obs.ready[0].owner.as_deref(), Some("subagent:owner"));
            } else {
                assert!(
                    summary.contains(&(
                        SteeringChange::Deleted,
                        SteeringDispatch::CancelClaimed,
                        before
                    )),
                    "{before} -> {after}: {summary:?}"
                );
                let added = obs
                    .ready
                    .iter()
                    .find(|item| item.verbatim == after)
                    .unwrap_or_else(|| panic!("{before} -> {after}: {summary:?}"));
                assert_eq!(added.change, SteeringChange::Added, "{summary:?}");
                assert_eq!(added.dispatch, SteeringDispatch::DrainAfterCurrent);
                assert_eq!(added.owner, None);
                assert_eq!(added.previous, None);
                assert_eq!(summary.len(), 2, "{summary:?}");
                let cancelled = obs
                    .ready
                    .iter()
                    .find(|item| item.dispatch == SteeringDispatch::CancelClaimed)
                    .unwrap();
                assert_eq!(cancelled.owner.as_deref(), Some("subagent:owner"));
                assert_eq!(cancelled.generation, Some(claim.generation));
            }
        }
    }

    /// `#claimedsteerwake`: the next cycle's seed keeps an unsurfaced edit of
    /// a claimed head pending, so the woken cycle's first hook delivers it.
    #[test]
    fn new_cycle_seed_keeps_unsurfaced_claimed_edits_pending() {
        let owned = BTreeSet::new();
        let url = "https://github.com/btakita/agent-doc/issues/126";
        let old_line = format!("#gh-fix {url}");
        let new_line = format!("#gh-fix {url}: note this is in a Coder environment");
        let baseline = doc(&format!("- {new_line}\n- other work\n"), EX);
        let mut seed = SteeringWatermark::seed("cycle-2", &baseline, None, Vec::new());
        let prior = vec![old_line.clone(), "other work".to_string()];
        assert_eq!(
            carry_unsurfaced_claimed_edits(&mut seed, &prior, |text| text.contains("issues/126")),
            1
        );
        let obs = observe(&seed, &baseline, &quiet_ctx(&owned));
        assert_eq!(obs.ready.len(), 1, "{:?}", obs.ready);
        assert_eq!(obs.ready[0].previous.as_deref(), Some(old_line.as_str()));
        assert_eq!(obs.ready[0].verbatim, new_line);
        // An unclaimed edit is absorbed by the seed as before.
        let mut plain = SteeringWatermark::seed("cycle-2", &baseline, None, Vec::new());
        assert_eq!(
            carry_unsurfaced_claimed_edits(&mut plain, &prior, |_| false),
            0
        );
        assert!(
            observe(&plain, &baseline, &quiet_ctx(&owned))
                .ready
                .is_empty()
        );
    }

    /// `#barereplymarker`: generated packet rows carry only a bare trailing
    /// `▫️`. Refreshing them is not steering; an operator answer typed under
    /// the item selects that item, even when the answer also ends in the glyph.
    #[test]
    fn bare_glyph_reply_rows_are_not_steering_but_operator_answers_are() {
        let owned = BTreeSet::new();
        let packet = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#pick] Pick a port <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Pick a port. ▫️\n",
        );
        let baseline = doc(packet, EX);
        let refreshed = doc(
            &packet.replace(
                "    - **Response:**",
                "    - **Next:** you — answer: Which port? (evidence: Question) ▫️\n    - **Response:**",
            ),
            EX,
        );
        let obs = observe(&seeded(&baseline, None), &refreshed, &quiet_ctx(&owned));
        assert!(obs.ready.is_empty(), "{:?}", obs.ready);

        // The answer selects its waiting item, which surfaces as steering.
        let answered = doc(&format!("{packet}    - use 8080 ▫️\n"), EX);
        let obs = observe(&seeded(&baseline, None), &answered, &quiet_ctx(&owned));
        assert!(
            obs.ready.iter().any(|item| item.verbatim.contains("[#pick]")),
            "{:?}",
            obs.ready
        );
    }

    /// `#queueskipsubagents`: only rows whose dispatch is a subagent are
    /// exempt from `#queueskip` — every row of a `subagents` run except an
    /// `[inline]` opt-out, plus a `#subagents`-tagged row of an inline queue.
    /// Rows of an inline queue stay the inline turn's.
    #[test]
    fn subagent_routed_queue_head_ids_cover_subagents_blocks_and_tagged_rows() {
        let content = concat!(
            "# Session\n\n",
            "<!-- agent:queue id=\"inline\" -->\n",
            "- do [#plain]\n",
            "- #subagents do [#tagged]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:run id=\"main\" subagents go -->\n",
            "- do [#fanned]\n",
            "- \u{23ed}\u{fe0f} do [#decisiontsiftrecall]\n",
            "- [inline] do [#optout]\n",
            "<!-- /agent:run -->\n",
        );
        let presets = agent_doc_frontmatter::frontmatter::PromptPresets::default();
        let ids = subagent_routed_queue_head_ids(content, &presets);
        let expected: std::collections::HashSet<String> =
            ["tagged", "fanned", "decisiontsiftrecall"]
                .into_iter()
                .map(str::to_string)
                .collect();
        assert_eq!(ids, expected);

        let inline_only =
            "# S\n\n<!-- agent:queue go -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n";
        assert!(subagent_routed_queue_head_ids(inline_only, &presets).is_empty());
    }
}
