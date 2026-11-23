//! Normal-operation queue closeout (`#transientcloseout`).
//!
//! Every live row in the transient set must leave that state before a normal
//! turn ends. Resting rows may remain. An interrupted or crashed turn may
//! leave transient rows in the document; that leftover is not a successful
//! closeout, and the next turn owes the same transitions. This module does
//! not mark a running claim Lost, does not delete [`crate::queue_claim::QueueClaimState::Lost`],
//! and does not change [`crate::queue_claim::QueueClaim::mark_lost`] or
//! [`crate::queue_claim::QueueClaim::project_expiry`].
//!
//! # Quiescent rows (`#stopquiescentrows`)
//!
//! Some transient rows cannot move however long the turn stays open, so they
//! do not hold it. They stay live: this module never strikes, prunes, or
//! clears them, and the operator's answer or the worker's report re-enters
//! them.
//!
//! - **Operator-owned.** A row that visibly carries `🙋` (`🙋📥`, `🙋☑️`, and
//!   the `🙋` part of `🙋🚧`) waits on the operator once one of its request
//!   ids has its packet recorded in `agent:exchange` and an open `agent:todo`
//!   action. A `🙋` row missing either still refuses: recording them is the
//!   closeout obligation that remains. `⏳` was already resting.
//!   `#orphanreportedrow`: a `☑️` row without the `🙋` projection waits on
//!   the operator the same way when the row carries an open, recorded
//!   request id or its kept response tree offers one
//!   (`**Request**: [#subagent-operator-request-…]`). Once every such
//!   request is answered, a row whose response tree made it a context parent
//!   owes a strike by its generated id (`queue consume --ack-id <q-…>`), not
//!   a generic integration.
//! - **Waiting on a claim.** A `🟡` row whose dispatch claim is still inside
//!   its acknowledgement grace, or a `🚧` row whose bound worker's heartbeat
//!   is fresh, is a worker the harness reports back on asynchronously
//!   (`state=waiting_on_claims`). The visible marker must agree with the
//!   durable phase: a `🚧` backed only by an unbound dispatch is not a running
//!   worker. A stale `🚧`, an unacknowledged `🟡`, or a row with no backing
//!   claim still refuses. The `🙋` part of `🙋🚧` does not exempt its worker
//!   part from this rule. A Codex coordinator keeps an unreported attempt
//!   as closeout debt unless that row has a durable operator wait.
//!   `#finishedheadrefill`: a `🟡`/`🚧` row whose
//!   current-generation attempt is report-owed (its worker ended and the
//!   harness recorded its hand-back, [`ReportOwedAttempt`]) is not a stale
//!   worker either: it refuses with the exact `queue report-complete` for
//!   that generation, the same debt a `⚠️🚧` row with a recorded hand-back
//!   owes.
//! - **Not due.** A `📥` row that a closed `agent:loop` schedule window holds
//!   back is not due until the next tick.
//! - **Held by its queue's control** (`#heldloopstophook`). A `📥` row in a
//!   queue whose control is the operator's hold (`pause`, or legacy marker
//!   `stop`, resolved by [`crate::control_binding::explicit_queue_pause_mode`],
//!   the same predicate queue selection uses for `Paused`) is not drainable
//!   until the operator lifts the hold. The control is read from the text, so
//!   every gate, text-only callers included, releases it the same way.
//! - **Awaiting the operator by its text** (`#humanfollowupowner`). A row
//!   whose work text carries `[operator-verify]` and no inline operator
//!   verdict is never drainable. It rests as `🙋`, not `📥` (`#opverifyrest`).
//!   A `📥` glyph painted before that projection does not hold the turn.
//!   `queue report-complete` writes human-owned action items this way.
//!   `#personrowselect`: neither does a `🟡`/`🚧` an in-session selection
//!   painted on such a row when no fresh claim backs it, no report is owed on
//!   it, and it carries no worker attempt's `[#owner#generation]`
//!   projection. `queue claim` refuses the row, so the glyph is not a claim
//!   and no agent transition moves it. Preflight no longer selects the row
//!   ([`crate::queue_projection::queue_prompt_projection_rows`]).
//! - **Awaiting the operator by its item** (`#gatedheadpending`). A `📥`
//!   `do [#id]` row whose tracked item waits on the operator (an open
//!   `[operator-verify]` item, a gated review row, or an item that is itself
//!   an open operator request, `[#subagent-operator-request-…]`) and that
//!   carries no operator answer is the `⏳` row preflight projects
//!   ([`crate::queue_continuation::awaiting_operator_head_ids`]).
//! - **Blocked** (`#gatedheadpending`, `#queueblocked`). A `📥` row whose
//!   tracked item still waits on other open work, or on an external or
//!   unknown blocker its stays-open clause records, is the `⛔` row preflight
//!   projects ([`crate::queue_blocked::QueueBlockers`]). No drain or dispatch
//!   runs it, so it does not hold the turn either. The IO evidence passes the
//!   external done archive ids ([`CloseoutEvidence::archived_done_ids`]) so a
//!   blocker that closed and was archived never reads as external.
//!
//! - **Integration in flight** (`#integrationinflight`). A `☑️` row whose
//!   coordinator already merged the worker's branch and is still running the
//!   verification on the merged tree is waiting on that verification, not on
//!   an agent action (`state=waiting_on_integration`). The coordinator records
//!   it with `queue lineage-record --event integrating`, bound to the
//!   attempt's claim generation, the worker's source sha, and the candidate
//!   integration sha, with a deadline. The IO evidence
//!   ([`CloseoutEvidence::integrations_in_flight`]) carries only the record of
//!   the row's live `Reported` claim and generation that no `integrated`
//!   receipt has superseded. An expired record, or one whose source sha or
//!   generation no longer matches the row, refuses again.
//!
//! The Waiting-on-you heading (`#waitingheadingpending`) is not a row at
//! all for this gate: it is the waiting tree's container, so a marker a stale
//! projection painted on it owes no transition
//! ([`crate::waiting_packet::is_waiting_on_you_root_text`]).
//!
//! Rows that are agent-actionable now still refuse: a drainable `📥`, a `☑️`
//! awaiting integration, a lost row whose continue is owed, `🔄`, and `❔`.
//! Claim freshness and schedule windows live in durable state, so callers
//! pass them in as [`CloseoutEvidence`]; the pure text-only entry points use
//! empty evidence and only release operator-owned rows.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use agent_doc_document::queue_projection::{self, QueueLifecycleMarker};
use agent_doc_element_queue::QueueItemIdentity;

use crate::document_queue::{QueueEntry, parse as parse_queue_body};

/// One live queue row that still owes a transition before a normal closeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientCloseoutDebt {
    /// The live row text, including its lifecycle prefix.
    pub row: String,
    pub marker: QueueLifecycleMarker,
    /// The transition a normal turn still owes for this row.
    pub owed: &'static str,
    /// `#orphanreportedrow`: the exact agent command that performs the owed
    /// transition, when one command does (a strike by generated id). Every
    /// refusal renders it beside the owed text, so the agent runs it rather
    /// than asking the operator to delete the row.
    pub close_command: Option<String>,
    /// `#dispatchheldstop`: subagent dispatch holds this `📥` row because its
    /// newest claim admission was refused for a deterministic blocker
    /// (`#waitingitemclaimmarker`). Dispatch never re-offers it, so it does
    /// not hold the turn through a recursive Stop; every refusal names the
    /// blocker and the exact re-admission command instead.
    pub dispatch_held: Option<DispatchHold>,
    /// `#interimhandback`: the row's current-generation worker ended on an
    /// interim note, not its result. The turn owes a continue of that same
    /// worker (or a restart from its worktree and branch when it is gone).
    pub continue_worker: Option<ContinueWorker>,
    /// `#lostrecoverhold`: the row's own attempt is report-owed or
    /// continue-owed by this coordinator, with an exact command. A `⚠️` row
    /// in that state is recoverable now, so it holds the turn through a
    /// recursive Stop like `⚠️🚧`.
    pub recoverable_attempt: bool,
}

/// The continue a worker that ended on an interim note is owed
/// (`#interimhandback`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueWorker {
    pub worker_id: String,
    /// `queue claim --refresh` for this generation, once the worker resumed.
    pub refresh_command: String,
    /// `queue release --requeue` for this generation, only when the worker
    /// no longer exists.
    pub release_command: String,
    /// `queue lineage --verbose` for this generation: the prior worktree,
    /// branch, and head a restart continues from.
    pub lineage_command: String,
}

/// A refused claim admission that holds a `📥` row from subagent dispatch
/// (`#dispatchheldstop`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchHold {
    /// The recorded blocker, e.g. `claim_projection_unrepresentable`.
    pub blocker: String,
    /// The explicit `agent-doc queue claim` that re-admits the row once the
    /// blocker is fixed; a newer generation lifts the hold.
    pub claim_command: String,
}

impl DispatchHold {
    /// What the blocker means and how to fix it, for the blockers the binary
    /// records today.
    fn remedy(&self) -> &'static str {
        match self.blocker.as_str() {
            "claim_projection_unrepresentable" => {
                "the claim marker could not be shown on the row, usually because the same row appears more than once in the document; remove the extra copy so one row carries this item"
            }
            _ => "fix what the blocker names",
        }
    }
}

impl TransientCloseoutDebt {
    /// `- row <row>: <owed>`, plus the exact close command when one exists.
    pub fn render(&self) -> String {
        if let Some(hold) = &self.dispatch_held {
            return format!(
                "- row {}: {} (blocker `{}`: {}). Then re-admit it with `{}`, or answer the row inline.",
                self.row,
                self.owed,
                hold.blocker,
                hold.remedy(),
                hold.claim_command
            );
        }
        if let Some(worker) = &self.continue_worker {
            return format!(
                "- row {}: {} Continue worker `{}` now (Claude Code: `SendMessage` to `{}`; another harness: its native continue of that worker), telling it to finish the task and hand back its complete result, then run `{}`. Only if the harness says that worker no longer exists, restart it: run `{}`, dispatch the fresh generation the pool offers, and point the new worker at the prior attempt's worktree and branch from `{}`.",
                self.row,
                self.owed,
                worker.worker_id,
                worker.worker_id,
                worker.refresh_command,
                worker.release_command,
                worker.lineage_command,
            );
        }
        match &self.close_command {
            Some(command) => format!(
                "- row {}: {} Close it now with `{command}`.",
                self.row, self.owed
            ),
            None => format!("- row {}: {}", self.row, self.owed),
        }
    }

    /// Whether this debt keeps refusing a recursive Stop over an unchanged
    /// debt set (`#transientcloseout`). A dispatch-held `📥` row does not:
    /// nothing in this turn re-offers it until its blocker is fixed, and the
    /// refusal already named the fix (`#dispatchheldstop`).
    pub fn holds_turn_through_recursive_stop(&self) -> bool {
        self.dispatch_held.is_none()
            && (self.recoverable_attempt || marker_holds_turn_through_recursive_stop(self.marker))
    }
}

/// Why a live transient row does not hold the turn open (`#stopquiescentrows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuiescentReason {
    /// The row carries `🙋` with its request packet and open Todo recorded.
    OperatorOwned,
    /// An active claim backs the row's `🟡`/`🚧` worker.
    WaitingOnClaim,
    /// A closed schedule window holds the row back until its next tick.
    ScheduleNotDue,
    /// The row's own text gates it on the operator: `[operator-verify]` with
    /// no inline verdict (`#humanfollowupowner`).
    AwaitingOperator,
    /// The row's queue is held by its control (`pause`, legacy marker `stop`),
    /// so no drain dispatches it until the hold is lifted
    /// (`#heldloopstophook`).
    QueueHeld,
    /// The row's tracked item waits on open work or on an external blocker
    /// its stays-open clause records (`#gatedheadpending`, `#queueblocked`).
    Blocked,
    /// A `☑️` row whose coordinator integration is recorded in flight and
    /// still inside its deadline (`#integrationinflight`).
    WaitingOnIntegration,
}

impl QuiescentReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OperatorOwned => "operator_owned",
            Self::WaitingOnClaim => "waiting_on_claim",
            Self::ScheduleNotDue => "schedule_not_due",
            Self::AwaitingOperator => "awaiting_operator",
            Self::QueueHeld => "queue_held",
            Self::Blocked => "blocked",
            Self::WaitingOnIntegration => "waiting_on_integration",
        }
    }
}

/// One live transient row that may stay live while the turn ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuiescentRow {
    pub row: String,
    pub marker: QueueLifecycleMarker,
    pub reason: QuiescentReason,
}

/// The durable phase of an active claim (`#stopquiescentrows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreshClaim {
    /// An unbound dispatch still inside its acknowledgement grace (`🟡`).
    Dispatching,
    /// A bound worker (`🚧`); heartbeat age is not a runtime limit.
    Running,
}

impl FreshClaim {
    /// Whether this phase backs a row showing `marker`.
    pub fn backs(self, marker: QueueLifecycleMarker) -> bool {
        matches!(
            (self, marker),
            (Self::Dispatching, QueueLifecycleMarker::Claimed)
                | (Self::Running, QueueLifecycleMarker::Running)
        )
    }
}

/// Whether a recorded integration in flight still holds its `☑️` row
/// quiescent (`#integrationinflight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationInFlightStatus {
    /// Inside its deadline and bound to the row's current claim generation
    /// and source sha.
    Live,
    /// Its deadline passed with no `integrated` receipt.
    Expired,
    /// The attempt's recorded source sha changed since the record (a newer
    /// report), so it describes a different integration.
    SourceMismatch,
}

/// The newest `integrating` lineage receipt of one reported attempt that no
/// `integrated` receipt has superseded (`#integrationinflight`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationInFlight {
    /// The claim generation the receipt belongs to.
    pub generation: u64,
    pub source_sha: String,
    /// The candidate integration commit whose verification is running.
    pub integration_sha: String,
    /// The verification command running on the candidate.
    pub verification: String,
    pub expires_at_secs: u64,
    pub status: IntegrationInFlightStatus,
}

