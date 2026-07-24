//! `#agent-doc-command-plane` — agent-doc control-plane operations expressed on
//! lazily's `command-plane-v1` (`CommandSubmit` / `CausalReceipt` RPC facade over
//! message passing), instead of bespoke `controller.sock` request/response JSON.
//!
//! This is the first vertical slice of the control-plane migration: the supervisor
//! recycle/restart **request** op. lazily owns the command envelope; agent-doc owns
//! the payload schema (`agent-doc.supervisor_recycle.v1`) and never leaks payload
//! decoding into lazily. Terminal authority is the causal receipt — a transport ACK
//! is never terminal (`command-plane-v1`'s "progress is not proof" rule).
//!
//! The pure command construction + payload codec + receipt mapping here are unit
//! tested end-to-end against `lazily::CommandRpcClient`. Wiring a `CommandTransport`
//! over the live controller socket + a loop frame handler that calls
//! [`super::rpc::handle_supervisor_recycle_requested`] and emits the receipt is the
//! scoped integration follow-up; this module is the proven foundation it builds on.

use anyhow::{Context, Result};
use lazily::{CausalReceipt, CommandPolicy, CommandSubmit, DedupePolicy, IpcValue};
use serde::{Deserialize, Serialize};

/// Domain namespace owning agent-doc command payloads. lazily never decodes these.
pub const NAMESPACE: &str = "agent-doc";
/// Command name within the namespace for a supervisor recycle/restart request.
pub const SUPERVISOR_RECYCLE_NAME: &str = "supervisor_recycle";
/// Fully-qualified payload schema id for the supervisor recycle request.
pub const SUPERVISOR_RECYCLE_PAYLOAD_TYPE: &str = "agent-doc.supervisor_recycle.v1";
/// Handler identity that services agent-doc control-plane commands.
pub const CONTROLLER_TARGET: &str = "project-controller";
/// Feature the target must advertise or the submit fails closed.
pub const REQUIRED_FEATURE_RECEIPTS: &str = "causal-receipts";

/// Payload body for `agent-doc.supervisor_recycle.v1`. Carries the recycle cause
/// (`stale_binary`, `admin_recycle`, install fan-out, a wedge trigger, …) — the
/// same `reason` the on-disk `recycle_request` marker recorded, now on the command
/// plane. lazily treats this as opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorRecyclePayload {
    pub reason: String,
}

impl SupervisorRecyclePayload {
    /// Serialize to the inline command payload bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("encode supervisor_recycle command payload")
    }

    /// Decode from a `CommandSubmit`'s inline payload, validating the payload type.
    pub fn decode(submit: &CommandSubmit) -> Result<Self> {
        anyhow::ensure!(
            submit.payload_type == SUPERVISOR_RECYCLE_PAYLOAD_TYPE,
            "unexpected command payload_type {:?} (want {SUPERVISOR_RECYCLE_PAYLOAD_TYPE})",
            submit.payload_type
        );
        let IpcValue::Inline(bytes) = &submit.payload else {
            anyhow::bail!("supervisor_recycle command payload must be inline bytes");
        };
        serde_json::from_slice(bytes).context("decode supervisor_recycle command payload")
    }
}

/// Content hash tag for a payload body (`sha256:…`), used for command dedupe/proof.
fn payload_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", agent_doc_hash::bytes_hash(bytes))
}

/// Build the `CommandSubmit` for a supervisor recycle/restart request. `command_id`
/// must be stable + replay-safe (the caller derives it from the recycle epoch);
/// `idempotency_key` dedupes concurrent requests for the same project.
pub fn build_supervisor_recycle_submit(
    command_id: impl Into<String>,
    source: impl Into<String>,
    idempotency_key: impl Into<String>,
    reason: impl Into<String>,
    authority_generation: u64,
) -> Result<CommandSubmit> {
    let command_id = command_id.into();
    let payload = SupervisorRecyclePayload {
        reason: reason.into(),
    };
    let bytes = payload.encode()?;
    let payload_hash = payload_hash(&bytes);
    Ok(CommandSubmit {
        causation_id: command_id.clone(),
        command_id,
        source: source.into(),
        target: CONTROLLER_TARGET.to_string(),
        namespace: NAMESPACE.to_string(),
        name: SUPERVISOR_RECYCLE_NAME.to_string(),
        authority_generation,
        idempotency_key: idempotency_key.into(),
        // A recycle request never has a hard deadline; it settles when the recycle
        // statechart folds `Requested`/`Started`.
        deadline_ms: 0,
        policy: CommandPolicy {
            // A duplicate request for the same project idempotency-key must fold onto
            // the same command (mirrors the recycle statechart's Settled-only arming).
            dedupe: DedupePolicy::SameIdempotencyKey,
            supersede: false,
            cancel_on_preempt: false,
        },
        payload_type: SUPERVISOR_RECYCLE_PAYLOAD_TYPE.to_string(),
        payload_hash,
        payload: IpcValue::Inline(bytes),
        required_features: vec![REQUIRED_FEATURE_RECEIPTS.to_string()],
    })
}

