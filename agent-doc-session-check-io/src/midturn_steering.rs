//! Mid-turn operator steering delivery (`#midturn-steering`).
//!
//! The deterministic core lives in
//! [`agent_doc_document_realtime::midturn_steering`]; this module owns the
//! durable watermark, the cheap observation gate, and the harness hook
//! envelope.
//!
//! **Why a stateless compare and not a Lazily actor edge.** The delivery
//! surface is a harness `PostToolUse` hook: a one-shot process spawned after
//! every tool call, with no long-lived scope of its own and a hard latency
//! budget. Joining the controller's `TurnScope` would cost an IPC round trip
//! per tool call and couple every tool call to controller liveness. So the
//! hook is the narrow actorless boundary `#lazily-reactive-first` allows: it
//! compares the document against durable, cycle-scoped state (the watermark
//! preflight seeds in `state.db` when it admits the cycle) and writes the
//! advanced watermark back. The cycle itself is consulted only on the rare
//! path where something is ready to surface, which is also where a closed or
//! superseded cycle silences the watermark for good.
//!
//! Storage: `state.db` `project_runtime_state`, one base record per document
//! (seeded by preflight) plus one progress record per consumer (`hook`, `cli`,
//! `follow`), so a polling CLI never steals steering from the in-turn hook.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use agent_doc_document_realtime::midturn_steering::{
    self as core, DEFAULT_STEERING_DEBOUNCE_MS, ObserveContext, SteeringItem, SteeringWatermark,
};

/// The in-turn harness hook consumer.
pub const CONSUMER_HOOK: &str = "hook";
/// `agent-doc steering <FILE>` polling consumer.
pub const CONSUMER_CLI: &str = "cli";
/// `agent-doc steering --follow <FILE>` stream consumer.
pub const CONSUMER_FOLLOW: &str = "follow";

/// What one observation produced for a consumer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SteeringReport {
    pub document: String,
    pub cycle_id: String,
    pub items: Vec<SteeringItem>,
    pub pending: usize,
    /// Observed at/after the turn boundary (the cycle no longer accepts
    /// mid-turn steering): items are framed for the next cycle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_close: bool,
    /// When a held item's settle decision can next change (ms from now).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recheck_after_ms: Option<u64>,
}

impl SteeringReport {
    /// Agent-facing context, `None` when nothing is ready.
    pub fn render(&self) -> Option<String> {
        if self.after_close {
            core::render_closeout_steering_context(&self.document, &self.items, self.pending)
        } else {
            core::render_steering_context(&self.document, &self.items, self.pending)
        }
    }
}

fn state_key(kind: &str, file: &Path) -> String {
    agent_doc_queue_io::subagent_dispatch::midturn_steering_state_key(kind, file)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn project_root(file: &Path) -> Option<PathBuf> {
    agent_doc_fs::find_project_root(file)
}

/// Seed the cycle watermark from preflight's admitted document.
///
/// `current_item` is the queue prompt this cycle selected (if any) and
/// `session_presets` the preset names this cycle's prompt requested; both
/// scope later classification. Re-seeding the same cycle (re-entrant
/// preflight) replaces the base, and consumers re-initialize because their
/// recorded baseline hash no longer matches.
pub fn seed_for_cycle(
    file: &Path,
    cycle_id: &str,
    baseline: &str,
    current_item: Option<&str>,
    session_presets: Vec<String>,
) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let mut watermark = SteeringWatermark::seed(cycle_id, baseline, current_item, session_presets);
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    // Keep the previous cycle's seed queue for `queue_subagent_dispatch`: a
    // re-entrant preflight of the same cycle inherits it, a new cycle takes
    // the outgoing base's queue.
    watermark.prior_cycle_queue = match load_watermark(&conn, &state_key("base", file))? {
        Some(existing) if existing.cycle_id == cycle_id => existing.prior_cycle_queue,
        Some(existing) => Some(existing.acknowledged_queue),
        None => None,
    };
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &state_key("base", file),
        &serde_json::to_string(&watermark)?,
        now_ms(),
    )
}

fn load_watermark(
    conn: &agent_doc_sqlite::state_store::Connection,
    key: &str,
) -> Result<Option<SteeringWatermark>> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(conn, key)?
        .map(|raw| serde_json::from_str(&raw).context("parse mid-turn steering watermark"))
        .transpose()
}

fn stat_fingerprint(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{mtime}:{}", meta.len())
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Resolve the debounce window: frontmatter, then project config, then default.
pub fn debounce_ms_for(file: &Path, content: &str) -> u64 {
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.steering_debounce_ms);
    resolve_debounce_ms(
        frontmatter,
        agent_doc_project_config_io::load_project_for_doc(file).agent_doc_steering_debounce_ms,
    )
}

/// The steering max-hold (`#steeringtypinggate`): project config, else the
/// shared default.
pub fn max_hold_ms_for(file: &Path) -> u64 {
    agent_doc_project_config_io::load_project_for_doc(file)
        .agent_doc_steering_max_hold_ms
        .unwrap_or(agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS)
}

