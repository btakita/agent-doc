//! GH #272 (`#runtypingadmit`): hold preflight admission for an editor replica
//! that is about to come back, instead of refusing `Run Agent Doc` outright.
//!
//! Operator-reported 2026-10-15: `Run Agent Doc` was pressed while the operator
//! was still typing a queue line, 54s after a controller self-recycle. The
//! document's replica rebuild had not landed yet, so preflight's initial read
//! met the attached-editor refusal (`missing_replica` recovery exhausted, disk
//! refused) and the turn ended "did not run". The replica returned unaided six
//! seconds later. Nothing retried the Run.
//!
//! The refusal itself is right: an attached editor never descends to disk. What
//! was wrong is treating a self-healing state as terminal. This module holds
//! admission for a bounded window when the refusal's recovery class is one the
//! editor can leave on its own, observing the authority only, and then resolves
//! the document again through the normal precedence. The hold never reads disk
//! and never supplies content: a replica that returns is read as the editor's
//! coherent current text (operator keystrokes included), so the turn cannot be
//! admitted on a stale or partial base. Edits that keep arriving after that read
//! are the existing typing gate's job (`#qheadcomposing`) and then mid-turn
//! steering's.

use std::path::Path;
use std::time::{Duration, Instant};

use agent_doc_turn::authority_recovery::{
    AdmissionReplicaHold, AdmissionReplicaHoldStep, AttachedEditorRecoveryDecision,
    AuthorityObservation, admission_replica_hold_step, decide_admission_replica_hold,
};

/// Upper bound on one admission hold. The incident's replica returned 6s after
/// the refusal; a rebuild after a controller recycle is normally well inside
/// this. Always clamped to the preflight admission deadline.
pub const ADMISSION_REPLICA_HOLD_MAX: Duration = Duration::from_secs(20);

/// How often the hold asks the attached editor to re-register its replica.
/// The refusal already spent its own re-registration attempts; a slower nudge
/// covers a request that was lost while the editor was busy.
pub const ADMISSION_REPLICA_HOLD_NUDGE_EVERY: Duration = Duration::from_secs(5);

/// One observation-only read the hold takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldObservation {
    pub observation: AuthorityObservation,
    /// The observed transition token, for the ops log and the named cause.
    pub state: &'static str,
}

/// What one hold saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaHoldReport {
    pub waited: Duration,
    pub budget: Duration,
    pub final_state: &'static str,
    pub observations: u32,
    pub reregister_nudges: u32,
}

/// How a hold ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaHoldOutcome {
    /// The authority answers again; resolve the document once more.
    Returned(ReplicaHoldReport),
    /// The budget is spent and the replica is still unavailable.
    Expired(ReplicaHoldReport),
}

/// A preflight document read that may have been admitted after a hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedDocument {
    pub content: String,
    /// `Some` when admission held for the replica before this read succeeded.
    pub replica_hold: Option<ReplicaHoldReport>,
}

/// The bounded hold loop, with every effect injected (SimWorld drives it with
/// a simulated replica timeline and clock).
pub fn hold_for_replica_with_effects<Observe, Nudge, Sleep, Now>(
    budget: Duration,
    poll: Duration,
    nudge_every: Duration,
    mut observe: Observe,
    mut nudge: Nudge,
    mut sleep: Sleep,
    mut now: Now,
) -> ReplicaHoldOutcome
where
    Observe: FnMut() -> HoldObservation,
    Nudge: FnMut(),
    Sleep: FnMut(Duration),
    Now: FnMut() -> Instant,
{
    let start = now();
    let mut last_nudge = start;
    let mut observations = 0u32;
    let mut reregister_nudges = 0u32;
    loop {
        let observed = observe();
        observations += 1;
        let at = now();
        let held = at.saturating_duration_since(start);
        let report = |reregister_nudges| ReplicaHoldReport {
            waited: held,
            budget,
            final_state: observed.state,
            observations,
            reregister_nudges,
        };
        match admission_replica_hold_step(observed.observation, held, budget) {
            AdmissionReplicaHoldStep::Reresolve => {
                return ReplicaHoldOutcome::Returned(report(reregister_nudges));
            }
            AdmissionReplicaHoldStep::Expired => {
                return ReplicaHoldOutcome::Expired(report(reregister_nudges));
            }
            AdmissionReplicaHoldStep::Wait => {}
        }
        if at.saturating_duration_since(last_nudge) >= nudge_every {
            nudge();
            reregister_nudges += 1;
            last_nudge = at;
        }
        // Never sleep past the budget: the last observation lands at it.
        sleep(poll.min(budget.saturating_sub(held)).max(Duration::from_millis(1)));
    }
}

