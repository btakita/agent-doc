//! `#netadv6` — deterministic simulation fuzzing over SimWorld.
//!
//! A seeded explorer drives the SAME SimWorld engine the seed corpus and the
//! scripted scenarios use (`SimWorld::apply`, the netadv4 `SimNet` routing, the
//! production decision predicates the engine calls) through random schedules:
//! process interleavings (idle ticks that let in-flight messages land between
//! steps), network faults (`coder_zscaler` / `hostile` SimNet profiles), operator
//! edits, recycles and installs, crashes and reconnects.
//!
//! After every step — and after every message delivery inside a step — invariant
//! oracles check the safety properties the TLA+ models in `formal/tla/` state.
//! Each oracle names the TLA invariant it mirrors ([`Oracle::tla`]). Liveness
//! ("a recycle request is eventually consumed") is checked once per schedule
//! after a deterministic fairness suffix: the channel drains, the open cycle and
//! IPC connection close, the supervisor reaches a turn boundary, and the idle
//! watch ticks.
//!
//! A schedule is a [`FuzzTrace`]: a list of steps, each sent message carrying the
//! explicit [`SendPlan`] the channel drew for it. A trace therefore replays
//! without the channel RNG, so a failing seed shrinks by dropping steps or
//! simplifying faults while the same finding kind still reproduces, and the
//! shrunk trace prints in a replayable text form that can be committed to
//! `src/sim_world/fuzz_seeds.txt` as a permanent regression seed.
//!
//! Budgets: `make check` (via `make test`) runs [`SHORT_BUDGET_SEEDS`] plus every
//! regression seed; `make sim-fuzz FUZZ_SECS=...` runs fresh seeds for a wall-clock
//! budget and writes each new finding kind's shrunk trace to `FUZZ_OUT`.

use super::net::{self, NetMode, link_for};
use super::*;
use agent_doc_sim_net::{NetProfile, SendPlan};
use agent_doc_supervisor::recycle_request::{
    RECYCLE_REQUEST_INSTALL_FANOUT, RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE, RecycleRequest,
    recycle_request, recycle_request_is_live,
};
use agent_doc_sync_io::layout_column_audit::{
    ColumnAdmission, ColumnGateFacts, PaneSupervisorFreshness, StaleRecycleRequestState,
    plan_column_admissions,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Seeds `make check` explores on every run (plus every regression seed).
pub(crate) const SHORT_BUDGET_SEEDS: std::ops::Range<u64> = 0..400;
/// Steps per generated schedule.
pub(crate) const FUZZ_STEPS: usize = 80;
/// Wall-clock seconds one `AdvanceWallClock` step represents.
pub(crate) const WALL_CLOCK_STEP_SECS: u64 = 300;

/// Diagnosed findings that are still intentionally open on this branch.
/// Reconciliation with netadv3/netadv5 and GH #136 leaves none: every retained
/// regression seed must now run clean, and any finding is a test failure.
pub(crate) const KNOWN_FUZZ_FINDINGS: [(&str, &str); 0] = [];

pub(crate) fn is_known_kind(kind: &str) -> bool {
    KNOWN_FUZZ_FINDINGS.iter().any(|(known, _)| *known == kind)
        || net::KNOWN_OPEN_NET_FINDINGS.contains(&kind)
}

/// The safety/liveness properties checked, each mirroring a TLA+ invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Oracle {
    UniqueOwner,
    NoLostOperatorText,
    ExactlyOnceResponseCommit,
    NoDispatchIntoBusySupervisor,
    NoStaleGenerationApply,
    StaleStashNeverWidens,
    RecycleEventuallyConsumed,
    /// netadv4's channel oracles (level-state reorder, duplicate dispatch).
    NetChannel,
    /// `assert_structural_invariants` / an engine error.
    Structural,
}

impl Oracle {
    pub(crate) const ALL: [Oracle; 9] = [
        Oracle::UniqueOwner,
        Oracle::NoLostOperatorText,
        Oracle::ExactlyOnceResponseCommit,
        Oracle::NoDispatchIntoBusySupervisor,
        Oracle::NoStaleGenerationApply,
        Oracle::StaleStashNeverWidens,
        Oracle::RecycleEventuallyConsumed,
        Oracle::NetChannel,
        Oracle::Structural,
    ];

