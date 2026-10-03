//! Steering completion-gate decision log (`#steergatelog`).
//!
//! Every settle decision the steering gate makes
//! (`agent_doc_debounce::edit_settle`, read by
//! `agent_doc_document_realtime::midturn_steering::observe_with_mode`) becomes a
//! feature row in `state.db`, and outcomes observed later label it:
//!
//! - **premature**: delivered, then the operator re-edited the same item within
//!   the label window (default 45s, `agent_doc_steering_label_window_ms`);
//! - **late**: the gate held the item past half the quiet window and the
//!   operator made no further edit before it settled (the hold bought nothing);
//! - **on_time**: everything else that resolved.
//!
//! # Shape (`#lazily-reactive-first`, `#lzdurablesink`)
//!
//! - **Source**: one [`GateLogEvent`] stream folded by [`advance`] into
//!   [`GateLogTracking`]: the decisions each consuming observation made, plus a
//!   cold [`GateLogEvent::Hydrated`] load of the document's recent rows the
//!   first time this process sees the document.
//! - **Computed**: the rows and pause profile whose revision changed
//!   ([`SteeringGateLog::pending_writes`]), and the operator's rolling median
//!   pause, which the next observation reads as a feature.
//! - **Effect**: the durable sink. Each changed row goes through a
//!   `lazily::LatestDurableProjection` (latest value per row key, one write in
//!   flight, `durable_through` per key) into
//!   `agent_doc_sqlite::steering_gate_log::merge_gate_row`. The merge is
//!   monotone, so the sink never reads storage to decide anything, and two
//!   processes that record the same decision converge on one row.
//!
//! The graph lives in a per-thread, per-project [`LocalProcessScope`]: a
//! one-shot hook process and the long-lived `steering --follow` loop share the
//! same code path, and a long-lived process hydrates each document once.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_doc_debounce::edit_settle::{GateFeatures, SettleDecision, SettleTier};
use agent_doc_debounce::learned_gate::{DEFAULT_LEARNING_RATE, GateWeights, LearnedGate};
use agent_doc_document_realtime::midturn_steering::{GateDecision, SteeringChange};
use agent_doc_sqlite::steering_gate_log::{self as store, StoredGateRow};
use agent_doc_state_scope::LocalProcessScope;
use anyhow::{Context, Result};
use lazily::{Computed, Effect, LatestDurableProjection, StateMachine};
use serde::{Deserialize, Serialize};

/// Default window after a delivery in which a re-edit marks it premature.
pub const DEFAULT_LABEL_WINDOW_MS: u64 = 45_000;
/// Pauses kept for the rolling median.
const PAUSE_SAMPLES: usize = 41;
/// Gaps longer than this are the operator stepping away, not a typing pause.
const MAX_PAUSE_MS: u64 = 60_000;
/// Gaps shorter than this are one save, not two edits.
const MIN_PAUSE_MS: u64 = 50;
/// Projection generation (the sink has one writer per process).
const SINK_GENERATION: u64 = 1;

/// Where a decision row sits in an item version's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatePhase {
    /// Held before half the quiet window elapsed.
    HeldEarly,
    /// Held after half the quiet window elapsed (the hold that can be late).
    HeldPastHalf,
    /// Delivered to the agent.
    Delivered,
}

impl GatePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HeldEarly => "held_early",
            Self::HeldPastHalf => "held_past_half",
            Self::Delivered => "delivered",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "held_early" => Some(Self::HeldEarly),
            "held_past_half" => Some(Self::HeldPastHalf),
            "delivered" => Some(Self::Delivered),
            _ => None,
        }
    }
}

/// The outcome label attached once the facts decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateLabel {
    Premature,
    Late,
    OnTime,
}

impl GateLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Premature => "premature",
            Self::Late => "late",
            Self::OnTime => "on_time",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "premature" => Some(Self::Premature),
            "late" => Some(Self::Late),
            "on_time" => Some(Self::OnTime),
            _ => None,
        }
    }
}

/// One dataset row: the decision as made, plus the outcome facts observed
/// since. This is the export schema of `agent-doc steering dataset --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRow {
    /// SQLite id, once written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// `consumer|document|change|text_hash|phase`.
    pub row_key: String,
    pub document: String,
    /// The steering consumer that made the decision (`hook`, `cli`, `follow`).
    pub consumer: String,
    /// The document's configured harness (frontmatter `agent:`).
    pub harness: String,
    /// The operator the decision was made for.
    pub operator: String,
    pub phase: GatePhase,
    pub change: SteeringChange,
    /// Hash of the item's normalized text (this version's identity).
    pub text_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_text_hash: Option<String>,
    /// The end of the item's text at this decision (`#steergatenamo`), so an
    /// offline text classifier can be scored on the real pause points
    /// (`scripts/steergate-model-eval --dataset`). Absent on rows logged
    /// before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_tail: Option<String>,
    pub decided_at_ms: u64,
    /// The gate's verdict: `settled`, `max_hold_expired`, or `held`.
    pub decision: String,
    /// Which deterministic tier fired.
    #[serde(with = "tier_codec")]
    pub tier: SettleTier,
    /// Observed at/after the turn boundary.
    #[serde(default)]
    pub boundary: bool,
    #[serde(with = "features_codec")]
    pub features: GateFeatures,
    // Outcome facts (monotone; merged into their own columns).
    #[serde(default)]
    pub delivered_at_ms: Option<u64>,
    #[serde(default)]
    pub superseded_at_ms: Option<u64>,
    #[serde(default)]
    pub re_edited_at_ms: Option<u64>,
    #[serde(default)]
    pub label: Option<GateLabel>,
    /// The learned gate's probability (in thousandths) that the item was
    /// finished, from the weights in force for this decision
    /// (`#steergateperceptron`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_probability_milli: Option<u32>,
    /// The learned gate's zone for this decision: `send`, `hold`, `defer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_zone: Option<String>,
    /// The learned gate was enabled (its verdict applied) for this decision.
    #[serde(default)]
    pub learned_gate: bool,
}

impl GateRow {
    fn version_matches(&self, other: &GateRow) -> bool {
        self.document == other.document
            && self.consumer == other.consumer
            && self.change == other.change
            && self.text_hash == other.text_hash
    }