/// Map the preflight authority observation onto the shared vocabulary.
fn hold_observation(file: &Path) -> HoldObservation {
    let observed = crate::debounce::observe_lazily_current(file, "preflight_admission_replica_hold");
    let observation = match observed.state {
        "detached" => AuthorityObservation::Detached,
        // A pending delivery already carries the controller's canonical cut
        // (`#preflightsteeradmit`); the replica is answering.
        "lazily_current" | "delivery_pending" => AuthorityObservation::Current,
        "missing_replica" => AuthorityObservation::MissingReplica,
        "current_pending" => AuthorityObservation::SyncPending,
        _ => AuthorityObservation::Error,
    };
    HoldObservation {
        observation,
        state: observed.state,
    }
}

/// The production hold for `file`: observation-only reads through the project
/// controller, best-effort re-registration nudges, clamped to the admission
/// deadline.
pub fn hold_for_replica(file: &Path) -> ReplicaHoldOutcome {
    hold_for_replica_with_effects(
        agent_doc_debounce::admission_deadline::clamp(ADMISSION_REPLICA_HOLD_MAX),
        agent_doc_debounce::SETTLE_POLL_INTERVAL,
        ADMISSION_REPLICA_HOLD_NUDGE_EVERY,
        || hold_observation(file),
        || {
            let status = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
                file,
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::EditorReplicaReregister,
                0,
            ) {
                Ok(()) => "requested".to_string(),
                Err(error) => format!("failed:{error:#}").replace('\n', " "),
            };
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_admission_replica_hold_reregister file={} reregister={} (#runtypingadmit)",
                    file.display(),
                    status,
                ),
            );
        },
        |wait| std::thread::sleep(agent_doc_debounce::admission_deadline::clamp(wait)),
        Instant::now,
    )
}

/// Resolve preflight's document, holding admission for a self-healing replica.
///
/// `resolve` is the normal precedence read (editor buffer, then disk only for
/// a proven-detached editor). `classify` names the attached-editor refusal's
/// recovery class, or `None` for any other failure, which passes through
/// unchanged. `hold` runs at most once.
pub fn resolve_admitting_transient_replica<Resolve, Classify, Hold>(
    file: &Path,
    source: &str,
    mut resolve: Resolve,
    classify: Classify,
    hold: Hold,
) -> anyhow::Result<AdmittedDocument>
where
    Resolve: FnMut() -> anyhow::Result<String>,
    Classify: Fn(&anyhow::Error) -> Option<AttachedEditorRecoveryDecision>,
    Hold: FnOnce() -> ReplicaHoldOutcome,
{
    let refusal = match resolve() {
        Ok(content) => {
            return Ok(AdmittedDocument {
                content,
                replica_hold: None,
            });
        }
        Err(error) => error,
    };
    let Some(recovery) = classify(&refusal) else {
        return Err(refusal);
    };
    if decide_admission_replica_hold(recovery) == AdmissionReplicaHold::RefuseNow {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "preflight_admission_replica_hold_skipped file={} source={} recovery={:?} reason=needs_operator_action (#runtypingadmit)",
                file.display(),
                source,
                recovery,
            ),
        );
        return Err(refusal);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "preflight_admission_replica_hold_started file={} source={} recovery={:?} (#runtypingadmit)",
            file.display(),
            source,
            recovery,
        ),
    );
    match hold() {
        ReplicaHoldOutcome::Returned(report) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_admission_replica_hold_returned file={} source={} waited_ms={} state={} observations={} reregister_nudges={} (#runtypingadmit)",
                    file.display(),
                    source,
                    report.waited.as_millis(),
                    report.final_state,
                    report.observations,
                    report.reregister_nudges,
                ),
            );
            match resolve() {
                Ok(content) => Ok(AdmittedDocument {
                    content,
                    replica_hold: Some(report),
                }),
                Err(error) => {
                    let cause = format!(
                        "preflight admission held {}ms until the editor replica for {} answered \
                         again ({}), but resolving the document a second time still failed, so \
                         this turn did not run (GH #272)",
                        report.waited.as_millis(),
                        file.display(),
                        report.final_state,
                    );
                    Err(error.context(cause))
                }
            }
        }
        ReplicaHoldOutcome::Expired(report) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_admission_replica_hold_expired file={} source={} waited_ms={} budget_ms={} state={} observations={} reregister_nudges={} (#runtypingadmit)",
                    file.display(),
                    source,
                    report.waited.as_millis(),
                    report.budget.as_millis(),
                    report.final_state,
                    report.observations,
                    report.reregister_nudges,
                ),
            );
            Err(refusal.context(replica_hold_expired_cause(file, &report)))
        }
    }
}

