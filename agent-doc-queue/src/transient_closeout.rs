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
//! - **Waiting on a claim.** A `🟡` row whose dispatch claim is still inside
//!   its acknowledgement grace, or a `🚧` row whose bound worker's heartbeat
//!   is fresh, is a worker the harness reports back on asynchronously
//!   (`state=waiting_on_claims`). The visible marker must agree with the
//!   durable phase: a `🚧` backed only by an unbound dispatch is not a running
//!   worker. A stale `🚧`, an unacknowledged `🟡`, or a row with no backing
//!   claim still refuses. The `🙋` part of `🙋🚧` does not exempt its worker
//!   part from this rule.
//! - **Not due.** A `📥` row that a closed `agent:loop` schedule window holds
//!   back is not due until the next tick.
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
}

/// Why a live transient row does not hold the turn open (`#stopquiescentrows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuiescentReason {
    /// The row carries `🙋` with its request packet and open Todo recorded.
    OperatorOwned,
    /// A heartbeat-fresh claim backs the row's `🟡`/`🚧` worker.
    WaitingOnClaim,
    /// A closed schedule window holds the row back until its next tick.
    ScheduleNotDue,
}

impl QuiescentReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OperatorOwned => "operator_owned",
            Self::WaitingOnClaim => "waiting_on_claim",
            Self::ScheduleNotDue => "schedule_not_due",
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

/// The durable phase of a heartbeat-fresh claim (`#stopquiescentrows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreshClaim {
    /// An unbound dispatch still inside its acknowledgement grace (`🟡`).
    Dispatching,
    /// A bound worker whose heartbeat is inside the frozen bound (`🚧`).
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

/// Durable-state facts the text-only gate cannot read (`#stopquiescentrows`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseoutEvidence {
    /// Work identities whose live claim is heartbeat-fresh, with its phase.
    pub fresh_claims: HashMap<QueueItemIdentity, FreshClaim>,
    /// Head texts that a closed schedule window holds back.
    pub schedule_held_heads: HashSet<String>,
    /// Work identities whose `Lost` attempt holds an ended worker's recorded,
    /// unreported hand-back (`#lostrowreportreopen`,
    /// [`crate::queue_claim::QueueClaim::lost_end_hand_back`]). The row owes a
    /// report of that hand-back, not a continue or a new generation.
    pub reportable_lost: HashSet<QueueItemIdentity>,
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
        } else if self.quiescent_count(QuiescentReason::OperatorOwned) > 0 {
            Some("waiting_on_operator")
        } else if self.quiescent_count(QuiescentReason::ScheduleNotDue) > 0 {
            Some("schedule_not_due")
        } else {
            None
        }
    }

    /// One ops-log field list naming each quiescent count.
    pub fn quiescent_log_fields(&self) -> String {
        format!(
            "waiting_on_claims={} operator_owned={} schedule_not_due={}",
            self.quiescent_count(QuiescentReason::WaitingOnClaim),
            self.quiescent_count(QuiescentReason::OperatorOwned),
            self.quiescent_count(QuiescentReason::ScheduleNotDue),
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
        QueueLifecycleMarker::Reported => {
            "integrate the report or otherwise leave the reported-but-open state"
        }
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
            "no heartbeat-fresh worker claim backs this row: refresh the heartbeat (`agent-doc queue claim --refresh`) if the worker is still running, or finish the running worker without converting the claim to Lost; a worker that is still running means the turn is not over; a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        }
    }
}

/// `#lostrowreportreopen`: what a `⚠️☑️` row owes.
const OWED_LOST_REPORTED: &str = "integrate the recovered report: the worker's result is already reported across a lost attempt, so do not continue the worker or retry a new generation; integrate it and strike the row (or `agent-doc queue consume --ack-text <row> --ack-owner <owner#generation>`)";