/// Durable-state facts the text-only gate cannot read (`#stopquiescentrows`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseoutEvidence {
    /// Work identities whose live claim is heartbeat-fresh, with its phase.
    pub fresh_claims: HashMap<QueueItemIdentity, FreshClaim>,
    /// Codex coordinator-owned attempts that still owe a worker report.
    pub codex_coordinator_claims: HashSet<QueueItemIdentity>,
    /// Head texts that a closed schedule window holds back.
    pub schedule_held_heads: HashSet<String>,
    /// Work identities whose `Lost` attempt holds an ended worker's recorded,
    /// unreported hand-back (`#lostrowreportreopen`,
    /// [`crate::queue_claim::QueueClaim::lost_end_hand_back`]). The row owes a
    /// report of that hand-back, not a continue or a new generation.
    pub reportable_lost: HashSet<QueueItemIdentity>,
    /// `#finishedheadrefill`: work identities whose current-generation attempt is
    /// report-owed ([`ReportOwedAttempt::of`]), whether still `Running` or
    /// already `Lost`. A `🟡`/`🚧` row owes the report instead of a heartbeat
    /// refresh, and every report-owed refusal names the exact
    /// `queue report-complete` command for that generation.
    pub report_owed: HashMap<QueueItemIdentity, ReportOwedAttempt>,
    /// The session document path owed commands name (`#orphanreportedrow`);
    /// `None` renders the `<FILE>` placeholder.
    pub document: Option<String>,
    /// `#gatedheadpending`: own ids of the document's external done archive,
    /// when the IO caller read it. `None` keeps the text-only blocker
    /// derivation ([`crate::queue_blocked::QueueBlockers::from_content`]).
    pub archived_done_ids: Option<HashSet<String>>,
    /// `#integrationinflight`: work identities whose live `Reported` claim's
    /// lineage attempt (same generation) carries an unsuperseded
    /// `integrating` receipt. Only [`IntegrationInFlightStatus::Live`] holds a
    /// `☑️` row quiescent.
    pub integrations_in_flight: HashMap<QueueItemIdentity, IntegrationInFlight>,
    /// `#dispatchheldstop`: work identities whose newest claim admission was
    /// refused, with the recorded blocker (`#waitingitemclaimmarker`).
    /// Subagent dispatch holds those rows, so a `📥` row among them owes the
    /// blocker's fix and an explicit re-admission, not a dispatch.
    pub dispatch_held: HashMap<QueueItemIdentity, String>,
}

/// `#finishedheadrefill`: a current-generation attempt whose worker finished and
/// whose result is not reported yet ([`crate::lost_running_recovery::report_owed`]).
/// The coordinator owes `queue report-complete` for this exact generation,
/// not a heartbeat refresh, a continue, or a new generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportOwedAttempt {
    /// The claim's CLI handle ([`crate::queue_claim::claim_item_handle`]).
    pub item: String,
    pub generation: u64,
    /// The claim owner, for the generation-fenced `--refresh` of a continue.
    pub owner: String,
    /// The bound worker that ended.
    pub worker_id: String,
    /// `#interimhandback`: the recorded hand-back is only an interim note
    /// ([`crate::queue_claim::hand_back_defers_result`]), so the worker is
    /// owed a continue, not a report.
    pub interim: bool,
}

impl ReportOwedAttempt {
    /// The report-owed attempt of `claim`: a bound worker's `Running` or
    /// `Lost` attempt that [`crate::lost_running_recovery::report_owed`]
    /// classifies as finished and unreported. `None` for every other claim.
    pub fn of(claim: &crate::queue_claim::QueueClaim) -> Option<Self> {
        use crate::queue_claim::QueueClaimState;
        if !matches!(
            claim.state,
            QueueClaimState::Running | QueueClaimState::Lost
        ) || claim.worker.is_none()
            || !crate::lost_running_recovery::report_owed(claim)
        {
            return None;
        }
        Some(Self {
            item: crate::queue_claim::claim_item_handle(&claim.item_text),
            generation: claim.generation,
            owner: claim.owner.clone(),
            worker_id: claim
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.clone())
                .unwrap_or_default(),
            interim: claim.interim_end_hand_back().is_some(),
        })
    }

    /// The continue an interim end owes (`#interimhandback`); `None` for a
    /// finished worker's hand-back, which owes [`Self::report_command`].
    pub fn continue_worker(&self, document: &str) -> Option<ContinueWorker> {
        let argv = |words: &[&str]| {
            crate::reported_closeout::shell_command(
                &words
                    .iter()
                    .map(|word| word.to_string())
                    .collect::<Vec<_>>(),
            )
        };
        let generation = self.generation.to_string();
        self.interim.then(|| ContinueWorker {
            worker_id: self.worker_id.clone(),
            refresh_command: argv(&[
                "agent-doc",
                "queue",
                "claim",
                document,
                "--item",
                &self.item,
                "--owner",
                &self.owner,
                "--refresh",
                "--generation",
                &generation,
            ]),
            release_command: argv(&[
                "agent-doc",
                "queue",
                "release",
                document,
                "--item",
                &self.item,
                "--generation",
                &generation,
                "--requeue",
            ]),
            lineage_command: argv(&[
                "agent-doc",
                "queue",
                "lineage",
                document,
                "--item",
                &self.item,
                "--generation",
                &generation,
                "--verbose",
            ]),
        })
    }

    /// `agent-doc queue report-complete <document> --item <row> --generation
    /// <n> --response '<complete worker hand-back>'`.
    pub fn report_command(&self, document: &str) -> String {
        crate::reported_closeout::shell_command(&[
            "agent-doc".to_string(),
            "queue".to_string(),
            "report-complete".to_string(),
            document.to_string(),
            "--item".to_string(),
            self.item.clone(),
            "--generation".to_string(),
            self.generation.to_string(),
            "--response".to_string(),
            "<complete worker hand-back>".to_string(),
        ])
    }
}

/// Every live transient row, split into the debts that refuse a normal
/// closeout and the quiescent rows that may stay live past it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransientCloseoutAssessment {
    pub debts: Vec<TransientCloseoutDebt>,
    pub quiescent: Vec<QuiescentRow>,
}

impl TransientCloseoutAssessment {
    /// Quiescent rows for `reason`.
    pub fn quiescent_count(&self, reason: QuiescentReason) -> usize {
        self.quiescent
            .iter()
            .filter(|row| row.reason == reason)
            .count()
    }

    /// The `state=` a turn end over these quiescent rows logs: worker waits
    /// first, as `runbooks/harness-invocation.md` documents.
    pub fn quiescent_state(&self) -> Option<&'static str> {
        if self.quiescent_count(QuiescentReason::WaitingOnClaim) > 0 {
            Some("waiting_on_claims")
        } else if self.quiescent_count(QuiescentReason::WaitingOnIntegration) > 0 {
            Some("waiting_on_integration")
        } else if self.quiescent_count(QuiescentReason::OperatorOwned) > 0
            || self.quiescent_count(QuiescentReason::AwaitingOperator) > 0
        {
            Some("waiting_on_operator")
        } else if self.quiescent_count(QuiescentReason::QueueHeld) > 0 {
            Some("queue_held")
        } else if self.quiescent_count(QuiescentReason::ScheduleNotDue) > 0 {
            Some("schedule_not_due")
        } else if self.quiescent_count(QuiescentReason::Blocked) > 0 {
            Some("blocked")
        } else {
            None
        }
    }

    /// One ops-log field list naming each quiescent count.
    pub fn quiescent_log_fields(&self) -> String {
        format!(
            "waiting_on_claims={} operator_owned={} schedule_not_due={} awaiting_operator={} queue_held={} blocked={} waiting_on_integration={}",
            self.quiescent_count(QuiescentReason::WaitingOnClaim),
            self.quiescent_count(QuiescentReason::OperatorOwned),
            self.quiescent_count(QuiescentReason::ScheduleNotDue),
            self.quiescent_count(QuiescentReason::AwaitingOperator),
            self.quiescent_count(QuiescentReason::QueueHeld),
            self.quiescent_count(QuiescentReason::Blocked),
            self.quiescent_count(QuiescentReason::WaitingOnIntegration),
        )
    }
}

/// Whether `marker` is in the transient set a normal turn must not end on.
///
/// The set is `📥` `🟡` `🚧` `☑️` `⚠️` `⚠️🟡` `⚠️🚧` `⚠️☑️` `🔄` `❔`.
/// `⛔`, `⏳`, and `⏭️` are not transient. `🙋` is an orthogonal resting
/// annotation, not a lifecycle marker.
pub fn is_transient_closeout_marker(marker: QueueLifecycleMarker) -> bool {
    matches!(
        marker,
        QueueLifecycleMarker::Pending
            | QueueLifecycleMarker::Claimed
            | QueueLifecycleMarker::Running
            | QueueLifecycleMarker::Reported
            | QueueLifecycleMarker::Lost
            | QueueLifecycleMarker::LostClaimed
            | QueueLifecycleMarker::LostRunning
            | QueueLifecycleMarker::LostReported
            | QueueLifecycleMarker::Reconciling
            | QueueLifecycleMarker::Unknown
    )
}

/// Whether a recursive Stop must keep refusing `marker` (`#transientcloseout`).
///
/// `#stophookactiveguard` may end a turn over an unchanged debt set the agent
/// did not move, but not while a row is still `📥`, `🚧`, `⚠️🚧`, or `☑️`.
/// Those markers are the turn. A `☑️` row leaves this set only when the binary
/// or the coordinator has written its response and struck it. A `🔄`, `❔`,
/// or plain `⚠️` debt may still release, so Claude Code's consecutive-block
/// cap is not the only bound for markers the binary cannot finish.
pub fn marker_holds_turn_through_recursive_stop(marker: QueueLifecycleMarker) -> bool {
    matches!(
        marker,
        QueueLifecycleMarker::Pending
            | QueueLifecycleMarker::Running
            | QueueLifecycleMarker::LostRunning
            | QueueLifecycleMarker::Reported
    )
}

/// The transition a normal turn still owes for `marker`.
///
/// Resting markers return `None`.
pub fn owed_closeout_transition(marker: QueueLifecycleMarker) -> Option<&'static str> {
    Some(match marker {
        QueueLifecycleMarker::Pending => {
            "leave pending before the turn ends; a head that is still pending means the turn is not over"
        }
        QueueLifecycleMarker::Claimed => {
            "start the claimed worker or otherwise leave claimed; a head that is only claimed means the turn is not over"
        }
        QueueLifecycleMarker::Running => {
            "finish the running worker without converting the claim to Lost; a worker that is still running means the turn is not over; a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        }
        QueueLifecycleMarker::Reported => OWED_REPORTED,
        QueueLifecycleMarker::Lost
        | QueueLifecycleMarker::LostClaimed
        | QueueLifecycleMarker::LostRunning => {
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        }
        // `#lostrowreportreopen`: the worker's result is already reported.
        // There is no worker to continue and no work for a new generation.
        QueueLifecycleMarker::LostReported => OWED_LOST_REPORTED,
        QueueLifecycleMarker::Reconciling => {
            "reconcile the row or close it; a strike retires the completion cell"
        }
        QueueLifecycleMarker::Unknown => "diagnose the contradiction and clear it",
        QueueLifecycleMarker::Blocked
        | QueueLifecycleMarker::AwaitingOperator
        | QueueLifecycleMarker::Skipped
        | QueueLifecycleMarker::Seen => return None,
    })
}

/// The owed transition for a `🟡`/`🚧` row that no fresh claim backs.
fn owed_stale_worker_transition(marker: QueueLifecycleMarker) -> &'static str {
    match marker {
        QueueLifecycleMarker::Claimed => {
            "no recent dispatch claim backs this row: start the claimed worker or otherwise leave claimed; a head that is only claimed means the turn is not over"
        }
        _ => {
            "no bound running worker claim backs this row: acknowledge the worker with `agent-doc queue start` if it is running, or finish the recorded stop/retry transition; a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        }
    }
}

/// What a `☑️` row owes. `#integrationinflight`: it names the receipt that
/// lets the gate wait while a merged integration's verification still runs.
const OWED_REPORTED: &str = "integrate the report or otherwise leave the reported-but-open state. If its integration is already merged and verification on the merged tree is still running, record that so the turn may end while it runs: `agent-doc queue lineage-record <FILE> --item <row> --generation <n> --event integrating --source-sha <worker-source-sha> --integration-sha <candidate-merge-sha> --verification '<command>'` (deadline `--deadline-secs`, default 3600; `--event integrated` supersedes it)";

/// `#integrationinflight`: a recorded integration whose deadline passed.
const OWED_REPORTED_INTEGRATION_EXPIRED: &str = "integrate the report: the integration recorded in flight for this row (`--event integrating`) passed its deadline with no `integrated` receipt. Finish it and record `agent-doc queue lineage-record ... --event integrated`, or, only if its verification is genuinely still running, record a fresh `--event integrating` receipt";

/// `#integrationinflight`: a recorded integration that no longer describes
/// the row's current attempt.
const OWED_REPORTED_INTEGRATION_STALE: &str = "integrate the report: the integration recorded in flight for this row (`--event integrating`) no longer matches it (the attempt's source sha or claim generation changed since). Integrate the current report and record `--event integrated`, or record a fresh `--event integrating` receipt for the current source and generation";

/// `#lostrowreportreopen`: what a `⚠️☑️` row owes.
const OWED_LOST_REPORTED: &str = "integrate the recovered report: the worker's result is already reported across a lost attempt, so do not continue the worker or retry a new generation; integrate it and strike the row (or `agent-doc queue consume --ack-text <row> --ack-owner <owner#generation>`)";

/// `#lostrowreportreopen`: what a `⚠️🚧` row owes when its ended worker's
/// hand-back is recorded and unreported.
const OWED_LOST_RECORDED_HANDBACK: &str = "report the hand-back its ended worker left: run `agent-doc queue report-complete` for this exact generation with that hand-back (the freeze recorded its opening); do not continue the ended worker or retry a new generation for finished work";

/// `#finishedheadrefill`: what a `🟡`/`🚧` row owes when its current-generation
/// worker ended and its hand-back is recorded and unreported. It is not a
/// stale worker: no heartbeat refresh revives it.
const OWED_INTERIM_HANDBACK_CONTINUE: &str = "continue the worker that ended on an interim note (#interimhandback): its last message says it is waiting or will finish later, so it is not finished and its result is not in; do not `queue report-complete` the note, do not claim a new generation while the worker exists, and do not end the turn with this row lost.";
const OWED_RUNNING_RECORDED_HANDBACK: &str = "report the hand-back its ended worker left: the worker finished and its hand-back is recorded but not reported, so run `agent-doc queue report-complete` for this exact generation with the worker's complete hand-back; do not refresh its heartbeat, continue the ended worker, or claim a new generation for finished work (only to discard the result, `agent-doc queue release --requeue` this generation)";

/// `#orphanreportedrow`: what a reported row owes once its kept response
/// tree's operator request is answered (or when it names none).
const OWED_REPORTED_RESPONSE_ORIGIN: &str = "the operator answered this reported row's request: act on the answer, then strike the row yourself with `agent-doc queue consume` (by its generated `--ack-id <q-…>`, fenced with `--ack-owner`); its kept response tree makes it a context parent, not an executable leaf, and no worker remains to dispatch. The agent closes this row: never ask the operator to delete it by hand.";

/// `#dispatchheldstop`: what a `📥` row owes while a refused claim admission
/// holds it from subagent dispatch.
pub const OWED_DISPATCH_HELD: &str = "subagent dispatch holds this row: its last claim admission was refused, so the pool will not offer it again. Fix the blocker";

