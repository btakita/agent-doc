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

/// Command name within the namespace for a closeout owner claim.
pub const CLOSEOUT_OWNER_CLAIM_NAME: &str = "closeout_owner_claim";
/// Fully-qualified payload schema id for a closeout owner claim.
pub const CLOSEOUT_OWNER_CLAIM_PAYLOAD_TYPE: &str = "agent-doc.closeout_owner_claim.v1";
/// Command name within the namespace for a closeout owner release.
pub const CLOSEOUT_OWNER_RELEASE_NAME: &str = "closeout_owner_release";
/// Fully-qualified payload schema id for a closeout owner release.
pub const CLOSEOUT_OWNER_RELEASE_PAYLOAD_TYPE: &str = "agent-doc.closeout_owner_release.v1";

/// Payload body for `agent-doc.closeout_owner_claim.v1`. A client asks the
/// controller authority to claim (or refresh) closeout ownership; the controller
/// decides the CAS from its live Lazily projection (`decide_owner_claim`), emits
/// the `CloseoutOwnerClaimed` fact when acquired, and returns the typed
/// [`CloseoutOwnerClaimOutcome`] — the authority result for this coordination
/// op (Acquired / HeldByOther / CycleSuperseded), not a coarse Applied/Rejected
/// receipt. lazily treats this body as opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutOwnerClaimPayload {
    /// Canonical path of the document whose closeout ownership is claimed.
    pub document_path: String,
    /// The claim request (expected cycle, owner id/pid/role, lease, takeover).
    #[serde(flatten)]
    pub request: agent_doc_state_backbone::CloseoutOwnerClaimRequest,
}

/// Payload body for `agent-doc.closeout_owner_release.v1`. A client asks the
/// controller authority to release a held closeout ownership; the controller
/// decides from its live projection whether the caller still owns the cycle,
/// emits `CloseoutOwnerReleased` when it does, and returns a `bool`
/// (released / not-owner). lazily treats this body as opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutOwnerReleasePayload {
    /// Canonical path of the document whose closeout ownership is released.
    pub document_path: String,
    pub cycle_id: String,
    pub owner_id: String,
    pub reason: String,
    pub released_secs: u64,
}

fn build_domain_submit(
    command_id: String,
    name: &str,
    payload_type: &str,
    idempotency_key: String,
    authority_generation: u64,
    bytes: Vec<u8>,
) -> Result<CommandSubmit> {
    let payload_hash = payload_hash(&bytes);
    Ok(CommandSubmit {
        causation_id: command_id.clone(),
        command_id,
        source: "cycle_state".to_string(),
        target: CONTROLLER_TARGET.to_string(),
        namespace: NAMESPACE.to_string(),
        name: name.to_string(),
        authority_generation,
        idempotency_key,
        deadline_ms: 0,
        policy: CommandPolicy {
            // Ownership CAS dedupes by command id; a duplicate claim/release for
            // the same owner+cycle folds onto the same command.
            dedupe: DedupePolicy::SameCommandId,
            supersede: false,
            cancel_on_preempt: false,
        },
        payload_type: payload_type.to_string(),
        payload_hash,
        payload: IpcValue::Inline(bytes),
        required_features: vec![REQUIRED_FEATURE_RECEIPTS.to_string()],
    })
}

/// Build the `CommandSubmit` for a closeout owner claim. `command_id` /
/// `idempotency_key` must be stable + replay-safe (derive from owner+cycle).
pub fn build_closeout_owner_claim_submit(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    authority_generation: u64,
    payload: CloseoutOwnerClaimPayload,
) -> Result<CommandSubmit> {
    let command_id = command_id.into();
    let bytes = serde_json::to_vec(&payload).context("encode closeout_owner_claim payload")?;
    build_domain_submit(
        command_id,
        CLOSEOUT_OWNER_CLAIM_NAME,
        CLOSEOUT_OWNER_CLAIM_PAYLOAD_TYPE,
        idempotency_key.into(),
        authority_generation,
        bytes,
    )
}

/// Build the `CommandSubmit` for a closeout owner release.
pub fn build_closeout_owner_release_submit(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    authority_generation: u64,
    payload: CloseoutOwnerReleasePayload,
) -> Result<CommandSubmit> {
    let command_id = command_id.into();
    let bytes = serde_json::to_vec(&payload).context("encode closeout_owner_release payload")?;
    build_domain_submit(
        command_id,
        CLOSEOUT_OWNER_RELEASE_NAME,
        CLOSEOUT_OWNER_RELEASE_PAYLOAD_TYPE,
        idempotency_key.into(),
        authority_generation,
        bytes,
    )
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

/// Map a legacy free-text commit `event` label to the typed
/// [`CommitObservation`] the command plane carries. The canonical labels map
/// exactly; a non-canonical diagnostic label (e.g. `repair_applied`,
/// `recover_apply`) canonicalizes to [`CommitObservation::CommitSuccess`] — the
/// representative "committed with content changes" outcome — because the command
/// plane carries typed events only (non-canonical labels do not belong on it).
/// The actorless fallback still preserves the caller's free-text label.
pub fn commit_observation_from_event_label(event: &str) -> CommitObservation {
    match event {
        "commit" => CommitObservation::Commit,
        "commit_already_current" => CommitObservation::CommitAlreadyCurrent,
        // "commit_success" and any non-canonical diagnostic label.
        _ => CommitObservation::CommitSuccess,
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
    /// The caller's exact `last_event` label to stamp into `CycleState`. The
    /// typed [`CloseoutPhaseEvent`] drives the phase-machine semantics; this
    /// preserves diagnostic commit labels (e.g. `capture_committed`,
    /// `repair_applied`) that the closed [`CommitObservation`] vocabulary does
    /// not name. When `None`, the label derives from the typed event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_label: Option<String>,
    /// Present only when `event == Abandoned`. An abandonment reason is
    /// inherently descriptive, so it is a named `reason` field — not a label.
    pub reason: Option<String>,
    pub snapshot_content: Option<String>,
    pub file_content: Option<String>,
    pub response_sha256: Option<String>,
    pub cycle_id_hint: Option<String>,
}

impl CloseoutAdvancePayload {
    /// The legacy `last_event` string to stamp into `CycleState`. If the caller
    /// supplied an explicit [`CloseoutAdvancePayload::event_label`], use it
    /// verbatim; otherwise derive it from the typed event (+ abandon reason).
    pub fn last_event_label(&self) -> String {
        if let Some(label) = &self.event_label {
            return label.clone();
        }
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