/// Pure precedence for [`debounce_ms_for`].
pub fn resolve_debounce_ms(frontmatter: Option<u64>, project: Option<u64>) -> u64 {
    frontmatter
        .or(project)
        .unwrap_or(DEFAULT_STEERING_DEBOUNCE_MS)
}

/// Whether the cycle still accepts mid-turn steering: the same cycle, open,
/// and no response captured yet. After capture, closeout `session-check` owns
/// steering (`realtime_steering_closeout_guidance`).
fn cycle_accepts_steering(cycle: &agent_doc_cycle_state_io::CycleState, cycle_id: &str) -> bool {
    cycle.cycle_id == cycle_id
        && matches!(cycle.phase, agent_doc_turn::CyclePhase::PreflightStarted)
        && cycle.capture_id.is_none()
        && cycle.response_sha256.is_none()
}

fn binary_owned_ids(cycle: &agent_doc_cycle_state_io::CycleState) -> BTreeSet<String> {
    cycle
        .pending_actionable_ids
        .iter()
        .chain(cycle.pending_added_ids.iter())
        .chain(cycle.requested_added_ids.iter())
        .map(|id| id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

/// A computed, not-yet-persisted observation for one consumer.
///
/// Splitting compute from persist is what lets the turn-boundary report
/// advance the watermark only after the report actually carried the items.
#[derive(Debug, Clone)]
pub struct PreparedObservation {
    consumer: String,
    consumer_key: String,
    root: PathBuf,
    next: Option<SteeringWatermark>,
    /// The report, `None` when there is nothing to say.
    pub report: Option<SteeringReport>,
    /// The observation happened at/after the turn boundary.
    pub boundary: bool,
    /// The settle decisions behind the report, for the completion-gate
    /// decision log (`#steergatelog`); recorded only when acknowledged.
    gate: Option<crate::steering_gate_log::GateObservation>,
    max_hold_ms: u64,
}

impl PreparedObservation {
    /// Persist the advanced watermark (and log what surfaced).
    pub fn acknowledge(&self, file: &Path) -> Result<()> {
        let Some(next) = &self.next else {
            return Ok(());
        };
        let conn = agent_doc_sqlite::state_store::open_state_db(&self.root)?;
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &self.consumer_key,
            &serde_json::to_string(next)?,
            now_ms(),
        )?;
        if let Some(report) = self
            .report
            .as_ref()
            .filter(|report| !report.items.is_empty())
        {
            let dispatches = report
                .items
                .iter()
                .map(|item| item.dispatch.as_str())
                .collect::<Vec<_>>()
                .join(",");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "midturn_steering_surfaced file={} consumer={} cycle={} count={} pending={} dispatch={dispatches} boundary={}",
                    file.display(),
                    self.consumer,
                    next.cycle_id,
                    report.items.len(),
                    report.pending,
                    self.boundary,
                ),
            );
        }
        // `#steergatelog`: the decisions this consumer just acted on feed the
        // decision log's graph; its Effect persists them. A log failure never
        // fails steering delivery.
        if let Some(gate) = &self.gate
            && let Err(err) =
                crate::steering_gate_log::record(&self.root, file, gate.clone(), self.max_hold_ms)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "steering_gate_log_failed file={} consumer={} error={err:#} (#steergatelog)",
                    file.display(),
                    self.consumer,
                ),
            );
        }
        Ok(())
    }
}

/// How an observation treats a cycle that no longer accepts mid-turn steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClosedCyclePolicy {
    /// The in-turn hook. While the cycle is open it reports in-turn
    /// steering; once the cycle closes it keeps reporting unsurfaced changes in
    /// boundary mode (`#steeringafterclose`). An agent that keeps working in
    /// the same harness turn after closeout (dispatching subagents, waiting on
    /// them) used to go deaf here: the 2026-10-03 `#subagent` additions to
    /// agent-doc-bugs.md landed while tool calls were still running and the
    /// silenced hook dropped them.
    Hook,
    /// Polls (`agent-doc steering`): after close, keep reporting unsurfaced
    /// queue/exchange changes in boundary mode until the next preflight.
    Boundary,
    /// The turn-boundary report itself: always boundary mode, no debounce.
    ForceBoundary,
}

/// Fallback base when no seeded watermark describes the latest cycle: the
/// last committed baseline, so a poll after close still reports items added
/// since the last commit.
fn committed_baseline_watermark(file: &Path) -> Result<Option<SteeringWatermark>> {
    Ok(
        agent_doc_snapshot_io::load_document_baseline(file)?.map(|baseline| {
            SteeringWatermark::seed(
                &format!("committed:{}", core::content_hash(&baseline)),
                &baseline,
                None,
                Vec::new(),
            )
        }),
    )
}

fn prepare(
    file: &Path,
    consumer: &str,
    policy: ClosedCyclePolicy,
) -> Result<Option<PreparedObservation>> {
    prepare_with_gate(file, consumer, policy, true)
}