/// `#finishactionable`: what a pending agent-owned `follow up on an action
/// item` row owes. The worker's hand-back listed it as agent work, so it is
/// done in this turn under the originating item's authorization; only a
/// concrete blocker recorded on the row leaves it for later.
pub const OWED_AGENT_ACTION_ITEM: &str = "do this agent-owned action item now, in this turn (#finishactionable): a worker hand-back listed it as agent work under the originating item's authorization, so it is not an operator question and not work for a later turn. Do it inline (or dispatch this row now when it is a substantial new task), verify the result, and strike the row with `agent-doc queue consume <FILE> --ack-text '<row>' --summary '<what was done, and where>'` so it carries its response tree (#ackresponse); if it is genuinely blocked, record the concrete blocker on the row instead";

const OWED_OPERATOR_RECORD: &str = "record this row's operator-request packet in agent:exchange and its open agent:todo action, or act on the operator's answer; a 🙋 row waits on the operator only once both are recorded";

/// Whether a `🙋` row's request is recorded: some id on the row has its packet
/// in `agent:exchange` and an open `agent:todo` action. `None` when the row
/// carries no visible `🙋`.
fn operator_request_recorded(
    text: &str,
    exchange_lower: &str,
    open_todos: &HashSet<String>,
) -> Option<bool> {
    if !queue_projection::operator_action_marked(text) {
        return None;
    }
    let mut ids = queue_projection::operator_action_ids(text);
    for id in queue_projection::operator_request_annotation_ids(text) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Some(ids.iter().any(|id| {
        let id = id.to_ascii_lowercase();
        open_todos.contains(&id) && exchange_lower.contains(&format!("[#{id}]"))
    }))
}

/// The active phase of `claim` at `now_secs`, if it backs a quiescent worker
/// wait: an unbound dispatch still inside its acknowledgement grace, or any
/// bound worker without authoritative stop evidence. `None` for a frozen,
/// reported, lost, or quota-blocked attempt.
pub fn fresh_claim_phase(
    claim: &crate::queue_claim::QueueClaim,
    now_secs: u64,
    frozen_secs: u64,
    grace_secs: u64,
) -> Option<FreshClaim> {
    use crate::queue_claim::QueueClaimState;
    use crate::subagent_pool::{AttemptSlot, classify_attempt};
    if !matches!(
        claim.state,
        QueueClaimState::Claimed | QueueClaimState::Running
    ) {
        return None;
    }
    match classify_attempt(claim, now_secs, frozen_secs) {
        AttemptSlot::Running => Some(FreshClaim::Running),
        AttemptSlot::Dispatching
            if !crate::queue_claim::dispatch_evidence_missing(claim, now_secs, grace_secs) =>
        {
            Some(FreshClaim::Dispatching)
        }
        _ => None,
    }
}

/// Classify every live, unstruck transient queue row against `evidence`.
pub fn assess_transient_closeout(
    content: &str,
    evidence: &CloseoutEvidence,
) -> Result<TransientCloseoutAssessment> {
    let components = agent_doc_element::element::parse(content)?;
    let mut assessment = TransientCloseoutAssessment::default();
    let mut operator_context: Option<(String, HashSet<String>)> = None;
    // `#gatedheadpending`: item-level gates, derived once on first use.
    let mut item_gates: Option<PendingItemGates> = None;
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .map(|(frontmatter, _)| frontmatter);
    let frontmatter_control = frontmatter
        .as_ref()
        .and_then(crate::control_binding::frontmatter_queue_control);
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let entries = parse_queue_body(component.content(content))?;
        // `#heldloopstophook`: the one hold predicate queue selection uses.
        let held_by_control = queue_held_by_operator_control(&component.attrs, frontmatter_control);
        for (index, entry) in entries.iter().enumerate() {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            // `#waitingheadingpending`: the Waiting-on-you heading is a
            // container, never an executable row. A marker a stale projection
            // painted on it owes no transition the agent could make.
            if crate::waiting_packet::is_waiting_on_you_root_text(&prompt.text) {
                continue;
            }
            let Some(marker) = queue_projection::lifecycle_marker(&prompt.text) else {
                continue;
            };
            let Some(owed) = owed_closeout_transition(marker) else {
                continue;
            };
            let row = prompt.text.trim().to_string();
            let (exchange_lower, open_todos) = operator_context.get_or_insert_with(|| {
                let exchange = components
                    .iter()
                    .filter(|component| component.name == "exchange")
                    .map(|component| component.content(content).to_ascii_lowercase())
                    .collect::<Vec<_>>()
                    .join("\n");
                (exchange, agent_doc_element::todo::open_todo_ids(content))
            });
            let operator = operator_request_recorded(&prompt.text, exchange_lower, open_todos);
            let quiescent = |reason| QuiescentRow {
                row: row.clone(),
                marker,
                reason,
            };
            let debt = |owed| TransientCloseoutDebt {
                row: row.clone(),
                marker,
                owed,
                close_command: None,
                dispatch_held: None,
                continue_worker: None,
                recoverable_attempt: false,
            };
            let identity = crate::queue_claim::claim_identity_at(&entries, index)
                .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text));
            let codex_worker_owned = matches!(
                marker,
                QueueLifecycleMarker::Pending
                    | QueueLifecycleMarker::Claimed
                    | QueueLifecycleMarker::Running
            ) && evidence.codex_coordinator_claims.contains(&identity);
            if codex_worker_owned && let Some(owed) = evidence.report_owed.get(&identity) {
                let mut owed_debt = debt(OWED_CODEX_RECORDED_HANDBACK);
                owed_debt.close_command =
                    Some(owed.report_command(evidence.document.as_deref().unwrap_or("<FILE>")));
                assessment.debts.push(owed_debt);
                continue;
            }
            if operator == Some(false) {
                assessment.debts.push(debt(OWED_OPERATOR_RECORD));
                continue;
            }
            if codex_worker_owned
                && operator != Some(true)
                && evidence.fresh_claims.contains_key(&identity)
            {
                assessment.debts.push(debt(OWED_CODEX_WORKER_REPORT));
                continue;
            }
            // `#orphanreportedrow`: a `☑️` row that carries an operator
            // request without the `🙋` projection (an unselected `agent:run`
            // the projection pass did not repaint), or whose kept response
            // tree offers one, waits on the operator while that request is
            // open. Once it is answered, the agent owes the strike, by the
            // generated id, since the response tree keeps the row out of the
            // executable leaves.
            if operator.is_none()
                && matches!(
                    marker,
                    QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported
                )
            {
                let origin = crate::document_queue::reported_response_origin(&entries, index);
                let request_ids = origin.as_ref().map_or_else(
                    || queue_projection::operator_request_annotation_ids(&prompt.text),
                    |origin| origin.request_ids.clone(),
                );
                let open = request_ids
                    .iter()
                    .filter(|id| open_todos.contains(*id))
                    .collect::<Vec<_>>();
                if !open.is_empty() {
                    if open
                        .iter()
                        .any(|id| exchange_lower.contains(&format!("[#{id}]")))
                    {
                        assessment
                            .quiescent
                            .push(quiescent(QuiescentReason::OperatorOwned));
                    } else {
                        assessment.debts.push(debt(OWED_OPERATOR_RECORD));
                    }
                    continue;
                }
                if origin.is_some() {
                    let mut owed = debt(OWED_REPORTED_RESPONSE_ORIGIN);
                    owed.close_command = reported_origin_close_command(
                        &prompt.text,
                        evidence.document.as_deref().unwrap_or("<FILE>"),
                    );
                    assessment.debts.push(owed);
                    continue;
                }
            }
            match marker {
                QueueLifecycleMarker::Claimed | QueueLifecycleMarker::Running => {
                    // `#finishedheadrefill`: a finished worker's unreported
                    // hand-back owes its report, not a heartbeat refresh.
                    if let Some(owed) = evidence.report_owed.get(&identity) {
                        assessment.debts.push(report_or_continue_debt(
                            debt(OWED_RUNNING_RECORDED_HANDBACK),
                            owed,
                            evidence.document.as_deref().unwrap_or("<FILE>"),
                        ));
                    } else if evidence
                        .fresh_claims
                        .get(&identity)
                        .is_some_and(|fresh| fresh.backs(marker))
                    {
                        assessment
                            .quiescent
                            .push(quiescent(QuiescentReason::WaitingOnClaim));
                    } else if in_session_marker_on_person_row(&prompt.text) {
                        // `#personrowselect`: no claim can stand behind this
                        // glyph, so no agent transition moves it.
                        assessment
                            .quiescent
                            .push(quiescent(QuiescentReason::AwaitingOperator));
                    } else {
                        assessment
                            .debts
                            .push(debt(owed_stale_worker_transition(marker)));
                    }
                }
                // `#lostrecoverhold`: a `⚠️` row whose attempt is still
                // `Running` with a recorded end hand-back (the subagent-end
                // hook froze it, and a frozen `Running` attempt paints `⚠️`)
                // is report-owed or continue-owed exactly like a `Lost` one.
                // Before this arm it fell through to the generic lost text,
                // with no worker id or command, and plain `⚠️` let a
                // recursive Stop end the turn (haiven-dev `subagent:adv929#1`).
                QueueLifecycleMarker::Lost
                | QueueLifecycleMarker::LostClaimed
                | QueueLifecycleMarker::LostRunning
                    if evidence.report_owed.contains_key(&identity) =>
                {
                    let owed = &evidence.report_owed[&identity];
                    assessment.debts.push(report_or_continue_debt(
                        debt(OWED_LOST_RECORDED_HANDBACK),
                        owed,
                        evidence.document.as_deref().unwrap_or("<FILE>"),
                    ));
                }
                QueueLifecycleMarker::Lost
                | QueueLifecycleMarker::LostClaimed
                | QueueLifecycleMarker::LostRunning
                    if !evidence.reportable_lost.is_empty()
                        && evidence.reportable_lost.contains(
                            &crate::queue_claim::claim_identity_at(&entries, index).unwrap_or_else(
                                || crate::queue_claim::claim_identity(&prompt.text),
                            ),
                        ) =>
                {
                    let mut owed_debt = debt(OWED_LOST_RECORDED_HANDBACK);
                    owed_debt.close_command =
                        crate::queue_claim::claim_identity_at(&entries, index)
                            .or_else(|| Some(crate::queue_claim::claim_identity(&prompt.text)))
                            .and_then(|identity| evidence.report_owed.get(&identity))
                            .map(|owed| {
                                owed.report_command(
                                    evidence.document.as_deref().unwrap_or("<FILE>"),
                                )
                            });
                    assessment.debts.push(owed_debt);
                }
                QueueLifecycleMarker::Pending | QueueLifecycleMarker::Reported
                    if operator == Some(true) =>
                {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::OperatorOwned));
                }
                // `#integrationinflight`: a recorded, unexpired integration of
                // this exact attempt is a wait on its verification.
                QueueLifecycleMarker::Reported if !evidence.integrations_in_flight.is_empty() => {
                    let identity = crate::queue_claim::claim_identity_at(&entries, index)
                        .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text));
                    match integration_in_flight_status(
                        &prompt.text,
                        evidence.integrations_in_flight.get(&identity),
                    ) {
                        Some(IntegrationInFlightStatus::Live) => assessment
                            .quiescent
                            .push(quiescent(QuiescentReason::WaitingOnIntegration)),
                        Some(IntegrationInFlightStatus::Expired) => assessment
                            .debts
                            .push(debt(OWED_REPORTED_INTEGRATION_EXPIRED)),
                        Some(IntegrationInFlightStatus::SourceMismatch) => {
                            assessment.debts.push(debt(OWED_REPORTED_INTEGRATION_STALE))
                        }
                        None => assessment.debts.push(debt(owed)),
                    }
                }
                QueueLifecycleMarker::Pending
                    if crate::queue_continuation::head_awaits_operator_inline(&prompt.text) =>
                {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::AwaitingOperator));
                }
                QueueLifecycleMarker::Pending
                    if item_gates
                        .get_or_insert_with(|| {
                            PendingItemGates::from_content(
                                content,
                                evidence.archived_done_ids.as_ref(),
                            )
                        })
                        .gate(&prompt.text)
                        .is_some() =>
                {
                    let reason = item_gates
                        .as_ref()
                        .and_then(|gates| gates.gate(&prompt.text))
                        .expect("the guard found an item gate");
                    assessment.quiescent.push(quiescent(reason));
                }
                QueueLifecycleMarker::Pending if held_by_control => {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::QueueHeld));
                }
                QueueLifecycleMarker::Pending
                    if evidence.schedule_held_heads.contains(&prompt.text)
                        || evidence.schedule_held_heads.contains(&row) =>
                {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::ScheduleNotDue));
                }
                // `#dispatchheldstop`: a refused claim admission holds this
                // row from dispatch; "leave pending" would only repeat a
                // dispatch that refuses again. Name the blocker and the fix.
                QueueLifecycleMarker::Pending if evidence.dispatch_held.contains_key(&identity) => {
                    let blocker = evidence.dispatch_held[&identity].clone();
                    let mut held = debt(OWED_DISPATCH_HELD);
                    held.dispatch_held = Some(DispatchHold {
                        blocker,
                        claim_command: crate::reported_closeout::shell_command(&[
                            "agent-doc".to_string(),
                            "queue".to_string(),
                            "claim".to_string(),
                            evidence
                                .document
                                .clone()
                                .unwrap_or_else(|| "<FILE>".to_string()),
                            "--item".to_string(),
                            crate::queue_claim::claim_item_handle(
                                &queue_projection::strip_in_progress_marker(&prompt.text),
                            ),
                            "--owner".to_string(),
                            "subagent:<label>".to_string(),
                        ]),
                    });
                    assessment.debts.push(held);
                }
                // `#finishactionable`: an agent-owned action item a worker
                // hand-back reported is this turn's work, not a later drain.
                QueueLifecycleMarker::Pending
                    if crate::followup_owner::is_agent_followup_row(&prompt.text) =>
                {
                    assessment.debts.push(debt(OWED_AGENT_ACTION_ITEM));
                }
                _ => assessment.debts.push(debt(owed)),
            }
        }
    }
    Ok(assessment)
}

/// The debt a report-owed attempt owes (`#finishedheadrefill`,
/// `#interimhandback`, `#lostrecoverhold`): an interim note owes a continue of
/// the same worker; a finished hand-back owes its exact `queue
/// report-complete`. Either way the row is recoverable by this coordinator
/// now, so it holds the turn through a recursive Stop.
fn report_or_continue_debt(
    mut debt: TransientCloseoutDebt,
    owed: &ReportOwedAttempt,
    document: &str,
) -> TransientCloseoutDebt {
    debt.recoverable_attempt = true;
    match owed.continue_worker(document) {
        Some(worker) => {
            debt.owed = OWED_INTERIM_HANDBACK_CONTINUE;
            debt.continue_worker = Some(worker);
        }
        None => debt.close_command = Some(owed.report_command(document)),
    }
    debt
}

