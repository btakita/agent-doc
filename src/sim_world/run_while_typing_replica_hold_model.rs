//! GH #272 (`#runtypingadmit`) SimWorld: `Run Agent Doc` pressed while the
//! operator is still typing and the document's editor replica is briefly
//! unavailable (a rebuild in flight after a controller recycle).
//!
//! Each scenario drives the PRODUCTION pieces: the admission orchestration and
//! bounded hold loop (`agent_doc_preflight_io::admission_replica_hold`), the
//! pure hold policy (`agent_doc_turn::authority_recovery`), the typed
//! attached-editor refusal and its classifier
//! (`agent_doc_document_realtime_io::AttachedEditorDiskReadRefused`). Only the
//! editor (its replica and the operator's keystrokes), the stale disk image,
//! and the clock are simulated.
//!
//! Invariants, over every recovery class x replica-return time x typing span:
//! - a self-healing refusal whose replica returns inside the hold is ADMITTED
//!   (the regression: it used to end "did not run");
//! - an admitted read is exactly the editor buffer at the instant of the
//!   re-resolve: never the stale disk image, never a duplicated or dropped
//!   keystroke;
//! - a refusal that stands keeps the typed refusal in its chain, and names its
//!   cause when a hold was spent on it.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_doc_document_realtime_io::{
    AttachedEditorDiskReadRefused, attached_editor_disk_read_refusal,
};
use agent_doc_preflight_io::admission_replica_hold::{
    HoldObservation, hold_for_replica_with_effects,
    resolve_admitting_transient_replica,
};
use agent_doc_turn::authority_recovery::{AttachedEditorRecoveryDecision, AuthorityObservation};

const BUDGET: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(100);
const NUDGE_EVERY: Duration = Duration::from_secs(5);
const DISK: &str = "# S\n\n<!-- agent:queue go -->\n- earlier item\n<!-- /agent:queue -->\n";
const QUEUE_LINE: &str = "- fix then close";

const RECOVERIES: [AttachedEditorRecoveryDecision; 5] = [
    AttachedEditorRecoveryDecision::ReloadEditor,
    AttachedEditorRecoveryDecision::ReregisterEditor,
    AttachedEditorRecoveryDecision::RestartEditor,
    AttachedEditorRecoveryDecision::WaitForSenderRecycle,
    AttachedEditorRecoveryDecision::WaitForEndpointGeneration,
];

/// The simulated editor: a buffer the operator types into one byte per tick,
/// and a replica that is missing until `returns_at` (or forever).
struct Editor {
    origin: Instant,
    clock: Cell<Duration>,
    returns_at: Option<Duration>,
    typing_ticks: usize,
}

impl Editor {
    fn now(&self) -> Instant {
        self.origin + self.clock.get()
    }

    fn replica_present(&self) -> bool {
        self.returns_at.is_some_and(|at| self.clock.get() >= at)
    }

    /// The operator typed one byte of the queue line per elapsed second.
    fn buffer(&self) -> String {
        let typed = (self.clock.get().as_secs() as usize)
            .min(self.typing_ticks)
            .min(QUEUE_LINE.len());
        DISK.replace(
            "<!-- /agent:queue -->",
            &format!("{}\n<!-- /agent:queue -->", &QUEUE_LINE[..typed]),
        )
    }
}

fn file() -> PathBuf {
    PathBuf::from("/nonexistent-agent-doc-root/gh272-sim.md")
}

/// The production precedence, simulated at its boundary: an attached editor
/// with a present replica answers with its buffer; a missing one raises the
/// typed refusal. Disk is never reachable while the editor is attached.
fn resolve(editor: &Editor, recovery: AttachedEditorRecoveryDecision) -> anyhow::Result<String> {
    if editor.replica_present() {
        return Ok(editor.buffer());
    }
    Err(anyhow::Error::new(AttachedEditorDiskReadRefused {
        file: file(),
        reason: "missing_replica".to_string(),
        remedy: "Remedy: simulated".to_string(),
        recovery,
    }))
    .map_err(|error| error.context("preflight initial: resolve current document"))
}

struct Run {
    result: anyhow::Result<agent_doc_preflight_io::admission_replica_hold::AdmittedDocument>,
    held: bool,
    clock_at_end: Duration,
}

fn run(
    recovery: AttachedEditorRecoveryDecision,
    returns_at: Option<Duration>,
    typing_ticks: usize,
) -> Run {
    let editor = Editor {
        origin: Instant::now(),
        clock: Cell::new(Duration::ZERO),
        returns_at,
        typing_ticks,
    };
    let held = RefCell::new(false);
    let path = file();
    let result = resolve_admitting_transient_replica(
        Path::new(&path),
        "initial",
        || resolve(&editor, recovery),
        |error| attached_editor_disk_read_refusal(error).map(|refusal| refusal.recovery),
        || {
            *held.borrow_mut() = true;
            hold_for_replica_with_effects(
                BUDGET,
                POLL,
                NUDGE_EVERY,
                || {
                    if editor.replica_present() {
                        HoldObservation {
                            observation: AuthorityObservation::Current,
                            state: "lazily_current",
                        }
                    } else {
                        HoldObservation {
                            observation: AuthorityObservation::MissingReplica,
                            state: "missing_replica",
                        }
                    }
                },
                || {},
                |wait| editor.clock.set(editor.clock.get() + wait),
                || editor.now(),
            )
        },
    );
    if let Ok(admitted) = &result {
        assert_eq!(
            admitted.content,
            editor.buffer(),
            "admitted text must be the editor buffer at the re-resolve instant",
        );
    }
    let held = *held.borrow();
    Run {
        result,
        held,
        clock_at_end: editor.clock.get(),
    }
}

