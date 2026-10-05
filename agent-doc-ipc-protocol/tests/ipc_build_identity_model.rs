//! Model-based test: `formal/tla/IpcBuildIdentity.tla` (`ContentDerived =
//! TRUE`, the shipped configuration) drives the real handshake wire path
//! (`#netadv7`).
//!
//! The model's `Handshake` verdict is `Identity(client) = Identity(listener)`.
//! Here the identities are real `IpcPeerIdentity` values derived from each
//! peer's source generation, the client's real `ipc_hello` line is validated
//! by the real `validate_ipc_hello`, the listener's real `ipc_hello_ack` (or
//! rejection receipt) is validated by the real `validate_ipc_hello_ack`, and
//! both verdicts must equal the model's on every step of random traces.
//! `EquivalentCodeIsAdmitted` and `DivergentCodeIsRejected` are checked after
//! every handshake, and the `Reach` non-vacuity (both outcomes occur) is
//! checked by a deterministic trace.

use agent_doc_ipc_protocol::{
    IPC_PROTOCOL_VERSION, IpcHandshakeError, IpcPeerIdentity, ipc_handshake_rejection,
    ipc_hello_ack_message, ipc_hello_message, validate_ipc_hello, validate_ipc_hello_ack,
};
use proptest::prelude::*;

const TLA_SOURCE: &str = include_str!("../../formal/tla/IpcBuildIdentity.tla");
const MAX_CODE: u32 = 3;
const MAX_CLOCK: u32 = 3;

#[derive(Debug, Clone, Copy)]
enum Peer {
    Client,
    Listener,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    RebuildUnchanged(Peer),
    EditMemberPackage(Peer),
    EditRootPackage(Peer),
    Handshake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    None,
    Admitted,
    Rejected,
}

#[derive(Debug, Clone, Copy)]
struct State {
    code: [u32; 2],
    stamp: [u32; 2],
    clock: u32,
    judged: [u32; 2],
    verdict: Verdict,
}

fn index(peer: Peer) -> usize {
    match peer {
        Peer::Client => 0,
        Peer::Listener => 1,
    }
}

/// `Identity(p) == IF ContentDerived THEN code[p] ELSE stamp[p]` with
/// `ContentDerived = TRUE`: a build id that is a function of the source
/// generation only (the shipped `source_digest`), never of the clock.
fn identity(state: &State, peer: Peer) -> IpcPeerIdentity {
    IpcPeerIdentity::new(
        IPC_PROTOCOL_VERSION,
        format!("0.0.0+src{:04}", state.code[index(peer)]),
    )
}

/// The real two-message handshake. Returns the listener's verdict and the
/// client's verdict on the listener's reply.
fn wire_handshake(state: &State) -> (bool, bool) {
    let client = identity(state, Peer::Client);
    let listener = identity(state, Peer::Listener);
    let hello = ipc_hello_message(&client).to_string();
    let listener_verdict = validate_ipc_hello(&hello, &listener);
    let reply = match &listener_verdict {
        Ok(()) => ipc_hello_ack_message(&listener).to_string(),
        Err(error) => ipc_handshake_rejection(error, &listener),
    };
    let client_verdict = validate_ipc_hello_ack(&reply, &client);
    if let Err(IpcHandshakeError::BuildMismatch {
        listener: reported_listener,
        client: reported_client,
    }) = &client_verdict
    {
        // `#ipcmismatchlabels`: roles are reported by role on both sides.
        assert_eq!(reported_listener, &listener.build_id);
        assert_eq!(reported_client, &client.build_id);
    }
    (listener_verdict.is_ok(), client_verdict.is_ok())
}