/// `#personrowselect`: a `🟡`/`🚧` that an in-session selection painted on a
/// row whose own text waits on a person (`[operator-verify]`, no inline
/// answer). `queue claim` refuses such a row (`#humanfollowupowner`), so the
/// glyph is not a claim and no worker reports on it; the caller has already
/// ruled out a fresh claim and a report-owed attempt. A row that carries a
/// worker attempt's `[#owner#generation]` projection is a worker, not a
/// selection marker, and keeps owing its worker transition.
fn in_session_marker_on_person_row(text: &str) -> bool {
    queue_projection::owner_projection(text).is_none()
        && crate::queue_continuation::head_awaits_operator_inline(text)
}

/// `#integrationinflight`: the status of `record` for the reported row
/// `text`. A row whose visible owner projection names another generation than
/// the record's is a different attempt, so the record is stale.
fn integration_in_flight_status(
    text: &str,
    record: Option<&IntegrationInFlight>,
) -> Option<IntegrationInFlightStatus> {
    let record = record?;
    let generation_moved = queue_projection::owner_projection(text)
        .is_some_and(|owner| owner.generation != record.generation);
    Some(if generation_moved {
        IntegrationInFlightStatus::SourceMismatch
    } else {
        record.status
    })
}

/// `#gatedheadpending`: the item-level gates that make a pending `do [#id]`
/// row undrainable however long the turn stays open. A blocked item shows
/// `⛔`; an item that waits on the operator, with no answer on the row, shows
/// `⏳`. The closeout gate treats both as quiescent and the queue-sync
/// payload never offers them as dispatch or inline work.
#[derive(Debug, Clone)]
pub struct PendingItemGates {
    awaiting: HashSet<String>,
    blockers: crate::queue_blocked::QueueBlockers,
    /// `#blockerreportterminal`: open prompt-blocker reviews. A row a review
    /// holds is not dispatchable, drainable, or claimable until the review is
    /// resolved, so it is blocked, not pending work the turn owes.
    review: crate::prompt_pressure::ReviewBlockedPrompts,
}

impl PendingItemGates {
    /// Derive the gates of `content`. `archived` is the external done
    /// archive's own ids when an IO caller read them
    /// ([`crate::queue_blocked::QueueBlockers::from_content_with_archived`]).
    pub fn from_content(content: &str, archived: Option<&HashSet<String>>) -> Self {
        let blockers = match archived {
            Some(archived) => {
                crate::queue_blocked::QueueBlockers::from_content_with_archived(content, archived)
            }
            None => crate::queue_blocked::QueueBlockers::from_content(content),
        };
        // Every queue's rows, not only the selected queue's: the ids no
        // drain scope runs because they wait on the operator, as
        // `awaiting_operator_head_ids` derives them for the `⏳` projection.
        let deferred = crate::queue_continuation::deferred_backlog_ids_split(
            content,
            crate::queue_continuation::DrainScope::Supervisor,
        );
        let mut awaiting = deferred.all;
        awaiting.extend(crate::queue_continuation::gated_review_ids(content));
        Self {
            awaiting,
            blockers,
            review: crate::prompt_pressure::review_blocked_prompts(content),
        }
    }

    /// The gate that holds the row `text`, or `None` when it is runnable.
    pub fn gate(&self, text: &str) -> Option<QuiescentReason> {
        if self.blockers.blocks_row(text) || self.review.contains(text) {
            return Some(QuiescentReason::Blocked);
        }
        if crate::queue_continuation::head_answers_operator(text) {
            return None;
        }
        crate::queue_continuation::extract_head_id(text)
            .is_some_and(|id| self.awaiting.contains(&id.to_ascii_lowercase()))
            .then_some(QuiescentReason::AwaitingOperator)
    }
}

/// `#blockerreportterminal`: whether every refused row is held by a recorded
/// blocker: an open prompt-blocker review for its work, or an open or
/// external blocker its tracked item's stays-open clause names. No agent
/// transition moves such a row before the operator resolves the blocker, so a
/// recursive Stop that finds the same refusal ends the turn
/// (`state=unchanged_debts`) instead of refusing forever. `false` for an
/// empty debt set.
pub fn every_debt_gated_by_recorded_blocker(
    content: &str,
    debts: &[TransientCloseoutDebt],
    archived: Option<&HashSet<String>>,
) -> bool {
    if debts.is_empty() {
        return false;
    }
    let gates = PendingItemGates::from_content(content, archived);
    debts
        .iter()
        .all(|debt| gates.gate(&debt.row) == Some(QuiescentReason::Blocked))
}

/// Whether a queue's resolved control is the operator's hold, so none of its
/// pending heads is drainable (`#heldloopstophook`). This is the predicate
/// queue selection uses to mark a queue `Paused`
/// ([`crate::queue_graph`]), so the closeout gates and the drains agree.
pub fn queue_held_by_operator_control(
    attrs: &HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    crate::control_binding::explicit_queue_pause_mode(attrs, frontmatter_queue)
}

/// Live, unstruck queue rows whose lifecycle marker is still transient and
/// that the text alone cannot prove quiescent.
pub fn live_transient_debts(content: &str) -> Result<Vec<TransientCloseoutDebt>> {
    Ok(assess_transient_closeout(content, &CloseoutEvidence::default())?.debts)
}

fn format_failure(header: &str, debts: &[TransientCloseoutDebt]) -> Option<String> {
    if debts.is_empty() {
        return None;
    }
    let mut message = header.to_string();
    for debt in debts {
        message.push('\n');
        message.push_str(&debt.render());
    }
    Some(message)
}

/// `#orphanreportedrow`: the exact strike for a reported row kept as its
/// response's context parent, fenced to the row's visible attempt. It is the
/// strike every closeout emitter shares
/// ([`crate::reported_closeout::reported_row_strike`]).
pub fn reported_origin_close_command(row_text: &str, document: &str) -> Option<String> {
    let owner = queue_projection::owner_projection(row_text);
    let strike = crate::reported_closeout::reported_row_strike(
        row_text,
        owner
            .as_ref()
            .map(|owner| (owner.owner.as_str(), owner.generation)),
    );
    strike
        .argv(document)
        .map(|argv| crate::reported_closeout::shell_command(&argv))
}

const NORMAL_CLOSEOUT_HEADER: &str =
    "normal closeout refused while live queue rows are still transient (#transientcloseout)";
/// Recognize the queue-debt diagnostic across the Stop/closeout status boundary.
pub fn is_normal_closeout_failure(message: &str) -> bool {
    message.contains(NORMAL_CLOSEOUT_HEADER)
}

const CRASH_RECOVERY_HEADER: &str = "crash recovery refused a successful closeout (#transientcloseout): leftover transient queue rows remain; the next turn finishes these transitions and this recovery does not treat them as closed";

/// Failure for a normal closeout, naming each live transient row and the
/// transition it still owes. `None` when every live row is resting, struck,
/// or quiescent under `evidence`.
pub fn normal_closeout_failure_with(
    content: &str,
    evidence: &CloseoutEvidence,
) -> Result<Option<String>> {
    Ok(format_failure(
        NORMAL_CLOSEOUT_HEADER,
        &assess_transient_closeout(content, evidence)?.debts,
    ))
}

/// The normal-closeout failure for an assessment already computed.
pub fn normal_closeout_failure_for(assessment: &TransientCloseoutAssessment) -> Option<String> {
    format_failure(NORMAL_CLOSEOUT_HEADER, &assessment.debts)
}

const OWED_CODEX_WORKER_REPORT: &str = "finish the current Codex worker dispatch or wait for its hand-back, then record the exact generation's report";
const OWED_CODEX_RECORDED_HANDBACK: &str = "report the hand-back preserved from the exact Codex worker turn before ending the coordinator's turn";

/// A strict response may not capture a final answer while the coordinator
/// still owes a worker hand-back. Other queue debt can be settled by the
/// response's own projected mutations and is checked after that projection.
pub fn codex_worker_capture_failure(
    content: &str,
    evidence: &CloseoutEvidence,
) -> Result<Option<String>> {
    if evidence.codex_coordinator_claims.is_empty() {
        return Ok(None);
    }
    let assessment = assess_transient_closeout(content, evidence)?;
    let debts = assessment
        .debts
        .into_iter()
        .filter(|debt| {
            matches!(
                debt.owed,
                OWED_CODEX_WORKER_REPORT | OWED_CODEX_RECORDED_HANDBACK
            )
        })
        .collect();
    Ok(normal_closeout_failure_for(&TransientCloseoutAssessment {
        debts,
        ..Default::default()
    }))
}

/// [`normal_closeout_failure_with`] over text-only evidence.
pub fn normal_closeout_failure(content: &str) -> Result<Option<String>> {
    normal_closeout_failure_with(content, &CloseoutEvidence::default())
}

/// Crash recovery may leave the document transient. It must not report that
/// leftover as a successful closeout. The next turn finishes the transitions.
pub fn crash_recovery_closeout_failure_with(
    content: &str,
    evidence: &CloseoutEvidence,
) -> Result<Option<String>> {
    Ok(format_failure(
        CRASH_RECOVERY_HEADER,
        &assess_transient_closeout(content, evidence)?.debts,
    ))
}

/// [`crash_recovery_closeout_failure_with`] over text-only evidence.
pub fn crash_recovery_closeout_failure(content: &str) -> Result<Option<String>> {
    crash_recovery_closeout_failure_with(content, &CloseoutEvidence::default())
}

/// Choose the normal or crash-recovery failure for an otherwise successful
/// closeout message. Crash recovery is the path that recovered a missing
/// commit boundary. `None` when the document has no transient live row that
/// `evidence` leaves owing a transition.
pub fn closeout_failure_for_success_message_with(
    success_message: &str,
    content: &str,
    evidence: &CloseoutEvidence,
) -> Result<Option<String>> {
    if success_message.contains("recovered the missing commit boundary") {
        crash_recovery_closeout_failure_with(content, evidence)
    } else {
        normal_closeout_failure_with(content, evidence)
    }
}