    fn to_stored(&self) -> Result<StoredGateRow> {
        Ok(StoredGateRow {
            id: self.id,
            row_key: self.row_key.clone(),
            document: self.document.clone(),
            consumer: self.consumer.clone(),
            phase: self.phase.as_str().to_string(),
            decided_at_ms: self.decided_at_ms,
            row_json: serde_json::to_string(self).context("serialize steering gate row")?,
            delivered_at_ms: self.delivered_at_ms,
            superseded_at_ms: self.superseded_at_ms,
            re_edited_at_ms: self.re_edited_at_ms,
            label: self.label.map(|label| label.as_str().to_string()),
        })
    }

    /// Rebuild a row from storage: the decision from `row_json`, the facts
    /// from their merged columns.
    pub fn from_stored(stored: &StoredGateRow) -> Result<Self> {
        let mut row: GateRow =
            serde_json::from_str(&stored.row_json).context("parse steering gate row")?;
        row.id = stored.id;
        row.phase = GatePhase::parse(&stored.phase).unwrap_or(row.phase);
        row.delivered_at_ms = stored.delivered_at_ms.or(row.delivered_at_ms);
        row.superseded_at_ms = stored.superseded_at_ms.or(row.superseded_at_ms);
        row.re_edited_at_ms = stored.re_edited_at_ms.or(row.re_edited_at_ms);
        row.label = stored
            .label
            .as_deref()
            .and_then(GateLabel::parse)
            .or(row.label);
        Ok(row)
    }
}

/// Serialized form of [`GateFeatures`] (`agent-doc-debounce` stays
/// dependency-free, so the codec lives with the log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureRecord {
    pub trailing_token: String,
    pub signal: String,
    pub unbalanced_delimiters: bool,
    pub closed_list_item: bool,
    pub quiet_ms: u64,
    #[serde(default)]
    pub median_pause_ms: Option<u64>,
    #[serde(default)]
    pub typing_chars_per_min: Option<u32>,
    pub item_age_ms: u64,
    pub component: String,
    pub debounce_ms: u64,
    pub max_hold_ms: u64,
}

impl From<&GateFeatures> for FeatureRecord {
    fn from(f: &GateFeatures) -> Self {
        Self {
            trailing_token: f.trailing_token.as_str().to_string(),
            signal: f.signal.as_str().to_string(),
            unbalanced_delimiters: f.unbalanced_delimiters,
            closed_list_item: f.closed_list_item,
            quiet_ms: f.quiet_ms,
            median_pause_ms: f.median_pause_ms,
            typing_chars_per_min: f.typing_chars_per_min,
            item_age_ms: f.item_age_ms,
            component: f.component.as_str().to_string(),
            debounce_ms: f.debounce_ms,
            max_hold_ms: f.max_hold_ms,
        }
    }
}

impl TryFrom<FeatureRecord> for GateFeatures {
    type Error = String;

    fn try_from(r: FeatureRecord) -> std::result::Result<Self, String> {
        use agent_doc_debounce::edit_settle::{CompletionSignal, GateComponent, TrailingToken};
        Ok(Self {
            trailing_token: TrailingToken::parse(&r.trailing_token)
                .ok_or_else(|| format!("unknown trailing_token {:?}", r.trailing_token))?,
            signal: CompletionSignal::parse(&r.signal)
                .ok_or_else(|| format!("unknown signal {:?}", r.signal))?,
            unbalanced_delimiters: r.unbalanced_delimiters,
            closed_list_item: r.closed_list_item,
            quiet_ms: r.quiet_ms,
            median_pause_ms: r.median_pause_ms,
            typing_chars_per_min: r.typing_chars_per_min,
            item_age_ms: r.item_age_ms,
            component: GateComponent::parse(&r.component)
                .ok_or_else(|| format!("unknown component {:?}", r.component))?,
            debounce_ms: r.debounce_ms,
            max_hold_ms: r.max_hold_ms,
        })
    }
}