/// The controller's terminal receipt for a serviced recycle-request command:
/// `applied` once the request folded onto the recycle statechart, `rejected` (with
/// a reason) if it could not. `receipt_id` is unique; `observer` is the controller
/// identity; `generation` is the authority generation the receipt is stamped at.
pub fn supervisor_recycle_receipt(
    submit: &CommandSubmit,
    receipt_id: impl Into<String>,
    observer: impl Into<String>,
    generation: u64,
    outcome: Result<(), String>,
) -> CausalReceipt {
    match outcome {
        Ok(()) => CausalReceipt::applied(receipt_id, &submit.command_id, observer, generation),
        Err(reason) => {
            CausalReceipt::rejected(receipt_id, &submit.command_id, observer, generation)
                .with_reason(reason)
        }
    }
}

// The closeout command-plane vocabulary (payload types + envelope builder +
// schema constants) is owned by the closeout-domain crate
// (`agent-doc-cycle-state-io`) so the `mark_*` chokepoint can build a submit
// without depending on the controller runtime. Re-export here so existing
// `command_plane::`-qualified usages resolve unchanged.
pub use agent_doc_cycle_state_io::command_plane::{
    build_closeout_advance_submit, CloseoutAdvancePayload, CloseoutPhaseEvent,
    CLOSEOUT_ADVANCE_NAME, CLOSEOUT_ADVANCE_PAYLOAD_TYPE, CommitObservation,
};

/// Decode a [`CloseoutAdvancePayload`] from a [`CommandSubmit`]'s inline
/// payload, validating the payload type. Lives here (not on the type) because it
/// touches the lazily envelope, which the closeout-domain crate does not depend
/// on.
pub fn decode_closeout_advance_payload(
    submit: &CommandSubmit,
) -> Result<CloseoutAdvancePayload> {
    anyhow::ensure!(
        submit.payload_type == CLOSEOUT_ADVANCE_PAYLOAD_TYPE,
        "unexpected command payload_type {:?} (want {CLOSEOUT_ADVANCE_PAYLOAD_TYPE})",
        submit.payload_type,
    );
    let IpcValue::Inline(bytes) = &submit.payload else {
        anyhow::bail!("closeout_advance command payload must be inline bytes");
    };
    serde_json::from_slice(bytes).context("decode closeout_advance command payload")
}

/// The controller's terminal receipt for a serviced closeout-advance command:
/// `applied` once the phase transition folded and the fact(s) landed in the
/// durable sink, `rejected` (with a reason) if the authority could not advance
/// (stale generation, no document, a no-op at the current phase, …). This
/// receipt — not a return value or transport ACK — is the terminal authority
/// the client resolves on.
pub fn closeout_advance_receipt(
    submit: &CommandSubmit,
    receipt_id: impl Into<String>,
    observer: impl Into<String>,
    generation: u64,
    outcome: Result<(), String>,
) -> CausalReceipt {
    match outcome {
        Ok(()) => CausalReceipt::applied(receipt_id, &submit.command_id, observer, generation),
        Err(reason) => {
            CausalReceipt::rejected(receipt_id, &submit.command_id, observer, generation)
                .with_reason(reason)
        }
    }
}

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Synchronous command-plane transport over the project-controller socket
/// (`command-plane-v1`, `#lzdurablesink`).
///
/// The controller socket is one-line-in / one-line-out, so a `CommandSubmit`
/// round-trips synchronously: [`CommandTransport::send`] frames the submit as a
/// `command_plane_submit` controller request, awaits the response, and stashes
/// the terminal [`CausalReceipt`] the controller's authority emitted. The lazily
/// envelope (`CommandSubmit` → terminal `CausalReceipt`, never a transport ACK)
/// stays the wire contract; this reuses the controller's existing NDJSON
/// request/response transport rather than introducing a second socket.
///
/// The transport owns the receipts it round-trips, so it cannot fold them into a
/// [`lazily::CommandRpcClient`] projection by itself (the client owns the
/// transport). Use [`ControllerCommandTransport::take_receipt`] after a `send`,
/// or the [`submit_closeout_advance_command`] helper, which resolves the call.
pub struct ControllerCommandTransport {
    project_root: PathBuf,
    pending_receipts: VecDeque<CausalReceipt>,
}