/// [`closeout_failure_for_success_message_with`] over text-only evidence.
pub fn closeout_failure_for_success_message(
    success_message: &str,
    content: &str,
) -> Result<Option<String>> {
    closeout_failure_for_success_message_with(
        success_message,
        content,
        &CloseoutEvidence::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!("<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n")
    }

    #[test]
    fn inbox_and_running_markers_hold_a_recursive_stop() {
        assert!(marker_holds_turn_through_recursive_stop(
            QueueLifecycleMarker::Pending
        ));
        assert!(marker_holds_turn_through_recursive_stop(
            QueueLifecycleMarker::Running
        ));
        assert!(marker_holds_turn_through_recursive_stop(
            QueueLifecycleMarker::LostRunning
        ));
        assert!(marker_holds_turn_through_recursive_stop(
            QueueLifecycleMarker::Reported
        ));
        for marker in [
            QueueLifecycleMarker::Claimed,
            QueueLifecycleMarker::Lost,
            QueueLifecycleMarker::LostClaimed,
            QueueLifecycleMarker::LostReported,
            QueueLifecycleMarker::Reconciling,
            QueueLifecycleMarker::Unknown,
            QueueLifecycleMarker::Blocked,
            QueueLifecycleMarker::AwaitingOperator,
        ] {
            assert!(
                !marker_holds_turn_through_recursive_stop(marker),
                "{marker:?} may still use the unchanged-debt release"
            );
        }
    }

    #[test]
    fn transient_rows_name_the_owed_transition_and_resting_rows_do_not() {
        let content = doc("\
- ~~integrated history~~
- ⛔ blocked on other open work
- ⏳ awaiting operator
- 🙋 operator action
- ⏭️ skipped earlier
- bare pending text is not a transient glyph
- 📥 still pending
- 🟡 only claimed
- 🚧 still running
- ☑️ reported but open
- ⚠️ lost
- ⚠️🟡 lost claimed
- ⚠️🚧 lost running
- ⚠️☑️ lost worker done
- 🔄 needs reconcile
- ❔ unknown contradiction
");
        let failure = normal_closeout_failure(&content)
            .unwrap()
            .expect("transient rows refuse closeout");
        assert!(failure.starts_with(
            "normal closeout refused while live queue rows are still transient (#transientcloseout)"
        ));
        for row in [
            "📥 still pending",
            "🟡 only claimed",
            "🚧 still running",
            "☑️ reported but open",
            "⚠️ lost",
            "⚠️🟡 lost claimed",
            "⚠️🚧 lost running",
            "⚠️☑️ lost worker done",
            "🔄 needs reconcile",
            "❔ unknown contradiction",
        ] {
            assert!(failure.contains(row), "missing {row} in {failure}");
        }
        assert!(failure.contains("no bound running worker claim backs this row"));
        assert!(failure.contains("finish the recorded stop/retry transition"));
        assert!(failure.contains(
            "a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        ));
        assert!(failure.contains(
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        ));
        assert!(
            failure.contains("integrate the report or otherwise leave the reported-but-open state")
        );
        assert!(
            failure.contains("- row ⚠️☑️ lost worker done: integrate the recovered report"),
            "a recovered report is integrated, never continued or retried (#lostrowreportreopen): {failure}"
        );
        assert!(
            failure.contains("reconcile the row or close it; a strike retires the completion cell")
        );
        assert!(failure.contains("diagnose the contradiction and clear it"));
        for resting in [
            "integrated history",
            "blocked on other open work",
            "awaiting operator",
            "operator action",
            "skipped earlier",
            "bare pending text",
        ] {
            assert!(
                !failure.contains(resting),
                "resting row {resting} was named in {failure}"
            );
        }

        let crash = crash_recovery_closeout_failure(&content)
            .unwrap()
            .expect("crash recovery is not success");
        assert!(crash.contains("crash recovery refused a successful closeout"));
        assert!(crash.contains("next turn finishes these transitions"));
        assert!(crash.contains("🚧 still running"));
        assert!(!crash.contains("normal closeout refused"));
    }

    #[test]
    fn a_resting_queue_does_not_refuse_closeout() {
        let content = doc("\
- ~~integrated history~~
- ⛔ blocked on other open work
- ⏳ awaiting operator
- 🙋 operator action
");
        assert!(normal_closeout_failure(&content).unwrap().is_none());
        assert!(crash_recovery_closeout_failure(&content).unwrap().is_none());
        assert!(
            closeout_failure_for_success_message(
                "[session-check] ok — recovered the missing commit boundary from head",
                &content
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn a_document_without_a_queue_has_no_transient_debt() {
        let content = "<!-- agent:exchange -->\n### Re: note\n\nDone.\n<!-- /agent:exchange -->\n";
        assert!(normal_closeout_failure(content).unwrap().is_none());
    }

    /// sdk-dev.md, 2026-11-11: a heading-level running leaf and a nested
    /// lost-running leaf under an `agent:run` pull-request parent both stayed
    /// live when the turn ended. Both are transient debt.
    #[test]
    fn sdk_dev_heading_and_nested_run_rows_are_transient_debt() {
        let content = "\
<!-- agent:run id=\"haiven-sdk-prs\" subagents -->
- ### https://github.com/haiven-dev/haiven-sdk/pull/228
  - %% ci(release): dispatch sdk-dev promotion from main %%
  - ⚠️🚧 [#subagent:pr228seventeen#17] #adversarial-pr [#q-abf0adec8270]
- ### 🚧 [#subagent:rpversionpr#1] Do we need to create a PR to make Release Please reliably update the version [#q-3a7937eee419]
<!-- /agent:run -->
";
        let failure = normal_closeout_failure(content)
            .unwrap()
            .expect("both sdk-dev rows refuse closeout");
        assert!(
            failure.contains(
                "🚧 [#subagent:rpversionpr#1] Do we need to create a PR to make Release Please reliably update the version [#q-3a7937eee419]"
            ),
            "{failure}"
        );
        assert!(
            failure
                .contains("⚠️🚧 [#subagent:pr228seventeen#17] #adversarial-pr [#q-abf0adec8270]"),
            "{failure}"
        );
        assert!(failure.contains("no bound running worker claim backs this row"));
        assert!(failure.contains("finish the recorded stop/retry transition"));
        assert!(failure.contains(
            "a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        ));
        assert!(failure.contains(
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        ));
        assert!(
            !failure.contains("ci(release)"),
            "a comment child is not a transient row: {failure}"
        );
    }

    // ---- `#stopquiescentrows` ----------------------------------------------

    const REQ: &str = "subagent-operator-request-flagset";

    /// A document with a queue, an exchange, and a Todo. `packet` records the
    /// request packet in the exchange; `todo` opens its Todo action.
    fn operator_doc(queue: &str, packet: bool, todo: bool) -> String {
        let exchange = if packet {
            format!("[#{REQ}] subagent operator request\n- *kind:* question\n")
        } else {
            "No packet here.\n".to_string()
        };
        let todo_row = if todo {
            format!("- [ ] [#{REQ}] Answer the blocking subagent question\n")
        } else {
            String::new()
        };
        format!(
            "<!-- agent:todo -->\n{todo_row}<!-- /agent:todo -->\n\
             <!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\
             <!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n"
        )
    }

    /// The claim identity of the live row containing `needle`, as the gate
    /// keys it.
    fn identity_of(content: &str, needle: &str) -> QueueItemIdentity {
        let components = agent_doc_element::element::parse(content).unwrap();
        for component in components.iter().filter(|c| c.name == "queue") {
            let entries = parse_queue_body(component.content(content)).unwrap();
            for (index, entry) in entries.iter().enumerate() {
                if let QueueEntry::Prompt(prompt) = entry
                    && prompt.text.contains(needle)
                {
                    return crate::queue_claim::claim_identity_at(&entries, index).unwrap();
                }
            }
        }
        panic!("no live row contains {needle:?}");
    }

    /// Evidence with a fresh claim, in the phase its visible marker shows,
    /// behind each row containing one of `needles`.
    fn fresh(content: &str, needles: &[&str]) -> CloseoutEvidence {
        CloseoutEvidence {
            fresh_claims: needles
                .iter()
                .map(|needle| {
                    let phase = if content
                        .lines()
                        .any(|line| line.contains(needle) && line.contains('🟡'))
                    {
                        FreshClaim::Dispatching
                    } else {
                        FreshClaim::Running
                    };
                    (identity_of(content, needle), phase)
                })
                .collect(),
            ..CloseoutEvidence::default()
        }
    }

    fn reasons(assessment: &TransientCloseoutAssessment) -> Vec<(String, QuiescentReason)> {
        assessment
            .quiescent
            .iter()
            .map(|row| (row.row.clone(), row.reason))
            .collect()
    }

    /// sdk-dev.md, 2026-11-13: the Stop hook looped over operator-gated
    /// `🙋📥` rows with no running worker. With the packet in the exchange and
    /// the Todo open, the row waits on the operator and stays live.
    #[test]
    fn operator_owned_pending_row_is_quiescent_once_packet_and_todo_exist() {
        let content = operator_doc(&format!("- 🙋📥 set the flag [#{REQ}]\n"), true, true);
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            reasons(&assessment),
            vec![(
                format!("🙋📥 set the flag [#{REQ}]"),
                QuiescentReason::OperatorOwned
            )]
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_operator"));
        assert!(normal_closeout_failure(&content).unwrap().is_none());
        assert!(crash_recovery_closeout_failure(&content).unwrap().is_none());

        // The heading form sdk-dev.md uses.
        let heading = operator_doc(&format!("- ### 🙋📥 set the flag [#{REQ}]\n"), true, true);
        assert!(normal_closeout_failure(&heading).unwrap().is_none());
    }

    /// The obligation that remains: a `🙋` row whose packet or Todo is not
    /// recorded still refuses and names what to record.
    #[test]
    fn operator_row_without_packet_or_todo_still_refuses() {
        for (packet, todo) in [(false, true), (true, false), (false, false)] {
            let content = operator_doc(&format!("- 🙋📥 set the flag [#{REQ}]\n"), packet, todo);
            let failure = normal_closeout_failure(&content)
                .unwrap()
                .unwrap_or_else(|| panic!("packet={packet} todo={todo} must refuse"));
            assert!(failure.contains("🙋📥 set the flag"), "{failure}");
            assert!(
                failure.contains("operator-request packet in agent:exchange"),
                "{failure}"
            );
        }
        // A `🙋` with no id at all cannot link a packet.
        let content = operator_doc("- 🙋📥 set the flag\n", true, true);
        assert!(normal_closeout_failure(&content).unwrap().is_some());
    }

    /// `🙋🚧`: the `🙋` part does not block, and the worker part follows the
    /// claim-freshness rule.
    #[test]
    fn operator_running_row_needs_a_recorded_request_and_a_fresh_claim() {
        let row = format!("- 🙋🚧 [#subagent:flag#1] set the flag [#{REQ}]\n");
        let content = operator_doc(&row, true, true);
        let live = fresh(&content, &["set the flag"]);
        let assessment = assess_transient_closeout(&content, &live).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            assessment.quiescent[0].reason,
            QuiescentReason::WaitingOnClaim
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_claims"));

        let stale = normal_closeout_failure_with(&content, &CloseoutEvidence::default())
            .unwrap()
            .expect("a stale worker under 🙋 still refuses");
        assert!(stale.contains("no bound running worker claim"), "{stale}");

        let unrecorded = operator_doc(&row, false, true);
        let failure =
            normal_closeout_failure_with(&unrecorded, &fresh(&unrecorded, &["set the flag"]))
                .unwrap()
                .expect("an unrecorded request refuses even with a fresh worker");
        assert!(
            failure.contains("operator-request packet in agent:exchange"),
            "{failure}"
        );
    }

    #[test]
    fn a_codex_operator_wait_never_hides_the_worker_report_after_completion() {
        let row = format!("- 🙋🚧 [#subagent:flag#1] set the flag [#{REQ}]\n");
        let content = operator_doc(&row, true, true);
        let identity = identity_of(&content, "set the flag");
        let mut evidence = fresh(&content, &["set the flag"]);
        evidence.codex_coordinator_claims.insert(identity.clone());
        assert!(
            normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .is_none()
        );

        evidence.fresh_claims.clear();
        evidence.report_owed.insert(
            identity,
            ReportOwedAttempt {
                item: "set the flag".to_string(),
                generation: 1,
                owner: "subagent:flag".to_string(),
                worker_id: "worker:flag".to_string(),
                interim: false,
            },
        );
        for body in [content, operator_doc(&row, false, true)] {
            let failure = codex_worker_capture_failure(&body, &evidence)
                .unwrap()
                .unwrap();
            assert!(failure.contains("queue report-complete"), "{failure}");
            assert!(failure.contains("--generation 1"), "{failure}");
        }
    }

    /// `#gatedheadpending` (an sdk session, 2026-11-14): the Stop hook refused
    /// closeout over `📥` heads no agent could advance, so the coordinator
    /// parked each behind an invented operator packet. A head whose item is
    /// itself an open operator request awaits the operator (`⏳`); a head whose
    /// item records an external blocker is blocked (`⛔`). A runnable head, or
    /// one the operator already answered, still refuses.
    fn gated_head_doc(done: &str) -> String {
        format!(
            concat!(
                "<!-- agent:queue -->\n",
                "- 📥 do [#subagent-operator-request-pushchoice]\n",
                "- 📥 do [#licensefix]\n",
                "- 📥 do [#plain]\n",
                "<!-- /agent:queue -->\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#subagent-operator-request-pushchoice] Push the scrub branches, or leave them local? (choices: push, keep-local)\n",
                "- [ ] [#licensefix] Remove the stale sentence. (stays open: blocked behind #vendorfix; the vendor PR still lacks the file)\n",
                "- [ ] [#plain] Runnable work.\n",
                "<!-- /agent:backlog -->\n",
                "{}",
            ),
            done
        )
    }

    #[test]
    fn pending_heads_whose_items_cannot_run_are_quiescent() {
        let content = gated_head_doc("");
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(
            reasons(&assessment),
            vec![
                (
                    "📥 do [#subagent-operator-request-pushchoice]".to_string(),
                    QuiescentReason::AwaitingOperator
                ),
                ("📥 do [#licensefix]".to_string(), QuiescentReason::Blocked),
            ],
            "{assessment:?}"
        );
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(assessment.debts[0].row, "📥 do [#plain]");
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_operator"));
        assert!(
            assessment
                .quiescent_log_fields()
                .ends_with("awaiting_operator=1 queue_held=0 blocked=1 waiting_on_integration=0"),
            "{}",
            assessment.quiescent_log_fields()
        );

        // The operator's inline answer makes the request head agent work.
        let answered = content.replace(
            "- 📥 do [#subagent-operator-request-pushchoice]\n",
            "- 📥 do [#subagent-operator-request-pushchoice]: push\n",
        );
        let failure = normal_closeout_failure(&answered)
            .unwrap()
            .expect("answered and runnable heads refuse");
        assert!(
            failure.contains("do [#subagent-operator-request-pushchoice]: push"),
            "{failure}"
        );
        assert!(failure.contains("do [#plain]"), "{failure}");
        assert!(!failure.contains("do [#licensefix]"), "{failure}");
    }

    /// `#blockerreportterminal` (agent-doc-bugs.md, 2026-11-21): the exact
    /// rows an older binary left after their workers handed back only
    /// prompt-blocker reviews. Each still showed `📥` with a stale
    /// `[#subagent:…#1]` tag, no worker was live, and an open review held
    /// it, one by an external hold (`**Evidence:**`) and one by the legacy
    /// short prompt `do [#ghrecvpolicy]`. Every such row is quiescent
    /// `blocked`; a runnable row beside them still refuses.
    fn blockerreportterminal_doc() -> String {
        concat!(
            "<!-- agent:run id=\"main\" subagents go -->\n",
            "- ### 📥 [#subagent:ghrecvpolicy#1] do [#ghrecvpolicy]: #advance whatever is actionable.\n",
            "- ### 📥 [#subagent:chromium_steps#1] [#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.\n",
            "- ### 📥 do [#plain]\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ghrecvpolicy] Recipient-side policy. (stays open: workspace access control and session isolation must land before enforcing the recipient policy)\n",
            "- [ ] [#plain] Runnable work.\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "<!-- agent-doc:prompt-blocker -->\n",
            "- [/] [#prompt-blocker-dfc6201b3410] Production activation awaits a signed frontend release.\n",
            "  - **Complete by:** verify the signed package\n",
            "  - **Prompt:** [#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.\n",
            "<!-- /agent-doc:prompt-blocker -->\n",
            "<!-- agent-doc:prompt-blocker -->\n",
            "- [/] [#prompt-blocker-01eda3b6d7de] Recipient policy awaits multi-user support.\n",
            "  - **Complete by:** enforce it once multi-user workspaces land\n",
            "  - **Prompt:** do [#ghrecvpolicy]\n",
            "<!-- /agent-doc:prompt-blocker -->\n",
            "<!-- /agent:review -->\n",
        )
        .to_string()
    }

    #[test]
    fn blockerreportterminal_review_held_pending_rows_are_quiescent_blocked() {
        let content = blockerreportterminal_doc();
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(
            reasons(&assessment),
            vec![
                (
                    "📥 [#subagent:ghrecvpolicy#1] do [#ghrecvpolicy]: #advance whatever is actionable.".to_string(),
                    QuiescentReason::Blocked
                ),
                (
                    "📥 [#subagent:chromium_steps#1] [#prompt-blocker-b0ddfbae2f94]: Use chromium-bridge to perform the steps.".to_string(),
                    QuiescentReason::Blocked
                ),
            ],
            "{assessment:?}"
        );
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(assessment.debts[0].row, "📥 do [#plain]");
        assert!(!every_debt_gated_by_recorded_blocker(
            &content,
            &assessment.debts,
            None
        ));

        // Without the runnable row the turn may end over the blocked rows.
        let only_blocked = content.replace("- ### 📥 do [#plain]\n", "");
        let assessment =
            assess_transient_closeout(&only_blocked, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(assessment.quiescent_state(), Some("blocked"));

        // Resolving a review makes its row agent work again.
        let resolved = only_blocked.replace(
            "- [/] [#prompt-blocker-01eda3b6d7de]",
            "- [x] [#prompt-blocker-01eda3b6d7de]",
        );
        let failure = normal_closeout_failure(&resolved)
            .unwrap()
            .expect("a resolved review releases its row");
        assert!(failure.contains("do [#ghrecvpolicy]"), "{failure}");
        assert!(!failure.contains("chromium-bridge"), "{failure}");
    }

    /// `#blockerreportterminal` (4): a debt row a recorded blocker gates
    /// (here a stale `🚧` no worker backs) is not one the agent can move, so a
    /// repeated refusal over only such rows may end the turn. Any ungated row
    /// keeps the refusal.
    #[test]
    fn blockerreportterminal_every_gated_debt_is_recognised() {
        let content = blockerreportterminal_doc()
            .replace(
                "- ### 📥 [#subagent:ghrecvpolicy#1] do [#ghrecvpolicy]",
                "- ### 🚧 [#subagent:ghrecvpolicy#1] do [#ghrecvpolicy]",
            )
            .replace("- ### 📥 do [#plain]\n", "");
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(
            assessment.debts[0].marker,
            QueueLifecycleMarker::Running,
            "{assessment:?}"
        );
        assert!(every_debt_gated_by_recorded_blocker(
            &content,
            &assessment.debts,
            None
        ));
        assert!(!every_debt_gated_by_recorded_blocker(&content, &[], None));

        let ungated = content.replace(
            "- ### 🚧 [#subagent:ghrecvpolicy#1]",
            "- ### 🚧 [#subagent:plain#1] do [#plain]\n- ### 🚧 [#subagent:ghrecvpolicy#1]",
        );
        let assessment = assess_transient_closeout(&ungated, &CloseoutEvidence::default()).unwrap();
        assert_eq!(assessment.debts.len(), 2, "{assessment:?}");
        assert!(!every_debt_gated_by_recorded_blocker(
            &ungated,
            &assessment.debts,
            None
        ));
    }

    /// The IO evidence carries the external done archive: a blocker that
    /// closed into it is resolved, so the head is runnable and refuses; one
    /// the archive does not hold is an external blocker.
    #[test]
    fn archived_blockers_resolve_through_the_evidence() {
        let content = gated_head_doc(
            "<!-- agent:done archive -->\n<!-- completed work archived in x.done.md -->\n<!-- /agent:done -->\n",
        );
        let archived = CloseoutEvidence {
            archived_done_ids: Some(["vendorfix".to_string()].into()),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &archived).unwrap();
        assert!(
            assessment
                .debts
                .iter()
                .any(|debt| debt.row == "📥 do [#licensefix]"),
            "{assessment:?}"
        );
        let unresolved = CloseoutEvidence {
            archived_done_ids: Some(HashSet::new()),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &unresolved).unwrap();
        assert_eq!(
            assessment.quiescent_count(QuiescentReason::Blocked),
            1,
            "{assessment:?}"
        );
    }

    /// `⏳` is resting whatever its packet state, alone or under `🙋`.
    #[test]
    fn awaiting_operator_rows_never_refuse() {
        let content = operator_doc(
            &format!("- ⏳ verify the release\n- ### 🙋⏳ approve PR 98 [#{REQ}]\n"),
            false,
            false,
        );
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
    }

    /// `#personrowselect` (haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-15):
    /// a turn whose only live rows wait on a person ends. The `[operator-verify]`
    /// human follow-ups pass whether they show `📥` or the `🚧`/`🟡` an
    /// in-session selection painted on them: `queue claim` refuses such a row
    /// (`#humanfollowupowner`), so no claim or worker stands behind that glyph
    /// and no agent transition can move it. An agent row beside them, and a
    /// person row with a worker attempt or a report owed, still refuse.
    #[test]
    fn personrowselect_rows_waiting_on_a_person_end_the_turn_and_agent_rows_still_refuse() {
        const TMSTORM: &str = "follow up on an action item from `q-37985c4b3fac`: human (tmstorm): the CHANGES_REQUESTED review still stands. [operator-verify] [#q-4837c3f0fb56]";
        const OPERATOR: &str = "follow up on an action item from `q-f43eb22a9e5c`: operator: decide how #244 lands. [operator-verify]";
        for (pending, painted) in [("📥", "🚧"), ("📥", "🟡"), ("", "### 🚧")] {
            let content = doc(&format!(
                "- ~~#adversarial-pr [#q-fc36a4bdbcd3]~~\n- {pending} {OPERATOR}\n- {painted} {TMSTORM}\n"
            ));
            let assessment =
                assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
            assert!(
                assessment.debts.is_empty(),
                "{pending:?}/{painted:?}: {assessment:?}"
            );
            assert!(normal_closeout_failure(&content).unwrap().is_none());
            let awaiting = assessment
                .quiescent
                .iter()
                .filter(|row| row.reason == QuiescentReason::AwaitingOperator)
                .count();
            assert_eq!(
                awaiting,
                if pending.is_empty() { 1 } else { 2 },
                "{assessment:?}"
            );
            assert_eq!(assessment.quiescent_state(), Some("waiting_on_operator"));
        }

        // A pending agent row beside them still holds the turn, and the
        // refusal names only that row.
        let mixed = doc(&format!(
            "- 📥 {OPERATOR}\n- 🚧 {TMSTORM}\n- 📥 follow up on an action item from `q-1`: agent: rebase PR 7 onto main.\n"
        ));
        let failure = normal_closeout_failure(&mixed)
            .unwrap()
            .expect("a pending agent row refuses");
        assert!(failure.contains("agent: rebase PR 7"), "{failure}");
        // `#finishactionable`: an agent-owned action item is this turn's work.
        assert!(failure.contains(OWED_AGENT_ACTION_ITEM), "{failure}");
        assert!(!failure.contains("tmstorm"), "{failure}");
        assert!(!failure.contains("decide how #244"), "{failure}");

        // A stale `🚧` on an agent row is still a running worker the turn owes.
        let agent_running =
            doc("- 🚧 follow up on an action item from `q-1`: agent: rebase PR 7.\n");
        assert!(
            normal_closeout_failure(&agent_running)
                .unwrap()
                .expect("an unbacked agent 🚧 refuses")
                .contains("no bound running worker claim backs this row")
        );

        // A worker attempt on the person row (`[#owner#generation]`) is not an
        // in-session selection marker: its lost or claimed worker still owes
        // its transition.
        for row in [
            format!("- 🚧 [#subagent:approve#1] {TMSTORM}\n"),
            format!("- 🟡 [#subagent:approve#1] {TMSTORM}\n"),
        ] {
            let assessment =
                assess_transient_closeout(&doc(&row), &CloseoutEvidence::default()).unwrap();
            assert_eq!(assessment.debts.len(), 1, "{row}: {assessment:?}");
        }

        // A report owed on the person row's attempt still names the report.
        let content = doc(&format!("- 🚧 {TMSTORM}\n"));
        let evidence = CloseoutEvidence {
            report_owed: HashMap::from([(
                identity_of(&content, "follow up on an action item"),
                ReportOwedAttempt {
                    item: "#q-4837c3f0fb56".to_string(),
                    generation: 1,
                    owner: "subagent:tmstorm".to_string(),
                    worker_id: "worker:tmstorm".to_string(),
                    interim: false,
                },
            )]),
            ..CloseoutEvidence::default()
        };
        let failure = normal_closeout_failure_with(&content, &evidence)
            .unwrap()
            .expect("a report-owed attempt refuses");
        assert!(failure.contains("queue report-complete"), "{failure}");

        // The operator's inline answer hands the row back to an agent.
        let answered = doc(&format!("- 🚧 {TMSTORM}: approved, merge it\n"));
        assert!(normal_closeout_failure(&answered).unwrap().is_some());
    }

    /// agent-doc-bugs.md, 2026-11-13: the Stop hook refused six stops over
    /// two running workers with durable bindings. A bound `🚧` and a `🟡`
    /// inside its dispatch grace wait on claims; the same rows with no active
    /// claim still refuse.
    #[test]
    fn active_worker_claims_are_quiescent_and_missing_ones_refuse() {
        let content = doc("\
- 🚧 [#subagent:alc#1] do [#admissionleasechurn]
- 🚧 [#subagent:ssb#1] do [#storeschemabatch]
- 🟡 [#subagent:new#1] do [#justdispatched]
");
        let live = fresh(
            &content,
            &["admissionleasechurn", "storeschemabatch", "justdispatched"],
        );
        let assessment = assess_transient_closeout(&content, &live).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            assessment.quiescent_count(QuiescentReason::WaitingOnClaim),
            3
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_claims"));
        assert_eq!(
            assessment.quiescent_log_fields(),
            "waiting_on_claims=3 operator_owned=0 schedule_not_due=0 awaiting_operator=0 queue_held=0 blocked=0 waiting_on_integration=0"
        );

        // One row has no bound claim: only that row refuses.
        let partly = fresh(&content, &["admissionleasechurn", "justdispatched"]);
        let failure = normal_closeout_failure_with(&content, &partly)
            .unwrap()
            .expect("an unbound running row refuses");
        assert!(failure.contains("storeschemabatch"), "{failure}");
        assert!(!failure.contains("admissionleasechurn"), "{failure}");
        assert!(failure.contains("agent-doc queue start"), "{failure}");
        assert!(failure.contains("finish the recorded stop/retry transition"));

        let unacknowledged = fresh(&content, &["admissionleasechurn", "storeschemabatch"]);
        let failure = normal_closeout_failure_with(&content, &unacknowledged)
            .unwrap()
            .expect("an unacknowledged dispatch refuses");
        assert!(failure.contains("justdispatched"), "{failure}");
        assert!(failure.contains("no recent dispatch claim"), "{failure}");
    }

    /// The visible marker must agree with the durable phase: a `🚧` row whose
    /// only claim is an unbound dispatch is not a running worker, and a `🟡`
    /// row is not backed by a running claim's heartbeat.
    #[test]
    fn a_fresh_claim_backs_only_the_marker_of_its_phase() {
        let content = doc("- 🚧 do [#mislabelled]\n- 🟡 do [#alreadyrunning]\n");
        let evidence = CloseoutEvidence {
            fresh_claims: [
                (
                    identity_of(&content, "mislabelled"),
                    FreshClaim::Dispatching,
                ),
                (identity_of(&content, "alreadyrunning"), FreshClaim::Running),
            ]
            .into_iter()
            .collect(),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 2, "{assessment:?}");
    }

    /// Agent-actionable rows keep refusing even beside quiescent ones, and
    /// a fresh claim never exempts a reported, lost, or pending row.
    #[test]
    fn agent_actionable_rows_still_refuse_beside_quiescent_rows() {
        let content = operator_doc(
            &format!(
                "- 🚧 [#subagent:live#1] do [#liveworker]\n\
                 - 🙋📥 set the flag [#{REQ}]\n\
                 - ☑️ [#subagent:done#1] do [#reportedwork]\n\
                 - 📥 do [#drainablehead]\n\
                 - ⚠️🚧 [#subagent:gone#2] do [#lostworker]\n"
            ),
            true,
            true,
        );
        let evidence = fresh(
            &content,
            &["liveworker", "reportedwork", "drainablehead", "lostworker"],
        );
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        let debts = assessment
            .debts
            .iter()
            .map(|debt| debt.row.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            debts,
            vec![
                "☑️ [#subagent:done#1] do [#reportedwork]",
                "📥 do [#drainablehead]",
                "⚠️🚧 [#subagent:gone#2] do [#lostworker]",
            ]
        );
        assert_eq!(
            assessment.quiescent_count(QuiescentReason::WaitingOnClaim),
            1
        );
        assert_eq!(
            assessment.quiescent_count(QuiescentReason::OperatorOwned),
            1
        );
        let failure = normal_closeout_failure_for(&assessment).unwrap();
        assert!(
            failure.contains("integrate the report or otherwise leave the reported-but-open state")
        );
        assert!(failure.contains("leave pending before the turn ends"));
        assert!(!failure.contains("liveworker"), "{failure}");
        assert!(!failure.contains("set the flag"), "{failure}");
    }

    /// `🙋☑️`: the reported integration is gated on the operator's answer.
    #[test]
    fn operator_gated_reported_row_is_quiescent() {
        let content = operator_doc(
            &format!("- 🙋☑️ [#subagent:review#1] confusion about markers [#{REQ}]\n"),
            true,
            true,
        );
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            assessment.quiescent[0].reason,
            QuiescentReason::OperatorOwned
        );
    }

    /// agent-doc-bugs.md, 2026-11-13: an hourly `agent:loop interval="1h"
    /// keep` row whose window is closed for this interval is not due. The
    /// same row inside an open window is a drainable head and refuses.
    #[test]
    fn a_loop_row_held_by_a_closed_schedule_window_is_not_due() {
        let content = "\
<!-- agent:loop interval=\"1h\" priority keep -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. [#q-241aecb69d4f]
<!-- /agent:loop -->
";
        let row = "📥 Watch https://github.com/btakita/agent-doc/issues for new issues. [#q-241aecb69d4f]";
        let held = CloseoutEvidence {
            schedule_held_heads: [row.to_string()].into_iter().collect(),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(content, &held).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            reasons(&assessment),
            vec![(row.to_string(), QuiescentReason::ScheduleNotDue)]
        );
        assert_eq!(assessment.quiescent_state(), Some("schedule_not_due"));

        let open_window = normal_closeout_failure_with(content, &CloseoutEvidence::default())
            .unwrap()
            .expect("a due loop row is a drainable head");
        assert!(
            open_window.contains("Watch https://github.com"),
            "{open_window}"
        );
    }

    /// `#heldloopstophook` (agent-doc-bugs.md, 2026-11-14): a `📥` head in a
    /// queue the operator holds (`pause`) is not drainable, so no gate refuses
    /// the turn over it. Queue selection reads the same control as `Paused`.
    #[test]
    fn a_pending_head_in_a_held_queue_is_quiescent() {
        const ROW: &str = "📥 Watch https://github.com/btakita/agent-doc/issues for new issues. [#q-241aecb69d4f]";
        for marker in [
            "agent:loop interval=\"1h\" priority keep pause",
            "agent:queue pause",
            "agent:queue id=\"side\" stop",
        ] {
            let name = marker.split_whitespace().next().unwrap();
            let content = format!("<!-- {marker} -->\n- {ROW}\n<!-- /{name} -->\n");
            let assessment =
                assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
            assert!(assessment.debts.is_empty(), "{marker}: {assessment:?}");
            assert_eq!(
                reasons(&assessment),
                vec![(ROW.to_string(), QuiescentReason::QueueHeld)],
                "{marker}"
            );
            assert_eq!(assessment.quiescent_state(), Some("queue_held"), "{marker}");
            assert!(
                assessment
                    .quiescent_log_fields()
                    .ends_with("queue_held=1 blocked=0 waiting_on_integration=0"),
                "{}",
                assessment.quiescent_log_fields()
            );
            assert_eq!(normal_closeout_failure(&content).unwrap(), None, "{marker}");
            assert_eq!(
                crash_recovery_closeout_failure(&content).unwrap(),
                None,
                "{marker}"
            );
        }
    }

    /// `#heldloopstophook`, the live agent-doc-bugs.md shape (2026-11-14): the
    /// `## Loop <!-- agent:loop interval="1h" priority keep stop -->` watch row
    /// inside an open pass refused every Stop with "a head that is still
    /// pending means the turn is not over". Marker `stop` is the operator's
    /// hold on a loop too, so the row is quiescent with no schedule evidence,
    /// while the `agent:run` rows beside it keep their own obligations.
    #[test]
    fn the_live_stopped_loop_watch_row_does_not_hold_the_turn() {
        const WATCH: &str = "📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45. [#q-241aecb69d4f]";
        let content = format!(
            "---\nagent_doc_format: template\n---\n\n\
## Run <!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push: #dogfood-install: approve all commit + integration + push: approve all worktree removals\" priority go -->\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop interval=\"1h\" priority keep stop -->\n\
- {WATCH}\n\
<!-- /agent:loop -->\n"
        );
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            reasons(&assessment),
            vec![(WATCH.to_string(), QuiescentReason::QueueHeld)]
        );
        assert_eq!(normal_closeout_failure(&content).unwrap(), None);

        let unheld = content.replace("priority keep stop", "priority keep");
        let failure = normal_closeout_failure(&unheld)
            .unwrap()
            .expect("without the hold the open-window watch row is due");
        assert!(failure.contains(WATCH), "{failure}");
    }

    /// `#heldloopstophook`: frontmatter `queue: pause` holds a queue whose
    /// marker carries no control; a marker `go` overrides it, and an unheld
    /// queue's due `📥` head still refuses beside a held one.
    #[test]
    fn frontmatter_pause_holds_only_queues_without_a_marker_control() {
        let content = "---\nqueue: pause\n---\n\
<!-- agent:run go -->\n- 📥 run work now\n<!-- /agent:run -->\n\
<!-- agent:loop interval=\"1h\" priority keep -->\n- 📥 held watch\n<!-- /agent:loop -->\n";
        let assessment = assess_transient_closeout(content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(
            reasons(&assessment),
            vec![("📥 held watch".to_string(), QuiescentReason::QueueHeld)]
        );
        let failure = normal_closeout_failure(content)
            .unwrap()
            .expect("the go queue's head is drainable");
        assert!(failure.contains("📥 run work now"), "{failure}");
        assert!(!failure.contains("held watch"), "{failure}");
    }

    /// `#heldloopstophook`: the hold stops new dispatch only. A held queue's
    /// reported row still owes integration, and its running worker still owes
    /// a fresh claim.
    #[test]
    fn a_held_queue_still_owes_its_non_pending_transitions() {
        let content = "\
<!-- agent:queue pause -->
- ☑️ [#subagent:work#1] reported but open
- 🚧 [#subagent:other#1] still running
- 📥 held head
<!-- /agent:queue -->
";
        let assessment = assess_transient_closeout(content, &CloseoutEvidence::default()).unwrap();
        let debts: Vec<_> = assessment.debts.iter().map(|debt| debt.marker).collect();
        assert_eq!(
            debts,
            vec![
                QueueLifecycleMarker::Reported,
                QueueLifecycleMarker::Running
            ]
        );
        assert_eq!(
            reasons(&assessment),
            vec![("📥 held head".to_string(), QuiescentReason::QueueHeld)]
        );
    }

    /// `#heldloopstophook`: an unheld loop whose window is open has a due,
    /// drainable `📥` head, and every gate still refuses over it.
    #[test]
    fn an_unheld_due_loop_head_still_refuses() {
        let content = "\
<!-- agent:loop interval=\"1h\" priority keep -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. [#q-241aecb69d4f]
<!-- /agent:loop -->
";
        let failure = normal_closeout_failure(content)
            .unwrap()
            .expect("a due loop head refuses");
        assert!(
            failure.contains("a head that is still pending means the turn is not over"),
            "{failure}"
        );
    }

    fn worker_claim(
        state: crate::queue_claim::QueueClaimState,
        claimed_at_secs: u64,
        worker_started: Option<u64>,
        heartbeat_at_secs: Option<u64>,
    ) -> crate::queue_claim::QueueClaim {
        crate::queue_claim::QueueClaim {
            identity: crate::queue_claim::claim_identity("do [#work]"),
            item_text: "do [#work]".to_string(),
            owner: "subagent:work".to_string(),
            generation: 1,
            state,
            lost_from: None,
            completion_recovered_after_loss: false,
            publication_state: None,
            native_spawn_pending: Some(false),
            worker: worker_started.map(|started_at_secs| crate::queue_claim::QueueWorkerBinding {
                worker_id: "worker:work".to_string(),
                started_at_secs,
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs,
            expires_at_secs: claimed_at_secs + 7_200,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            reported_at_secs: None,
            heartbeat_at_secs,
            frozen: None,
            quota_block: None,
            lost_continue: None,
        }
    }

    /// Freshness is the heartbeat and dispatch-grace evidence the work pool
    /// already uses, never the visible marker.
    #[test]
    fn claim_freshness_follows_heartbeat_and_dispatch_grace() {
        use crate::queue_claim::QueueClaimState;
        const FROZEN: u64 = 600;
        const GRACE: u64 = 10;
        let running = worker_claim(QueueClaimState::Running, 1_000, Some(1_000), Some(1_500));
        assert_eq!(
            fresh_claim_phase(&running, 1_600, FROZEN, GRACE),
            Some(FreshClaim::Running)
        );
        assert_eq!(
            fresh_claim_phase(&running, u64::MAX, FROZEN, GRACE),
            Some(FreshClaim::Running),
            "a bound worker has no runtime timeout"
        );

        let dispatching = worker_claim(QueueClaimState::Claimed, 1_000, None, None);
        assert_eq!(
            fresh_claim_phase(&dispatching, 1_005, FROZEN, GRACE),
            Some(FreshClaim::Dispatching)
        );
        assert!(
            fresh_claim_phase(&dispatching, 1_000 + GRACE, FROZEN, GRACE).is_none(),
            "an unacknowledged dispatch past its grace is a coordinator obligation"
        );

        for state in [
            QueueClaimState::Reported,
            QueueClaimState::Lost,
            QueueClaimState::QuotaBlocked,
        ] {
            let claim = worker_claim(state, 1_000, Some(1_000), Some(1_500));
            assert!(
                fresh_claim_phase(&claim, 1_600, FROZEN, GRACE).is_none(),
                "{state:?} is not a worker wait"
            );
        }
    }

    // ---- `#orphanreportedrow` ----------------------------------------------

    const ORPHAN_REQ: &str = "subagent-operator-request-bossroom-tag-obsolete";

    /// The live sdk-dev.md shape (2026-11-14): a heading-bullet `☑️` row in
    /// `agent:run id=admin` whose kept response tree waits on an operator
    /// request. The row carries the request id after its generated `[#q-…]`
    /// but no `🙋`, and nothing under it is a live prompt.
    fn orphan_reported_doc(todo_open: bool, packet: bool) -> String {
        let todo_box = if todo_open { " " } else { "x" };
        let exchange = if packet {
            format!(
                "**BOSS_ROOM_TAG [#q-5afca7ba8417]: no change made.**\n\n\
                 [#{ORPHAN_REQ}] subagent operator request\n- *kind:* question\n"
            )
        } else {
            "No packet here.\n".to_string()
        };
        r##"## Admin <!-- agent:run id="admin" subagents preset="#ip" priority -->
- ### ☑️ [#subagent:bossroom-tag#1] In haiven-sdk, bump `BOSS_ROOM_TAG` in `assemble.py` from `v2.5.0` to `v3.0.0`. [#q-5afca7ba8417] [#REQ]
  - **Response [#r-5d0ca16f4d91]**
    - **Timestamp:** 2026-11-14T07:45:39.000Z
    - **Summary:** **BOSS_ROOM_TAG [#q-5afca7ba8417]: no change made. The row is obsolete.** Close this row as obsolete, or change what it asks for?
    - **State:** Waiting on operator input.
    - **Guidance:** More work remains. Keep this tree and answer the operator request below.
    - **Recommendation:** **close-obsolete** — both halves are settled by merged work.
    - **Choices:**
      - **Request**: [#REQ]
      - **close-obsolete**: no change. Delete or close the row. — **Recommended**
      - **comment-pr**: a small PR into `sdk-dev` adding one comment line.
<!-- /agent:run -->

## Todo <!-- agent:todo priority -->
- [BOX] [#REQ] Answer the blocking subagent question: Close this row as obsolete? (choices: close-obsolete, comment-pr)
<!-- /agent:todo -->

<!-- agent:exchange -->
EXCHANGE<!-- /agent:exchange -->
"##
        .replace("REQ", ORPHAN_REQ)
        .replace("BOX", todo_box)
        .replace("EXCHANGE", &exchange)
    }

    /// While the response's request is open and recorded, the row waits on
    /// the operator: the Stop hook ends the turn quietly over it.
    #[test]
    fn orphanreportedrow_open_request_response_tree_is_operator_owned() {
        let content = orphan_reported_doc(true, true);
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(assessment.quiescent.len(), 1, "{assessment:?}");
        assert_eq!(
            assessment.quiescent[0].reason,
            QuiescentReason::OperatorOwned
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_operator"));
        assert!(normal_closeout_failure(&content).unwrap().is_none());
    }

    /// An open request whose packet is not recorded still owes the record.
    #[test]
    fn orphanreportedrow_open_request_without_packet_owes_the_record() {
        let content = orphan_reported_doc(true, false);
        let failure = normal_closeout_failure(&content)
            .unwrap()
            .expect("an unrecorded request refuses");
        assert!(
            failure.contains("operator-request packet in agent:exchange"),
            "{failure}"
        );
    }

    /// Once the request is answered, the agent owes the strike, and the
    /// refusal names the command that performs it rather than a generic
    /// integration no command could finish.
    #[test]
    fn orphanreportedrow_answered_request_owes_a_strike_by_generated_id() {
        let content = orphan_reported_doc(false, true);
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(assessment.debts[0].owed, OWED_REPORTED_RESPONSE_ORIGIN);
        let failure = normal_closeout_failure(&content).unwrap().unwrap();
        assert!(failure.contains("[#q-5afca7ba8417]"), "{failure}");
        assert!(
            failure.contains("never ask the operator to delete it by hand"),
            "{failure}"
        );

        // With the document path the turn-end gates supply, the refusal
        // names the exact agent command that closes this row.
        let evidence = CloseoutEvidence {
            document: Some("tasks/sdk/sdk-dev.md".to_string()),
            ..CloseoutEvidence::default()
        };
        let failure = normal_closeout_failure_with(&content, &evidence)
            .unwrap()
            .unwrap();
        assert!(
            failure.contains(
                "Close it now with `agent-doc queue consume tasks/sdk/sdk-dev.md --ack-id q-5afca7ba8417 --ack-owner 'subagent:bossroom-tag#1'`."
            ),
            "{failure}"
        );
        let unfilled = normal_closeout_failure(&content).unwrap().unwrap();
        assert!(
            unfilled.contains("agent-doc queue consume '<FILE>' --ack-id q-5afca7ba8417")
                || unfilled.contains("agent-doc queue consume <FILE> --ack-id q-5afca7ba8417"),
            "{unfilled}"
        );
    }

    /// A row that carries an open, recorded request id after its q-id but
    /// has no response tree also waits on the operator without `🙋`.
    #[test]
    fn orphanreportedrow_trailing_request_annotation_alone_is_operator_owned() {
        let content = operator_doc(
            &format!(
                "- ☑️ [#subagent:review#1] confusion about markers [#q-0123456789ab] [#{REQ}]\n"
            ),
            true,
            true,
        );
        let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            assessment.quiescent[0].reason,
            QuiescentReason::OperatorOwned
        );
    }

    /// A genuine reported leaf (no operator request, no kept response tree)
    /// still owes integration.
    #[test]
    fn orphanreportedrow_genuine_reported_leaf_still_owes_integration() {
        let content = "\
<!-- agent:run id=\"admin\" subagents -->
- ### ☑️ [#subagent:fixit#2] Fix the flaky test [#q-feedfacecafe]
<!-- /agent:run -->
";
        let assessment = assess_transient_closeout(content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(assessment.debts[0].owed, OWED_REPORTED);
        assert!(
            assessment.debts[0]
                .owed
                .starts_with("integrate the report or otherwise leave the reported-but-open state")
        );
    }

    /// `#waitingheadingpending`: a marker on the Waiting-on-you heading owes
    /// nothing. The 2026-11-14 handoff painted `📥` there (agent-doc-bugs.md)
    /// and the 2026-11-13 one left it until a hand patch (`tasks/fpe.md`).
    /// Whatever marker it carries, with or without items under it, the
    /// heading never refuses closeout, while a real pending row beside it
    /// still does.
    #[test]
    fn waiting_on_you_heading_marker_never_refuses_closeout() {
        let heading = "Waiting on you \u{2014} answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->";
        for marker in ["📥", "🟡", "🚧", "☑️", "⚠️", "🔄", "❔"] {
            for children in [
                "",
                "  - 🙋 [#vscodecodersmoke] Run the smoke. <!-- agent-doc:waiting-item -->\n",
            ] {
                let content = format!(
                    "<!-- agent:run id=\"main\" subagents -->\n- ### {marker} {heading}\n{children}<!-- /agent:run -->\n"
                );
                let assessment =
                    assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
                assert!(
                    assessment.debts.is_empty() && assessment.quiescent.is_empty(),
                    "{marker} heading must not hold the turn: {assessment:?}"
                );
            }
        }
        // The short heading an older binary rendered, and a legacy stamped
        // `[#q-…]` id, are the same container.
        for row in [
            "- ### 📥 Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "- 📥 Waiting on you \u{2014} answer by adding a child line under an item [#q-0123456789ab] <!-- agent-doc:waiting-on-you -->\n",
        ] {
            assert!(
                normal_closeout_failure(&doc(row)).unwrap().is_none(),
                "{row}"
            );
        }
        let content = format!(
            "<!-- agent:run id=\"main\" subagents -->\n- ### 📥 {heading}\n- 📥 fix the flaky test\n<!-- /agent:run -->\n"
        );
        let debts = live_transient_debts(&content).unwrap();
        assert_eq!(debts.len(), 1, "{debts:?}");
        assert_eq!(debts[0].row, "📥 fix the flaky test");
    }

    // ---- `#integrationinflight` --------------------------------------------

    const INFLIGHT_ROW: &str = "☑️ [#subagent:inflight#1] do [#integrationinflight]";

    fn in_flight(
        content: &str,
        generation: u64,
        status: IntegrationInFlightStatus,
    ) -> CloseoutEvidence {
        CloseoutEvidence {
            integrations_in_flight: HashMap::from([(
                identity_of(content, "do [#integrationinflight]"),
                IntegrationInFlight {
                    generation,
                    source_sha: "a".repeat(40),
                    integration_sha: "b".repeat(40),
                    verification: "make check".to_string(),
                    expires_at_secs: 4_600,
                    status,
                },
            )]),
            ..CloseoutEvidence::default()
        }
    }

    /// 2026-11-15, `tasks/agent-doc/agent-doc-bugs.md`: the coordinator had
    /// merged `#resumemaintrace`, `#gatedheadpending`, and
    /// `#installreattachambiguous` in its integration worktree and `make
    /// check` was still running, yet every Stop refused the `☑️` row. A live
    /// in-flight record of the row's own generation is a wait, not a debt.
    #[test]
    fn integrationinflight_a_live_record_holds_the_reported_row_quiescent() {
        let content = doc(&format!("- {INFLIGHT_ROW}\n"));
        let assessment = assess_transient_closeout(
            &content,
            &in_flight(&content, 1, IntegrationInFlightStatus::Live),
        )
        .unwrap();
        assert!(assessment.debts.is_empty(), "{assessment:?}");
        assert_eq!(
            reasons(&assessment),
            vec![(
                INFLIGHT_ROW.to_string(),
                QuiescentReason::WaitingOnIntegration
            )]
        );
        assert_eq!(assessment.quiescent_state(), Some("waiting_on_integration"));
        assert!(
            assessment
                .quiescent_log_fields()
                .ends_with("waiting_on_integration=1"),
            "{}",
            assessment.quiescent_log_fields()
        );
        // The row stays live: nothing strikes it, and a turn with other
        // agent-actionable rows still refuses over them alone.
        let mixed = doc(&format!("- {INFLIGHT_ROW}\n- 📥 drainable work\n"));
        let assessment = assess_transient_closeout(
            &mixed,
            &in_flight(&mixed, 1, IntegrationInFlightStatus::Live),
        )
        .unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert_eq!(assessment.debts[0].row, "📥 drainable work");
    }

    #[test]
    fn integrationinflight_an_expired_or_stale_record_refuses_again() {
        let content = doc(&format!("- {INFLIGHT_ROW}\n"));
        let expired = normal_closeout_failure_with(
            &content,
            &in_flight(&content, 1, IntegrationInFlightStatus::Expired),
        )
        .unwrap()
        .expect("an expired in-flight record refuses");
        assert!(expired.contains(INFLIGHT_ROW), "{expired}");
        assert!(expired.contains("passed its deadline"), "{expired}");

        let mismatched = normal_closeout_failure_with(
            &content,
            &in_flight(&content, 1, IntegrationInFlightStatus::SourceMismatch),
        )
        .unwrap()
        .expect("a record for another source refuses");
        assert!(mismatched.contains("no longer matches it"), "{mismatched}");

        // The visible row names generation 2: a record of generation 1 is a
        // different attempt and never quiets the gate, even while live.
        let regenerated = doc("- ☑️ [#subagent:inflight#2] do [#integrationinflight]\n");
        let assessment = assess_transient_closeout(
            &regenerated,
            &in_flight(&regenerated, 1, IntegrationInFlightStatus::Live),
        )
        .unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert!(
            assessment.debts[0].owed.contains("no longer matches it"),
            "{assessment:?}"
        );

        // A record for another row does not cover this one.
        let other = doc("- ☑️ [#subagent:other#1] do [#otherrow]\n");
        let evidence = in_flight(&content, 1, IntegrationInFlightStatus::Live);
        let assessment = assess_transient_closeout(&other, &evidence).unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
    }

    /// The reported-row refusal tells the coordinator how to record an
    /// integration in flight, so it does not take a refusal every turn while
    /// verification runs.
    #[test]
    fn integrationinflight_the_reported_refusal_names_the_integrating_receipt() {
        let content = doc(&format!("- {INFLIGHT_ROW}\n"));
        let failure = normal_closeout_failure(&content)
            .unwrap()
            .expect("a reported row with no record refuses");
        assert!(
            failure.contains("integrate the report or otherwise leave the reported-but-open state"),
            "{failure}"
        );
        assert!(
            failure.contains(
                "agent-doc queue lineage-record <FILE> --item <row> --generation <n> --event integrating --source-sha <worker-source-sha> --integration-sha <candidate-merge-sha> --verification '<command>'"
            ),
            "{failure}"
        );
        assert!(
            failure.contains("`--event integrated` supersedes it"),
            "{failure}"
        );
    }

    /// A worker that ended with `hand_back` recorded by the subagent-end
    /// freeze, its attempt still `Running`, generation `generation`.
    fn finished_worker_claim(
        generation: u64,
        hand_back: Option<&str>,
    ) -> crate::queue_claim::QueueClaim {
        let mut claim = worker_claim(
            crate::queue_claim::QueueClaimState::Running,
            1_000,
            Some(1_000),
            Some(1_100),
        );
        claim.generation = generation;
        claim.frozen = Some(crate::queue_claim::QueueWorkerFreeze {
            frozen_at_secs: 1_200,
            reason: crate::queue_claim::subagent_end_freeze_reason(hand_back),
            abandoned: false,
        });
        claim
    }

    /// `#finishedheadrefill`, at the Stop gate: the subagent-end
    /// hook froze a worker that FINISHED before the coordinator ran `queue
    /// report-complete`, so its `🚧` row's `Running` attempt is report-owed.
    /// The Stop hook said "no bound running worker claim backs this row:
    /// refresh the heartbeat", which no refresh can satisfy. It now names the
    /// exact `queue report-complete` for that generation, as it already did
    /// for a `⚠️🚧` row with a recorded hand-back.
    #[test]
    fn finishedheadrefill_a_finished_running_worker_owes_the_report_not_a_refresh() {
        let content = doc("- 🚧 [#subagent:work#3] do [#work]\n");
        let finished = finished_worker_claim(3, Some("**Summary** fixed it."));
        let owed = ReportOwedAttempt::of(&finished).expect("a finished worker is report-owed");
        assert_eq!(
            owed,
            ReportOwedAttempt {
                item: "#work".to_string(),
                generation: 3,
                owner: "subagent:work".to_string(),
                worker_id: "worker:work".to_string(),
                interim: false,
            }
        );
        let evidence = CloseoutEvidence {
            report_owed: HashMap::from([(identity_of(&content, "#work"), owed)]),
            document: Some("tasks/session.md".to_string()),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert!(assessment.quiescent.is_empty(), "{assessment:?}");
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        let failure = normal_closeout_failure_for(&assessment).unwrap();
        assert!(
            failure.contains("report the hand-back its ended worker left"),
            "{failure}"
        );
        assert!(
            failure.contains(
                "agent-doc queue report-complete tasks/session.md --item '#work' --generation 3 --response '<complete worker hand-back>'"
            ),
            "{failure}"
        );
        assert!(
            !failure.contains("no bound running worker claim backs this row"),
            "{failure}"
        );
        assert!(
            !failure.contains("agent-doc queue claim --refresh"),
            "{failure}"
        );

        // A `🟡` row whose attempt is report-owed owes the same report, not
        // "start the claimed worker".
        let claimed = doc("- 🟡 [#subagent:work#3] do [#work]\n");
        let failure = normal_closeout_failure_with(&claimed, &evidence)
            .unwrap()
            .expect("a report-owed row refuses");
        assert!(
            failure.contains("queue report-complete tasks/session.md"),
            "{failure}"
        );
        assert!(!failure.contains("no recent dispatch claim"), "{failure}");

        // Without the report-owed evidence the row is the generic stale worker.
        let failure = normal_closeout_failure(&content)
            .unwrap()
            .expect("a stale running worker refuses");
        assert!(
            failure.contains("no bound running worker claim backs this row"),
            "{failure}"
        );
    }

    /// `#lostrecoverhold` + `#interimhandback` (haiven-dev
    /// `subagent:adv929#1`, 2026-11-23): the subagent-end hook froze a worker
    /// whose last message was "... I'm waiting for that." Its attempt stayed
    /// `Running`, so the row painted plain `⚠️`. The Stop gate gave the generic
    /// lost text with no worker id or command, and plain `⚠️` let a recursive
    /// Stop end the turn with that resumable work stranded. It now names the
    /// exact continue of that worker (and the restart path), and the row holds
    /// the turn. A finished hand-back on the same `⚠️` row names its report
    /// and holds the turn too.
    #[test]
    fn lostrecoverhold_a_lost_row_with_an_interim_end_owes_a_continue_and_holds_the_turn() {
        let content = doc("- ⚠️ [#subagent:work#1] do [#work]\n");
        let waiting = finished_worker_claim(
            1,
            Some(
                "The fixer confirmed it is re-running the three sqlc steps directly. After they land it will finish the fix and return its full result. I'm waiting for that.",
            ),
        );
        let owed = ReportOwedAttempt::of(&waiting).expect("an ended worker is report-owed");
        assert!(owed.interim, "{owed:?}");
        let evidence = CloseoutEvidence {
            report_owed: HashMap::from([(identity_of(&content, "#work"), owed)]),
            document: Some("tasks/session.md".to_string()),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert!(
            assessment.debts[0].holds_turn_through_recursive_stop(),
            "a recoverable ⚠️ row must not let a recursive Stop end the turn: {assessment:?}"
        );
        let failure = normal_closeout_failure_for(&assessment).unwrap();
        assert!(
            failure.contains("`SendMessage` to `worker:work`"),
            "{failure}"
        );
        assert!(
            failure.contains(
                "agent-doc queue claim tasks/session.md --item '#work' --owner subagent:work --refresh --generation 1"
            ),
            "{failure}"
        );
        assert!(
            failure.contains(
                "agent-doc queue release tasks/session.md --item '#work' --generation 1 --requeue"
            ),
            "{failure}"
        );
        assert!(failure.contains("--generation 1 --verbose"), "{failure}");
        assert!(
            !failure.contains("Close it now with `agent-doc queue report-complete"),
            "an interim note is not reported: {failure}"
        );

        // The same `⚠️` row with a finished hand-back owes its exact report.
        let finished = finished_worker_claim(1, Some("**Summary** fixed it."));
        let evidence = CloseoutEvidence {
            report_owed: HashMap::from([(
                identity_of(&content, "#work"),
                ReportOwedAttempt::of(&finished).unwrap(),
            )]),
            document: Some("tasks/session.md".to_string()),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(&content, &evidence).unwrap();
        assert_eq!(assessment.debts.len(), 1, "{assessment:?}");
        assert!(assessment.debts[0].holds_turn_through_recursive_stop());
        let failure = normal_closeout_failure_for(&assessment).unwrap();
        assert!(
            failure.contains(
                "agent-doc queue report-complete tasks/session.md --item '#work' --generation 1"
            ),
            "{failure}"
        );
        assert!(!failure.contains("SendMessage"), "{failure}");

        // Without that evidence a plain `⚠️` still may release.
        let bare = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
        assert_eq!(bare.debts.len(), 1, "{bare:?}");
        assert!(!bare.debts[0].holds_turn_through_recursive_stop());
    }

    /// Only a finished, unreported attempt is report-owed: an end with no
    /// captured hand-back, an abandon, a live worker, and a reported attempt
    /// are not.
    #[test]
    fn finishedheadrefill_only_a_recorded_unreported_hand_back_is_report_owed() {
        use crate::queue_claim::QueueClaimState;
        assert!(ReportOwedAttempt::of(&finished_worker_claim(1, None)).is_none());
        let live = worker_claim(QueueClaimState::Running, 1_000, Some(1_000), Some(1_100));
        assert!(ReportOwedAttempt::of(&live).is_none());
        let mut abandoned = finished_worker_claim(1, Some("done"));
        abandoned.frozen.as_mut().unwrap().abandoned = true;
        assert!(ReportOwedAttempt::of(&abandoned).is_none());
        let mut reported = finished_worker_claim(1, Some("done"));
        reported.state = QueueClaimState::Reported;
        assert!(ReportOwedAttempt::of(&reported).is_none());
        let mut lost = finished_worker_claim(2, Some("done"));
        lost.mark_lost();
        assert_eq!(
            ReportOwedAttempt::of(&lost).map(|owed| owed.generation),
            Some(2)
        );
    }

    /// The `⚠️🚧` refusal with a recorded hand-back names the same exact
    /// command once the evidence carries the attempt.
    #[test]
    fn finishedheadrefill_the_lost_recorded_hand_back_names_the_report_command() {
        let content = doc("- ⚠️🚧 [#subagent:work#2] do [#work]\n");
        let mut lost = finished_worker_claim(2, Some("done"));
        lost.mark_lost();
        let identity = identity_of(&content, "#work");
        let evidence = CloseoutEvidence {
            reportable_lost: HashSet::from([identity.clone()]),
            report_owed: HashMap::from([(identity, ReportOwedAttempt::of(&lost).unwrap())]),
            ..CloseoutEvidence::default()
        };
        let failure = normal_closeout_failure_with(&content, &evidence)
            .unwrap()
            .expect("the lost row owes its report");
        assert!(
            failure.contains("report the hand-back its ended worker left"),
            "{failure}"
        );
        assert!(
            failure
                .contains("agent-doc queue report-complete '<FILE>' --item '#work' --generation 2"),
            "{failure}"
        );
    }

    /// `#dispatchheldstop` (haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-23):
    /// `do [#pushreadmechanges]` appeared twice, so its claim admission was
    /// refused (`claim_projection_unrepresentable`) and dispatch held it for
    /// three hours. Every Stop said only "leave pending before the turn ends".
    /// The refusal now names the blocker, its fix, and the exact re-admission,
    /// and the held row alone does not keep refusing a recursive Stop.
    #[test]
    fn a_dispatch_held_pending_row_names_its_blocker_and_does_not_hold_a_recursive_stop() {
        let content = "<!-- agent:queue -->\n- 📥 do [#pushreadmechanges]\n- 📥 do [#drainablehead]\n<!-- /agent:queue -->\n";
        let evidence = CloseoutEvidence {
            document: Some("/tmp/sdk-dev.md".to_string()),
            dispatch_held: [(
                identity_of(content, "pushreadmechanges"),
                "claim_projection_unrepresentable".to_string(),
            )]
            .into(),
            ..CloseoutEvidence::default()
        };
        let assessment = assess_transient_closeout(content, &evidence).unwrap();
        assert_eq!(assessment.debts.len(), 2, "{assessment:#?}");
        let held = &assessment.debts[0];
        assert_eq!(held.row, "📥 do [#pushreadmechanges]");
        let rendered = held.render();
        assert!(rendered.contains(OWED_DISPATCH_HELD), "{rendered}");
        assert!(
            rendered.contains("blocker `claim_projection_unrepresentable`"),
            "{rendered}"
        );
        assert!(rendered.contains("remove the extra copy"), "{rendered}");
        assert!(
            rendered.contains(
                "`agent-doc queue claim /tmp/sdk-dev.md --item '#pushreadmechanges' --owner 'subagent:<label>'`"
            ),
            "{rendered}"
        );
        assert!(!held.holds_turn_through_recursive_stop(), "{held:#?}");

        // The drainable head beside it is unchanged: it still holds the turn.
        let drainable = &assessment.debts[1];
        assert!(drainable.dispatch_held.is_none(), "{drainable:#?}");
        assert!(
            drainable
                .render()
                .contains("leave pending before the turn ends")
        );
        assert!(drainable.holds_turn_through_recursive_stop());

        // With no hold recorded, the same row owes the ordinary pending text.
        let plain = assess_transient_closeout(content, &CloseoutEvidence::default()).unwrap();
        assert!(plain.debts.iter().all(|debt| debt.dispatch_held.is_none()));
        assert!(plain.debts[0].holds_turn_through_recursive_stop());
    }
}