fn self_healing(recovery: AttachedEditorRecoveryDecision) -> bool {
    matches!(
        recovery,
        AttachedEditorRecoveryDecision::ReloadEditor
            | AttachedEditorRecoveryDecision::ReregisterEditor
    )
}

#[test]
fn run_while_typing_through_a_transient_missing_replica_is_admitted_on_coherent_editor_text() {
    let mut admitted_mid_typing = 0usize;
    let mut refused_after_hold = 0usize;
    for recovery in RECOVERIES {
        let mut returns = vec![None];
        returns.extend((0..=25).map(|secs| Some(Duration::from_secs(secs))));
        for returns_at in returns {
            for typing_ticks in [0usize, 3, 8, QUEUE_LINE.len()] {
                let outcome = run(recovery, returns_at, typing_ticks);
                let returns_in_budget = returns_at.is_some_and(|at| at <= BUDGET);
                // Replica present at Run time: no hold, same as before.
                if returns_at == Some(Duration::ZERO) {
                    assert!(outcome.result.is_ok(), "{recovery:?}");
                    assert!(!outcome.held);
                    continue;
                }
                match (&outcome.result, self_healing(recovery) && returns_in_budget) {
                    (Ok(admitted), true) => {
                        assert!(outcome.held, "{recovery:?} {returns_at:?}");
                        let report = admitted.replica_hold.as_ref().expect("hold report");
                        assert_eq!(report.final_state, "lazily_current");
                        assert!(report.waited <= BUDGET);
                        let line_lines = admitted
                            .content
                            .lines()
                            .filter(|line| line.starts_with("- f"))
                            .count();
                        assert!(line_lines <= 1, "operator text duplicated: {}", admitted.content);
                        if admitted.content.contains("- f")
                            && !admitted.content.contains(QUEUE_LINE)
                        {
                            admitted_mid_typing += 1;
                        }
                    }
                    (Err(error), false) => {
                        let refusal = attached_editor_disk_read_refusal(error)
                            .expect("the typed refusal survives in the chain");
                        assert_eq!(refusal.recovery, recovery);
                        let rendered = format!("{error:#}");
                        if self_healing(recovery) {
                            assert!(outcome.held);
                            assert!(
                                rendered.starts_with("replica_hold_expired:"),
                                "a refusal that outlived its hold names the cause: {rendered}"
                            );
                            assert_eq!(outcome.clock_at_end, BUDGET, "hold is bounded");
                            refused_after_hold += 1;
                        } else {
                            assert!(!outcome.held, "{recovery:?} needs an operator action");
                            assert_eq!(outcome.clock_at_end, Duration::ZERO);
                            assert!(!rendered.contains("replica_hold_expired"));
                        }
                    }
                    (result, expected_admit) => panic!(
                        "{recovery:?} returns_at={returns_at:?} typing={typing_ticks}: \
                         expected admit={expected_admit}, got {:?}",
                        result.as_ref().map(|admitted| &admitted.content)
                    ),
                }
            }
        }
    }
    // The model reached both the "admitted while still typing" shape (the
    // operator's request: the in-progress edit is a coherent snapshot, and the
    // typing gate takes it from there) and the bounded refusal.
    assert!(admitted_mid_typing > 0);
    assert!(refused_after_hold > 0);
}

/// The incident timeline: refusal at Run, replica back six seconds later while
/// the operator kept typing. Before GH #272 this ended "did not run".
#[test]
fn incident_timeline_replica_back_after_six_seconds_is_admitted() {
    let outcome = run(
        AttachedEditorRecoveryDecision::ReloadEditor,
        Some(Duration::from_secs(6)),
        QUEUE_LINE.len(),
    );
    let admitted = outcome.result.expect("admitted after the hold");
    let report = admitted.replica_hold.expect("held");
    assert_eq!(report.waited, Duration::from_secs(6));
    assert_eq!(report.reregister_nudges, 1);
    // Six keystrokes landed during the hold; all six, once each, and not the
    // stale disk image that lacks them.
    assert!(admitted.content.contains(&format!("{}\n<!--", &QUEUE_LINE[..6])));
    assert_ne!(admitted.content, DISK);
}

/// A replica that never returns still refuses, after exactly one bounded hold.
#[test]
fn a_replica_that_never_returns_refuses_after_one_bounded_hold() {
    let outcome = run(AttachedEditorRecoveryDecision::ReregisterEditor, None, 4);
    assert!(outcome.held);
    assert_eq!(outcome.clock_at_end, BUDGET);
    let error = outcome.result.expect_err("refused");
    assert!(attached_editor_disk_read_refusal(&error).is_some());
}