impl ControllerCommandTransport {
    /// Connect a transport to the controller owning `project_root`.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            pending_receipts: VecDeque::new(),
        }
    }

    /// Synchronous round-trip for one submit; returns the terminal receipt the
    /// controller authority emitted. A transport/decode failure (or a stale
    /// controller binary) is an `Err`; an authority `rejected` receipt is `Ok` —
    /// the transport succeeded, the authority's decision is in the receipt.
    fn round_trip_submit(&self, submit: &lazily::CommandSubmit) -> Result<CausalReceipt> {
        let submit_json = serde_json::to_string(submit).context("encode command_plane_submit")?;
        let request = super::ControllerRequest::command_plane_submit(submit_json);
        let receipt: CausalReceipt = super::rpc::request_controller(&self.project_root, request)?;
        Ok(receipt)
    }

    /// Take the terminal receipt stashed by the most recent `send` of a submit.
    pub fn take_receipt(&mut self) -> Option<CausalReceipt> {
        self.pending_receipts.pop_front()
    }
}

impl lazily::CommandTransport for ControllerCommandTransport {
    type Error = anyhow::Error;
    fn send(&mut self, message: &lazily::CommandMessage) -> Result<(), Self::Error> {
        let submit = match message {
            lazily::CommandMessage::CommandSubmit(submit) => submit.as_ref(),
            // Cancel/events/projection are inbound-only (events/projection) or
            // out-of-scope (cancel) on the synchronous request/response socket;
            // nothing to write.
            _ => return Ok(()),
        };
        let receipt = self.round_trip_submit(submit)?;
        self.pending_receipts.push_back(receipt);
        Ok(())
    }
}

