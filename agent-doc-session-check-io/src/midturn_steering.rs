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
    let outgoing_base = load_watermark(&conn, &state_key("base", file))?;
    // `#claimedsteerwake`: what the agent channel last surfaced. The hook
    // consumer advances past the base as it surfaces; fall back to the base.
    let prior_acknowledged = match (
        &outgoing_base,
        load_watermark(&conn, &state_key(CONSUMER_HOOK, file))?,
    ) {
        (Some(base), _) if base.cycle_id == cycle_id => None,
        (Some(base), Some(hook)) if hook.cycle_id == base.cycle_id => Some(hook.acknowledged_queue),
        (Some(base), _) => Some(base.acknowledged_queue.clone()),
        (None, _) => None,
    };
    watermark.prior_cycle_queue = match outgoing_base {
        Some(existing) if existing.cycle_id == cycle_id => existing.prior_cycle_queue,
        Some(existing) => Some(existing.acknowledged_queue),
        None => None,
    };
    if let Some(prior) = prior_acknowledged {
        let owners = agent_doc_queue_io::queue_claim::claim_owners_for_content(file, baseline);
        if !owners.is_empty() {
            let carried = core::carry_unsurfaced_claimed_edits(&mut watermark, &prior, |text| {
                owners.contains_key(&agent_doc_queue::queue_claim::claim_identity(text))
            });
            if carried > 0 {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "midturn_steering_seed_carried_claimed_edits file={} cycle={cycle_id} count={carried} (#claimedsteerwake)",
                        file.display()
                    ),
                );
            }
        }
    }
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &state_key("base", file),
        &serde_json::to_string(&watermark)?,
        now_ms(),
    )
}

/// How long after an explicit send a document save still counts as part of
/// it: the editor may flush its buffer to disk just after the action fires.
pub const EXPLICIT_SEND_SAVE_GRACE_MS: u64 = 5_000;

/// The operator's explicit send (`#claimedsteerwake`): `Run Agent Doc` /
/// `agent-doc route` fired at `sent_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ExplicitSend {
    pub sent_at_ms: u64,
}

impl ExplicitSend {
    /// Whether a document last changed at `document_changed_ms` is covered:
    /// every edit up to the send (plus the save grace) was sent; anything typed
    /// later goes back through the passive typing gate.
    pub fn covers(&self, document_changed_ms: Option<u64>) -> bool {
        document_changed_ms
            .is_none_or(|changed| changed <= self.sent_at_ms + EXPLICIT_SEND_SAVE_GRACE_MS)
    }
}

fn explicit_send_key(file: &Path) -> String {
    state_key("explicit_send", file)
}

/// Record an explicit operator send for `file` (`#claimedsteerwake`). Every
/// steering consumer (the PostToolUse hook, the turn-boundary report, the idle
/// wake) then flushes the pending steering at once, bypassing the typing gate,
/// and marks it `sent=explicit`.
pub fn record_explicit_send(file: &Path) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let send = ExplicitSend {
        sent_at_ms: now_ms(),
    };
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &explicit_send_key(file),
        &serde_json::to_string(&send)?,
        send.sent_at_ms,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "steering_explicit_send file={} sent_at_ms={} (#claimedsteerwake)",
            file.display(),
            send.sent_at_ms
        ),
    );
    Ok(())
}

fn load_explicit_send(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
) -> Option<ExplicitSend> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        conn,
        &explicit_send_key(file),
    )
    .ok()
    .flatten()
    .and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Route queue items a worker claimed to their owner (`#claimedsteerwake`).