/// The named cause for a refusal that outlived its hold (GH #272 item 3).
pub fn replica_hold_expired_cause(file: &Path, report: &ReplicaHoldReport) -> String {
    format!(
        "replica_hold_expired: preflight admission held {}ms for the editor replica of {} and it \
         stayed `{}` the whole time, so this turn did not run. The editor still holds the document \
         and any text typed meanwhile stays in its buffer; the replica was not reported gone for \
         good, so a replica rebuild after a controller recycle or a plugin re-registration may \
         still be in flight. Retry the same command once the editor has settled; follow the remedy \
         below only if that retry is refused again (GH #272)",
        report.waited.as_millis(),
        file.display(),
        report.final_state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Debug)]
    struct Refused(AttachedEditorRecoveryDecision);

    impl std::fmt::Display for Refused {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "editor is still attached; missing_replica recovery exhausted")
        }
    }

    impl std::error::Error for Refused {}

    fn classify(error: &anyhow::Error) -> Option<AttachedEditorRecoveryDecision> {
        error
            .chain()
            .find_map(|cause| cause.downcast_ref::<Refused>())
            .map(|refused| refused.0)
    }

    fn file() -> &'static Path {
        Path::new("/nonexistent-agent-doc-root/gh272.md")
    }

    /// A simulated clock: `sleep` advances it, so the loop is deterministic.
    struct Clock {
        origin: Instant,
        offset: Cell<Duration>,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                origin: Instant::now(),
                offset: Cell::new(Duration::ZERO),
            }
        }
        fn now(&self) -> Instant {
            self.origin + self.offset.get()
        }
        fn elapsed(&self) -> Duration {
            self.offset.get()
        }
        fn sleep(&self, wait: Duration) {
            self.offset.set(self.offset.get() + wait);
        }
    }

    fn run_hold(clock: &Clock, returns_after: Option<Duration>) -> (ReplicaHoldOutcome, u32) {
        let nudges = Cell::new(0u32);
        let outcome = hold_for_replica_with_effects(
            Duration::from_secs(20),
            Duration::from_millis(100),
            Duration::from_secs(5),
            || match returns_after {
                Some(at) if clock.elapsed() >= at => HoldObservation {
                    observation: AuthorityObservation::Current,
                    state: "lazily_current",
                },
                _ => HoldObservation {
                    observation: AuthorityObservation::MissingReplica,
                    state: "missing_replica",
                },
            },
            || nudges.set(nudges.get() + 1),
            |wait| clock.sleep(wait),
            || clock.now(),
        );
        (outcome, nudges.get())
    }

    #[test]
    fn hold_returns_when_the_replica_comes_back_inside_the_budget() {
        let clock = Clock::new();
        let (outcome, nudges) = run_hold(&clock, Some(Duration::from_secs(6)));
        let ReplicaHoldOutcome::Returned(report) = outcome else {
            panic!("replica returned at 6s inside a 20s hold: {outcome:?}");
        };
        assert_eq!(report.final_state, "lazily_current");
        assert!(report.waited >= Duration::from_secs(6));
        assert!(report.waited < Duration::from_secs(7));
        assert_eq!(nudges, 1, "one re-registration nudge at 5s");
    }

    #[test]
    fn hold_expires_at_the_budget_and_never_overshoots_it() {
        let clock = Clock::new();
        let (outcome, nudges) = run_hold(&clock, None);
        let ReplicaHoldOutcome::Expired(report) = outcome else {
            panic!("a replica that never returns must expire: {outcome:?}");
        };
        assert_eq!(report.final_state, "missing_replica");
        assert_eq!(report.waited, Duration::from_secs(20));
        assert_eq!(clock.elapsed(), Duration::from_secs(20));
        assert_eq!(nudges, 3, "nudges at 5s, 10s, 15s");
    }

    #[test]
    fn a_spent_admission_budget_expires_without_waiting() {
        let clock = Clock::new();
        let outcome = hold_for_replica_with_effects(
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_secs(5),
            || HoldObservation {
                observation: AuthorityObservation::MissingReplica,
                state: "missing_replica",
            },
            || panic!("no nudge with a spent budget"),
            |wait| clock.sleep(wait),
            || clock.now(),
        );
        assert!(matches!(outcome, ReplicaHoldOutcome::Expired(_)));
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    #[test]
    fn self_healing_refusal_is_held_then_admitted_on_the_reresolved_editor_text() {
        let reads = RefCell::new(vec![
            Err(anyhow::Error::new(Refused(
                AttachedEditorRecoveryDecision::ReloadEditor,
            ))),
            Ok("editor text with the finished queue line\n".to_string()),
        ]);
        let admitted = resolve_admitting_transient_replica(
            file(),
            "initial",
            || reads.borrow_mut().remove(0),
            classify,
            || {
                ReplicaHoldOutcome::Returned(ReplicaHoldReport {
                    waited: Duration::from_secs(6),
                    budget: Duration::from_secs(20),
                    final_state: "lazily_current",
                    observations: 60,
                    reregister_nudges: 1,
                })
            },
        )
        .expect("a self-healing refusal is admitted once the replica returns");
        assert_eq!(admitted.content, "editor text with the finished queue line\n");
        assert_eq!(
            admitted.replica_hold.map(|report| report.waited),
            Some(Duration::from_secs(6))
        );
        assert!(reads.borrow().is_empty(), "resolved exactly twice");
    }

    #[test]
    fn expired_hold_keeps_the_typed_refusal_and_names_its_cause() {
        let error = resolve_admitting_transient_replica(
            file(),
            "initial",
            || Err(anyhow::Error::new(Refused(AttachedEditorRecoveryDecision::ReregisterEditor))),
            classify,
            || {
                ReplicaHoldOutcome::Expired(ReplicaHoldReport {
                    waited: Duration::from_secs(20),
                    budget: Duration::from_secs(20),
                    final_state: "missing_replica",
                    observations: 200,
                    reregister_nudges: 3,
                })
            },
        )
        .expect_err("a replica that never returned still refuses");
        assert_eq!(
            classify(&error),
            Some(AttachedEditorRecoveryDecision::ReregisterEditor),
            "the typed refusal stays in the chain"
        );
        let rendered = format!("{error:#}");
        assert!(rendered.starts_with("replica_hold_expired:"), "{rendered}");
        assert!(rendered.contains("held 20000ms"), "{rendered}");
        assert!(rendered.contains("missing_replica"), "{rendered}");
    }

    #[test]
    fn operator_action_refusals_and_other_errors_never_hold() {
        for recovery in [
            AttachedEditorRecoveryDecision::RestartEditor,
            AttachedEditorRecoveryDecision::WaitForSenderRecycle,
            AttachedEditorRecoveryDecision::WaitForEndpointGeneration,
        ] {
            let error = resolve_admitting_transient_replica(
                file(),
                "initial",
                || Err(anyhow::Error::new(Refused(recovery))),
                classify,
                || panic!("{recovery:?} must refuse without holding"),
            )
            .expect_err("refused");
            assert_eq!(classify(&error), Some(recovery));
        }
        let error = resolve_admitting_transient_replica(
            file(),
            "initial",
            || Err(anyhow::anyhow!("unrelated failure")),
            classify,
            || panic!("an unrelated failure must not hold"),
        )
        .expect_err("passes through");
        assert_eq!(format!("{error:#}"), "unrelated failure");
    }
}