/// [`prepare`] with the unchanged-document gate optional. The gate is a pure
/// optimization for advancing consumers; a non-advancing reader that needs the
/// full unsurfaced set relative to a watermark (the idle wake) turns it off.
fn prepare_with_gate(
    file: &Path,
    consumer: &str,
    policy: ClosedCyclePolicy,
    unchanged_gate: bool,
) -> Result<Option<PreparedObservation>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let stored_base = load_watermark(&conn, &state_key("base", file))?;
    // Boundary-capable consumers need the cycle up front to pick the base;
    // the hook keeps its rare-path cycle lookup.
    let cycle = if policy == ClosedCyclePolicy::Hook {
        None
    } else {
        agent_doc_cycle_state_io::load_with_closeout_projection(file)?
    };
    // The boundary report speaks for a committed turn; a closeout that
    // deferred (cycle still open) reports at the later session-check.
    if policy == ClosedCyclePolicy::ForceBoundary && cycle.as_ref().is_some_and(|c| c.is_open()) {
        return Ok(None);
    }
    let base_describes_cycle = |base: &SteeringWatermark| {
        cycle
            .as_ref()
            .is_none_or(|cycle| cycle.cycle_id == base.cycle_id)
    };
    let base = match stored_base {
        Some(base) if policy == ClosedCyclePolicy::Hook || base_describes_cycle(&base) => base,
        // No seed for the latest cycle (or none at all): compare with the
        // last committed baseline.
        _ if policy != ClosedCyclePolicy::Hook => match committed_baseline_watermark(file)? {
            Some(base) => base,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    let consumer_key = state_key(consumer, file);
    let mut watermark = match load_watermark(&conn, &consumer_key)? {
        Some(existing)
            if existing.cycle_id == base.cycle_id && existing.baseline == base.baseline =>
        {
            existing
        }
        _ => base,
    };
    let cycle_open = cycle
        .as_ref()
        .is_some_and(|cycle| cycle_accepts_steering(cycle, &watermark.cycle_id));
    let mut boundary = match policy {
        // A watermark a pre-`#steeringafterclose` hook silenced already knows
        // its cycle closed.
        ClosedCyclePolicy::Hook => watermark.closed,
        ClosedCyclePolicy::Boundary => !cycle_open,
        ClosedCyclePolicy::ForceBoundary => true,
    };
    // A silenced watermark still owes the boundary report.
    watermark.closed = false;
    let max_hold_ms = max_hold_ms_for(file);
    let prepared = |next: Option<SteeringWatermark>,
                    report: Option<SteeringReport>,
                    boundary: bool,
                    gate: Option<crate::steering_gate_log::GateObservation>| {
        Ok(Some(PreparedObservation {
            consumer: consumer.to_string(),
            consumer_key: consumer_key.clone(),
            root: root.clone(),
            next,
            report,
            boundary,
            gate,
            max_hold_ms,
        }))
    };

    // Hot-path gate: unchanged file stat and nothing settling → no read.
    let meta = std::fs::metadata(file).with_context(|| format!("stat {}", file.display()))?;
    let fingerprint = stat_fingerprint(&meta);
    if unchanged_gate
        && watermark.pending.is_empty()
        && watermark.last_observed_stat.as_deref() == Some(fingerprint.as_str())
    {
        return prepared(None, None, boundary, None);
    }
    let content =
        std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    if unchanged_gate
        && watermark.pending.is_empty()
        && watermark.last_observed_content_hash.as_deref()
            == Some(core::content_hash(&content).as_str())
    {
        watermark.last_observed_stat = Some(fingerprint);
        return prepared(Some(watermark), None, boundary, None);
    }

    // The turn boundary is the last chance before the loop re-enters: an
    // item that is complete surfaces now rather than waiting out the window.
    let debounce_ms = if policy == ClosedCyclePolicy::ForceBoundary {
        0
    } else {
        debounce_ms_for(file, &content)
    };
    let mode = if boundary {
        core::ObserveMode::Boundary
    } else {
        core::ObserveMode::InTurn
    };
    let empty = BTreeSet::new();
    let owned = cycle.as_ref().map(binary_owned_ids).unwrap_or_default();
    let observed_at_ms = now_ms();
    let document = crate::steering_gate_log::document_key(&root, file);
    // `#steergateperceptron`: the online-learned gate answers first when
    // enabled; the deterministic floors stay inside the settle decision, and
    // the gate lives in the binary, so every harness gets the same answer.
    let learned = crate::steering_gate_log::learned_classifier(
        &root,
        file,
        &document,
        observed_at_ms,
        max_hold_ms,
    );
    let deterministic = agent_doc_debounce::edit_settle::DeterministicOnly;
    let classifier: &dyn agent_doc_debounce::edit_settle::CompletionClassifier = match &learned {
        Some(gate) => gate,
        None => &deterministic,
    };
    let ctx = ObserveContext {
        now_ms: observed_at_ms,
        document_changed_ms: mtime_ms(&meta),
        debounce_ms,
        binary_owned_queue_ids: if boundary { &owned } else { &empty },
        max_hold_ms,
        classifier,
        median_pause_ms: crate::steering_gate_log::median_pause_ms(
            &root,
            file,
            &document,
            observed_at_ms,
            max_hold_ms,
        ),
    };
    let mut observation = core::observe_with_mode(&watermark, &content, &ctx, mode);
    if !boundary && !observation.ready.is_empty() {
        // Rare path: confirm the cycle is still open before handing anything
        // to the agent, and exclude this cycle's own queue bookkeeping.
        let cycle = match cycle {
            Some(cycle) => Some(cycle),
            None => agent_doc_cycle_state_io::load_with_closeout_projection(file)?,
        };
        match cycle {
            Some(cycle) if cycle_accepts_steering(&cycle, &watermark.cycle_id) => {
                let owned = binary_owned_ids(&cycle);
                if !owned.is_empty() {
                    observation = core::observe(
                        &watermark,
                        &content,
                        &ObserveContext {
                            binary_owned_queue_ids: &owned,
                            ..ctx
                        },
                    );
                }
            }
            closed => {
                // The cycle closed while this harness turn kept running:
                // report the same edits in boundary terms (queue work for the
                // next cycle), excluding the closed cycle's own bookkeeping.
                boundary = true;
                let owned = closed.as_ref().map(binary_owned_ids).unwrap_or_default();
                observation = core::observe_with_mode(
                    &watermark,
                    &content,
                    &ObserveContext {
                        binary_owned_queue_ids: &owned,
                        ..ctx
                    },
                    core::ObserveMode::Boundary,
                );
            }
        }
    }
    let gate = crate::steering_gate_log::GateObservation {
        document,
        consumer: consumer.to_string(),
        harness: document_harness(&content),
        operator: crate::steering_gate_log::operator_id(),
        now_ms: ctx.now_ms,
        document_changed_ms: ctx.document_changed_ms,
        boundary,
        learned_gate: learned.is_some(),
        decisions: std::mem::take(&mut observation.decisions),
    };
    let mut next = observation.next;
    next.last_observed_stat = Some(fingerprint);
    let report = SteeringReport {
        document: file.display().to_string(),
        cycle_id: next.cycle_id.clone(),
        items: observation.ready,
        pending: observation.pending,
        after_close: boundary,
        recheck_after_ms: observation.recheck_after_ms,
    };
    prepared(Some(next), Some(report), boundary, Some(gate))
}

/// The document's configured harness (frontmatter `agent:`), for the
/// decision log. The gate itself never reads it: decisions are identical
/// across harnesses.
fn document_harness(content: &str) -> String {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.agent)
        .map(|agent| agent.trim().to_ascii_lowercase())
        .filter(|agent| !agent.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Observe `file` for `consumer`. Returns `Ok(None)` when there is no active
/// cycle watermark, the cycle closed (hook only), or the document is
/// unchanged; otherwise a report (possibly with zero ready items while edits
/// are still settling).
///
/// Every consumer keeps reporting unsurfaced changes after the cycle closes,
/// in boundary mode, until the next preflight re-seeds (`#closeout-steering`,
/// `#steeringafterclose`).
///
/// When `advance` is false the watermark is not written (a peek).
pub fn observe(file: &Path, consumer: &str, advance: bool) -> Result<Option<SteeringReport>> {
    let policy = if consumer == CONSUMER_HOOK {
        ClosedCyclePolicy::Hook
    } else {
        ClosedCyclePolicy::Boundary
    };
    let Some(prepared) = prepare(file, consumer, policy)? else {
        return Ok(None);
    };
    if advance {
        prepared.acknowledge(file)?;
    }
    Ok(prepared.report)
}

/// The turn-boundary observation for the in-turn agent channel: unsurfaced
/// steering relative to the hook consumer's watermark (so nothing the hook
/// already delivered repeats), in boundary mode, with no debounce. The caller
/// renders it, emits it, and only then calls
/// [`PreparedObservation::acknowledge`].
pub fn prepare_closeout(file: &Path) -> Result<Option<PreparedObservation>> {
    prepare(file, CONSUMER_HOOK, ClosedCyclePolicy::ForceBoundary)
}

/// What the idle supervisor's steering wake sees (`#steeringwake`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WakeObservation {
    /// Settled steering no agent-facing consumer has surfaced and no worker
    /// has claimed, in document order.
    pub items: Vec<SteeringItem>,
    /// Identity of the whole set: the steering base's cycle plus every item.
    /// Empty when `items` is empty.
    pub fingerprint: String,
    /// Candidates still held by the settle gate.
    pub pending: usize,
    /// When a held candidate can next settle, in ms from now. The supervisor
    /// schedules exactly one re-observation for it instead of polling.
    pub recheck_after_ms: Option<u64>,
}

fn steering_item_identity(item: &SteeringItem) -> String {
    format!(
        "{}:{}:{}",
        match item.source {
            core::SteeringSource::Exchange => "exchange",
            core::SteeringSource::Queue => "queue",
        },
        match item.change {
            core::SteeringChange::Added => "added",
            core::SteeringChange::Edited => "edited",
            core::SteeringChange::Deleted => "deleted",
        },
        core::normalize_queue_text(&item.verbatim)
    )
}

/// Non-consuming observation for the idle steering wake (`#steeringwake`).
///
/// The same derivation every steering consumer uses ([`prepare`]), read
/// against the agent channel's watermark (the hook consumer, which the
/// turn-boundary report also advances) without advancing it. The `steering`
/// poll watermark is deliberately NOT consulted: a consuming diagnostic read
/// (`agent-doc steering` run by an operator or another agent) must never
/// suppress the wake. The cost is at most one duplicate for a hookless
/// harness that polled an item after closeout; the alternative is loss.
/// Queue items a worker has
/// claimed (`agent-doc queue claim`) are in flight elsewhere and never wake
/// the session. The woken turn receives the items through its normal
/// channels (preflight, the hook, or the boundary report), so the wake itself
/// never consumes steering.
pub fn observe_for_wake(file: &Path) -> Result<WakeObservation> {
    let hook = prepare_with_gate(file, CONSUMER_HOOK, ClosedCyclePolicy::Boundary, false)?;
    let Some(hook_report) = hook.as_ref().and_then(|prepared| prepared.report.clone()) else {
        return Ok(WakeObservation::default());
    };
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(file, &content);
    let items: Vec<SteeringItem> = hook_report
        .items
        .iter()
        .filter(|item| {
            item.source != core::SteeringSource::Queue || !claimed.claims(&item.verbatim)
        })
        .cloned()
        .collect();
    let fingerprint = if items.is_empty() {
        String::new()
    } else {
        let mut basis = hook_report.cycle_id.clone();
        for item in &items {
            basis.push('\n');
            basis.push_str(&steering_item_identity(item));
        }
        core::content_hash(&basis)
    };
    let pending = hook_report.pending;
    let recheck_after_ms = hook_report.recheck_after_ms;
    Ok(WakeObservation {
        items,
        fingerprint,
        pending,
        recheck_after_ms,
    })
}

fn wake_receipt_key(file: &Path) -> String {
    state_key("wake", file)
}

/// The durable receipt of the last steering set a wake was delivered for.
pub fn load_wake_receipt(file: &Path) -> Result<Option<String>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        &wake_receipt_key(file),
    )
}