fn step(state: &State, action: Action) -> Option<State> {
    let mut next = *state;
    match action {
        Action::RebuildUnchanged(peer) => {
            if state.clock >= MAX_CLOCK {
                return None;
            }
            next.clock += 1;
            next.stamp[index(peer)] = state.clock + 1;
        }
        Action::EditMemberPackage(peer) => {
            if state.code[index(peer)] >= MAX_CODE {
                return None;
            }
            next.code[index(peer)] += 1;
        }
        Action::EditRootPackage(peer) => {
            if state.code[index(peer)] >= MAX_CODE || state.clock >= MAX_CLOCK {
                return None;
            }
            next.code[index(peer)] += 1;
            next.clock += 1;
            next.stamp[index(peer)] = state.clock + 1;
        }
        Action::Handshake => {
            let model_admits = state.code[0] == state.code[1];
            let (listener_admits, client_admits) = wire_handshake(state);
            assert_eq!(listener_admits, model_admits, "listener verdict vs model: {state:?}");
            assert_eq!(client_admits, model_admits, "client verdict vs model: {state:?}");
            next.verdict = if listener_admits {
                Verdict::Admitted
            } else {
                Verdict::Rejected
            };
            next.judged = state.code;
        }
    }
    Some(next)
}

fn check_invariants(state: &State) -> Result<(), String> {
    if state.verdict == Verdict::Rejected && state.judged[0] == state.judged[1] {
        return Err(format!("EquivalentCodeIsAdmitted violated: {state:?}"));
    }
    if state.verdict == Verdict::Admitted && state.judged[0] != state.judged[1] {
        return Err(format!("DivergentCodeIsRejected violated: {state:?}"));
    }
    Ok(())
}

#[test]
fn tla_identity_and_handshake_text_match_the_transcription() {
    assert!(TLA_SOURCE.contains("Identity(p) == IF ContentDerived THEN code[p] ELSE stamp[p]"));
    assert!(TLA_SOURCE.contains(
        "verdict' = IF Identity(\"client\") = Identity(\"listener\")\n                    THEN \"admitted\"\n                    ELSE \"rejected\""
    ));
    let cfg = include_str!("../../formal/tla/IpcBuildIdentity.cfg");
    assert!(cfg.contains("ContentDerived = TRUE"), "shipped config is the fix");
}

fn arb_peer() -> impl Strategy<Value = Peer> {
    prop_oneof![Just(Peer::Client), Just(Peer::Listener)]
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        arb_peer().prop_map(Action::RebuildUnchanged),
        arb_peer().prop_map(Action::EditMemberPackage),
        arb_peer().prop_map(Action::EditRootPackage),
        Just(Action::Handshake),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn real_handshake_matches_the_model_on_random_traces(
        schedule in proptest::collection::vec(arb_action(), 0..24),
    ) {
        let mut state = State {
            code: [1, 1],
            stamp: [0, 0],
            clock: 0,
            judged: [1, 1],
            verdict: Verdict::None,
        };
        for action in schedule {
            if let Some(next) = step(&state, action) {
                state = next;
            }
            prop_assert!(check_invariants(&state).is_ok(), "{:?}", check_invariants(&state));
        }
    }
}

/// `IpcBuildIdentityReach.cfg` non-vacuity, as a deterministic trace: the
/// fixed handshake still makes BOTH decisions, including the two the clock
/// stamp got wrong (a rebuild of unchanged code is admitted; a member-crate
/// edit is rejected).
#[test]
fn both_outcomes_are_reachable_including_the_two_clock_failures() {
    let mut state = State {
        code: [1, 1],
        stamp: [0, 0],
        clock: 0,
        judged: [1, 1],
        verdict: Verdict::None,
    };
    state = step(&state, Action::RebuildUnchanged(Peer::Listener)).unwrap();
    state = step(&state, Action::Handshake).unwrap();
    assert_eq!(state.verdict, Verdict::Admitted, "false mismatch is gone");
    state = step(&state, Action::EditMemberPackage(Peer::Client)).unwrap();
    state = step(&state, Action::Handshake).unwrap();
    assert_eq!(state.verdict, Verdict::Rejected, "false match is gone");
    check_invariants(&state).unwrap();
}