/// `#lostrowreportreopen`: what a `⚠️🚧` row owes when its ended worker's
/// hand-back is recorded and unreported.
const OWED_LOST_RECORDED_HANDBACK: &str = "report the hand-back its ended worker left: run `agent-doc queue report-complete` for this exact generation with that hand-back (the freeze recorded its opening); do not continue the ended worker or retry a new generation for finished work";

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

/// The fresh phase of `claim` at `now_secs`, if it backs a quiescent worker
/// wait: an unbound dispatch still inside its acknowledgement grace, or a
/// bound worker whose heartbeat is inside the frozen bound and whose claim
/// has not lapsed. `None` for a frozen, lapsed, reported, lost, or
/// quota-blocked attempt.
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
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let entries = parse_queue_body(component.content(content))?;
        for (index, entry) in entries.iter().enumerate() {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
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
            };
            if operator == Some(false) {
                assessment.debts.push(debt(OWED_OPERATOR_RECORD));
                continue;
            }
            match marker {
                QueueLifecycleMarker::Claimed | QueueLifecycleMarker::Running => {
                    let identity = crate::queue_claim::claim_identity_at(&entries, index)
                        .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text));
                    if evidence
                        .fresh_claims
                        .get(&identity)
                        .is_some_and(|fresh| fresh.backs(marker))
                    {
                        assessment
                            .quiescent
                            .push(quiescent(QuiescentReason::WaitingOnClaim));
                    } else {
                        assessment
                            .debts
                            .push(debt(owed_stale_worker_transition(marker)));
                    }
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
                    assessment.debts.push(debt(OWED_LOST_RECORDED_HANDBACK));
                }
                QueueLifecycleMarker::Pending | QueueLifecycleMarker::Reported
                    if operator == Some(true) =>
                {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::OperatorOwned));
                }
                QueueLifecycleMarker::Pending
                    if evidence.schedule_held_heads.contains(&prompt.text)
                        || evidence.schedule_held_heads.contains(&row) =>
                {
                    assessment
                        .quiescent
                        .push(quiescent(QuiescentReason::ScheduleNotDue));
                }
                _ => assessment.debts.push(debt(owed)),
            }
        }
    }
    Ok(assessment)
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
        message.push_str("\n- row ");
        message.push_str(&debt.row);
        message.push_str(": ");
        message.push_str(debt.owed);
    }
    Some(message)
}

const NORMAL_CLOSEOUT_HEADER: &str =
    "normal closeout refused while live queue rows are still transient (#transientcloseout)";
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
        assert!(failure.contains("finish the running worker without converting the claim to Lost"));
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
        assert!(failure.contains("finish the running worker without converting the claim to Lost"));
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
        assert!(stale.contains("no heartbeat-fresh worker claim"), "{stale}");

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

    /// agent-doc-bugs.md, 2026-11-13: the Stop hook refused six stops over
    /// two running workers with refreshed heartbeats. A fresh `🚧` and a
    /// `🟡` inside its dispatch grace wait on claims; the same rows with no
    /// fresh claim still refuse.
    #[test]
    fn fresh_worker_claims_are_quiescent_and_stale_ones_refuse() {
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
            "waiting_on_claims=3 operator_owned=0 schedule_not_due=0"
        );

        // One worker went stale: only that row refuses.
        let partly = fresh(&content, &["admissionleasechurn", "justdispatched"]);
        let failure = normal_closeout_failure_with(&content, &partly)
            .unwrap()
            .expect("a stale running worker refuses");
        assert!(failure.contains("storeschemabatch"), "{failure}");
        assert!(!failure.contains("admissionleasechurn"), "{failure}");
        assert!(
            failure.contains("agent-doc queue claim --refresh"),
            "{failure}"
        );
        assert!(failure.contains("finish the running worker without converting the claim to Lost"));

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
        assert!(
            fresh_claim_phase(&running, 1_500 + FROZEN, FROZEN, GRACE).is_none(),
            "a heartbeat past the frozen bound is stale"
        );
        assert!(
            fresh_claim_phase(&running, 1_000 + 7_200, 100_000, GRACE).is_none(),
            "a lapsed claim is not fresh"
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
}