mod features_codec {
    use super::{FeatureRecord, GateFeatures};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(f: &GateFeatures, s: S) -> Result<S::Ok, S::Error> {
        FeatureRecord::from(f).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<GateFeatures, D::Error> {
        GateFeatures::try_from(FeatureRecord::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

mod tier_codec {
    use super::SettleTier;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(tier: &SettleTier, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(tier.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SettleTier, D::Error> {
        let text = String::deserialize(d)?;
        SettleTier::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown tier {text:?}")))
    }
}

/// The label the facts decide for `row`, `None` while still open.
///
/// `held_past_half` says whether the same version of the item was held past
/// half the quiet window (by the same consumer) before it was delivered.
pub fn derive_label(
    row: &GateRow,
    held_past_half: bool,
    now_ms: u64,
    window_ms: u64,
) -> Option<GateLabel> {
    match row.phase {
        GatePhase::Delivered => {
            if row
                .re_edited_at_ms
                .is_some_and(|at| at.saturating_sub(row.decided_at_ms) <= window_ms)
            {
                Some(GateLabel::Premature)
            } else if now_ms.saturating_sub(row.decided_at_ms) >= window_ms
                || row.re_edited_at_ms.is_some()
            {
                Some(if held_past_half {
                    GateLabel::Late
                } else {
                    GateLabel::OnTime
                })
            } else {
                None
            }
        }
        // Delivered later with no further edit: the hold bought nothing.
        GatePhase::HeldPastHalf if row.delivered_at_ms.is_some() => Some(GateLabel::Late),
        // Re-edited or removed while held: holding was right.
        GatePhase::HeldPastHalf if row.superseded_at_ms.is_some() => Some(GateLabel::OnTime),
        GatePhase::HeldEarly if row.delivered_at_ms.is_some() || row.superseded_at_ms.is_some() => {
            Some(GateLabel::OnTime)
        }
        _ => None,
    }
}

/// The operator's recent pauses between observed document edits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PauseProfile {
    #[serde(default)]
    pub last_change_ms: Option<u64>,
    #[serde(default)]
    pub pauses_ms: Vec<u64>,
}

impl PauseProfile {
    /// Rolling median pause, once a few pauses are known.
    pub fn median_ms(&self) -> Option<u64> {
        if self.pauses_ms.len() < 3 {
            return None;
        }
        let mut sorted = self.pauses_ms.clone();
        sorted.sort_unstable();
        Some(sorted[sorted.len() / 2])
    }

    fn observe_change(&mut self, changed_ms: u64) -> bool {
        match self.last_change_ms {
            Some(previous) if changed_ms <= previous => false,
            Some(previous) => {
                let pause = changed_ms - previous;
                self.last_change_ms = Some(changed_ms);
                if (MIN_PAUSE_MS..=MAX_PAUSE_MS).contains(&pause) {
                    self.pauses_ms.push(pause);
                    if self.pauses_ms.len() > PAUSE_SAMPLES {
                        self.pauses_ms.remove(0);
                    }
                }
                true
            }
            None => {
                self.last_change_ms = Some(changed_ms);
                true
            }
        }
    }
}

/// One consuming observation's decisions, as fed to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateObservation {
    pub document: String,
    pub consumer: String,
    pub harness: String,
    pub operator: String,
    pub now_ms: u64,
    pub document_changed_ms: Option<u64>,
    pub boundary: bool,
    /// The learned gate's verdicts applied to these decisions.
    pub learned_gate: bool,
    pub decisions: Vec<GateDecision>,
}

/// Input to the decision-log graph.
#[derive(Debug, Clone, PartialEq)]
pub enum GateLogEvent {
    /// Cold hydration: a document's recent rows and the pause profile, loaded
    /// once per process before its first observation is folded.
    Hydrated {
        document: String,
        rows: Vec<GateRow>,
        profile: Option<PauseProfile>,
        weights: Option<GateWeights>,
    },
    Observed(GateObservation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedRow {
    pub row: GateRow,
    /// Epoch of the last change; 0 for a row hydrated unchanged.
    pub revision: u64,
}

/// Everything the event stream folds to.
#[derive(Debug, Clone, PartialEq)]
pub struct GateLogTracking {
    pub label_window_ms: u64,
    pub epoch: u64,
    pub hydrated_documents: BTreeSet<String>,
    pub rows: BTreeMap<String, TrackedRow>,
    pub profile: PauseProfile,
    pub profile_revision: u64,
    /// The operator's learned gate weights (`#steergateperceptron`), trained
    /// online as labels attach.
    pub weights: GateWeights,
    pub weights_revision: u64,
    pub learning_rate: f64,
}

impl GateLogTracking {
    pub fn new(label_window_ms: u64) -> Self {
        Self {
            label_window_ms,
            epoch: 0,
            hydrated_documents: BTreeSet::new(),
            rows: BTreeMap::new(),
            profile: PauseProfile::default(),
            profile_revision: 0,
            weights: GateWeights::seeded(),
            weights_revision: 0,
            learning_rate: DEFAULT_LEARNING_RATE,
        }
    }
}

/// What a labelled row teaches the learned gate (`#steergateperceptron`):
/// 1 = the operator had finished (send), 0 = still typing (hold). `None`
/// when the row is no evidence either way.
pub fn training_target(row: &GateRow) -> Option<f64> {
    match (row.phase, row.label?) {
        (GatePhase::Delivered, GateLabel::Premature) => Some(0.0),
        (GatePhase::Delivered, GateLabel::OnTime | GateLabel::Late) => Some(1.0),
        (GatePhase::HeldPastHalf, GateLabel::Late) => Some(1.0),
        (GatePhase::HeldPastHalf | GatePhase::HeldEarly, GateLabel::OnTime)
            if row.superseded_at_ms.is_some() =>
        {
            Some(0.0)
        }
        _ => None,
    }
}

fn row_key(
    consumer: &str,
    document: &str,
    change: SteeringChange,
    hash: &str,
    phase: GatePhase,
) -> String {
    let change = match change {
        SteeringChange::Added => "added",
        SteeringChange::Edited => "edited",
        SteeringChange::Deleted => "deleted",
    };
    format!("{consumer}|{document}|{change}|{hash}|{}", phase.as_str())
}

fn decision_name(decision: SettleDecision) -> &'static str {
    match decision {
        SettleDecision::Settled => "settled",
        SettleDecision::MaxHoldExpired => "max_hold_expired",
        SettleDecision::Held { .. } => "held",
    }
}

/// The fold. Total and pure.
pub fn advance(current: &GateLogTracking, event: &GateLogEvent) -> Option<GateLogTracking> {
    let mut next = current.clone();
    match event {
        GateLogEvent::Hydrated {
            document,
            rows,
            profile,
            weights,
        } => {
            next.hydrated_documents.insert(document.clone());
            for row in rows {
                next.rows
                    .entry(row.row_key.clone())
                    .or_insert_with(|| TrackedRow {
                        row: row.clone(),
                        revision: 0,
                    });
            }
            if let Some(profile) = profile
                && next.profile_revision == 0
            {
                next.profile = profile.clone();
            }
            if let Some(weights) = weights
                && next.weights_revision == 0
            {
                next.weights = weights.clone().normalized();
            }
        }
        GateLogEvent::Observed(observation) => fold_observation(&mut next, observation),
    }
    Some(next)
}

fn fold_observation(next: &mut GateLogTracking, obs: &GateObservation) {
    let mut epoch = next.epoch;
    let mut bump = || {
        epoch += 1;
        epoch
    };
    if let Some(changed) = obs.document_changed_ms
        && next.profile.observe_change(changed)
    {
        next.profile_revision = bump();
    }

    let live: BTreeSet<&str> = obs
        .decisions
        .iter()
        .map(|decision| decision.text_hash.as_str())
        .collect();
    for decision in &obs.decisions {
        let phase = match decision.decision {
            SettleDecision::Held { .. }
                if decision.features.quiet_ms >= decision.features.debounce_ms / 2 =>
            {
                GatePhase::HeldPastHalf
            }
            SettleDecision::Held { .. } => GatePhase::HeldEarly,
            SettleDecision::Settled | SettleDecision::MaxHoldExpired => GatePhase::Delivered,
        };
        let key = row_key(
            &obs.consumer,
            &obs.document,
            decision.change,
            &decision.text_hash,
            phase,
        );
        if !next.rows.contains_key(&key) {
            let row = GateRow {
                id: None,
                row_key: key.clone(),
                document: obs.document.clone(),
                consumer: obs.consumer.clone(),
                harness: obs.harness.clone(),
                operator: obs.operator.clone(),
                phase,
                change: decision.change,
                text_hash: decision.text_hash.clone(),
                previous_text_hash: decision.previous_text_hash.clone(),
                text_tail: Some(decision.text_tail.clone()),
                decided_at_ms: obs.now_ms,
                decision: decision_name(decision.decision).to_string(),
                tier: decision.tier,
                boundary: obs.boundary,
                features: decision.features,
                delivered_at_ms: (phase == GatePhase::Delivered).then_some(obs.now_ms),
                superseded_at_ms: None,
                re_edited_at_ms: None,
                label: None,
                model_probability_milli: Some(
                    (next.weights.probability(&decision.features) * 1000.0).round() as u32,
                ),
                model_zone: Some(next.weights.zone(&decision.features).as_str().to_string()),
                learned_gate: obs.learned_gate,
            };
            next.rows.insert(
                key.clone(),
                TrackedRow {
                    row,
                    revision: bump(),
                },
            );
        }
        if phase == GatePhase::Delivered {
            let delivered = next.rows[&key].row.clone();
            for tracked in next.rows.values_mut() {
                let row = &mut tracked.row;
                if row.phase != GatePhase::Delivered
                    && row.version_matches(&delivered)
                    && row.delivered_at_ms.is_none()
                    && row.superseded_at_ms.is_none()
                {
                    row.delivered_at_ms = Some(obs.now_ms);
                    tracked.revision = bump();
                }
            }
        }
        // An edit of a delivered version is the re-edit fact.
        if let Some(previous) = &decision.previous_text_hash
            && decision.change == SteeringChange::Edited
        {
            let edited_at = obs.document_changed_ms.unwrap_or(obs.now_ms);
            for tracked in next.rows.values_mut() {
                let row = &mut tracked.row;
                if row.phase == GatePhase::Delivered
                    && row.document == obs.document
                    && row.consumer == obs.consumer
                    && &row.text_hash == previous
                    && row.re_edited_at_ms.is_none()
                {
                    row.re_edited_at_ms = Some(edited_at.max(row.decided_at_ms));
                    tracked.revision = bump();
                }
            }
        }
    }

    // A held version that is no longer a candidate was re-edited or removed.
    for tracked in next.rows.values_mut() {
        let row = &mut tracked.row;
        if row.phase != GatePhase::Delivered
            && row.document == obs.document
            && row.consumer == obs.consumer
            && row.delivered_at_ms.is_none()
            && row.superseded_at_ms.is_none()
            && !live.contains(row.text_hash.as_str())
        {
            row.superseded_at_ms = Some(obs.now_ms);
            tracked.revision = bump();
        }
    }

    // Attach every label the facts now decide.
    let held_past_half: BTreeSet<(String, String, String)> = next
        .rows
        .values()
        .filter(|tracked| tracked.row.phase == GatePhase::HeldPastHalf)
        .map(|tracked| {
            (
                tracked.row.consumer.clone(),
                tracked.row.document.clone(),
                tracked.row.text_hash.clone(),
            )
        })
        .collect();
    for tracked in next.rows.values_mut() {
        if tracked.row.label.is_some() || tracked.row.document != obs.document {
            continue;
        }
        let row = &tracked.row;
        let past_half = held_past_half.contains(&(
            row.consumer.clone(),
            row.document.clone(),
            row.text_hash.clone(),
        ));
        if let Some(label) = derive_label(row, past_half, obs.now_ms, next.label_window_ms) {
            tracked.row.label = Some(label);
            tracked.revision = bump();
            // Online learning: each label trains the gate once, in the fold
            // that attaches it.
            if let Some(target) = training_target(&tracked.row) {
                next.weights
                    .update(&tracked.row.features, target, next.learning_rate);
                next.weights_revision = bump();
            }
        }
    }
    next.epoch = epoch;
}

/// A write the sink owes storage.
#[derive(Debug, Clone, PartialEq)]
pub enum SinkWrite {
    Row(Box<GateRow>),
    Profile {
        operator: String,
        profile: PauseProfile,
    },
    Weights {
        operator: String,
        weights: GateWeights,
    },
}

/// Serialized form of [`GateWeights`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightsRecord {
    pub weights: Vec<f64>,
    #[serde(default)]
    pub updates: u64,
}

impl From<&GateWeights> for WeightsRecord {
    fn from(w: &GateWeights) -> Self {
        Self {
            weights: w.weights.clone(),
            updates: w.updates,
        }
    }
}

impl From<WeightsRecord> for GateWeights {
    fn from(r: WeightsRecord) -> Self {
        GateWeights {
            weights: r.weights,
            updates: r.updates,
        }
        .normalized()
    }
}

fn model_state_key(operator: &str) -> String {
    format!("steering_gate_model:{operator}")
}

/// The persisted weights for `operator` in `root`, else the seed.
pub fn load_weights(root: &Path, operator: &str) -> Result<GateWeights> {
    if !agent_doc_sqlite::state_store::state_db_path(root).exists() {
        return Ok(GateWeights::seeded());
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    load_weights_from(&conn, operator)
}

fn load_weights_from(
    conn: &agent_doc_sqlite::state_store::Connection,
    operator: &str,
) -> Result<GateWeights> {
    Ok(
        agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
            conn,
            &model_state_key(operator),
        )?
        .and_then(|raw| serde_json::from_str::<WeightsRecord>(&raw).ok())
        .map(GateWeights::from)
        .unwrap_or_else(GateWeights::seeded),
    )
}

fn profile_state_key(operator: &str) -> String {
    format!("steering_gate_profile:{operator}")
}

/// The live decision-log graph for one project.
pub struct SteeringGateLog {
    scope: LocalProcessScope,
    machine: StateMachine<GateLogTracking, GateLogEvent>,
    pending_writes: Computed<Vec<(String, u64, SinkWrite)>>,
    median_pause: Computed<Option<u64>>,
    weights: Computed<GateWeights>,
    projection: LatestDurableProjection<String, SinkWrite>,
    _sink: Effect,
}

impl SteeringGateLog {
    /// Build the graph in its own process scope, sinking into `root`'s
    /// `state.db` (`#stategraphjoin`: the scope is the process lifetime).
    pub fn new(root: &Path, operator: &str, label_window_ms: u64) -> Self {
        Self::new_in(LocalProcessScope::new(), root, operator, label_window_ms)
    }

    pub fn new_in(
        scope: LocalProcessScope,
        root: &Path,
        operator: &str,
        label_window_ms: u64,
    ) -> Self {
        let machine =
            StateMachine::new(scope.ctx(), GateLogTracking::new(label_window_ms), advance);
        let state = machine.state_handle();
        let operator = operator.to_string();
        let pending_operator = operator.clone();
        let pending_writes = scope.ctx().computed(move |ctx| {
            let tracking = ctx.get(&state);
            let mut writes: Vec<(String, u64, SinkWrite)> = tracking
                .rows
                .iter()
                .filter(|(_, tracked)| tracked.revision > 0)
                .map(|(key, tracked)| {
                    (
                        key.clone(),
                        tracked.revision,
                        SinkWrite::Row(Box::new(tracked.row.clone())),
                    )
                })
                .collect();
            if tracking.profile_revision > 0 {
                writes.push((
                    profile_state_key(&pending_operator),
                    tracking.profile_revision,
                    SinkWrite::Profile {
                        operator: pending_operator.clone(),
                        profile: tracking.profile.clone(),
                    },
                ));
            }
            if tracking.weights_revision > 0 {
                writes.push((
                    model_state_key(&pending_operator),
                    tracking.weights_revision,
                    SinkWrite::Weights {
                        operator: pending_operator.clone(),
                        weights: tracking.weights.clone(),
                    },
                ));
            }
            writes
        });
        let median_pause = scope
            .ctx()
            .computed(move |ctx| ctx.get(&state).profile.median_ms());
        let weights = scope.ctx().computed(move |ctx| ctx.get(&state).weights);
        let projection = LatestDurableProjection::new(scope.ctx(), SINK_GENERATION);
        let sink = {
            let projection = projection.clone();
            let root = root.to_path_buf();
            scope.ctx().effect(move |ctx| {
                let writes = ctx.get(&pending_writes);
                sink_pending(ctx.untracked(), &projection, &root, &writes);
            })
        };
        Self {
            scope,
            machine,
            pending_writes,
            median_pause,
            weights,
            projection,
            _sink: sink,
        }
    }

    pub fn send(&self, event: GateLogEvent) {
        self.machine.send(self.scope.ctx(), event);
    }

    pub fn tracking(&self) -> GateLogTracking {
        self.machine.state(self.scope.ctx())
    }

    /// The rows and profile whose latest revision is not yet known durable.
    pub fn pending_writes(&self) -> Vec<(String, u64, SinkWrite)> {
        self.scope
            .ctx()
            .get(&self.pending_writes)
            .into_iter()
            .filter(|(key, epoch, _)| {
                self.projection
                    .durable_through(key)
                    .is_none_or(|durable| durable < *epoch)
            })
            .collect()
    }

    /// The operator's rolling median pause (a feature of the next decision).
    pub fn median_pause_ms(&self) -> Option<u64> {
        self.scope.ctx().get(&self.median_pause)
    }

    /// The learned gate weights the next decision uses.
    pub fn weights(&self) -> GateWeights {
        self.scope.ctx().get(&self.weights)
    }

    fn hydrated(&self, document: &str) -> bool {
        self.tracking().hydrated_documents.contains(document)
    }
}

fn sink_pending(
    ctx: &lazily::Context,
    projection: &LatestDurableProjection<String, SinkWrite>,
    root: &Path,
    writes: &[(String, u64, SinkWrite)],
) {
    let mut conn: Option<agent_doc_sqlite::state_store::Connection> = None;
    let mut wrote_row = false;
    for (key, epoch, write) in writes {
        match projection.upsert_desired(ctx, key.clone(), *epoch, write.clone()) {
            lazily::LatestDurableUpsert::Accepted | lazily::LatestDurableUpsert::Unchanged => {}
            _ => continue,
        }
        let lazily::LatestDurableClaim::Claimed(envelope) =
            projection.claim(ctx, key, SINK_GENERATION)
        else {
            continue;
        };
        let outcome = (|| -> Result<()> {
            if conn.is_none() {
                conn = Some(agent_doc_sqlite::state_store::open_state_db(root)?);
            }
            let conn = conn.as_ref().expect("connection opened above");
            match &envelope.value {
                SinkWrite::Row(row) => {
                    store::merge_gate_row(conn, &row.to_stored()?)?;
                    wrote_row = true;
                }
                SinkWrite::Profile { operator, profile } => {
                    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
                        conn,
                        &profile_state_key(operator),
                        &serde_json::to_string(profile)?,
                        profile.last_change_ms.unwrap_or(0),
                    )?;
                }
                SinkWrite::Weights { operator, weights } => {
                    store::upsert_gate_model(
                        conn,
                        &model_state_key(operator),
                        &serde_json::to_string(&WeightsRecord::from(weights))?,
                        weights.updates,
                    )?;
                }
            }
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                projection.ack_applied(ctx, key, envelope.generation, envelope.epoch);
            }
            Err(err) => {
                // The write stays pending in live state and is retried on the
                // next change (`#lzdurablesink`); the gate never waits on it.
                projection.fail_retryable(ctx, key, envelope.generation, envelope.epoch);
                eprintln!("[agent-doc] steering gate log write deferred for {key}: {err:#}");
            }
        }
    }
    if wrote_row
        && let Some(conn) = &conn
        && let Err(err) = store::prune_gate_rows(conn, store::STEERING_GATE_LOG_MAX_ROWS)
    {
        eprintln!("[agent-doc] steering gate log retention failed: {err:#}");
    }
}

/// Most unlabelled rows a process hydrates per document, whatever their age.
const HYDRATE_UNLABELLED_MAX: usize = 500;

/// How far back a process hydrates a document's recent rows. Unlabelled rows
/// load regardless (`HYDRATE_UNLABELLED_MAX`): the next observation of the
/// document can come long after this horizon.
fn hydration_horizon_ms(label_window_ms: u64, max_hold_ms: u64) -> u64 {
    label_window_ms
        .saturating_add(max_hold_ms)
        .saturating_add(5 * 60_000)
}

/// Cold hydration (`#lzdurablesink`): load what this process needs before it
/// folds its first observation of `document`.
fn hydrate(
    log: &SteeringGateLog,
    root: &Path,
    document: &str,
    operator: &str,
    now_ms: u64,
    max_hold_ms: u64,
) -> Result<()> {
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    let since = now_ms.saturating_sub(hydration_horizon_ms(
        log.tracking().label_window_ms,
        max_hold_ms,
    ));
    // Recent rows (re-edit detection, the pause profile) plus every row still
    // owed a label, whatever its age: with only the recent horizon, a delivery
    // whose next observation was the closeout write was never labelled, so the
    // learned gate never trained.
    let mut stored = store::load_gate_rows_since(&conn, document, since)?;
    let recent: BTreeSet<String> = stored.iter().map(|row| row.row_key.clone()).collect();
    stored.extend(
        store::load_unlabelled_gate_rows(&conn, document, HYDRATE_UNLABELLED_MAX)?
            .into_iter()
            .filter(|row| !recent.contains(&row.row_key)),
    );
    let rows = stored
        .iter()
        .filter_map(|stored| GateRow::from_stored(stored).ok())
        .collect();
    let profile = agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        &profile_state_key(operator),
    )?
    .and_then(|raw| serde_json::from_str(&raw).ok());
    let weights = Some(load_weights_from(&conn, operator)?);
    log.send(GateLogEvent::Hydrated {
        document: document.to_string(),
        rows,
        profile,
        weights,
    });
    Ok(())
}

thread_local! {
    static GATE_LOGS: RefCell<BTreeMap<PathBuf, Rc<SteeringGateLog>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// The decision log's document identity: the path relative to the project
/// root, so every consumer's spelling of the path names one document.
pub fn document_key(root: &Path, file: &Path) -> String {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    canonical
        .strip_prefix(&root)
        .unwrap_or(&canonical)
        .display()
        .to_string()
}

/// Who the decisions are made for. Per-operator state (the pause profile)
/// is keyed by this.
pub fn operator_id() -> String {
    ["AGENT_DOC_OPERATOR", "USER", "USERNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "operator".to_string())
}

/// The label window: project config, else the default.
pub fn label_window_ms_for(file: &Path) -> u64 {
    agent_doc_project_config_io::load_project_for_doc(file)
        .agent_doc_steering_label_window_ms
        .unwrap_or(DEFAULT_LABEL_WINDOW_MS)
}

/// This thread's live log for `root`, hydrated for `document`.
pub fn live_log(
    root: &Path,
    file: &Path,
    document: &str,
    now_ms: u64,
    max_hold_ms: u64,
) -> Result<Rc<SteeringGateLog>> {
    let operator = operator_id();
    let log = GATE_LOGS.with(|logs| {
        logs.borrow_mut()
            .entry(root.to_path_buf())
            .or_insert_with(|| {
                Rc::new(SteeringGateLog::new(
                    root,
                    &operator,
                    label_window_ms_for(file),
                ))
            })
            .clone()
    });
    if !log.hydrated(document) {
        hydrate(&log, root, document, &operator, now_ms, max_hold_ms)?;
    }
    Ok(log)
}

/// Fold one consuming observation into the live log; the sink persists it.
pub fn record(
    root: &Path,
    file: &Path,
    observation: GateObservation,
    max_hold_ms: u64,
) -> Result<()> {
    let log = live_log(
        root,
        file,
        &observation.document,
        observation.now_ms,
        max_hold_ms,
    )?;
    log.send(GateLogEvent::Observed(observation));
    Ok(())
}

/// The operator's median pause for the next decision, `None` when unknown
/// or when the log cannot be read (the gate never waits on the log).
pub fn median_pause_ms(
    root: &Path,
    file: &Path,
    document: &str,
    now_ms: u64,
    max_hold_ms: u64,
) -> Option<u64> {
    live_log(root, file, document, now_ms, max_hold_ms)
        .ok()
        .and_then(|log| log.median_pause_ms())
}

/// Whether the learned gate is on for `file`'s project
/// (`agent_doc_steering_learned_gate`, default on: the seeded weights are
/// proven to decide exactly like the deterministic gate).
pub fn learned_gate_enabled(file: &Path) -> bool {
    agent_doc_project_config_io::load_project_for_doc(file)
        .agent_doc_steering_learned_gate
        .unwrap_or(true)
}

/// The learned classifier for the next decision, `None` when disabled or
/// when the log cannot be read (the deterministic gate always works).
pub fn learned_classifier(
    root: &Path,
    file: &Path,
    document: &str,
    now_ms: u64,
    max_hold_ms: u64,
) -> Option<LearnedGate> {
    if !learned_gate_enabled(file) {
        return None;
    }
    live_log(root, file, document, now_ms, max_hold_ms)
        .ok()
        .map(|log| LearnedGate::new(log.weights()))
}

/// The most recent logged decision (optionally for one document).
pub fn latest_row(root: &Path, document: Option<&str>) -> Result<Option<GateRow>> {
    if !agent_doc_sqlite::state_store::state_db_path(root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    store::latest_gate_row(&conn, document)?
        .map(|stored| GateRow::from_stored(&stored))
        .transpose()
}

/// One exported dataset row: the stored row with its label resolved as of
/// the export.
#[derive(Debug, Clone, Serialize)]
pub struct DatasetRow {
    #[serde(flatten)]
    pub row: GateRow,
    /// `stored` when the live log attached it, `derived` when the facts
    /// decide it as of this export, absent while the outcome is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_source: Option<&'static str>,
}

/// Export the dataset (`agent-doc steering dataset --json`).
pub fn export_dataset(
    root: &Path,
    document: Option<&str>,
    now_ms: u64,
    label_window_ms: u64,
) -> Result<Vec<DatasetRow>> {
    if !agent_doc_sqlite::state_store::state_db_path(root).exists() {
        return Ok(Vec::new());
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    let mut rows = Vec::new();
    let mut after = 0;
    loop {
        let page = store::list_gate_rows(&conn, document, after, 1_000)?;
        let Some(last) = page.last() else { break };
        after = last.id.unwrap_or(i64::MAX);
        for stored in &page {
            rows.push(GateRow::from_stored(stored)?);
        }
    }
    let held_past_half: BTreeSet<(String, String, String)> = rows
        .iter()
        .filter(|row| row.phase == GatePhase::HeldPastHalf)
        .map(|row| {
            (
                row.consumer.clone(),
                row.document.clone(),
                row.text_hash.clone(),
            )
        })
        .collect();
    Ok(rows
        .into_iter()
        .map(|mut row| {
            let label_source = if row.label.is_some() {
                Some("stored")
            } else {
                let past_half = held_past_half.contains(&(
                    row.consumer.clone(),
                    row.document.clone(),
                    row.text_hash.clone(),
                ));
                row.label = derive_label(&row, past_half, now_ms, label_window_ms);
                row.label.map(|_| "derived")
            };
            DatasetRow { row, label_source }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_debounce::edit_settle::{CompletionSignal, GateComponent, TrailingToken};
    use agent_doc_document_realtime::midturn_steering::SteeringSource;

    fn features(quiet_ms: u64) -> GateFeatures {
        GateFeatures {
            trailing_token: TrailingToken::Word,
            signal: CompletionSignal::Inconclusive,
            unbalanced_delimiters: false,
            closed_list_item: false,
            quiet_ms,
            median_pause_ms: None,
            typing_chars_per_min: None,
            item_age_ms: quiet_ms,
            component: GateComponent::Queue,
            debounce_ms: 2_000,
            max_hold_ms: 45_000,
        }
    }

    fn decision(
        hash: &str,
        previous: Option<&str>,
        decision: SettleDecision,
        quiet_ms: u64,
    ) -> GateDecision {
        GateDecision {
            key: format!("queue:add:{hash}"),
            text_hash: hash.to_string(),
            previous_text_hash: previous.map(str::to_string),
            text_tail: format!("item {hash}"),
            source: SteeringSource::Queue,
            change: if previous.is_some() {
                SteeringChange::Edited
            } else {
                SteeringChange::Added
            },
            decision,
            tier: SettleTier::FullWindow,
            features: features(quiet_ms),
        }
    }

    fn observed(now_ms: u64, changed: u64, decisions: Vec<GateDecision>) -> GateLogEvent {
        GateLogEvent::Observed(GateObservation {
            document: "plan.md".into(),
            consumer: "hook".into(),
            harness: "claude".into(),
            operator: "op".into(),
            now_ms,
            document_changed_ms: Some(changed),
            boundary: false,
            learned_gate: true,
            decisions,
        })
    }

    fn fold(events: &[GateLogEvent]) -> GateLogTracking {
        events.iter().fold(
            GateLogTracking::new(DEFAULT_LABEL_WINDOW_MS),
            |state, event| advance(&state, event).unwrap(),
        )
    }

    fn row<'a>(state: &'a GateLogTracking, hash: &str, phase: GatePhase) -> &'a GateRow {
        &state
            .rows
            .values()
            .find(|tracked| tracked.row.text_hash == hash && tracked.row.phase == phase)
            .unwrap_or_else(|| panic!("no {hash} {phase:?} row"))
            .row
    }

    const HELD: SettleDecision = SettleDecision::Held {
        recheck_after_ms: 1_000,
    };

    /// A delivery whose next observation arrives long after the recent
    /// hydration horizon (no mid-turn edit; the closeout write is next) is
    /// still labelled by it and trains the learned gate. Before, a fresh hook
    /// process never loaded the row, so nothing ever trained.
    #[test]
    fn an_old_unlabelled_hydrated_delivery_is_labelled_and_trains() {
        let decided = fold(&[observed(
            3_000,
            2_000,
            vec![decision("v1", None, SettleDecision::Settled, 2_000)],
        )]);
        let old = row(&decided, "v1", GatePhase::Delivered).clone();
        assert_eq!(old.label, None);

        // A fresh process, 20 minutes later.
        let later = 3_000 + 20 * 60_000;
        let state = fold(&[
            GateLogEvent::Hydrated {
                document: "plan.md".into(),
                rows: vec![old],
                profile: None,
                weights: None,
            },
            observed(later, later - 1_000, vec![]),
        ]);
        assert_eq!(
            row(&state, "v1", GatePhase::Delivered).label,
            Some(GateLabel::OnTime)
        );
        assert_eq!(state.weights.updates, 1, "the label trained the gate once");
    }

    /// `#steergatenamo`: each row keeps the text tail it was decided on, so a
    /// text classifier can be scored offline on the real pause points; it
    /// survives storage, and rows logged before the field existed still load.
    #[test]
    fn a_row_keeps_its_decision_text_tail_through_storage() {
        let state = fold(&[observed(
            3_000,
            2_000,
            vec![decision("v1", None, SettleDecision::Settled, 2_000)],
        )]);
        let delivered = row(&state, "v1", GatePhase::Delivered);
        assert_eq!(delivered.text_tail.as_deref(), Some("item v1"));
        let stored = delivered.to_stored().unwrap();
        let reloaded = GateRow::from_stored(&stored).unwrap();
        assert_eq!(reloaded.text_tail.as_deref(), Some("item v1"));

        let mut legacy: serde_json::Value = serde_json::from_str(&stored.row_json).unwrap();
        legacy.as_object_mut().unwrap().remove("text_tail");
        let legacy_stored = StoredGateRow {
            row_json: legacy.to_string(),
            ..stored
        };
        assert_eq!(
            GateRow::from_stored(&legacy_stored).unwrap().text_tail,
            None
        );
    }

    /// Delivered, then re-edited inside the window: premature. The superseded
    /// hold that preceded it was right to hold.
    #[test]
    fn a_re_edit_inside_the_window_labels_the_delivery_premature() {
        let state = fold(&[
            observed(1_000, 900, vec![decision("v1", None, HELD, 100)]),
            observed(
                3_000,
                2_000,
                vec![decision("v2", None, SettleDecision::Settled, 2_000)],
            ),
            observed(20_000, 19_000, vec![decision("v3", Some("v2"), HELD, 500)]),
        ]);
        assert_eq!(
            row(&state, "v1", GatePhase::HeldEarly).label,
            Some(GateLabel::OnTime)
        );
        let delivered = row(&state, "v2", GatePhase::Delivered);
        assert_eq!(delivered.re_edited_at_ms, Some(19_000));
        assert_eq!(delivered.label, Some(GateLabel::Premature));
    }

    /// Held past half the window, then settled with no further edit: late.
    #[test]
    fn a_hold_with_no_further_edit_is_late_and_a_quiet_delivery_on_time() {
        let state = fold(&[
            observed(1_500, 0, vec![decision("v1", None, HELD, 1_500)]),
            observed(
                2_500,
                0,
                vec![decision("v1", None, SettleDecision::Settled, 2_500)],
            ),
            observed(
                2_600,
                0,
                vec![decision("w1", None, SettleDecision::Settled, 2_600)],
            ),
        ]);
        assert_eq!(
            row(&state, "v1", GatePhase::HeldPastHalf).label,
            Some(GateLabel::Late)
        );
        // Delivery labels wait out the window.
        assert_eq!(row(&state, "v1", GatePhase::Delivered).label, None);
        let later = advance(&state, &observed(60_000, 0, Vec::new())).unwrap();
        assert_eq!(
            row(&later, "v1", GatePhase::Delivered).label,
            Some(GateLabel::Late)
        );
        assert_eq!(
            row(&later, "w1", GatePhase::Delivered).label,
            Some(GateLabel::OnTime)
        );
    }

    /// `#steergateperceptron`: labels attached by the live log train the
    /// weights online, the Effect persists them, and the next process loads
    /// the trained model. Repeated premature full-window deliveries move the
    /// model to hold that shape.
    #[test]
    fn premature_labels_train_and_persist_the_learned_gate() {
        let dir = tempfile::tempdir().unwrap();
        let log = SteeringGateLog::new(dir.path(), "op", DEFAULT_LABEL_WINDOW_MS);
        let seeded = GateWeights::seeded();
        assert_eq!(
            seeded.zone(&features(2_000)),
            agent_doc_debounce::learned_gate::GateZone::Send
        );
        let mut now = 0;
        for i in 0..12 {
            let v = format!("v{i}");
            let edit = format!("e{i}");
            now += 3_000;
            log.send(observed(
                now,
                now - 2_000,
                vec![decision(&v, None, SettleDecision::Settled, 2_000)],
            ));
            now += 1_000;
            log.send(observed(now, now, vec![decision(&edit, Some(&v), HELD, 0)]));
        }
        let trained = log.weights();
        assert!(trained.updates >= 12, "{}", trained.updates);
        let idx = agent_doc_debounce::learned_gate::FEATURE_NAMES
            .iter()
            .position(|name| *name == "inconclusive_quiet_full_window")
            .unwrap();
        assert!(trained.weights[idx] < seeded.weights[idx]);
        assert_eq!(
            trained.zone(&features(2_000)),
            agent_doc_debounce::learned_gate::GateZone::Hold
        );
        assert!(log.pending_writes().is_empty());
        assert_eq!(load_weights(dir.path(), "op").unwrap(), trained);
        assert_eq!(load_weights(dir.path(), "someone-else").unwrap(), seeded);
    }

    #[test]
    fn pause_profile_tracks_a_rolling_median() {
        let mut profile = PauseProfile::default();
        for at in [0, 400, 1_000, 1_300, 100_000, 100_500] {
            profile.observe_change(at);
        }
        // 400, 600, 300 kept; the 98.7s gap is the operator stepping away.
        assert_eq!(profile.pauses_ms, vec![400, 600, 300, 500]);
        assert_eq!(profile.median_ms(), Some(500));
    }

    /// The Effect is the only writer, and a second process that hydrates the
    /// rows sees the same dataset.
    #[test]
    fn the_effect_sinks_rows_and_a_new_process_hydrates_them() {
        let dir = tempfile::tempdir().unwrap();
        let log = SteeringGateLog::new(dir.path(), "op", DEFAULT_LABEL_WINDOW_MS);
        log.send(observed(1_000, 900, vec![decision("v1", None, HELD, 100)]));
        log.send(observed(
            3_000,
            2_000,
            vec![decision("v2", None, SettleDecision::Settled, 2_000)],
        ));
        assert!(
            log.pending_writes().is_empty(),
            "{:?}",
            log.pending_writes()
        );
        let exported =
            export_dataset(dir.path(), Some("plan.md"), 3_000, DEFAULT_LABEL_WINDOW_MS).unwrap();
        assert_eq!(exported.len(), 2);
        assert_eq!(exported[0].row.phase, GatePhase::HeldEarly);
        assert_eq!(exported[0].row.label, Some(GateLabel::OnTime));
        assert_eq!(exported[1].row.phase, GatePhase::Delivered);
        assert_eq!(exported[1].row.label, None);
        // `steering --explain` reads the latest decision back.
        let latest = latest_row(dir.path(), Some("plan.md")).unwrap().unwrap();
        assert_eq!(latest.phase, GatePhase::Delivered);
        assert_eq!(latest.model_zone.as_deref(), Some("send"));
        assert!(latest.learned_gate);

        // A later process observes the re-edit against the hydrated row.
        let second = SteeringGateLog::new(dir.path(), "op", DEFAULT_LABEL_WINDOW_MS);
        hydrate(&second, dir.path(), "plan.md", "op", 20_000, 45_000).unwrap();
        second.send(observed(
            20_000,
            19_000,
            vec![decision("v3", Some("v2"), HELD, 500)],
        ));
        let exported =
            export_dataset(dir.path(), Some("plan.md"), 20_000, DEFAULT_LABEL_WINDOW_MS).unwrap();
        let delivered = exported
            .iter()
            .find(|r| r.row.phase == GatePhase::Delivered)
            .unwrap();
        assert_eq!(delivered.row.label, Some(GateLabel::Premature));
        assert_eq!(delivered.label_source, Some("stored"));
    }
}