/// Record that a wake for `fingerprint` was submitted to the owning pane.
pub fn record_wake_receipt(file: &Path, fingerprint: &str, items: usize) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &wake_receipt_key(file),
        fingerprint,
        now_ms(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "steering_wake_delivered file={} fingerprint={} items={items} (#steeringwake)",
            file.display(),
            fingerprint.get(..12).unwrap_or(fingerprint),
        ),
    );
    Ok(())
}

/// Render a report for the turn-boundary surface.
pub fn render_closeout(report: &SteeringReport) -> Option<String> {
    core::render_closeout_steering_context(&report.document, &report.items, report.pending)
}

/// Emit unsurfaced steering into the terminal report of `respond` /
/// `write --commit` / post-commit `session-check` (`#closeout-steering`).
///
/// The watermark advances only after `writer` accepted the rendered report.
/// Failures are reported (stderr + ops.log) and never fail the already-
/// committed closeout.
pub fn emit_closeout_steering(file: &Path, writer: &mut impl std::io::Write) {
    let outcome = (|| -> Result<bool> {
        let Some(prepared) = prepare_closeout(file)? else {
            return Ok(false);
        };
        let rendered = prepared.report.as_ref().and_then(render_closeout);
        if let Some(text) = &rendered {
            writeln!(writer, "{text}").context("write closeout steering report")?;
            writer.flush().context("flush closeout steering report")?;
        }
        prepared.acknowledge(file)?;
        Ok(rendered.is_some())
    })();
    if let Err(err) = outcome {
        eprintln!(
            "[agent-doc] WARNING: closeout steering report failed for {}; run `agent-doc steering {}` to read operator edits made during the turn: {err:#}",
            file.display(),
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "midturn_steering_closeout_error file={} error={err:#}",
                file.display()
            ),
        );
    }
}

