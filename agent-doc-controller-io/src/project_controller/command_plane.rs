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
        let submit =
            build_supervisor_recycle_submit("cmd-1", "supervisor", "root:recycle", "stale_binary", 7)
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
        let submit =
            build_supervisor_recycle_submit("cmd-9", "supervisor", "root:recycle", "admin_recycle", 3)
                .unwrap();
        let id = client.submit(submit.clone()).unwrap();
        assert_eq!(client.poll_call(&id), CallState::Pending);

        // The controller services it (folds Requested onto the statechart) and emits
        // an applied receipt — the terminal authority that resolves the call.
        let receipt = supervisor_recycle_receipt(&submit, "rcpt-1", "project-controller", 3, Ok(()));
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
            build_supervisor_recycle_submit("cmd-r", "supervisor", "root:recycle", "wedge", 4).unwrap();
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
}