    /// The TLA+ invariant(s) this oracle mirrors.
    pub(crate) fn tla(self) -> &'static str {
        match self {
            Oracle::UniqueOwner => {
                "PaneExecutionAuthority!AtMostOneOwnerGenerationMutates + \
                 PassiveTmuxSync!VisibleAndStashedActorsRemainAUniquePartition"
            }
            Oracle::NoLostOperatorText => {
                "RetainedProjectionHold!NoOperatorTextLoss + ConflictReconciliation!OperatorNeverLost"
            }
            Oracle::ExactlyOnceResponseCommit => {
                "CloseoutChurn!ResponseAppliedAtMostOnce + RetainedTransitionFixedPoint!AtMostOneResponse \
                 + AgentDocCloseout!RetainedResponseIsUnique"
            }
            Oracle::NoDispatchIntoBusySupervisor => {
                "RecycleSettleDispatch!DeliveredAtMostOnce (no inject across the recycle boundary); \
                 no TLA module states the busy-gate itself: gap, see HANDOFF"
            }
            Oracle::NoStaleGenerationApply => {
                "ReactiveTopology!StaleEffectNeverMutates + PaneExecutionAuthority!NonOwnerNeverMutates \
                 (+ NetChannelRetransmit!NoStaleApply on main)"
            }
            Oracle::StaleStashNeverWidens => {
                "StaleColumnRecycle!StashPromotionNeverWidens (GH #136, main)"
            }
            Oracle::RecycleEventuallyConsumed => {
                "SupervisorGenerationTransition!TerminalRequestEventuallyReplaced + \
                 StaleColumnRecycle!EventuallyConsumed (GH #136, main)"
            }
            Oracle::NetChannel => "NetChannelRetransmit!ExactlyOnce/NoStaleApply (main)",
            Oracle::Structural => "SimWorld structural invariants",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FuzzFinding {
    pub(crate) kind: String,
    pub(crate) oracle: Oracle,
    /// Index of the trace step during which the oracle fired (`steps.len()` =
    /// the fairness suffix / end-of-schedule check).
    pub(crate) step: usize,
    pub(crate) detail: String,
}

/// GH #136 sub-model: the durable safe-boundary recycle request for the stale
/// supervisor behind a stash pane, on a wall clock the fuzzer advances.
#[derive(Debug, Default)]
pub(crate) struct StaleColumnModel {
    pub(crate) request: Option<RecycleRequest>,
    pub(crate) now_secs: u64,
    /// A request was written while the supervisor ran replaced bytes.
    pub(crate) requested_while_stale: bool,
    /// What the last `SyncFocusStaleStashPane` did: (promoted a stale pane, widened).
    pub(crate) last_promotion: Option<(bool, bool)>,
}

/// Oracle state threaded through one fuzz schedule. `None` on every
/// non-fuzz world, so the corpus and scripted scenarios are unaffected.
#[derive(Debug, Default)]
pub(crate) struct FuzzState {
    pub(crate) step: usize,
    pub(crate) findings: Vec<FuzzFinding>,
    /// Operator prompt lines written so far; each must survive.
    pub(crate) operator_lines: Vec<String>,
    /// Logical clock ordering sends and local events.
    pub(crate) clock: u64,
    pub(crate) send_order: BTreeMap<u64, u64>,
    /// Newest known supervisor lifecycle per generation:
    /// (logical order, lifecycle, came from a supervisor report message).
    pub(crate) truth: BTreeMap<u64, (u64, SupervisorLifecycle, bool)>,
    /// The controller's durable lifecycle currently comes from a report older
    /// than the newest known lifecycle fact (a straggler overwrite).
    pub(crate) stale_overwrite_in_effect: bool,
    pub(crate) capture_epoch: u64,
    pub(crate) applied_epochs: BTreeSet<u64>,
    pub(crate) stale_column: StaleColumnModel,
    pub(crate) checks: BTreeMap<Oracle, usize>,
}

impl FuzzState {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn count(&mut self, oracle: Oracle) {
        *self.checks.entry(oracle).or_insert(0) += 1;
    }

    fn report(&mut self, oracle: Oracle, kind: &str, detail: String) {
        // First occurrence per kind per schedule is enough to shrink from.
        if self.findings.iter().any(|f| f.kind == kind) {
            return;
        }
        self.findings.push(FuzzFinding {
            kind: kind.to_string(),
            oracle,
            step: self.step,
            detail,
        });
    }
}

/// What changed across one event; compared by the per-event oracles.
#[derive(Debug, Clone)]
pub(crate) struct FuzzPre {
    route: RouteSnap,
    snapshot: String,
    doc_responses: usize,
    acceptances: usize,
    go_drains: usize,
    next_prompt: usize,
    stale_lifecycle_in_effect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteSnap {
    durable: ActorState,
    projection: ActorState,
    queue_control: QueueControlState,
    pending_dispatch: Option<DispatchReceipt>,
    lease: Option<u64>,
    starting_timeout: Option<(u64, String)>,
}

impl RouteSnap {
    fn of(route: &RouteModel) -> Self {
        Self {
            durable: route.durable.clone(),
            projection: route.projection.clone(),
            queue_control: route.queue_control,
            pending_dispatch: route.pending_dispatch.clone(),
            lease: route.supervisor_lease_generation,
            starting_timeout: route.starting_timeout.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FuzzEvent {
    Local(SimCommand),
    Delivered {
        command: SimCommand,
        id: u64,
        generation_at_send: u64,
    },
}

const RESPONSE_HEADING: &str = "### Re: sim closeout";

fn lifecycle_reported(command: SimCommand) -> Option<SupervisorLifecycle> {
    use SimCommand::*;
    Some(match command {
        SupervisorReady | SupervisorHeartbeatReattach | PromoteStartingPromptReady => {
            SupervisorLifecycle::Ready
        }
        SupervisorBusy => SupervisorLifecycle::Busy,
        SupervisorWaitingInput => SupervisorLifecycle::WaitingInput,
        SupervisorBlocked => SupervisorLifecycle::Blocked,
        SupervisorClosed => SupervisorLifecycle::Closed,
        _ => return None,
    })
}

/// Messages whose production counterpart carries the sender's generation and
/// must be fenced by it. The `*Stale` variants always act on generation - 1.
fn generation_fenced(command: SimCommand) -> Option<bool> {
    use SimCommand::*;
    match command {
        StaleSupervisorUpdate
        | SupervisorHeartbeatStale
        | AdminPauseQueueStale
        | AdminHandoffStale
        | AdminReapStale => Some(true),
        SupervisorReady
        | SupervisorBusy
        | SupervisorWaitingInput
        | SupervisorBlocked
        | SupervisorClosed
        | SupervisorHeartbeatReattach
        | PromoteStartingPromptReady
        | ProveDispatchAccepted
        | AdminPauseQueue
        | AdminResumeQueue
        | AdminDrainQueue
        | AdminHandoff
        | AdminReap => Some(false),
        _ => None,
    }
}

impl SimWorld {
    pub(crate) fn fuzz_pre(&self) -> Option<FuzzPre> {
        self.fuzz.as_ref()?;
        Some(FuzzPre {
            route: RouteSnap::of(&self.route),
            snapshot: self.snapshot.clone(),
            doc_responses: self.doc.matches(RESPONSE_HEADING).count(),
            acceptances: self.coverage.route_dispatch_acceptances,
            go_drains: self.coverage.go_drain_dispatches,
            next_prompt: self.next_prompt,
            stale_lifecycle_in_effect: self
                .net
                .as_ref()
                .is_some_and(|net| net.stale_lifecycle_in_effect.is_some()),
        })
    }

    /// A message left the sender: stamp its logical order.
    pub(crate) fn fuzz_on_send(&mut self, id: u64) {
        if let Some(fuzz) = self.fuzz.as_mut() {
            let order = fuzz.tick();
            fuzz.send_order.insert(id, order);
        }
    }

    /// Run every per-event oracle after `event`.
    pub(crate) fn fuzz_post(&mut self, pre: Option<FuzzPre>, event: FuzzEvent) {
        let Some(pre) = pre else {
            return;
        };
        let Some(mut fuzz) = self.fuzz.take() else {
            return;
        };
        let order = match event {
            FuzzEvent::Local(_) => fuzz.tick(),
            FuzzEvent::Delivered { id, .. } => fuzz.send_order.get(&id).copied().unwrap_or(0),
        };
        self.fuzz_track_operator_text(&mut fuzz, &pre, event);
        self.fuzz_track_truth(&mut fuzz, &pre, event, order);
        self.fuzz_check_unique_owner(&mut fuzz);
        self.fuzz_check_operator_text(&mut fuzz);
        self.fuzz_check_exactly_once_commit(&mut fuzz, &pre, event);
        self.fuzz_check_dispatch_gate(&mut fuzz, &pre, event);
        self.fuzz_check_stale_generation_apply(&mut fuzz, &pre, event);
        self.fuzz_check_stash_promotion(&mut fuzz, event);
        self.fuzz = Some(fuzz);
    }

    fn fuzz_track_operator_text(&self, fuzz: &mut FuzzState, pre: &FuzzPre, event: FuzzEvent) {
        let FuzzEvent::Local(command) = event else {
            return;
        };
        let n = pre.next_prompt;
        let line = match command {
            SimCommand::EditPrompt => {
                format!("❯ do #sim{n}. spec-test-build-install-commit-push\n")
            }
            SimCommand::EditLaterPrompt => format!("❯ later follow-up #sim{n}\n"),
            SimCommand::CaptureResponse | SimCommand::CaptureFallbackResponse => {
                fuzz.capture_epoch += 1;
                return;
            }
            SimCommand::ApplyCapturedResponse | SimCommand::Recover => {
                if self.doc.matches(RESPONSE_HEADING).count() > pre.doc_responses
                    && self.captured_response.is_some()
                {
                    fuzz.applied_epochs.insert(fuzz.capture_epoch);
                }
                return;
            }
            _ => return,
        };
        if self.next_prompt > n {
            fuzz.operator_lines.push(line);
        }
    }

    fn fuzz_track_truth(&self, fuzz: &mut FuzzState, pre: &FuzzPre, event: FuzzEvent, order: u64) {
        let after = (self.route.durable.generation, self.route.durable.lifecycle);
        let before = (pre.route.durable.generation, pre.route.durable.lifecycle);
        match event {
            FuzzEvent::Delivered {
                command,
                generation_at_send,
                ..
            } if lifecycle_reported(command).is_some()
                && generation_at_send == pre.route.durable.generation =>
            {
                // The newest lifecycle fact the controller has received for this
                // generation, in logical (send) order, whatever it applied.
                let reported = lifecycle_reported(command).expect("checked");
                let entry = fuzz
                    .truth
                    .entry(generation_at_send)
                    .or_insert((0, before.1, false));
                if order >= entry.0 {
                    *entry = (order, reported, true);
                    if after != before {
                        fuzz.stale_overwrite_in_effect = false;
                    }
                } else if after != before {
                    // An older report overwrote a newer fact. Report-vs-report
                    // reorders are netadv4's `stale_actor_lifecycle_applied_out_of_order`;
                    // a report overwriting a controller-side transition is not.
                    fuzz.stale_overwrite_in_effect = true;
                    if !entry.2 {
                        let newest = entry.1;
                        fuzz.report(
                            Oracle::NoStaleGenerationApply,
                            "stale_actor_lifecycle_overwrote_local_transition",
                            format!(
                                "{command:?} sent before the controller's {newest:?} transition (generation {generation_at_send}) applied after it: {:?} -> {:?}",
                                before.1, after.1
                            ),
                        );
                    }
                }
            }
            _ if after != before => {
                let from_report = matches!(event, FuzzEvent::Delivered { command, .. } if lifecycle_reported(command).is_some());
                fuzz.truth.insert(after.0, (order, after.1, from_report));
                fuzz.stale_overwrite_in_effect = false;
            }
            _ => {}
        }
    }

    /// PaneExecutionAuthority!AtMostOneOwnerGenerationMutates +
    /// PassiveTmuxSync!VisibleAndStashedActorsRemainAUniquePartition.
    fn fuzz_check_unique_owner(&self, fuzz: &mut FuzzState) {
        fuzz.count(Oracle::UniqueOwner);
        let generation = self.route.durable.generation;
        if let Some(lease) = self.route.supervisor_lease_generation
            && lease != generation
        {
            fuzz.report(
                Oracle::UniqueOwner,
                "owner_lease_held_by_non_current_generation",
                format!("lease generation {lease} != durable owner generation {generation}"),
            );
        }
        if let Some(receipt) = &self.route.pending_dispatch
            && receipt.generation > generation
        {
            fuzz.report(
                Oracle::UniqueOwner,
                "dispatch_receipt_from_future_owner",
                format!("receipt {receipt:?} vs owner generation {generation}"),
            );
        }
        let overlap: Vec<&String> = self
            .sync
            .visible
            .iter()
            .filter(|doc| self.sync.stashed.contains(*doc))
            .collect();
        if !overlap.is_empty() {
            fuzz.report(
                Oracle::UniqueOwner,
                "visible_and_stashed_partition_overlap",
                format!(
                    "{overlap:?} both visible and stashed: visible={:?} stashed={:?}",
                    self.sync.visible, self.sync.stashed
                ),
            );
        }
    }

    /// RetainedProjectionHold!NoOperatorTextLoss.
    fn fuzz_check_operator_text(&self, fuzz: &mut FuzzState) {
        fuzz.count(Oracle::NoLostOperatorText);
        if let Some(lost) = fuzz
            .operator_lines
            .iter()
            .find(|line| !self.doc.contains(line.as_str()))
            .cloned()
        {
            fuzz.report(
                Oracle::NoLostOperatorText,
                "operator_prompt_text_lost",
                format!("operator line {lost:?} no longer in the document"),
            );
        }
    }

    /// CloseoutChurn!ResponseAppliedAtMostOnce / RetainedTransitionFixedPoint!AtMostOneResponse:
    /// a commit never records more response blocks than distinct captures applied.
    fn fuzz_check_exactly_once_commit(
        &self,
        fuzz: &mut FuzzState,
        pre: &FuzzPre,
        event: FuzzEvent,
    ) {
        if self.snapshot == pre.snapshot {
            return;
        }
        fuzz.count(Oracle::ExactlyOnceResponseCommit);
        let committed = self.snapshot.matches(RESPONSE_HEADING).count();
        let distinct = fuzz.applied_epochs.len();
        if committed > distinct {
            fuzz.report(
                Oracle::ExactlyOnceResponseCommit,
                "committed_response_count_exceeds_distinct_captures",
                format!(
                    "{event:?} committed {committed} response block(s) for {distinct} distinct applied capture(s); phase={:?}",
                    self.phase
                ),
            );
        }
    }

    /// No dispatch is accepted into a supervisor whose newest received lifecycle
    /// report says it is not Ready, nor across a recycle boundary.
    fn fuzz_check_dispatch_gate(&self, fuzz: &mut FuzzState, pre: &FuzzPre, event: FuzzEvent) {
        let accepted = self.coverage.route_dispatch_acceptances > pre.acceptances
            || self.coverage.go_drain_dispatches > pre.go_drains;
        if !accepted {
            return;
        }
        fuzz.count(Oracle::NoDispatchIntoBusySupervisor);
        let generation = self.route.durable.generation;
        if self.route.recycle_inflight {
            fuzz.report(
                Oracle::NoDispatchIntoBusySupervisor,
                "dispatch_into_recycling_supervisor",
                format!("{event:?} accepted while recycle_inflight (generation {generation})"),
            );
        }
        let Some(&(_, truth, _)) = fuzz.truth.get(&generation) else {
            return;
        };
        if truth == SupervisorLifecycle::Ready {
            return;
        }
        let stale_now = self
            .net
            .as_ref()
            .is_some_and(|net| net.stale_lifecycle_in_effect.is_some());
        let kind = if pre.stale_lifecycle_in_effect || stale_now || fuzz.stale_overwrite_in_effect {
            "dispatch_accepted_on_reordered_stale_lifecycle"
        } else {
            "dispatch_into_busy_supervisor"
        };
        fuzz.report(
            Oracle::NoDispatchIntoBusySupervisor,
            kind,
            format!(
                "{event:?} accepted at generation {generation}: controller lifecycle {:?}, newest received report {truth:?}",
                self.route.durable.lifecycle
            ),
        );
    }

    /// ReactiveTopology!StaleEffectNeverMutates / PaneExecutionAuthority!NonOwnerNeverMutates.
    fn fuzz_check_stale_generation_apply(
        &self,
        fuzz: &mut FuzzState,
        pre: &FuzzPre,
        event: FuzzEvent,
    ) {
        let FuzzEvent::Delivered {
            command,
            generation_at_send,
            ..
        } = event
        else {
            return;
        };
        let Some(always_stale) = generation_fenced(command) else {
            return;
        };
        let stale = always_stale || generation_at_send != pre.route.durable.generation;
        if !stale {
            return;
        }
        fuzz.count(Oracle::NoStaleGenerationApply);
        let after = RouteSnap::of(&self.route);
        if after != pre.route {
            fuzz.report(
                Oracle::NoStaleGenerationApply,
                &format!("stale_generation_{}_mutated_route", snake(command)),
                format!(
                    "{command:?} sent at generation {generation_at_send} delivered at generation {}: route {:?} -> {:?}",
                    pre.route.durable.generation, pre.route, after
                ),
            );
        }
    }

    /// StaleColumnRecycle!StashPromotionNeverWidens.
    fn fuzz_check_stash_promotion(&self, fuzz: &mut FuzzState, event: FuzzEvent) {
        if !matches!(event, FuzzEvent::Local(SimCommand::SyncFocusStaleStashPane)) {
            return;
        }
        let Some((stale_promoted, widened)) = fuzz.stale_column.last_promotion.take() else {
            return;
        };
        fuzz.count(Oracle::StaleStashNeverWidens);
        if stale_promoted && widened {
            fuzz.report(
                Oracle::StaleStashNeverWidens,
                "stale_stash_pane_widened_layout",
                format!(
                    "a stale-supervisor stash pane was promoted by ADDING a column: visible={:?}",
                    self.sync.visible
                ),
            );
        }
    }

    // ---- GH #136 sub-model (fuzz-only commands) --------------------------------

    /// True when a fuzz world holds a recycle request the supervisor must honour
    /// like an `explicit_admin` recycle (production `recycle_request_is_live`).
    pub(crate) fn fuzz_recycle_request_live(&self) -> bool {
        self.fuzz.as_ref().is_some_and(|fuzz| {
            fuzz.stale_column.request.as_ref().is_some_and(|request| {
                recycle_request_is_live(
                    request,
                    fuzz.stale_column.now_secs,
                    self.recycle_clear.binary_stale,
                )
            })
        })
    }

    /// A recycle consumed the request.
    pub(crate) fn fuzz_settle_recycle_request(&mut self) {
        if let Some(fuzz) = self.fuzz.as_mut() {
            fuzz.stale_column.request = None;
            // Consumed: a later staleness with no new request is the opted-out
            // surface-only state, not an outstanding obligation.
            fuzz.stale_column.requested_while_stale = false;
        }
    }

    /// `make install` fan-out: the binary is replaced and every open supervisor
    /// gets an `install_fanout` request (replacing any prior request, as the
    /// base projection does).
    pub(crate) fn fuzz_install_fanout(&mut self) {
        self.recycle_clear.binary_stale = true;
        if let Some(fuzz) = self.fuzz.as_mut() {
            let now = fuzz.stale_column.now_secs;
            fuzz.stale_column.request = Some(recycle_request(RECYCLE_REQUEST_INSTALL_FANOUT, now));
            fuzz.stale_column.requested_while_stale = true;
        }
    }

    pub(crate) fn fuzz_advance_wall_clock(&mut self) {
        if let Some(fuzz) = self.fuzz.as_mut() {
            fuzz.stale_column.now_secs += WALL_CLOCK_STEP_SECS;
        }
    }

    /// GH #136: an editor focus publication asks for the first stashed document's
    /// pane as an extra column. Admission goes through the production
    /// `plan_column_admissions`; a stale supervisor (binary replaced) requests a
    /// safe-boundary recycle when it has no live request.
    pub(crate) fn fuzz_sync_focus_stale_stash_pane(&mut self) {
        let Some(focus) = self.sync.stashed.iter().next().cloned() else {
            return;
        };
        let stale = self.recycle_clear.binary_stale;
        let fresh = PaneSupervisorFreshness::Current {
            supervisor_pid: 1,
            title_marker: false,
        };
        let request_live = self.fuzz_recycle_request_live();
        let mut facts: Vec<ColumnGateFacts> = self
            .sync
            .visible
            .iter()
            .map(|doc| ColumnGateFacts {
                file: PathBuf::from(format!("{doc}.md")),
                pane: doc.clone(),
                freshness: fresh.clone(),
                own_pane: true,
                is_focus: false,
                outside_target_window: false,
                recycle: StaleRecycleRequestState::NotRequested,
                turn_active: self.sync.protected_open_cycle.contains(doc),
            })
            .collect();
        facts.push(ColumnGateFacts {
            file: PathBuf::from(format!("{focus}.md")),
            pane: focus.clone(),
            freshness: if stale {
                PaneSupervisorFreshness::Stale {
                    supervisor_pid: Some(2),
                    evidence: "sim_binary_replaced",
                }
            } else {
                fresh
            },
            own_pane: true,
            is_focus: true,
            outside_target_window: true,
            recycle: if request_live {
                StaleRecycleRequestState::Pending {
                    reason: "sim_live_recycle_request".to_string(),
                    age_secs: 0,
                    deferred_by_turn: self.recycle_clear.cycle_open,
                }
            } else {
                StaleRecycleRequestState::NotRequested
            },
            turn_active: self.sync.protected_open_cycle.contains(&focus),
        });
        let live_turn: Vec<String> = self
            .sync
            .visible
            .iter()
            .filter(|doc| self.sync.protected_open_cycle.contains(*doc))
            .cloned()
            .collect();
        let window_pane_count = self.sync.visible.len();
        let plan =
            plan_column_admissions(&facts, &|| Some(window_pane_count), &|| live_turn.clone());
        let admission = plan.last().map(|(admission, _)| *admission);
        let mut promotion = (false, false);
        if matches!(
            admission,
            Some(ColumnAdmission::Admit | ColumnAdmission::AdmitStaleFocused)
        ) {
            let before = self.sync.visible.len();
            self.sync.stashed.remove(&focus);
            self.sync.visible.push(focus.clone());
            self.sync.active = Some(focus);
            promotion = (
                admission == Some(ColumnAdmission::AdmitStaleFocused),
                self.sync.visible.len() > before,
            );
        }
        if let Some(fuzz) = self.fuzz.as_mut() {
            fuzz.stale_column.last_promotion = Some(promotion);
            if stale && !request_live {
                let now = fuzz.stale_column.now_secs;
                fuzz.stale_column.request = Some(recycle_request(
                    RECYCLE_REQUEST_STALE_SUPERVISOR_TURN_STAGE,
                    now,
                ));
                fuzz.stale_column.requested_while_stale = true;
            }
        }
    }

    /// Fairness suffix + liveness oracles, run once at the end of a schedule with
    /// the channel already drained and detached.
    fn fuzz_fair_suffix_and_liveness(&mut self) {
        for _ in 0..4 {
            for command in [
                SimCommand::SetAgentDocCycleOpen(false),
                SimCommand::MarkIpcInflight(false),
                SimCommand::SettleSupervisorRecycle,
                SimCommand::RepairProjection,
            ] {
                let _ = self.apply(command);
            }
            let to_ready: &[SimCommand] = match self.route.durable.lifecycle {
                SupervisorLifecycle::Ready => &[],
                SupervisorLifecycle::Starting => &[SimCommand::PromoteStartingPromptReady],
                SupervisorLifecycle::Busy
                | SupervisorLifecycle::WaitingInput
                | SupervisorLifecycle::Blocked => &[SimCommand::SupervisorReady],
                SupervisorLifecycle::Closed | SupervisorLifecycle::Dead => &[
                    SimCommand::BindRouteOwner,
                    SimCommand::PromoteStartingPromptReady,
                ],
            };
            for &command in to_ready {
                let _ = self.apply(command);
            }
            for _ in 0..3 {
                let _ = self.apply(SimCommand::SupervisorIdleQueueTick);
            }
        }
        let request_live = self.fuzz_recycle_request_live();
        let Some(mut fuzz) = self.fuzz.take() else {
            return;
        };
        fuzz.count(Oracle::RecycleEventuallyConsumed);
        let rc = &self.recycle_clear;
        if !rc.recycle_disabled && (rc.operator_recycle_marked || rc.restart_requested) {
            fuzz.report(
                Oracle::RecycleEventuallyConsumed,
                "recycle_request_never_consumed",
                format!(
                    "after the fairness suffix: operator_recycle_marked={} restart_requested={} lifecycle={:?} cycle_open={} binary_stale={}",
                    rc.operator_recycle_marked,
                    rc.restart_requested,
                    self.route.durable.lifecycle,
                    rc.cycle_open,
                    rc.binary_stale
                ),
            );
        }
        if fuzz.stale_column.requested_while_stale && rc.binary_stale && !rc.recycle_disabled {
            let kind = if fuzz.stale_column.request.is_some() && !request_live {
                "stale_recycle_request_lapsed_unconsumed"
            } else {
                "stale_recycle_request_never_consumed"
            };
            fuzz.report(
                Oracle::RecycleEventuallyConsumed,
                kind,
                format!(
                    "supervisor still stale after the fairness suffix: request={:?} live={request_live} now_secs={} auto_recycle={}",
                    fuzz.stale_column.request,
                    fuzz.stale_column.now_secs,
                    rc.auto_recycle
                ),
            );
        }
        self.fuzz = Some(fuzz);
    }
}

fn snake(command: SimCommand) -> String {
    let name = format!("{command:?}");
    let name = name.split(['(', ' ']).next().unwrap_or_default();
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

// ---- schedules ---------------------------------------------------------------

/// The commands the explorer draws from, with weights.
pub(crate) fn palette() -> Vec<(u32, SimCommand)> {
    use SimCommand::*;
    let mut palette = vec![
        // Operator edits.
        (5, EditPrompt),
        (2, EditLaterPrompt),
        (1, AddMalformedBacklogItem),
        // Closeout.
        (3, CaptureResponse),
        (1, CaptureFallbackResponse),
        (3, ApplyCapturedResponse),
        (3, Commit),
        (1, FailCommit),
        (1, RepairBoundary),
        (1, DuplicateVisibleResponse),
        (1, PostCommitIpcRepositionSignal),
        // Crash / recovery.
        (3, Recover),
        (1, AbandonSupervisorToDeadSocket),
        (1, RecoverControllerDispatchMarkers),
        // Session and ownership.
        (1, SessionClear),
        (1, SessionRestart),
        (1, SessionRestartForce),
        (1, SessionRestartForcePreInterruptIdle),
        (2, BindRouteOwner),
        // Supervisor reports.
        (4, SupervisorReady),
        (3, SupervisorBusy),
        (1, SupervisorWaitingInput),
        (1, SupervisorBlocked),
        (1, SupervisorClosed),
        (1, StaleSupervisorUpdate),
        (1, SupervisorHeartbeatReattach),
        (1, SupervisorHeartbeatStale),
        (2, PromoteStartingPromptReady),
        (1, BusyInterruptRecoveryReady),
        (1, RepairBusyProjectionWithReadyPrompt),
        // Dispatch.
        (4, DispatchRoutePrompt),
        (1, DispatchOperatorPrompt),
        (3, ProveDispatchAccepted),
        (1, DispatchAutoStartRoutePrompt),
        (1, DispatchIdleQueueDrainAfterRestart),
        (1, DispatchDuringSupervisorRecycle),
        // tmux observations.
        (1, ObserveStalePane),
        (1, ObserveMissingPane),
        (1, DriftProjection),
        (2, RepairProjection),
        // Layout sync.
        (1, SyncProtectedGrowthManual),
        (1, SyncProtectedGrowthPassive),
        (1, SyncProtectedGrowthFocusVisible),
        (1, SyncDetachableReplaceManual),
        (1, SyncDetachableReplacePassive),
        (1, SyncVisibleFocusPreserve),
        (1, SyncRerequestVisibleEditorManual),
        (1, SyncRerequestVisibleEditorPassive),
        (1, SyncFocusStashedMoveBeforeSelect),
        (1, SyncExactVisibleFocusResolvesOffscreenOwner),
        (1, SyncExactVisibleFocusUnprovenPreserve),
        (3, SyncFocusStaleStashPane),
        // Admin.
        (1, AdminPauseQueue),
        (1, AdminPauseQueueStale),
        (2, AdminResumeQueue),
        (1, AdminDrainQueue),
        (1, AdminHandoff),
        (1, AdminHandoffStale),
        (1, AdminReap),
        (1, AdminReapStale),
        // Recycle / install.
        (2, InstallFanout),
        (1, MarkSupervisorBinaryStale),
        (1, OperatorRecycleMark),
        (1, RequestSupervisorRestart),
        (5, SupervisorIdleQueueTick),
        (2, SetAgentDocCycleOpen(true)),
        (2, SetAgentDocCycleOpen(false)),
        (1, MarkIpcInflight(true)),
        (1, MarkIpcInflight(false)),
        (1, SupervisorRecycleBoot),
        (1, MarkReexecWillFail),
        (1, MarkWriteWedged),
        (1, MarkSupervisorRecycleInflight),
        (2, SettleSupervisorRecycle),
        (1, DisableSupervisorAutoRecycle),
        (1, EnableSupervisorAutoRecycle),
        (1, ActivateGoModeQueueHead),
        (1, SupervisorContextResetClear),
        (1, SetTriggerAlreadyPending(true)),
        (1, SetTriggerAlreadyPending(false)),
        (1, QueueBetweenTurnFreshContextHandoff),
        (1, DeferOperatorClearPending),
        (2, AdvanceWallClock),
    ];
    for fault in FaultPoint::ALL {
        palette.push((1, CrashAt(fault)));
    }
    palette
}

fn parse_command(text: &str) -> Option<SimCommand> {
    palette()
        .into_iter()
        .map(|(_, command)| command)
        .find(|command| format!("{command:?}") == text)
}

/// Happy-path protocol fragments spliced into random schedules.
const FRAGMENTS: [&[SimCommand]; 4] = [
    // One closeout cycle.
    &[
        SimCommand::EditPrompt,
        SimCommand::CaptureResponse,
        SimCommand::ApplyCapturedResponse,
        SimCommand::Commit,
    ],
    // One routed turn.
    &[
        SimCommand::SupervisorReady,
        SimCommand::DispatchRoutePrompt,
        SimCommand::ProveDispatchAccepted,
        SimCommand::SupervisorBusy,
        SimCommand::SupervisorReady,
    ],
    // Install, then reach a safe recycle boundary.
    &[
        SimCommand::InstallFanout,
        SimCommand::SetAgentDocCycleOpen(false),
        SimCommand::SupervisorReady,
        SimCommand::SupervisorIdleQueueTick,
    ],
    // Ownership change, then readiness.
    &[
        SimCommand::BindRouteOwner,
        SimCommand::PromoteStartingPromptReady,
        SimCommand::DispatchRoutePrompt,
    ],
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FuzzStep {
    /// Apply one command. `plan` is the delivery schedule of the message it
    /// sends, when it crosses a process hop under an adversarial profile.
    Cmd {
        command: SimCommand,
        plan: Option<SendPlan>,
    },
    /// Advance virtual time by `n` scheduler ticks with no new step, letting
    /// in-flight messages land (a process interleaving).
    Tick(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FuzzTrace {
    pub(crate) profile: NetProfile,
    pub(crate) steps: Vec<FuzzStep>,
}

impl FuzzTrace {
    /// Draw a schedule for `seed`. Plans are filled in when it first runs.
    pub(crate) fn generate(seed: u64, len: usize) -> Self {
        let profile = NetProfile::ALL[(seed % 3) as usize];
        let palette = palette();
        let total: u32 = palette.iter().map(|(weight, _)| weight).sum();
        let mut rng = DeterministicRng::new(seed ^ 0x6e65_7461_6476_3600);
        let mut steps = Vec::with_capacity(len + 8);
        while steps.len() < len {
            if rng.next_usize(8) == 0 {
                steps.push(FuzzStep::Tick(1 + rng.next_usize(20) as u32));
                continue;
            }
            if rng.next_usize(6) == 0 {
                // A protocol fragment: the happy-path sequence the random walk
                // rarely assembles on its own, so faults land INSIDE real cycles.
                let fragment = FRAGMENTS[rng.next_usize(FRAGMENTS.len())];
                steps.extend(fragment.iter().map(|&command| FuzzStep::Cmd {
                    command,
                    plan: None,
                }));
                continue;
            }
            let mut pick = rng.next_usize(total as usize) as u32;
            let command = palette
                .iter()
                .find(|(weight, _)| {
                    if pick < *weight {
                        true
                    } else {
                        pick -= weight;
                        false
                    }
                })
                .map(|(_, command)| *command)
                .expect("weighted pick");
            steps.push(FuzzStep::Cmd {
                command,
                plan: None,
            });
        }
        Self { profile, steps }
    }

    /// Replayable text form.
    pub(crate) fn to_text(&self) -> String {
        let mut out = format!("profile {}\n", self.profile.name());
        for step in &self.steps {
            match step {
                FuzzStep::Tick(n) => {
                    let _ = writeln!(out, "tick {n}");
                }
                FuzzStep::Cmd { command, plan } => {
                    let _ = write!(out, "step {command:?}");
                    if let Some(plan) = plan {
                        let _ = write!(out, " {}", plan.to_token());
                    }
                    out.push('\n');
                }
            }
        }
        out
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        let mut profile = None;
        let mut steps = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix("profile ") {
                profile = Some(
                    NetProfile::parse(name).ok_or_else(|| anyhow!("unknown profile {name:?}"))?,
                );
            } else if let Some(n) = line.strip_prefix("tick ") {
                steps.push(FuzzStep::Tick(n.trim().parse()?));
            } else if let Some(rest) = line.strip_prefix("step ") {
                let (command_text, plan) = match rest.rsplit_once(' ') {
                    Some((head, token)) if token.starts_with('@') => (
                        head,
                        Some(
                            SendPlan::parse_token(token)
                                .ok_or_else(|| anyhow!("bad plan token {token:?}"))?,
                        ),
                    ),
                    _ => (rest, None),
                };
                let command = parse_command(command_text.trim())
                    .ok_or_else(|| anyhow!("unknown fuzz command {command_text:?}"))?;
                steps.push(FuzzStep::Cmd { command, plan });
            } else {
                bail!("unparseable fuzz trace line {line:?}");
            }
        }
        Ok(Self {
            profile: profile.ok_or_else(|| anyhow!("fuzz trace has no `profile` line"))?,
            steps,
        })
    }
}

/// The outcome of one schedule.
#[derive(Debug, Default)]
pub(crate) struct FuzzRun {
    pub(crate) findings: Vec<FuzzFinding>,
    pub(crate) checks: BTreeMap<Oracle, usize>,
    pub(crate) acceptances: usize,
    pub(crate) commits: usize,
    pub(crate) recycles: usize,
    pub(crate) net_stats: Option<agent_doc_sim_net::NetStats>,
    pub(crate) reconnects_handled: u64,
}

impl FuzzRun {
    pub(crate) fn kinds(&self) -> BTreeSet<&str> {
        self.findings.iter().map(|f| f.kind.as_str()).collect()
    }

    pub(crate) fn new_findings(&self) -> Vec<&FuzzFinding> {
        self.findings
            .iter()
            .filter(|finding| !is_known_kind(&finding.kind))
            .collect()
    }

    pub(crate) fn has_kind(&self, kind: &str) -> bool {
        self.findings.iter().any(|f| f.kind == kind)
    }
}

fn absorb_net_findings(world: &mut SimWorld) {
    let Some(net) = world.net.as_mut() else {
        return;
    };
    let found = std::mem::take(&mut net.findings);
    if let Some(fuzz) = world.fuzz.as_mut() {
        for finding in found {
            fuzz.report(Oracle::NetChannel, finding.kind, finding.detail);
        }
    }
}

fn structural(world: &mut SimWorld, err: &anyhow::Error) {
    let message = err.to_string();
    let head = message
        .split(';')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let kind: String = head
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if let Some(fuzz) = world.fuzz.as_mut() {
        fuzz.report(Oracle::Structural, &format!("structural_{kind}"), message);
    }
}

/// Commands whose fault model presupposes some state. A disabled step is a
/// no-op, in generation and in replay alike, so shrinking can never manufacture
/// a schedule the real system cannot reach.
fn step_enabled(world: &SimWorld, command: SimCommand) -> bool {
    match command {
        // A duplicated visible response duplicates one that is already visible.
        // A duplicated visible response duplicates the response that is CURRENTLY
        // the last exchange block. After a later operator prompt, a same-heading
        // block is indistinguishable from a legitimate answer to that prompt (or
        // operator-authored text), so it is not a duplicate fault.
        SimCommand::DuplicateVisibleResponse => world
            .component_content("exchange")
            .ok()
            .and_then(|body| {
                body.rfind(RESPONSE_HEADING)
                    .map(|at| !body[at..].contains("❯ "))
            })
            .unwrap_or(false),
        // `recycle_inflight` is the marker a supervisor publishes immediately
        // before its own `execve`; its idle watch does not run again until the
        // replacement settles the marker.
        SimCommand::SupervisorIdleQueueTick => !world.route.recycle_inflight,
        _ => true,
    }
}

/// Run `trace`. With `record = Some(net_seed)` the channel draws faults from its
/// seeded RNG and the drawn plans are written back into `trace`; with `None`
/// every sent message follows its recorded plan (a missing plan = clean).
pub(crate) fn execute(trace: &mut FuzzTrace, record: Option<u64>) -> FuzzRun {
    let mut world = SimWorld::new_local(0x006e_6574_6164_7636).with_net(
        trace.profile,
        record.unwrap_or(0),
        NetMode::Async,
    );
    world.fuzz = Some(Box::default());
    let mut failed = false;
    for index in 0..trace.steps.len() {
        if let Some(fuzz) = world.fuzz.as_mut() {
            fuzz.step = index;
        }
        let result = match &mut trace.steps[index] {
            FuzzStep::Tick(n) => {
                let mut result = Ok(());
                for _ in 0..*n {
                    result = world.net_tick();
                    if result.is_err() {
                        break;
                    }
                }
                result
            }
            FuzzStep::Cmd { command, .. } if !step_enabled(&world, *command) => Ok(()),
            FuzzStep::Cmd { command, plan } => {
                let crosses = world.net.is_some() && link_for(*command).is_some();
                if crosses && record.is_none() {
                    let next = plan.clone().unwrap_or_else(SendPlan::clean);
                    world.net.as_mut().expect("crosses").next_plan = Some(next);
                }
                let result = world.apply(*command);
                if let Some(net) = world.net.as_mut() {
                    let sent = net.last_plan.take();
                    if crosses && record.is_some() {
                        *plan = sent;
                    }
                }
                result.and_then(|()| world.assert_structural_invariants())
            }
        };
        absorb_net_findings(&mut world);
        if let Err(err) = result {
            structural(&mut world, &err);
            failed = true;
            break;
        }
    }
    let mut run = FuzzRun::default();
    if !failed {
        if let Some(fuzz) = world.fuzz.as_mut() {
            fuzz.step = trace.steps.len();
        }
        // Fairness: every retransmitted message eventually arrives.
        let drained = world.net_drain();
        absorb_net_findings(&mut world);
        match drained {
            Ok(()) => {
                if let Some(mut net) = world.net.take() {
                    run.net_stats = Some(net.stats());
                    run.reconnects_handled = net.reconnect_resyncs;
                    net.findings.clear();
                }
                world.fuzz_fair_suffix_and_liveness();
            }
            Err(err) => structural(&mut world, &err),
        }
    }
    if run.net_stats.is_none()
        && let Some(net) = world.net.as_mut()
    {
        run.net_stats = Some(net.stats());
        net.findings.clear();
    }
    let fuzz = world.fuzz.take().expect("fuzz state");
    run.findings = fuzz.findings;
    run.checks = fuzz.checks;
    run.acceptances =
        world.coverage.route_dispatch_acceptances + world.coverage.go_drain_dispatches;
    run.commits = world.coverage.commits;
    run.recycles =
        world.coverage.supervisor_recycles + world.coverage.supervisor_restart_drain_reexecs;
    run
}

/// Generate and run the schedule for `seed`, recording its plans.
pub(crate) fn explore_seed(seed: u64, len: usize) -> (FuzzTrace, FuzzRun) {
    let mut trace = FuzzTrace::generate(seed, len);
    let run = execute(&mut trace, Some(seed));
    (trace, run)
}

fn replay_has(trace: &FuzzTrace, kind: &str) -> bool {
    let mut candidate = trace.clone();
    execute(&mut candidate, None).has_kind(kind)
}

/// Reduce `trace` (already recorded) to a minimal schedule that still produces a
/// finding of `kind`: drop chunks of steps (delta debugging), then simplify each
/// remaining fault to a clean delivery and shorten idle ticks, to a fixpoint.
pub(crate) fn shrink(trace: &FuzzTrace, kind: &str) -> FuzzTrace {
    shrink_where(trace, &|candidate| replay_has(candidate, kind))
}

fn shrink_where(trace: &FuzzTrace, predicate: &impl Fn(&FuzzTrace) -> bool) -> FuzzTrace {
    let mut best = trace.clone();
    if !predicate(&best) {
        // The recorded plans should reproduce exactly; keep the original if not.
        return best;
    }
    let mut changed = true;
    let mut rounds = 0;
    while changed && rounds < 8 {
        changed = false;
        rounds += 1;
        let mut chunk = best.steps.len().div_ceil(2).max(1);
        loop {
            let mut start = 0;
            while start < best.steps.len() {
                let end = (start + chunk).min(best.steps.len());
                let mut candidate = best.clone();
                candidate.steps.drain(start..end);
                if predicate(&candidate) {
                    best = candidate;
                    changed = true;
                } else {
                    start = end;
                }
            }
            if chunk == 1 {
                break;
            }
            chunk = chunk.div_ceil(2);
        }
        for index in 0..best.steps.len() {
            let simpler = match &best.steps[index] {
                FuzzStep::Cmd {
                    command,
                    plan: Some(plan),
                } if *plan != SendPlan::clean() => FuzzStep::Cmd {
                    command: *command,
                    plan: Some(SendPlan::clean()),
                },
                FuzzStep::Tick(n) if *n > 1 => FuzzStep::Tick(1),
                _ => continue,
            };
            let mut candidate = best.clone();
            candidate.steps[index] = simpler;
            if predicate(&candidate) {
                best = candidate;
                changed = true;
            }
        }
        if best.profile != NetProfile::Local {
            let mut candidate = best.clone();
            candidate.profile = NetProfile::Local;
            for step in &mut candidate.steps {
                if let FuzzStep::Cmd { plan, .. } = step {
                    *plan = None;
                }
            }
            if predicate(&candidate) {
                best = candidate;
                changed = true;
            }
        }
    }
    best
}

/// One committed regression seed.
#[derive(Debug)]
pub(crate) struct RegressionSeed {
    pub(crate) name: String,
    /// `None` = the trace must run clean; `Some(kind)` = a still-open known
    /// defect the trace must keep reproducing (flip to `clean` once fixed).
    pub(crate) expect_known: Option<String>,
    pub(crate) trace: FuzzTrace,
}

pub(crate) const REGRESSION_SEEDS: &str = include_str!("fuzz_seeds.txt");

pub(crate) fn parse_regression_seeds(text: &str) -> Result<Vec<RegressionSeed>> {
    let mut seeds = Vec::new();
    let mut current: Option<(String, Option<Option<String>>, String)> = None;
    let finish = |current: Option<(String, Option<Option<String>>, String)>,
                  seeds: &mut Vec<RegressionSeed>|
     -> Result<()> {
        if let Some((name, expect, body)) = current {
            let expect_known =
                expect.ok_or_else(|| anyhow!("regression seed {name:?} has no `expect` line"))?;
            seeds.push(RegressionSeed {
                trace: FuzzTrace::parse(&body)
                    .map_err(|err| anyhow!("regression seed {name:?}: {err}"))?,
                name,
                expect_known,
            });
        }
        Ok(())
    };
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("=== ") {
            finish(current.take(), &mut seeds)?;
            current = Some((name.trim().to_string(), None, String::new()));
        } else if let Some((_, expect, body)) = current.as_mut() {
            if let Some(rest) = line.trim().strip_prefix("expect ") {
                *expect = Some(match rest.trim() {
                    "clean" => None,
                    other => Some(
                        other
                            .strip_prefix("known ")
                            .ok_or_else(|| anyhow!("bad expect line {line:?}"))?
                            .trim()
                            .to_string(),
                    ),
                });
            } else {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    finish(current, &mut seeds)?;
    Ok(seeds)
}

/// Printable block for a finding: kind, oracle, TLA mirror, shrunk trace in the
/// regression-seed format.
pub(crate) fn report_block(seed: u64, finding: &FuzzFinding, shrunk: &FuzzTrace) -> String {
    let mut replay = shrunk.clone();
    let run = execute(&mut replay, None);
    let detail = run
        .findings
        .iter()
        .find(|f| f.kind == finding.kind)
        .map_or(finding.detail.as_str(), |f| f.detail.as_str())
        .to_string();
    format!(
        "# finding {kind} (oracle {oracle:?} mirrors {tla}); found by seed {seed}\n# detail: {detail}\n=== {kind}-seed{seed}\nexpect known {kind}\n{trace}",
        kind = finding.kind,
        oracle = finding.oracle,
        tla = finding.oracle.tla(),
        detail = detail.replace('\n', " "),
        trace = shrunk.to_text()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_round_trip_through_text() {
        for seed in 0..12 {
            let (trace, _) = explore_seed(seed, 40);
            let parsed = FuzzTrace::parse(&trace.to_text()).unwrap();
            assert_eq!(parsed, trace, "seed {seed}");
        }
    }

    #[test]
    fn a_recorded_trace_replays_the_same_findings_without_the_channel_rng() {
        for seed in 0..60 {
            let (trace, recorded) = explore_seed(seed, FUZZ_STEPS);
            let mut replay = trace.clone();
            let replayed = execute(&mut replay, None);
            assert_eq!(replayed.kinds(), recorded.kinds(), "seed {seed}");
            assert_eq!(replayed.acceptances, recorded.acceptances, "seed {seed}");
            assert_eq!(replayed.commits, recorded.commits, "seed {seed}");
        }
    }

    /// `make check` budget: fixed seeds, every profile, plus non-vacuity.
    #[test]
    fn sim_fuzz_short_fixed_budget_finds_no_new_finding_kinds() {
        let started = Instant::now();
        let mut checks: BTreeMap<Oracle, usize> = BTreeMap::new();
        let mut kinds: BTreeMap<String, (usize, u64)> = BTreeMap::new();
        let (mut acceptances, mut commits, mut recycles, mut drops, mut reconnects) =
            (0, 0, 0, 0, 0);
        let mut new_blocks = Vec::new();
        for seed in SHORT_BUDGET_SEEDS {
            let (trace, run) = explore_seed(seed, FUZZ_STEPS);
            for (oracle, count) in &run.checks {
                *checks.entry(*oracle).or_insert(0) += count;
            }
            for finding in &run.findings {
                kinds.entry(finding.kind.clone()).or_insert((0, seed)).0 += 1;
            }
            acceptances += run.acceptances;
            commits += run.commits;
            recycles += run.recycles;
            if let Some(stats) = run.net_stats {
                drops += stats.drops;
                reconnects += stats.reconnects;
            }
            for finding in run.new_findings() {
                if new_blocks.len() < 3 {
                    let shrunk = shrink(&trace, &finding.kind);
                    new_blocks.push(report_block(seed, finding, &shrunk));
                }
            }
        }
        eprintln!(
            "sim-fuzz short budget: seeds={:?} elapsed_ms={} acceptances={acceptances} commits={commits} recycles={recycles} net_drops={drops} reconnects={reconnects}\n  checks={checks:?}\n  kinds={kinds:?}",
            SHORT_BUDGET_SEEDS,
            started.elapsed().as_millis()
        );
        assert!(
            new_blocks.is_empty(),
            "NEW sim-fuzz finding kinds (shrunk, replayable):\n{}",
            new_blocks.join("\n")
        );
        // Non-vacuity: every oracle ran, and the schedules did real work.
        for oracle in Oracle::ALL {
            if matches!(oracle, Oracle::NetChannel | Oracle::Structural) {
                continue;
            }
            assert!(
                checks.get(&oracle).copied().unwrap_or(0) > 0,
                "oracle {oracle:?} never checked anything: {checks:?}"
            );
        }
        assert!(
            acceptances > 50 && commits > 50 && recycles > 10,
            "{acceptances} {commits} {recycles}"
        );
        assert!(drops > 0 && reconnects > 0, "{drops} {reconnects}");
    }

    #[test]
    fn sim_fuzz_regression_seeds_replay() {
        let seeds = parse_regression_seeds(REGRESSION_SEEDS).unwrap();
        assert!(!seeds.is_empty(), "fuzz_seeds.txt has no regression seeds");
        let mut failures = Vec::new();
        for seed in &seeds {
            let mut trace = seed.trace.clone();
            let run = execute(&mut trace, None);
            match &seed.expect_known {
                None => {
                    if !run.findings.is_empty() {
                        failures.push(format!(
                            "{}: expected clean, found {:#?}",
                            seed.name, run.findings
                        ));
                    }
                }
                Some(kind) => {
                    assert!(
                        is_known_kind(kind),
                        "{}: `expect known {kind}` names a kind not on KNOWN_FUZZ_FINDINGS",
                        seed.name
                    );
                    if !run.has_kind(kind) {
                        failures.push(format!(
                            "{}: known defect {kind} no longer reproduces — if it was fixed, flip the seed to `expect clean` and drop the kind from KNOWN_FUZZ_FINDINGS; found {:?}",
                            seed.name,
                            run.kinds()
                        ));
                    }
                    let new: Vec<_> = run.new_findings();
                    if !new.is_empty() {
                        failures.push(format!("{}: NEW finding kinds {new:#?}", seed.name));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Shrinking reduces a reachable behavior to a short trace that replays from
    /// text. This stays useful when every committed finding has been fixed.
    #[test]
    fn shrinking_reduces_a_reachable_seed_to_a_replayable_minimal_trace() {
        let mut seeds = SHORT_BUDGET_SEEDS;
        let (seed, trace) = seeds
            .find_map(|seed| {
                let (trace, run) = explore_seed(seed, FUZZ_STEPS);
                (run.acceptances > 0).then_some((seed, trace))
            })
            .expect("the short budget reaches a dispatch acceptance");
        let accepted = |candidate: &FuzzTrace| {
            let mut replay = candidate.clone();
            execute(&mut replay, None).acceptances > 0
        };
        let shrunk = shrink_where(&trace, &accepted);
        assert!(
            shrunk.steps.len() < trace.steps.len() / 4,
            "seed {seed}: {} -> {} steps\n{}",
            trace.steps.len(),
            shrunk.steps.len(),
            shrunk.to_text()
        );
        let replay = FuzzTrace::parse(&shrunk.to_text()).unwrap();
        assert!(accepted(&replay));
    }

    /// Triage helper: `AGENT_DOC_SIM_FUZZ_SHOW_KIND=<kind>` prints the shrunk
    /// trace of the first short-budget seed (or `AGENT_DOC_SIM_FUZZ_SEED`) that
    /// produces it, in regression-seed form.
    #[test]
    #[ignore = "triage helper"]
    fn sim_fuzz_show_kind() {
        let kind = std::env::var("AGENT_DOC_SIM_FUZZ_SHOW_KIND")
            .expect("set AGENT_DOC_SIM_FUZZ_SHOW_KIND");
        let seeds: Vec<u64> = match std::env::var("AGENT_DOC_SIM_FUZZ_SEED") {
            Ok(raw) => vec![raw.trim().parse().unwrap()],
            Err(_) => (0..20_000).collect(),
        };
        let len: usize = std::env::var("AGENT_DOC_SIM_FUZZ_STEPS")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or(FUZZ_STEPS);
        for seed in seeds {
            let (trace, run) = explore_seed(seed, len);
            if let Some(finding) = run.findings.iter().find(|f| f.kind == kind) {
                let shrunk = shrink(&trace, &kind);
                eprintln!("{}", report_block(seed, finding, &shrunk));
                return;
            }
        }
        panic!("no seed produced {kind}");
    }

    /// Triage helper: replay the trace in `AGENT_DOC_SIM_FUZZ_TRACE` (a file in
    /// the regression-seed / FUZZ_OUT format) and print its findings.
    #[test]
    #[ignore = "triage helper"]
    fn sim_fuzz_replay_trace_file() {
        let path = std::env::var("AGENT_DOC_SIM_FUZZ_TRACE").expect("set AGENT_DOC_SIM_FUZZ_TRACE");
        let text = std::fs::read_to_string(&path).unwrap();
        let body: String = text
            .lines()
            .filter(|line| !line.starts_with("===") && !line.trim_start().starts_with("expect "))
            .map(|line| format!("{line}\n"))
            .collect();
        let mut trace = FuzzTrace::parse(&body).unwrap();
        let run = execute(&mut trace, None);
        eprintln!(
            "replayed {} steps: acceptances={} commits={} recycles={}\nfindings={:#?}",
            trace.steps.len(),
            run.acceptances,
            run.commits,
            run.recycles,
            run.findings
        );
    }

    /// `make sim-fuzz`: fresh seeds for `AGENT_DOC_SIM_FUZZ_SECS` seconds. Each new
    /// finding kind's shrunk trace is printed and written to `AGENT_DOC_SIM_FUZZ_OUT`.
    #[test]
    #[ignore = "long budget: run by `make sim-fuzz` and the nightly workflow"]
    fn sim_fuzz_long_budget() {
        let secs: u64 = std::env::var("AGENT_DOC_SIM_FUZZ_SECS")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or(60);
        let base: u64 = std::env::var("AGENT_DOC_SIM_FUZZ_SEED")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1)
            });
        let len: usize = std::env::var("AGENT_DOC_SIM_FUZZ_STEPS")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or(FUZZ_STEPS * 2);
        let out = std::env::var("AGENT_DOC_SIM_FUZZ_OUT")
            .ok()
            .map(PathBuf::from);
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut kinds: BTreeMap<String, (usize, u64)> = BTreeMap::new();
        let mut new_blocks: BTreeMap<String, String> = BTreeMap::new();
        let mut schedules = 0u64;
        let mut seed = base;
        while Instant::now() < deadline {
            let (trace, run) = explore_seed(seed, len);
            schedules += 1;
            for finding in &run.findings {
                kinds.entry(finding.kind.clone()).or_insert((0, seed)).0 += 1;
                if !is_known_kind(&finding.kind) && !new_blocks.contains_key(&finding.kind) {
                    let shrunk = shrink(&trace, &finding.kind);
                    let block = report_block(seed, finding, &shrunk);
                    eprintln!("NEW finding:\n{block}");
                    if let Some(dir) = &out {
                        let _ = std::fs::create_dir_all(dir);
                        let _ = std::fs::write(dir.join(format!("{}.trace", finding.kind)), &block);
                    }
                    new_blocks.insert(finding.kind.clone(), block);
                }
            }
            seed = seed.wrapping_add(1);
        }
        eprintln!(
            "sim-fuzz long budget: base_seed={base} schedules={schedules} steps={len} secs={secs}\n  kinds={kinds:#?}"
        );
        assert!(
            new_blocks.is_empty(),
            "NEW sim-fuzz finding kinds: {:?}\n{}",
            new_blocks.keys().collect::<Vec<_>>(),
            new_blocks.values().cloned().collect::<Vec<_>>().join("\n")
        );
    }
}