/// Harness `PostToolUse` payload fields this hook reads. Claude Code and Codex
/// both send `session_id` and `cwd`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PostToolUseInput {
    pub session_id: String,
    pub cwd: String,
}

/// Resolve the document this harness session is driving, from the binding
/// the `UserPromptSubmit` hook recorded.
pub fn session_document(input: &PostToolUseInput) -> Result<Option<PathBuf>> {
    let roots = agent_doc_codex_hook_io::project_roots_for(Path::new(&input.cwd));
    if roots.is_empty() {
        return Ok(None);
    }
    if !roots
        .iter()
        .any(|root| agent_doc_sqlite::state_store::state_db_path(root).exists())
    {
        return Ok(None);
    }
    let Some((_, state)) = agent_doc_codex_hook_io::load_state_any(&roots, &input.session_id)?
    else {
        return Ok(None);
    };
    if state.preflight_admitted == Some(false) {
        return Ok(None);
    }
    let doc = PathBuf::from(&state.doc_path);
    Ok(doc.is_file().then_some(doc))
}

/// The hook envelope Claude Code and Codex inject into the running turn.
pub fn post_tool_use_output(context: &str) -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context,
        }
    })
}

/// Pure hook decision: the JSON to print, or `None` for silence.
pub fn post_tool_use_response(payload: &str) -> Result<Option<serde_json::Value>> {
    let input: PostToolUseInput =
        serde_json::from_str(payload).context("parse PostToolUse payload")?;
    let Some(file) = session_document(&input)? else {
        return Ok(None);
    };
    let report = match observe(&file, CONSUMER_HOOK, true) {
        Ok(report) => report,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "midturn_steering_hook_error file={} error={err:#}",
                    file.display()
                ),
            );
            return Err(err);
        }
    };
    Ok(report
        .and_then(|report| report.render())
        .map(|context| post_tool_use_output(&context)))
}

