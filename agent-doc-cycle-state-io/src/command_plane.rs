//! Closeout command-plane payload schema (`#lzdurablesink`, `command-plane-v1`).
//!
//! The pure (serde-only) payload types a closeout client builds to ask the
//! controller authority to advance a document's phase machine over the lazily
//! command plane, and that the controller's `service_closeout_advance` decodes.
//! These live in the closeout-domain crate (below `controller-io`) so the
//! `mark_*` chokepoint can build them without depending on the controller
//! runtime; the lazily-bound envelope helpers (`build_closeout_advance_submit`,
//! `decode_closeout_advance_payload`) stay in `agent-doc-controller-io`.
//!
//! `mark_*` is the migration chokepoint (`#lazily-hot-path`): when a controller
//! is live it submits a `closeout_advance` command so the transition is decided
//! from the live Lazily projection; when no controller exists it keeps the local
//! `load→decide→save→append` path as the explicit actorless/bootstrap fallback.

use anyhow::{Context, Result};
use lazily::{CommandPolicy, CommandSubmit, DedupePolicy, IpcValue};
use serde::{Deserialize, Serialize};

/// Domain namespace owning agent-doc command payloads. lazily never decodes these.
/// NOTE: must match `agent-doc-controller-io`'s `command_plane::NAMESPACE` — the
/// controller dispatch refuses a foreign namespace; the integration test guards it.
pub const NAMESPACE: &str = "agent-doc";
/// Command name within the namespace for a closeout phase advance request.
pub const CLOSEOUT_ADVANCE_NAME: &str = "closeout_advance";
/// Fully-qualified payload schema id for the closeout phase advance request.
pub const CLOSEOUT_ADVANCE_PAYLOAD_TYPE: &str = "agent-doc.closeout_advance.v1";
/// Handler identity that services agent-doc control-plane commands.
/// NOTE: must match `agent-doc-controller-io`'s `command_plane::CONTROLLER_TARGET`.
pub const CONTROLLER_TARGET: &str = "project-controller";
/// Feature the target must advertise or the submit fails closed.
/// NOTE: must match `agent-doc-controller-io`'s `command_plane::REQUIRED_FEATURE_RECEIPTS`.
pub const REQUIRED_FEATURE_RECEIPTS: &str = "causal-receipts";

/// Content hash tag for a payload body (`sha256:…`), used for command dedupe/proof.
fn payload_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", agent_doc_hash::bytes_hash(bytes))
}

/// Which closeout phase transition a `closeout_advance` command requests. This
/// enum **is** the event label — no free-text label crosses the command
/// boundary. The controller authority runs the pure `CyclePhaseMachine` over the
/// transition; the legacy `last_event` vocabulary is derived from it (plus the
/// committed observation / abandon reason) by
/// [`CloseoutAdvancePayload::last_event_label`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloseoutPhaseEvent {
    WriteApplied,
    ResponseCaptured,
    /// Carries the closed commit-observation vocabulary — the only labels that
    /// are behaviorally significant (stable / no-op commit idempotency).
    Committed(CommitObservation),
    Abandoned,
}

/// The closed commit-observation vocabulary — the labels
/// `is_stable_commit_event` / `is_noop_commit_event` recognize. Anything else a
/// caller might have stamped is a diagnostic tag that does not belong on the
/// command plane.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommitObservation {
    Commit,
    CommitSuccess,
    CommitAlreadyCurrent,
}

impl CommitObservation {
    pub fn as_str(self) -> &'static str {
        match self {
            CommitObservation::Commit => "commit",
            CommitObservation::CommitSuccess => "commit_success",
            CommitObservation::CommitAlreadyCurrent => "commit_already_current",
        }
    }
}

/// Payload body for `agent-doc.closeout_advance.v1`. A client asks the controller
/// authority to advance a document's closeout phase machine; the controller
/// decides from its live Lazily projection, emits the phase fact(s) as the
/// durable sink (`#lzdurablesink`), and acknowledges with a terminal
/// [`lazily::CausalReceipt`] — never a transport ACK. This retires the bespoke
/// `closeout_phase_advance` / `closeout_owner_*` controller-socket verbs onto
/// the command plane. lazily treats this body as opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutAdvancePayload {
    /// Canonical path of the document whose closeout phase advances. The
    /// `CommandSubmit` envelope is document-agnostic, so the target document is
    /// part of the domain payload.
    pub document_path: String,
    /// The typed transition (and label). See [`CloseoutPhaseEvent`].
    pub event: CloseoutPhaseEvent,
    /// Present only when `event == Abandoned`. An abandonment reason is
    /// inherently descriptive, so it is a named `reason` field — not a label.
    pub reason: Option<String>,
    pub snapshot_content: Option<String>,
    pub file_content: Option<String>,
    pub response_sha256: Option<String>,
    pub cycle_id_hint: Option<String>,
}

impl CloseoutAdvancePayload {
    /// The legacy `last_event` string to stamp into `CycleState`, derived purely
    /// from the typed event (+ abandon reason). No free-text label crosses the
    /// command boundary.
    pub fn last_event_label(&self) -> String {
        match self.event {
            CloseoutPhaseEvent::WriteApplied => "write_applied".to_string(),
            CloseoutPhaseEvent::ResponseCaptured => "response_captured".to_string(),
            CloseoutPhaseEvent::Committed(obs) => obs.as_str().to_string(),
            CloseoutPhaseEvent::Abandoned => self
                .reason
                .clone()
                .unwrap_or_else(|| "abandoned".to_string()),
        }
    }

    /// Serialize to the inline command payload bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("encode closeout_advance command payload")
    }
}

/// Build the `CommandSubmit` for a closeout phase advance. `command_id` must be
/// stable + replay-safe; `idempotency_key` dedupes concurrent advances for the
/// same document/cycle/event (the caller derives it from the same inputs as the
/// phase-fact event id, so a duplicate fold is exactly the sink's idempotent
/// re-delivery). `authority_generation` is the live closeout-owner generation
/// the command is stamped against; a stale generation is ignored by the
/// authority (the `#lzdurablesink` "no reload at the decision seam" rule).
pub fn build_closeout_advance_submit(
    command_id: impl Into<String>,
    source: impl Into<String>,
    idempotency_key: impl Into<String>,
    authority_generation: u64,
    payload: CloseoutAdvancePayload,
) -> Result<CommandSubmit> {
    let command_id = command_id.into();
    let bytes = payload.encode()?;
    let payload_hash = payload_hash(&bytes);
    Ok(CommandSubmit {
        causation_id: command_id.clone(),
        command_id,
        source: source.into(),
        target: CONTROLLER_TARGET.to_string(),
        namespace: NAMESPACE.to_string(),
        name: CLOSEOUT_ADVANCE_NAME.to_string(),
        authority_generation,
        idempotency_key: idempotency_key.into(),
        // A phase advance has no hard deadline; it settles when the authority
        // folds the transition and emits the terminal receipt.
        deadline_ms: 0,
        policy: CommandPolicy {
            // A duplicate advance for the same document/cycle/event folds onto
            // the same command — the phase machine is idempotent at a fixed
            // generation, mirroring the sink's idempotent re-delivery.
            dedupe: DedupePolicy::SameIdempotencyKey,
            supersede: false,
            cancel_on_preempt: false,
        },
        payload_type: CLOSEOUT_ADVANCE_PAYLOAD_TYPE.to_string(),
        payload_hash,
        payload: IpcValue::Inline(bytes),
        required_features: vec![REQUIRED_FEATURE_RECEIPTS.to_string()],
    })
}
