//! Model-based tests: `formal/tla/SupervisorGenerationTransition.tla` drives
//! the production gate `lifecycle::generation_transition_admission`
//! (`#netadv7`).
//!
//! The TLA+ model and the Rust gate share one transition table. The model's
//! `Replace` guard is transcribed below as [`tla_replace_enabled`] and pinned
//! to the `.tla` source text, the production gate is asserted equal to it on
//! every input, and then the model is executed with the production gate in
//! place of the transcribed guard:
//!
//! * exhaustively, by a TLC-style breadth-first search of the finite state
//!   space, checking every model invariant in every reachable state; and
//! * by proptest, over random initial states and random action schedules,
//!   checking the invariants after every step and the model's liveness
//!   property (`TerminalRequestEventuallyReplaced`) under a fair schedule.
//!
//! A drift in either direction fails here: editing the `.tla` guard breaks the
//! text pin, and editing the Rust gate breaks the table equality and the
//! invariants.

use agent_doc_supervisor::lifecycle::{
    DurableReplayCheckpoint, GenerationTransitionAdmission, generation_transition_admission,
};
use proptest::prelude::*;
use std::collections::{HashSet, VecDeque};

const TLA_SOURCE: &str = include_str!("../../formal/tla/SupervisorGenerationTransition.tla");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cause {
    Install,
    Stale,
    Restart,
    CapturedRecovery,
}

const CAUSES: [Cause; 4] = [
    Cause::Install,
    Cause::Stale,
    Cause::Restart,
    Cause::CapturedRecovery,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Phase {
    Requested,
    Replaced,
}

/// The model's `VARIABLES`, one field each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct State {
    cycle_open: bool,
    replay_checkpoint: bool,
    ipc_drained: bool,
    cause: Cause,
    phase: Phase,
    replacements: u32,
    unsafe_replacements: u32,
    uncheckpointed_open_replacements: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    CloseCycle,
    DrainIpc,
    PublishCapturedReplay,
    Replace,
}

const ACTIONS: [Action; 4] = [
    Action::CloseCycle,
    Action::DrainIpc,
    Action::PublishCapturedReplay,
    Action::Replace,
];

/// `Init`: every combination of the free booleans and causes.
fn initial_states() -> Vec<State> {
    let mut states = Vec::new();
    for cycle_open in [false, true] {
        for replay_checkpoint in [false, true] {
            for ipc_drained in [false, true] {
                for cause in CAUSES {
                    states.push(State {
                        cycle_open,
                        replay_checkpoint,
                        ipc_drained,
                        cause,
                        phase: Phase::Requested,
                        replacements: 0,
                        unsafe_replacements: 0,
                        uncheckpointed_open_replacements: 0,
                    });
                }
            }
        }
    }
    states
}

/// `CapturedRecovery == cause = "captured_recovery" /\ replayCheckpoint`.
fn captured_recovery(s: &State) -> bool {
    s.cause == Cause::CapturedRecovery && s.replay_checkpoint
}

/// The model's `Replace` guard, transcribed literally:
/// `ipcDrained /\ (~cycleOpen \/ CapturedRecovery)`.
fn tla_replace_enabled(s: &State) -> bool {
    s.ipc_drained && (!s.cycle_open || captured_recovery(s))
}

/// The production gate, fed the model state. The durable replay checkpoint the
/// Rust gate sees exists exactly when the model's `CapturedRecovery` holds.
fn production_admission(s: &State) -> GenerationTransitionAdmission {
    let replay = if captured_recovery(s) {
        DurableReplayCheckpoint::CapturedResponse
    } else {
        DurableReplayCheckpoint::Absent
    };
    generation_transition_admission(s.cycle_open, replay, s.ipc_drained)
}

/// `Next`, with the `Replace` guard supplied by the PRODUCTION gate.
fn step(s: &State, action: Action) -> Option<State> {
    if s.phase != Phase::Requested {
        return None; // `Done` is a stutter.
    }
    let mut next = *s;
    match action {
        Action::CloseCycle => {
            if !s.cycle_open {
                return None;
            }
            next.cycle_open = false;
        }
        Action::DrainIpc => {
            if s.ipc_drained {
                return None;
            }
            next.ipc_drained = true;
        }
        Action::PublishCapturedReplay => {
            if s.cause != Cause::CapturedRecovery || s.replay_checkpoint {
                return None;
            }
            next.replay_checkpoint = true;
        }
        Action::Replace => {
            if production_admission(s) != GenerationTransitionAdmission::Permit {
                return None;
            }
            next.phase = Phase::Replaced;
            next.replacements += 1;
            next.unsafe_replacements += u32::from(!s.ipc_drained);
            next.uncheckpointed_open_replacements += u32::from(s.cycle_open && !captured_recovery(s));
        }
    }
    Some(next)
}

/// Every `INVARIANT` in `SupervisorGenerationTransition.cfg`.
fn check_invariants(s: &State) -> Result<(), String> {
    if s.uncheckpointed_open_replacements != 0 {
        return Err(format!("OpenUncheckpointedNeverReplaced violated: {s:?}"));
    }
    if s.unsafe_replacements != 0 {
        return Err(format!("UnsafeCheckpointNeverReplaced violated: {s:?}"));
    }
    if s.replacements > 1 {
        return Err(format!("AtMostOneReplacement violated: {s:?}"));
    }
    if s.phase == Phase::Replaced && s.cycle_open && !captured_recovery(s) {
        return Err(format!("OnlyCapturedRecoveryCrossesOpenCycle violated: {s:?}"));
    }
    Ok(())
}