/// Submit a `closeout_advance` command over the live controller socket and
/// resolve the terminal [`CausalReceipt`]. This is the synchronous client entry
/// point for the durable closeout sink (`#lzdurablesink`): the controller
/// authority decides from its live Lazily projection, persists the phase fact(s),
/// and acknowledges with the receipt — never a transport ACK.
///
/// Returns the receipt regardless of outcome (`Applied` or `Rejected`); only a
/// transport/decode failure (or a stale controller binary) is an `Err`. Pass the
/// `CommandSubmit` built by [`build_closeout_advance_submit`].
pub fn submit_closeout_advance_command(
    project_root: &Path,
    submit: lazily::CommandSubmit,
) -> Result<CausalReceipt> {
    let mut transport = ControllerCommandTransport::new(project_root);
    lazily::CommandTransport::send(
        &mut transport,
        &lazily::CommandMessage::CommandSubmit(Box::new(submit)),
    )?;
    transport
        .take_receipt()
        .ok_or_else(|| anyhow::anyhow!("command_plane_submit produced no terminal receipt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazily::{CallState, CommandMessage, CommandRpcClient, CommandStatus, CommandTransport};

    /// In-memory transport capturing sent frames (mirrors lazily's own test double).
    struct VecTransport {
        sent: Vec<CommandMessage>,
    }
    impl CommandTransport for VecTransport {
        type Error = ();
        fn send(&mut self, message: &CommandMessage) -> Result<(), ()> {
            self.sent.push(message.clone());
            Ok(())
        }
    }

    #[test]
    fn payload_roundtrips_and_validates_type() {
        let submit = build_supervisor_recycle_submit(
            "cmd-1",
            "supervisor",
            "root:recycle",
            "stale_binary",
            7,
        )
        .unwrap();
        assert_eq!(submit.namespace, NAMESPACE);
        assert_eq!(submit.payload_type, SUPERVISOR_RECYCLE_PAYLOAD_TYPE);
        let decoded = SupervisorRecyclePayload::decode(&submit).unwrap();
        assert_eq!(decoded.reason, "stale_binary");
    }

    #[test]
    fn wrong_payload_type_fails_closed() {
        let mut submit =
            build_supervisor_recycle_submit("cmd-1", "supervisor", "root:recycle", "x", 1).unwrap();
        submit.payload_type = "agent-doc.editor_route.v1".to_string();
        assert!(SupervisorRecyclePayload::decode(&submit).is_err());
    }

    #[test]
    fn recycle_request_resolves_only_on_terminal_receipt() {
        // The full command-plane RPC round-trip for the recycle op: submit → the
        // call stays Pending until the controller's terminal receipt folds in.
        let mut client = CommandRpcClient::new(VecTransport { sent: Vec::new() });
        let submit = build_supervisor_recycle_submit(
            "cmd-9",
            "supervisor",
            "root:recycle",
            "admin_recycle",
            3,
        )
        .unwrap();
        let id = client.submit(submit.clone()).unwrap();
        assert_eq!(client.poll_call(&id), CallState::Pending);

        // The controller services it (folds Requested onto the statechart) and emits
        // an applied receipt — the terminal authority that resolves the call.
        let receipt =
            supervisor_recycle_receipt(&submit, "rcpt-1", "project-controller", 3, Ok(()));
        client.ingest_receipt(&receipt);
        match client.poll_call(&id) {
            CallState::Resolved(entry) => assert_eq!(entry.status, CommandStatus::Applied),
            other => panic!("expected Resolved(Applied), got {other:?}"),
        }
    }

    #[test]
    fn rejected_recycle_request_resolves_rejected() {
        let mut client = CommandRpcClient::new(VecTransport { sent: Vec::new() });
        let submit =
            build_supervisor_recycle_submit("cmd-r", "supervisor", "root:recycle", "wedge", 4)
                .unwrap();
        let id = client.submit(submit.clone()).unwrap();
        let receipt = supervisor_recycle_receipt(
            &submit,
            "rcpt-2",
            "project-controller",
            4,
            Err("controller_unavailable".to_string()),
        );
        client.ingest_receipt(&receipt);
        match client.poll_call(&id) {
            CallState::Resolved(entry) => {
                assert_eq!(entry.status, CommandStatus::Rejected);
                assert_eq!(entry.reason.as_deref(), Some("controller_unavailable"));
            }
            other => panic!("expected Resolved(Rejected), got {other:?}"),
        }
    }

    #[test]
    fn closeout_advance_payload_roundtrips_and_validates_type() {
        let payload = CloseoutAdvancePayload {
            document_path: "/tmp/doc.md".to_string(),
            event: CloseoutPhaseEvent::WriteApplied,
            reason: None,
            snapshot_content: Some("snap".to_string()),
            file_content: Some("body".to_string()),
            response_sha256: None,
            cycle_id_hint: None,
        };
        let submit = build_closeout_advance_submit(
            "cmd-adv-1",
            "cycle_state",
            "doc:cycle:write_applied:body",
            7,
            payload,
        )
        .unwrap();
        assert_eq!(submit.namespace, NAMESPACE);
        assert_eq!(submit.name, CLOSEOUT_ADVANCE_NAME);
        assert_eq!(submit.payload_type, CLOSEOUT_ADVANCE_PAYLOAD_TYPE);
        assert_eq!(submit.policy.dedupe, DedupePolicy::SameIdempotencyKey);
        let decoded = decode_closeout_advance_payload(&submit).unwrap();
        assert_eq!(decoded.event, CloseoutPhaseEvent::WriteApplied);
        assert_eq!(decoded.file_content.as_deref(), Some("body"));
    }

    #[test]
    fn closeout_advance_last_event_label_is_derived_from_the_typed_event() {
        // No free-text label crosses the boundary: last_event derives from the enum.
        let write = CloseoutAdvancePayload {
            document_path: "/tmp/doc.md".to_string(),
            event: CloseoutPhaseEvent::WriteApplied,
            reason: None,
            snapshot_content: None,
            file_content: None,
            response_sha256: None,
            cycle_id_hint: None,
        };
        assert_eq!(write.last_event_label(), "write_applied");

        // The closed commit-observation vocabulary is the only behaviorally
        // significant label set, carried typed on the Committed variant.
        for (obs, label) in [
            (CommitObservation::Commit, "commit"),
            (CommitObservation::CommitSuccess, "commit_success"),
            (CommitObservation::CommitAlreadyCurrent, "commit_already_current"),
        ] {
            let committed = CloseoutAdvancePayload {
                document_path: "/tmp/doc.md".to_string(),
                event: CloseoutPhaseEvent::Committed(obs),
                reason: None,
                snapshot_content: None,
                file_content: None,
                response_sha256: None,
                cycle_id_hint: None,
            };
            assert_eq!(committed.last_event_label(), label);
        }

        // An abandon reason is a named field, not a label.
        let abandoned = CloseoutAdvancePayload {
            document_path: "/tmp/doc.md".to_string(),
            event: CloseoutPhaseEvent::Abandoned,
            reason: Some("stalled_preflight".to_string()),
            snapshot_content: None,
            file_content: None,
            response_sha256: None,
            cycle_id_hint: None,
        };
        assert_eq!(abandoned.last_event_label(), "stalled_preflight");
    }

    #[test]
    fn closeout_advance_wrong_payload_type_fails_closed() {
        let mut submit = build_closeout_advance_submit(
            "cmd-adv-2",
            "cycle_state",
            "doc:cycle:write_applied:body",
            1,
            CloseoutAdvancePayload {
                document_path: "/tmp/doc.md".to_string(),
                event: CloseoutPhaseEvent::WriteApplied,
                reason: None,
                snapshot_content: None,
                file_content: None,
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();
        submit.payload_type = "agent-doc.supervisor_recycle.v1".to_string();
        assert!(decode_closeout_advance_payload(&submit).is_err());
    }

    #[test]
    fn closeout_advance_resolves_only_on_terminal_receipt() {
        // The command-plane RPC round-trip for a phase advance: submit → the call
        // stays Pending until the controller folds the transition, persists the
        // fact(s) as the sink, and emits the terminal applied receipt.
        let mut client = CommandRpcClient::new(VecTransport { sent: Vec::new() });
        let submit = build_closeout_advance_submit(
            "cmd-adv-9",
            "cycle_state",
            "doc:cycle:write_applied:body",
            3,
            CloseoutAdvancePayload {
                document_path: "/tmp/doc.md".to_string(),
                event: CloseoutPhaseEvent::WriteApplied,
                reason: None,
                snapshot_content: None,
                file_content: Some("body".to_string()),
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();
        let id = client.submit(submit.clone()).unwrap();
        assert_eq!(client.poll_call(&id), CallState::Pending);

        let receipt =
            closeout_advance_receipt(&submit, "rcpt-adv-1", "project-controller", 3, Ok(()));
        client.ingest_receipt(&receipt);
        match client.poll_call(&id) {
            CallState::Resolved(entry) => assert_eq!(entry.status, CommandStatus::Applied),
            other => panic!("expected Resolved(Applied), got {other:?}"),
        }
    }

    #[test]
    fn closeout_advance_rejected_resolves_rejected() {
        let mut client = CommandRpcClient::new(VecTransport { sent: Vec::new() });
        let submit = build_closeout_advance_submit(
            "cmd-adv-r",
            "cycle_state",
            "doc:cycle:committed:body",
            4,
            CloseoutAdvancePayload {
                document_path: "/tmp/doc.md".to_string(),
                event: CloseoutPhaseEvent::Committed(CommitObservation::CommitSuccess),
                reason: None,
                snapshot_content: None,
                file_content: None,
                response_sha256: None,
                cycle_id_hint: None,
            },
        )
        .unwrap();
        let id = client.submit(submit.clone()).unwrap();
        // The authority rejects (e.g. a stale generation or a no-op at the current
        // phase); the terminal receipt carries the reason.
        let receipt = closeout_advance_receipt(
            &submit,
            "rcpt-adv-2",
            "project-controller",
            4,
            Err("stale_authority_generation".to_string()),
        );
        client.ingest_receipt(&receipt);
        match client.poll_call(&id) {
            CallState::Resolved(entry) => {
                assert_eq!(entry.status, CommandStatus::Rejected);
                assert_eq!(entry.reason.as_deref(), Some("stale_authority_generation"));
            }
            other => panic!("expected Resolved(Rejected), got {other:?}"),
        }
    }
}