/// `agent-doc hook steering-post-tool-use` entry point. Never fails the tool
/// call: every error is reported on stderr (and ops.log when the document is
/// known) and the hook prints nothing.
pub fn handle_post_tool_use() -> Result<()> {
    use std::io::Read;
    let mut payload = String::new();
    if let Err(err) = std::io::stdin().read_to_string(&mut payload) {
        eprintln!("[agent-doc] steering hook payload read failed: {err}");
        return Ok(());
    }
    match post_tool_use_response(&payload) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(err) => eprintln!("[agent-doc] steering hook skipped: {err:#}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_envelope_matches_the_harness_contract() {
        let value = post_tool_use_output("steer");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "steer");
    }

    #[test]
    fn debounce_precedence_is_frontmatter_then_project_then_default() {
        assert_eq!(resolve_debounce_ms(Some(10), Some(20)), 10);
        assert_eq!(resolve_debounce_ms(None, Some(20)), 20);
        assert_eq!(
            resolve_debounce_ms(None, None),
            DEFAULT_STEERING_DEBOUNCE_MS
        );
    }

    #[test]
    fn hook_is_silent_outside_an_agent_doc_project() {
        let dir = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "session_id": "s1",
            "cwd": dir.path(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        })
        .to_string();
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }

    #[test]
    fn seeded_watermark_without_an_open_cycle_reports_once_per_consumer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();

        // Unchanged document: silent.
        assert_eq!(observe(&file, CONSUMER_CLI, true).unwrap(), None);

        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        // `#steeringafterclose`: with no open cycle the hook still reports the
        // unsurfaced addition, in boundary terms, exactly once.
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("hook report");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert!(hook.after_close);
        assert!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .is_none_or(|report| report.items.is_empty())
        );
        // A poll reports the unsurfaced addition after close, once.
        let report = observe(&file, CONSUMER_CLI, true).unwrap().expect("report");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert!(report.after_close);
        assert_eq!(report.items[0].verbatim, "new work");
        let rerun = observe(&file, CONSUMER_CLI, true).unwrap();
        assert!(
            rerun.as_ref().is_none_or(|report| report.items.is_empty()),
            "{rerun:?}"
        );
    }

    fn close_cycle(file: &Path, content: &str) {
        agent_doc_cycle_state_io::mark_committed(file, "test_commit", Some(content), Some(content))
            .unwrap();
    }

    /// The operator report: a `#subagents` queue addition made after the seed,
    /// with no PostToolUse hook ever running, must reach the agent in the
    /// `respond` terminal report exactly once, as a dispatch directive; a
    /// `steering` poll after close must still show it.
    #[test]
    fn closeout_report_carries_a_hookless_subagent_addition_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 600000\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // The closeout consumed the current head; the operator's line stays.
        let closed = baseline.replace("- current task\n", "- #subagents do [#preflightdeadline]\n");
        std::fs::write(&file, &closed).unwrap();
        // A closeout that deferred (cycle still open) does not report yet and
        // does not advance the watermark.
        let mut deferred = Vec::new();
        emit_closeout_steering(&file, &mut deferred);
        assert!(
            deferred.is_empty(),
            "{}",
            String::from_utf8_lossy(&deferred)
        );
        close_cycle(&file, &closed);

        let mut out = Vec::new();
        emit_closeout_steering(&file, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with(core::CLOSEOUT_STEERING_MARKER), "{text}");
        assert!(text.contains("dispatch=subagent"), "{text}");
        assert!(
            text.contains("verbatim: #subagents do [#preflightdeadline]"),
            "{text}"
        );
        assert!(text.contains("DISPATCH NOW"), "{text}");
        assert!(
            text.contains("agent-doc queue claim") && text.contains("--item '#preflightdeadline'"),
            "{text}"
        );
        assert!(
            !text.contains("REMOVED the queue item"),
            "consuming the finished head is not steering: {text}"
        );
        assert_eq!(text.matches("[steering ").count(), 1, "{text}");

        // Exactly once on the agent channel: a second closeout (e.g. the
        // post-commit session-check) is silent.
        let mut again = Vec::new();
        emit_closeout_steering(&file, &mut again);
        assert!(again.is_empty(), "{}", String::from_utf8_lossy(&again));

        // A steering poll after close still shows it (its own consumer) once
        // the operator's edit is quiet for the debounce window.
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        let peek = observe(&file, CONSUMER_CLI, false).unwrap().expect("peek");
        assert_eq!(peek.items.len(), 1, "{peek:?}");
        assert!(peek.after_close);
        assert_eq!(peek.items[0].dispatch, core::SteeringDispatch::Subagent);
        assert!(peek.render().unwrap().contains("DISPATCH NOW"));
        // Peek never advances.
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn closeout_report_skips_what_the_hook_already_surfaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let mid = baseline.replace("- current task\n", "- current task\n- first addition\n");
        std::fs::write(&file, &mid).unwrap();
        assert_eq!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
        let late = mid.replace("- first addition\n", "- first addition\n- late addition\n");
        std::fs::write(&file, &late).unwrap();
        close_cycle(&file, &late);
        let mut out = Vec::new();
        emit_closeout_steering(&file, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("verbatim: late addition"), "{text}");
        assert!(!text.contains("first addition"), "{text}");
        assert!(text.contains("dispatch=drain_after_current"), "{text}");
    }

    #[test]
    fn open_cycle_surfaces_steering_once_through_the_hook_consumer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        let report = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("report");
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].verbatim, "new work");
        let context = report.render().unwrap();
        assert!(
            context.contains("dispatch=drain_after_current"),
            "{context}"
        );
        // Exactly once: the same document never re-injects.
        assert_eq!(observe(&file, CONSUMER_HOOK, true).unwrap(), None);
        // Consumers are independent: the CLI still sees it once.
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn post_tool_use_hook_emits_the_harness_envelope_then_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        agent_doc_codex_hook_io::save_state(
            &root,
            &agent_doc_codex_hook_io::SessionState {
                session_id: "sess-1".into(),
                identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                doc_path: file.display().to_string(),
                last_turn_id: String::new(),
                last_prompt: "agent-doc plan.md".into(),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let payload = serde_json::json!({
            "session_id": "sess-1",
            "cwd": root,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo build"},
            "tool_response": {"stdout": "ok"},
        })
        .to_string();
        // No seeded cycle yet: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // Seeded, nothing new: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        std::fs::write(
            &file,
            baseline.replace(
                "- current task\n",
                "- current task\n- #subagents fix issue 111\n",
            ),
        )
        .unwrap();
        let output = post_tool_use_response(&payload)
            .unwrap()
            .expect("steering output");
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.starts_with(core::STEERING_MARKER), "{context}");
        assert!(context.contains("dispatch=subagent"), "{context}");
        assert!(context.contains("#subagents fix issue 111"), "{context}");
        // Exactly once.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }

    fn backdate(file: &Path) {
        std::fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }

    /// The 2026-10-03 incident (`#steeringwake`): the cycle closed, the agent
    /// went idle, and the operator queued `#subagent: <issue>` items. The idle
    /// wake must see them, must not consume them (the woken turn's hook or
    /// boundary report still delivers them exactly once), and must skip items
    /// a worker already claimed.
    #[test]
    fn idle_wake_sees_post_close_additions_without_consuming_and_skips_claims() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 2500\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- release + publish\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let closed = baseline.replace("- current task\n", "");
        std::fs::write(&file, &closed).unwrap();
        close_cycle(&file, &closed);
        let mut report = Vec::new();
        emit_closeout_steering(&file, &mut report);

        // Nothing new since the boundary report: no wake.
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        let added = closed.replace(
            "- release + publish\n",
            "- #subagent: https://github.com/btakita/agent-doc/issues/116\n\
             - #subagent: https://github.com/btakita/agent-doc/issues/117\n\
             - release + publish\n",
        );
        std::fs::write(&file, &added).unwrap();
        // Still being typed (inside the debounce window): held, with exactly
        // one scheduled re-observation instead of a poll.
        let typing = observe_for_wake(&file).unwrap();
        assert!(typing.items.is_empty(), "{typing:?}");
        assert_eq!(typing.pending, 2);
        assert!(typing.recheck_after_ms.is_some_and(|ms| ms <= 2500));

        backdate(&file);
        let wake = observe_for_wake(&file).unwrap();
        assert_eq!(wake.items.len(), 2, "{wake:?}");
        assert!(
            wake.items
                .iter()
                .all(|item| item.dispatch == core::SteeringDispatch::Subagent)
        );
        assert!(!wake.fingerprint.is_empty());
        // Non-consuming: the same set, the same fingerprint.
        assert_eq!(observe_for_wake(&file).unwrap(), wake);

        // A worker claims one: it is in flight elsewhere and never wakes.
        agent_doc_queue_io::queue_claim::claim(
            &file,
            "#subagent: https://github.com/btakita/agent-doc/issues/116",
            "subagent:gh-116",
            3600,
        )
        .unwrap();
        let after_claim = observe_for_wake(&file).unwrap();
        assert_eq!(after_claim.items.len(), 1, "{after_claim:?}");
        assert!(after_claim.items[0].verbatim.ends_with("/117"));
        assert_ne!(after_claim.fingerprint, wake.fingerprint);

        // The agent channel still delivers both, once: the wake ate nothing.
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len(), 2, "{hook:?}");
        assert!(hook.after_close);
        // ...and once surfaced there, nothing is left to wake for.
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        // `#claimdispatchidentity`: the operator re-tags the CLAIMED line
        // while its subagent works. The edit is the same work, still claimed,
        // so it never wakes the session as new steering.
        let retagged = added.replace(
            "- #subagent: https://github.com/btakita/agent-doc/issues/116\n",
            "- #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/116\n",
        );
        assert_ne!(retagged, added);
        std::fs::write(&file, &retagged).unwrap();
        backdate(&file);
        let after_edit = observe_for_wake(&file).unwrap();
        assert!(after_edit.items.is_empty(), "{after_edit:?}");

        // Retargeting it to a different issue is new work: it wakes.
        let retargeted = retagged.replace("issues/116", "issues/118");
        std::fs::write(&file, &retargeted).unwrap();
        backdate(&file);
        let after_retarget = observe_for_wake(&file).unwrap();
        assert_eq!(after_retarget.items.len(), 1, "{after_retarget:?}");
        assert!(after_retarget.items[0].verbatim.ends_with("/118"));
    }

    /// The operator's complaint: a diagnostic `agent-doc steering` read
    /// consumed steering. A consuming poll must never suppress the idle wake.
    #[test]
    fn a_consuming_steering_poll_never_suppresses_the_idle_wake() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        backdate(&file);
        assert_eq!(observe_for_wake(&file).unwrap().items.len(), 1);
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(observe_for_wake(&file).unwrap().items.len(), 1);
    }

    /// `#steergatelog`: every consuming observation records its settle
    /// decisions with features, and the outcome labels attach from what the
    /// operator does next: the held fragment was right to hold (on time), the
    /// delivered line the operator immediately re-edited was premature.
    #[test]
    fn consuming_observations_log_each_settle_decision_with_its_outcome() {
        use crate::steering_gate_log::{GateLabel, GatePhase, export_dataset};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent: codex\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();
        let with = |line: &str| {
            baseline.replace("- current task\n", &format!("- current task\n- {line}\n"))
        };

        std::fs::write(&file, with("Should we release + publish the")).unwrap();
        let held = observe(&file, CONSUMER_CLI, true).unwrap().unwrap();
        assert!(held.items.is_empty(), "{:?}", held.items);
        std::fs::write(&file, with("Should we release + publish the C++ bindings?")).unwrap();
        let delivered = observe(&file, CONSUMER_CLI, true).unwrap().unwrap();
        assert_eq!(delivered.items.len(), 1, "{:?}", delivered.items);
        std::fs::write(
            &file,
            with("Should we release + publish the C++ bindings? Tag it too."),
        )
        .unwrap();
        observe(&file, CONSUMER_CLI, true).unwrap().unwrap();

        let rows = export_dataset(dir.path(), None, now_ms(), 45_000).unwrap();
        let fragment = rows
            .iter()
            .find(|r| r.row.phase != GatePhase::Delivered)
            .expect("the held fragment is logged");
        assert_eq!(fragment.row.document, "plan.md");
        assert_eq!(fragment.row.consumer, CONSUMER_CLI);
        assert_eq!(fragment.row.harness, "codex");
        assert_eq!(
            fragment.row.features.trailing_token,
            agent_doc_debounce::edit_settle::TrailingToken::Article
        );
        assert_eq!(
            fragment.row.tier,
            agent_doc_debounce::edit_settle::SettleTier::HeldUnfinished
        );
        assert!(fragment.row.superseded_at_ms.is_some());
        assert_eq!(fragment.row.label, Some(GateLabel::OnTime));
        let first_delivery = rows
            .iter()
            .find(|r| {
                r.row.phase == GatePhase::Delivered
                    && r.row.text_hash
                        == core::gate_text_hash("Should we release + publish the C++ bindings?")
            })
            .expect("the delivery is logged");
        assert!(first_delivery.row.re_edited_at_ms.is_some());
        assert_eq!(first_delivery.row.label, Some(GateLabel::Premature));
    }

    #[test]
    fn wake_receipt_round_trips_through_state_db() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "# S\n").unwrap();
        assert_eq!(load_wake_receipt(&file).unwrap(), None);
        record_wake_receipt(&file, "abc123", 2).unwrap();
        assert_eq!(load_wake_receipt(&file).unwrap().as_deref(), Some("abc123"));
    }
}