fn route_claimed_items(file: &Path, content: &str, items: &mut [SteeringItem]) {
    if !items
        .iter()
        .any(|item| item.source == core::SteeringSource::Queue)
    {
        return;
    }
    let owners = agent_doc_queue_io::queue_claim::claim_owners_for_content(file, content);
    if owners.is_empty() {
        return;
    }
    core::apply_claim_owners(items, |text| {
        owners
            .get(&agent_doc_queue::queue_claim::claim_identity(text))
            .cloned()
    });
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
            // `#steerbaselineabsorb`: a turn-boundary report frames each
            // exchange item as the NEXT cycle's prompt. When the closeout commit
            // already absorbed its text, the next preflight diffs clean, so
            // persist it for that contract to carry.
            if self.boundary {
                crate::absorbed_steering::record_reported_logged(
                    file,
                    &report.items,
                    &next.cycle_id,
                );
            }
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
    let selected_head_open = core::current_item_is_present(&watermark, &content);
    // `#openheadsteering`: stale-lock repair can terminalize the cycle record
    // while the selected queue head and its owning harness turn are still live.
    // Queue lifecycle evidence wins over that stale terminal projection. A
    // terminal report must stay silent; ordinary consumers keep in-turn wording.
    if selected_head_open && boundary {
        if policy == ClosedCyclePolicy::ForceBoundary {
            return Ok(None);
        }
        boundary = false;
    }
    if unchanged_gate
        && watermark.pending.is_empty()
        && watermark.last_observed_content_hash.as_deref()
            == Some(core::content_hash(&content).as_str())
    {
        watermark.last_observed_stat = Some(fingerprint);
        return prepared(Some(watermark), None, boundary, None);
    }

    let explicit_send =
        load_explicit_send(&conn, file).is_some_and(|send| send.covers(mtime_ms(&meta)));
    // The turn boundary is the last chance before the loop re-enters: an
    // item that is complete surfaces now rather than waiting out the window.
    let debounce_ms = if policy == ClosedCyclePolicy::ForceBoundary || explicit_send {
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
        explicit_send,
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
            closed if !core::current_item_is_present(&watermark, &content) => {
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
            Some(_) => {
                // A stale terminal cycle record does not close a queue head
                // that is visibly still selected in the authoritative text.
            }
            None => {
                // A seed without any surviving cycle record has no live turn
                // whose head can outrank the boundary. Preserve the legacy
                // committed-baseline fallback for polling-only consumers.
                boundary = true;
                observation = core::observe_with_mode(
                    &watermark,
                    &content,
                    &ctx,
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
        explicit: explicit_send,
        decisions: std::mem::take(&mut observation.decisions),
    };
    route_claimed_items(file, &content, &mut observation.ready);
    // `#steerbaselineabsorb`: a prompt a cycle already carried or answered is
    // never re-surfaced (the watermark still advances past it).
    crate::absorbed_steering::drop_consumed(file, &mut observation.ready);
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
    // A claimed item is in flight elsewhere, so its ADDITION never wakes the
    // session, nor does a tag/marker-only re-tag (same work identity,
    // `#claimdispatchidentity`). An edit of its substance (an appended note,
    // `#claimfollowsedit`) is operator steering for that work
    // (`#claimedsteerwake`): it wakes the coordinator, which forwards it to the
    // owner.
    let forwards_substance = |item: &SteeringItem| {
        item.dispatch == core::SteeringDispatch::ForwardToOwner
            && item.change == core::SteeringChange::Edited
            && item.previous.as_deref().is_some_and(|previous| {
                agent_doc_queue::queue_claim::claim_identity(previous)
                    != agent_doc_queue::queue_claim::claim_identity(&item.verbatim)
            })
    };
    let items: Vec<SteeringItem> = hook_report
        .items
        .iter()
        .filter(|item| {
            item.source != core::SteeringSource::Queue
                || forwards_substance(item)
                || !claimed.claims(&item.verbatim)
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

/// Steering an explicit send (`#claimedsteerwake`) handed to the owning
/// turn: the settled-or-not items the agent channel has not surfaced yet,
/// read without consuming (the owning turn's hook, its boundary report, or
/// the idle wake delivers them, `sent=explicit`).
pub fn explicit_send_pending_items(file: &Path) -> Result<Vec<SteeringItem>> {
    Ok(
        prepare_with_gate(file, CONSUMER_HOOK, ClosedCyclePolicy::Boundary, false)?
            .and_then(|prepared| prepared.report)
            .map(|report| report.items)
            .unwrap_or_default(),
    )
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

/// `#steerinterruptexit`: every settled steering item a committed cycle left
/// unanswered, each with its typed `dispatch`, for the `session-check`
/// `steering pending` status.
///
/// Stateless on purpose: it reads no consumer watermark and writes none, so a
/// repeated `session-check` keeps reporting the same pending steering until a
/// cycle answers it, and the hook / boundary report still deliver each item
/// exactly once through their own watermarks. The base is the committed turn
/// baseline (HEAD when there is no snapshot), observed in boundary mode with no
/// debounce, excluding the closed cycle's own queue bookkeeping. Items the
/// typing gate still holds are omitted; the status line's marker carries them
/// verbatim regardless.
pub fn pending_steering_items(file: &Path, current: &str) -> Vec<SteeringItem> {
    let baseline = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
        .or_else(|| agent_doc_git_io::revision::show_head(file).ok().flatten());
    let Some(baseline) = baseline else {
        return Vec::new();
    };
    let base = SteeringWatermark::seed(
        &format!("committed:{}", core::content_hash(&baseline)),
        &baseline,
        None,
        Vec::new(),
    );
    let owned = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten()
        .map(|cycle| binary_owned_ids(&cycle))
        .unwrap_or_default();
    let deterministic = agent_doc_debounce::edit_settle::DeterministicOnly;
    let ctx = ObserveContext {
        now_ms: now_ms(),
        document_changed_ms: None,
        debounce_ms: 0,
        binary_owned_queue_ids: &owned,
        explicit_send: false,
        max_hold_ms: max_hold_ms_for(file),
        classifier: &deterministic,
        median_pause_ms: None,
    };
    let mut ready =
        core::observe_with_mode(&base, current, &ctx, core::ObserveMode::Boundary).ready;
    route_claimed_items(file, current, &mut ready);
    crate::absorbed_steering::drop_consumed(file, &mut ready);
    ready
}

/// Whether `item` came from the exchange (a prompt only an agent cycle can
/// answer), as opposed to queue work the queue drain owns.
pub fn is_exchange_item(item: &SteeringItem) -> bool {
    item.source == core::SteeringSource::Exchange
}

/// Render [`pending_steering_items`] for the `steering pending` status: one
/// block per item, its dispatch and source first, then the operator's text
/// verbatim. `None` when there is nothing settled to list.
pub fn render_pending_steering_items(items: &[SteeringItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "[steering {}/{}] dispatch={} source={} change={}{}{}",
            idx + 1,
            items.len(),
            item.dispatch.as_str(),
            match item.source {
                core::SteeringSource::Exchange => "exchange",
                core::SteeringSource::Queue => "queue",
            },
            match item.change {
                core::SteeringChange::Added => "added",
                core::SteeringChange::Edited => "edited",
                core::SteeringChange::Deleted => "deleted",
            },
            if item.possibly_partial {
                " possibly_partial=true"
            } else {
                ""
            },
            item.owner
                .as_deref()
                .map(|owner| format!(" owner={owner}"))
                .unwrap_or_default(),
        ));
        if let Some(previous) = &item.previous {
            out.push_str(&format!("\nprevious: {previous}"));
        }
        let label = if item.change == core::SteeringChange::Deleted {
            "removed"
        } else {
            "verbatim"
        };
        out.push_str(&format!("\n{label}: {}", item.verbatim));
    }
    Some(out)
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
    /// Claude Code sets `agent_id` (and `agent_type`) only when the tool call
    /// ran inside a subagent; the parent's calls carry neither. Subagents
    /// share the parent's `session_id`, so without this the coordinator's
    /// steering leaked into (and was consumed by) its subagents' tool calls.
    #[serde(default)]
    pub agent_id: Option<String>,
}

impl PostToolUseInput {
    /// The tool call ran inside a subagent, not the turn that owns the
    /// document (`#steerowneronly`).
    pub fn is_subagent(&self) -> bool {
        self.agent_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
    }
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
    // `#steerowneronly`: steering belongs to the turn that owns the document.
    // A subagent's tool call neither receives it nor advances the watermark,
    // so the coordinator still gets every item at its own next tool call.
    if input.is_subagent() {
        return Ok(None);
    }
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

    /// GH #162: stale-lock repair marked the cycle committed while the selected
    /// head and owning harness turn were still open. Explicit steering must use
    /// in-turn wording, and the closeout renderer must remain silent.
    #[test]
    fn explicit_send_to_terminalized_cycle_with_live_head_addresses_now() {
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
        let sent = baseline.replace(
            "- current task\n",
            "- 🚧 current task\n- publish the release\n",
        );
        std::fs::write(&file, &sent).unwrap();
        record_explicit_send(&file).unwrap();
        // Models repair_preflight_stale_lock committing marker bookkeeping
        // without consuming/responding to the selected head.
        close_cycle(&file, &sent);

        let mut closeout = Vec::new();
        emit_closeout_steering(&file, &mut closeout);
        assert!(
            closeout.is_empty(),
            "{}",
            String::from_utf8_lossy(&closeout)
        );

        let report = observe(&file, CONSUMER_HOOK, false)
            .unwrap()
            .expect("in-turn steering report");
        assert!(!report.after_close, "{report:?}");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert!(report.items[0].explicit);
        assert_eq!(report.items[0].dispatch, core::SteeringDispatch::AddressNow);
        let rendered = report.render().unwrap();
        assert!(rendered.starts_with(core::STEERING_MARKER), "{rendered}");
        assert!(
            !rendered.contains("Your response is already committed"),
            "{rendered}"
        );
        assert!(rendered.contains("Address it in THIS turn"), "{rendered}");
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
        let late = mid
            .replace("- current task\n", "")
            .replace("- first addition\n", "- first addition\n- late addition\n");
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

    fn bind_session(root: &Path, file: &Path, session_id: &str) {
        agent_doc_codex_hook_io::save_state(
            root,
            &agent_doc_codex_hook_io::SessionState {
                session_id: session_id.to_string(),
                identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                doc_path: file.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("/agent-doc {}", file.display()),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: Some(true),
                updated_at: 1,
            },
        )
        .unwrap();
    }

    fn hook_payload(root: &Path, agent_id: Option<&str>) -> String {
        let mut payload = serde_json::json!({
            "session_id": "coord",
            "cwd": root,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        });
        if let Some(agent_id) = agent_id {
            payload["agent_id"] = serde_json::json!(agent_id);
            payload["agent_type"] = serde_json::json!("general-purpose");
        }
        payload.to_string()
    }

    /// `#steerowneronly` (#steerworks): a subagent's tool calls share the
    /// coordinator's `session_id`. They must neither receive the coordinator's
    /// steering nor consume it; the coordinator's own next tool call gets it.
    #[test]
    fn subagent_tool_calls_never_receive_or_consume_coordinator_steering() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
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
        bind_session(root, &file, "coord");
        std::fs::write(
            &file,
            baseline.replace(
                "- current task\n",
                "- current task\n- #subagents: https://github.com/btakita/agent-doc/issues/127\n",
            ),
        )
        .unwrap();
        backdate(&file);

        for _ in 0..2 {
            assert_eq!(
                post_tool_use_response(&hook_payload(root, Some("agent-a1b2"))).unwrap(),
                None,
                "a subagent tool call must stay silent"
            );
        }
        let delivered = post_tool_use_response(&hook_payload(root, None))
            .unwrap()
            .expect("the coordinator's own tool call delivers the item");
        let context = delivered["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("issues/127"), "{context}");
        assert_eq!(
            post_tool_use_response(&hook_payload(root, None)).unwrap(),
            None
        );
    }

    /// `#claimedsteerwake`: `Run Agent Doc` / `agent-doc route` is an explicit
    /// send. A line the typing gate would hold (unfinished shape, edited a
    /// moment ago) is delivered at once, final, marked `sent=explicit`.
    #[test]
    fn explicit_send_flushes_pending_steering_past_the_typing_gate() {
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
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- also publish the\n"),
        )
        .unwrap();
        let held = observe(&file, CONSUMER_HOOK, false).unwrap().unwrap();
        assert!(held.items.is_empty(), "passive delivery holds it: {held:?}");
        assert_eq!(held.pending, 1);

        record_explicit_send(&file).unwrap();
        let sent = observe(&file, CONSUMER_HOOK, true).unwrap().unwrap();
        assert_eq!(sent.items.len(), 1, "{sent:?}");
        assert!(sent.items[0].explicit && !sent.items[0].possibly_partial);
        assert!(sent.render().unwrap().contains("sent=explicit"));
        assert!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .is_none_or(|report| report.items.is_empty()),
            "exactly once"
        );

        // An edit typed after the send (past the save grace) is passive again.
        let later = std::fs::read_to_string(&file).unwrap().replace(
            "- also publish the\n",
            "- also publish the\n- and then the\n",
        );
        std::fs::write(&file, later).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(
                std::time::SystemTime::now()
                    + std::time::Duration::from_millis(EXPLICIT_SEND_SAVE_GRACE_MS + 60_000),
            )
            .unwrap();
        let passive = observe(&file, CONSUMER_HOOK, false).unwrap().unwrap();
        assert!(passive.items.is_empty(), "{passive:?}");
        assert_eq!(passive.pending, 1);
    }

    /// `#steerworks` items 1 + 3 (agent-doc-bugs.md, 2026-10-04): the operator
    /// annotates a CLAIMED `#gh-fix` head while the coordinator is idle. The
    /// claim follows the edit, the idle wake fires for it, the woken cycle's
    /// seed keeps it pending, and its first hook says "forward to owner", not
    /// "dispatch a new subagent". A further trailing-period edit routes the
    /// same way with no re-claim.
    #[test]
    fn claimed_head_edit_wakes_the_idle_coordinator_and_forwards_to_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let url = "https://github.com/btakita/agent-doc/issues/126";
        let head = format!("#subagents: #gh-fix {url}");
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 2500\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&file, &head, "subagent:ghfix126", 3600).unwrap();
        let closed = baseline.replace("- current task\n", "");
        std::fs::write(&file, &closed).unwrap();
        close_cycle(&file, &closed);
        let mut report = Vec::new();
        emit_closeout_steering(&file, &mut report);
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        // Idle: the operator annotates the claimed head.
        let annotated_head = format!("{head}: note this is in a Coder environment");
        let annotated = closed.replace(&format!("- {head}\n"), &format!("- {annotated_head}\n"));
        std::fs::write(&file, &annotated).unwrap();
        backdate(&file);
        let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &annotated);
        assert!(
            claimed.claims(&annotated_head),
            "the claim followed the edit"
        );
        let wake = observe_for_wake(&file).unwrap();
        assert_eq!(
            wake.items.len(),
            1,
            "the idle coordinator is woken: {wake:?}"
        );
        assert_eq!(
            wake.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner
        );
        assert_eq!(wake.items[0].owner.as_deref(), Some("subagent:ghfix126"));

        // The woken cycle: preflight seeds a NEW cycle from the annotated text.
        let woken =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&annotated), Some(&annotated))
                .unwrap();
        seed_for_cycle(&file, &woken.cycle_id, &annotated, None, Vec::new()).unwrap();
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("first hook");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        let text = hook.render().unwrap();
        assert!(
            text.contains(
                "dispatch=forward_to_owner source=queue change=edited owner=subagent:ghfix126"
            ),
            "{text}"
        );
        assert!(text.contains(&format!("previous: {head}")), "{text}");
        assert!(!text.contains("dispatch=subagent"), "{text}");

        // A trailing period: still the owner's work, no re-claim needed.
        let period = annotated.replace(&annotated_head, &format!("{annotated_head}."));
        std::fs::write(&file, &period).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert_eq!(
            hook.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner
        );
        let ledger =
            agent_doc_queue_io::queue_claim::load_ledger_following(&file, &period).unwrap();
        assert_eq!(ledger.claims.len(), 1, "{ledger:?}");
        assert_eq!(ledger.claims[0].owner, "subagent:ghfix126");
        // Closeout pruning keeps (and persists) the followed claim.
        agent_doc_queue_io::queue_claim::prune_closed_claims(&file, &period).unwrap();
        let stored = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims.len(), 1, "{stored:?}");
        assert_eq!(stored.claims[0].item_text, format!("{annotated_head}."));
    }

    /// `#steerbaselineabsorb` (agent-doc-bugs.md, 2026-10-04): the operator
    /// finished a prompt line while `respond` was committing, the commit
    /// absorbed it, and the steering hook kept re-surfacing the same item after
    /// a later cycle had answered and committed it. Once the absorbed prompt is
    /// answered, no steering consumer lists it again.
    #[test]
    fn answered_absorbed_steering_is_never_resurfaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let prompt = "testing to see if you pick this up.";
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange -->\nEarlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, baseline).unwrap();
        let first =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &first.cycle_id, baseline, None, Vec::new()).unwrap();

        // The closeout commit absorbs the operator's line into its baseline.
        let absorbed = baseline.replace("Answer.\n", &format!("Answer.\n\n{prompt}\n"));
        std::fs::write(&file, &absorbed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &absorbed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &absorbed);
        backdate(&file);

        // Control: the hook's stale cycle base still derives the item.
        let peek = observe(&file, CONSUMER_HOOK, false)
            .unwrap()
            .expect("hook peek");
        assert_eq!(peek.items.len(), 1, "{peek:?}");
        assert_eq!(peek.items[0].verbatim, prompt);
        assert_eq!(peek.items[0].dispatch, core::SteeringDispatch::AddressNow);
        // The `steering pending` report persists it: the committed baseline
        // carries the text, so the next preflight would otherwise diff clean.
        assert_eq!(
            crate::absorbed_steering::record_reported(&file, &peek.items, &first.cycle_id).unwrap(),
            vec![prompt.to_string()]
        );
        assert_eq!(
            crate::absorbed_steering::unanswered_prompts(&file).unwrap(),
            vec![prompt.to_string()]
        );

        // A later response cycle answers it and commits without a preflight
        // re-seed (a `respond` that reopens a committed cycle from HEAD).
        let second =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&absorbed), Some(&absorbed))
                .unwrap();
        assert_ne!(second.cycle_id, first.cycle_id);
        let answered = absorbed.replace(
            "<!-- /agent:exchange -->",
            "### Re: testing to see — opus\n\nYes, picked up.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&file, &answered).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &file,
            "test_capture",
            Some(&answered),
            Some(&answered),
            "sha-answer",
            Some(&second.cycle_id),
        )
        .unwrap();
        close_cycle(&file, &answered);
        backdate(&file);

        assert!(
            crate::absorbed_steering::unanswered_prompts(&file)
                .unwrap()
                .is_empty()
        );
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap();
        assert!(
            hook.as_ref().is_none_or(|report| report.items.is_empty()),
            "{hook:?}"
        );
        assert!(observe_for_wake(&file).unwrap().items.is_empty());
        assert!(pending_steering_items(&file, &answered).is_empty());
    }
}