#[test]
fn tla_replace_guard_text_is_the_transcribed_guard() {
    let replace = TLA_SOURCE
        .split("Replace ==")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .expect("Replace action in the .tla source");
    for conjunct in [
        "/\\ phase = \"requested\"",
        "/\\ ipcDrained",
        "/\\ (~cycleOpen \\/ CapturedRecovery)",
    ] {
        assert!(
            replace.contains(conjunct),
            "Replace guard drifted from the transcription; missing `{conjunct}` in:\n{replace}"
        );
    }
    assert!(
        TLA_SOURCE.contains(
            "CapturedRecovery ==\n    /\\ cause = \"captured_recovery\"\n    /\\ replayCheckpoint"
        ),
        "CapturedRecovery definition drifted from the transcription"
    );
}

#[test]
fn production_gate_equals_the_tla_guard_on_every_state() {
    // The shared transition table: one row per (cycleOpen, CapturedRecovery,
    // ipcDrained), expected Replace-enabled. Both sides must match it.
    const TABLE: [(bool, bool, bool, bool); 8] = [
        // (cycle_open, captured_recovery, ipc_drained) -> replace enabled
        (false, false, false, false),
        (false, false, true, true),
        (false, true, false, false),
        (false, true, true, true),
        (true, false, false, false),
        (true, false, true, false),
        (true, true, false, false),
        (true, true, true, true),
    ];
    for state in initial_states() {
        let row = TABLE
            .iter()
            .find(|(open, captured, drained, _)| {
                *open == state.cycle_open
                    && *captured == captured_recovery(&state)
                    && *drained == state.ipc_drained
            })
            .expect("table covers every state");
        assert_eq!(tla_replace_enabled(&state), row.3, "TLA guard vs table: {state:?}");
        assert_eq!(
            production_admission(&state) == GenerationTransitionAdmission::Permit,
            row.3,
            "production gate vs table: {state:?}"
        );
    }
}

#[test]
fn exhaustive_state_space_satisfies_every_invariant() {
    let mut seen: HashSet<State> = HashSet::new();
    let mut queue: VecDeque<State> = initial_states().into_iter().collect();
    let mut replaced_states = 0usize;
    while let Some(state) = queue.pop_front() {
        if !seen.insert(state) {
            continue;
        }
        check_invariants(&state).unwrap();
        if state.phase == Phase::Replaced {
            replaced_states += 1;
        }
        for action in ACTIONS {
            if let Some(next) = step(&state, action) {
                queue.push_back(next);
            }
        }
    }
    // Non-vacuity: the gate must actually permit replacements, including the
    // captured-recovery crossing of an open cycle.
    assert!(replaced_states > 0, "no state ever replaced the supervisor");
    assert!(
        seen.iter()
            .any(|s| s.phase == Phase::Replaced && s.cycle_open && captured_recovery(s)),
        "captured recovery never crossed an open cycle"
    );
}

fn arb_state() -> impl Strategy<Value = State> {
    (any::<bool>(), any::<bool>(), any::<bool>(), 0usize..4).prop_map(
        |(cycle_open, replay_checkpoint, ipc_drained, cause)| State {
            cycle_open,
            replay_checkpoint,
            ipc_drained,
            cause: CAUSES[cause],
            phase: Phase::Requested,
            replacements: 0,
            unsafe_replacements: 0,
            uncheckpointed_open_replacements: 0,
        },
    )
}

fn arb_action() -> impl Strategy<Value = Action> {
    (0usize..4).prop_map(|index| ACTIONS[index])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Random traces: every invariant holds after every step, and a disabled
    /// action is a stutter, exactly as in TLC.
    #[test]
    fn random_traces_preserve_every_invariant(
        init in arb_state(),
        schedule in proptest::collection::vec(arb_action(), 0..16),
    ) {
        let mut state = init;
        for action in schedule {
            if let Some(next) = step(&state, action) {
                state = next;
            }
            prop_assert!(check_invariants(&state).is_ok(), "{:?}", check_invariants(&state));
        }
    }

    /// `TerminalRequestEventuallyReplaced` under `WF_vars(Replace)`: from any
    /// state an arbitrary prefix reaches, once the cycle has closed and IPC has
    /// drained, Replace is enabled and the request terminates replaced. There is
    /// no timeout input that could make this wait on a wall clock.
    #[test]
    fn fair_schedule_eventually_replaces_a_terminal_request(
        init in arb_state(),
        prefix in proptest::collection::vec(arb_action(), 0..8),
    ) {
        let mut state = init;
        for action in prefix {
            if let Some(next) = step(&state, action) {
                state = next;
            }
        }
        // Environment edges are fair: the cycle closes and IPC drains.
        for action in [Action::CloseCycle, Action::DrainIpc] {
            if let Some(next) = step(&state, action) {
                state = next;
            }
        }
        if state.phase == Phase::Requested {
            prop_assert!(!state.cycle_open && state.ipc_drained);
            prop_assert_eq!(production_admission(&state), GenerationTransitionAdmission::Permit);
            state = step(&state, Action::Replace).expect("Replace enabled under fairness");
        }
        prop_assert_eq!(state.phase, Phase::Replaced);
        prop_assert!(check_invariants(&state).is_ok());
    }
}
