//! Pure queue-item claim policy (`#queueclaim`).
//!
//! A queue head reserved for a worker outside the in-session loop — a
//! background subagent under a `#subagents` preset, for example — is
//! **claimed** and therefore not drainable by the in-session loop. A claim is
//! dispatch intent, not evidence that a worker is running. After a successful
//! harness spawn, `queue start` binds that claim generation to the concrete
//! worker identity and advances it to `Running`. Without a claim, the Stop hook saw the dispatched
//! head as the next "drainable head" and forced a `/loop` re-entry while the
//! parent turn was only waiting on its subagents. The re-entry then either
//! churned a no-op cycle or consumed the queue item before the work landed.
//! Observed 2026-10-02 on tasks/agent-doc/agent-doc-bugs.md (`#gh-fix` heads
//! for GH #109/#110/#111 dispatched to subagents).
//!
//! # Identity
//!
//! A claim keys on [`queue_head_identity`], the queue head's **work
//! identity** that dispatch, mid-turn steering, drainability and preflight
//! selection share (`#claimdispatchidentity`): `#id` for id-backed heads,
//! otherwise the normalized text left after lifecycle markers (`🚧`, `⏭️`,
//! pins) and leading intent/preset tags (`#subagents`, `#subagent:`,
//! `#gh-fix`) are stripped. So `🚧 do [#a]`, `#subagents do [#a]` and `[#a]`
//! are one claim, and an operator edit of a claimed
//! `#subagents: <url>` into `#subagents: #gh-fix <url>` keeps its claim
//! instead of re-appearing as new, unclaimed dispatch work.
//!
//! # Line binding (`#claimfollowedit`)
//!
//! The identity names the work, but a claim is bound to the queue LINE that
//! carries it. Each claim stores a [`ClaimAnchor`] (the queue's heads and the
//! claimed line indexes), and [`QueueClaimLedger::follow_edits`] aligns it
//! with the live heads by the edit rule mid-turn steering reports
//! ([`crate::queue_head_alignment`]). Any structurally aligned replacement of
//! a claimed row, from a corrected URL through a complete prose or tracked-id rewrite,
//! carries the claim to the edited line, and steering forwards that edit to
//! the claim's owner. Observed 2026-10-15 on
//! agent-doc-bugs.md: the operator corrected `…/agent-foc/issues/272` to
//! `…/agent-doc/issues/272`; the text-keyed claim matched nothing, was pruned
//! at closeout, and the edited line was offered to a second worker. A claimed
//! line deleted while a similar one is added elsewhere is still distinct and
//! does not inherit the claim. The narrow cross-hunk exception is an id-backed
//! mirror replaced by the exact tracked item prose captured with the claim
//! (`#claimtrackedalias`).
//!
//! # Lifetime
//!
//! A claim ends on whichever comes first:
//!
//! * **closure** — the item is no longer a live head in the queue, even after
//!   following edits of its line (it was answered and struck, reaped, or
//!   deleted by the operator). A claim on an
//!   item that no longer exists is meaningless, so it is ignored on read and
//!   pruned at closeout reconciliation;
//! * **explicit release** — `agent-doc queue release` when the worker reports
//!   back, so the in-session loop can take the item again and close it;
//! * **expiry** — a TTL ([`DEFAULT_QUEUE_CLAIM_TTL_SECS`], overridable per
//!   claim). Agent-doc receives the worker identity at `queue start`, but has
//!   no native heartbeat it can observe after dispatch. Without a TTL a lost
//!   subagent would strand the head forever; with one, the attempt becomes
//!   nonblocking `Lost` while its row remains visibly worked. `queue claim --refresh`
//!   ([`QueueClaimLedger::refresh`]) extends only an already-started attempt,
//!   which lets a long-running owner keep its claim alive; it refuses an
//!   unstarted, expired, foreign, or missing claim rather than promoting or
//!   (re)creating one. Under the
//!   queue-level `subagents` attribute an expired claim returns the head to the
//!   dispatch set, never to inline drainage.
//!
//! # Owner session liveness (`#claimownerliveness`)
//!
//! A claim also records the **coordinator session** that made it
//! ([`ClaimOwnerSession`]: harness session id, tmux pane, harness pid), read
//! from the claiming process's environment. A background subagent has no
//! observable heartbeat, but the coordinator session that dispatched it does.
//! Observed 2026-10-11 on haiven-dev `tasks/sdk.md`: `#cligenport` was claimed
//! by a subagent dispatched before the coordinator `/clear`ed; the subagent
//! finished a different item and never started this one, yet for the rest of
//! the 2h TTL preflight told the post-`/clear` coordinator the work was "in
//! flight elsewhere ... do NOT release", and the coordinator reported a
//! subagent it had no record of as running.
//!
//! [`owner_session_liveness`] judges a claim against what the caller can see:
//!
//! * the recorded harness **pid exited** — the coordinator process is gone;
//! * the **pane's harness session changed** — the caller, or the pane's newest
//!   harness hook, runs a different session id in the claim's pane (`/clear`,
//!   a restart, a resume of another session);
//! * otherwise the owner session is **live**, or **unrecorded** (a claim made
//!   outside a harness, or before this field existed) and judged as before.
//!
//! An ended owner session does NOT release the claim. A subagent spawned
//! before `/clear` can legitimately outlive it and report back (that happened
//! in the same coordinator session that hit the stale claim), so the claim
//! keeps holding the head out of inline drainage until its TTL. What changes is
//! what agent-doc asserts: the claim is reported as `owner_session_ended` —
//! unverifiable, not "in flight" — with the two concrete exits for the new
//! coordinator: for an already-started worker, **adopt** it (`queue claim
//! --refresh` under the same `--owner` rebinds the claim to the caller's
//! session, for a subagent it still hears from); for an unstarted claim, run
//! `queue start` with the worker identity returned by the successful spawn; or
//! **release** it (`queue release`, so the head is dispatched again).
//!
//! This module is pure: no clock, no storage. The caller supplies `now`.

use std::collections::{HashMap, HashSet};

use agent_doc_element_queue::{QueueItemIdentity, queue_head_identity};
use serde::{Deserialize, Serialize};

/// Default claim TTL: two hours. Long enough for a background subagent to fix
/// and verify one issue; short enough that a crashed one strands the queue for
/// a bounded, visible interval rather than indefinitely.
pub const DEFAULT_QUEUE_CLAIM_TTL_SECS: u64 = 2 * 60 * 60;

/// One durable claim on a queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueClaim {
    pub identity: QueueItemIdentity,
    /// The item text as given when claimed, for diagnostics.
    pub item_text: String,
    /// Who holds the claim, e.g. `subagent:gh109`.
    pub owner: String,
    /// Monotonic attempt generation for this queue-item identity. Zero denotes
    /// a legacy pre-generation claim and is accepted only by legacy-unfenced
    /// transitions during rolling upgrades.
    #[serde(default)]
    pub generation: u64,
    /// Typed lifecycle for this exact `(identity, owner, generation)` attempt.
    #[serde(default)]
    pub state: QueueClaimState,
    /// The concrete harness worker that accepted this exact attempt. A claim
    /// is merely dispatching until `queue start` binds this field; labels in
    /// `owner` are coordinator-chosen and are not worker-liveness evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<QueueWorkerBinding>,
    /// Resolved subagent launch policy captured when this attempt was claimed.
    /// `None` preserves the historical inherit-the-coordinator behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_policy: Option<SubagentDispatchPolicy>,
    /// Exact tracked-item prose resolved when an id-backed queue head was
    /// claimed. If queue synchronization later replaces the id directive with
    /// that same prose, the claim follows the alias instead of disappearing
    /// with the retired id (`#claimtrackedalias`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_item_aliases: Vec<String>,
    pub claimed_at_secs: u64,
    pub expires_at_secs: u64,
    /// The coordinator session that made (or last adopted) the claim
    /// (`#claimownerliveness`). `None` for a claim made outside a harness or
    /// written before the field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session: Option<ClaimOwnerSession>,
    /// Where the claimed line sits in the queue (`#claimfollowedit`): the
    /// queue's heads when the claim was made or last followed, and which of
    /// them the claim holds. Modern repeated free-text effects have distinct
    /// visible `⟦effect:N⟧` identities; a legacy unannotated claim can still
    /// hold several identical slots until preflight migrates it. An operator
    /// edit of a claimed line carries the claim to the edited line by aligning
    /// this snapshot with the live heads
    /// (the same edit rule mid-turn steering reports). `None` for a claim
    /// stored before the field existed; it is anchored on the next read that
    /// sees its line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ClaimAnchor>,
    /// Earlier `item_text`s this claim followed from, oldest first
    /// (`#claimfollowedit`), so `release` and `--refresh` by the text the
    /// worker was handed still find the claim after an operator edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub followed_from: Vec<String>,
    /// The attempt generation that last recorded a `followed_from` entry
    /// (`#claimfollowstruck`). A re-claim keeps the line history but starts a
    /// new attempt on the row as it is now, so only this generation's own
    /// follows are evidence against it. `0` for a claim stored before the
    /// field existed: its follows are unattributable and never repaired.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub followed_generation: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Durable audit tombstone for one exact operator-cancelled claim attempt.
///
/// This is deliberately separate from live claims: cancellation never
/// projects lifecycle state onto a queue row. Keeping the full fenced attempt
/// preserves worker/artifact provenance and supplies the generation floor if
/// the operator later re-adds identical text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueClaimCancellationReceipt {
    pub claim: QueueClaim,
    pub cancelled_at_secs: u64,
    /// Exact completion-cell key for a bound worker attempt. A missing key
    /// means the attempt never reached a keyable worker binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_key: Option<String>,
}

impl QueueClaimCancellationReceipt {
    /// Whether this tombstone retires the exact attempt that produced a
    /// reported-release receipt. Identity alone is insufficient because the
    /// operator may later re-add the same work under a fresh generation.
    pub fn cancels_attempt(&self, identity: &QueueItemIdentity, generation: u64) -> bool {
        self.claim.identity == *identity && self.claim.generation == generation
    }

    /// Whether this tombstone retires one exact binary completion cell.
    pub fn cancels_completion_key(&self, key: &str) -> bool {
        self.completion_key.as_deref() == Some(key)
    }
}

/// Optional per-dispatch harness launch overrides recorded for cost attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentDispatchPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Positive acknowledgement that one concrete harness worker accepted one
/// fenced queue attempt (`#queueclaimworker`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueWorkerBinding {
    /// Stable worker/session identity returned by the harness spawn primitive.
    pub worker_id: String,
    /// When the coordinator acknowledged successful dispatch.
    pub started_at_secs: u64,
}

/// Lifecycle of one fenced worker attempt (`#queueclaimstate`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueClaimState {
    /// Claimed/dispatching; no positive worker-liveness evidence yet.
    #[default]
    Claimed,
    /// A concrete harness worker is durably bound to this attempt.
    Running,
    /// The worker reported; integration/verification is still pending.
    Reported,
    /// The attempt expired or was explicitly abandoned. It does not block a
    /// new claim, but remains visible until reclaimed or the queue head closes.
    Lost,
}

/// What the invariant detector can prove about the worker for an attempt.
/// `Unknown` is first-class: process discovery must never invent a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLivenessEvidence {
    /// A generation-fenced worker binding or report receipt exists.
    Proven,
    /// The owner/session is known gone or the attempt expired.
    Absent,
    /// No worker observation is available.
    Unknown,
}

/// Typed real-time queue invariant failures (`#queuedogfood`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueLifecycleInvariant {
    /// The visible prefix disagrees with the durable attempt state. Re-projecting
    /// the expected marker is uniquely safe because prefixes carry no authority.
    ProjectionDrift,
    /// Dispatch was claimed but no generation-fenced start/heartbeat receipt
    /// arrived within the grace period. Worker state remains Unknown.
    DispatchEvidenceMissing,
}

/// How long a newly-created claim may remain unbound while its coordinator is
/// completing the harness spawn boundary. After this grace, `Claimed` is a
/// coordinator obligation rather than evidence that work is in flight.
pub const DISPATCH_EVIDENCE_GRACE_SECS: u64 = 10;

/// Whether this exact attempt has missed its required `queue start`
/// acknowledgement. This predicate is independent of the visible lifecycle
/// marker: projection drift and a missing worker binding can coexist, and the
/// latter must remain visible at preflight/closeout boundaries.
pub fn dispatch_evidence_missing(claim: &QueueClaim, now_secs: u64, grace_secs: u64) -> bool {
    claim.state == QueueClaimState::Claimed
        && claim.worker.is_none()
        && !claim.is_expired(now_secs)
        && now_secs.saturating_sub(claim.claimed_at_secs) >= grace_secs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueInvariantCorrection {
    ProjectExpectedMarker,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueLifecycleDiagnosis {
    pub invariant: QueueLifecycleInvariant,
    pub expected: agent_doc_document::queue_projection::QueueLifecycleMarker,
    pub observed: Option<agent_doc_document::queue_projection::QueueLifecycleMarker>,
    pub worker_liveness: WorkerLivenessEvidence,
    pub correction: QueueInvariantCorrection,
}

/// Diagnose one live queue attempt from authoritative state.
///
/// Formal policy, for attempt `a` and visible row `q`:
///
/// * `Expected(q) = projection(state(a))`.
/// * `Visible(q) != Expected(q) => ProjectionDrift` and projection is safe.
/// * `state(a)=Claimed && age(a)>=grace => DispatchEvidenceMissing`.
/// * Missing worker evidence is `Unknown`; it never authorizes a lifecycle
///   transition. Only a generation-fenced start/heartbeat/report can prove work.
pub fn diagnose_lifecycle(
    claim: &QueueClaim,
    observed: Option<agent_doc_document::queue_projection::QueueLifecycleMarker>,
    now_secs: u64,
    dispatch_grace_secs: u64,
) -> Option<QueueLifecycleDiagnosis> {
    let expected = if claim.state != QueueClaimState::Reported && claim.is_expired(now_secs) {
        agent_doc_document::queue_projection::QueueLifecycleMarker::Lost
    } else {
        claim.state.projection_marker()
    };
    let worker_liveness = match claim.state {
        QueueClaimState::Running | QueueClaimState::Reported => WorkerLivenessEvidence::Proven,
        QueueClaimState::Lost => WorkerLivenessEvidence::Absent,
        QueueClaimState::Claimed if claim.is_expired(now_secs) => WorkerLivenessEvidence::Absent,
        QueueClaimState::Claimed => WorkerLivenessEvidence::Unknown,
    };
    if observed != Some(expected) {
        return Some(QueueLifecycleDiagnosis {
            invariant: QueueLifecycleInvariant::ProjectionDrift,
            expected,
            observed,
            worker_liveness,
            correction: QueueInvariantCorrection::ProjectExpectedMarker,
        });
    }
    if dispatch_evidence_missing(claim, now_secs, dispatch_grace_secs) {
        return Some(QueueLifecycleDiagnosis {
            invariant: QueueLifecycleInvariant::DispatchEvidenceMissing,
            expected,
            observed,
            worker_liveness: WorkerLivenessEvidence::Unknown,
            correction: QueueInvariantCorrection::None,
        });
    }
    None
}

/// `#strikeisterminal`: the one rule for whether durable state may reopen a
/// struck (`~~...~~`) queue row. Every projection that can reopen a struck
/// row (preflight lifecycle markers, the claim command projection, and the
/// relay attempt projection) calls this instead of keeping its own copy.
///
/// `claim` is the row identity's expiry-adjusted claim attempt. A strike is
/// the row's terminal presentation, and finished-work evidence agrees with
/// it: a `Reported` or `Lost` attempt, a reported-release receipt, and a
/// binary completion cell all say the worker is done, and striking reported
/// work is exactly how integration closes it. None of them reopens the row.
/// Only an attempt that is still dispatching or running (`Claimed` /
/// `Running`) contradicts the strike, so only it reopens the row as `❔`.
///
/// Before this rule, receipts and completion cells also reopened struck
/// rows. Neither is retired by every strike path (only closeout `queue
/// consume` strips completion cells, and receipts are pruned at closeout
/// reconciliation), so the preflight residue strike, the orphan-id strike,
/// and the controller's completion projection were each undone by the next
/// projection: the row came back as `❔`, then `☑️` (receipt) or `🔄`
/// (completion cell), and a go-mode queue offered finished work for a new
/// dispatch.
pub fn struck_row_reopened_by_claim(claim: Option<&QueueClaim>) -> bool {
    claim.is_some_and(|claim| claim.state.reopens_struck_row())
}

impl QueueClaimState {
    pub fn blocks_dispatch(self) -> bool {
        matches!(self, Self::Claimed | Self::Running | Self::Reported)
    }

    /// Whether this (expiry-adjusted) attempt state contradicts a strike on
    /// its row. See [`struck_row_reopened_by_claim`].
    pub fn reopens_struck_row(self) -> bool {
        matches!(self, Self::Claimed | Self::Running)
    }

    pub fn projection_marker(self) -> agent_doc_document::queue_projection::QueueLifecycleMarker {
        use agent_doc_document::queue_projection::QueueLifecycleMarker;
        match self {
            Self::Claimed => QueueLifecycleMarker::Claimed,
            Self::Running => QueueLifecycleMarker::Running,
            Self::Reported => QueueLifecycleMarker::Reported,
            Self::Lost => QueueLifecycleMarker::Lost,
        }
    }
}

/// A claim's position in its queue (`#claimfollowedit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimAnchor {
    /// The resolved id of the queue that holds the claimed row
    /// (`#claimfollowstruck`). A claim is aligned only against this queue's
    /// rows, never against a sibling queue the document happens to select.
    /// `None` for an anchor written before the field existed, or by the
    /// single-list [`QueueClaimLedger::follow_edits`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_id: Option<String>,
    /// The queue's live heads, normalized
    /// ([`crate::queue_head_alignment::normalize_queue_head_text`]), in order.
    pub heads: Vec<String>,
    /// Indexes into `heads` of the lines the claim holds: the exact effect line,
    /// plus every line edited from it since the claim was made. A legacy claim
    /// made before effect annotations can temporarily contain every identical
    /// slot so migration deterministically follows the first descendant.
    pub slots: Vec<usize>,
}

/// One queue of a document, as the claim ledger's edit follow reads it
/// (`#claimfollowstruck`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimQueueRows {
    /// The queue's resolved id; `None` for the unnamed single-list view.
    pub queue_id: Option<String>,
    /// Live head rows in queue order, each still carrying its binary-owned
    /// lifecycle and `[#owner#generation]` projection (`#claimrowmigrate`).
    pub rows: Vec<String>,
    /// Work texts of the queue's struck (completed) rows, markers and any
    /// baked-in auto-struck note removed.
    pub struck: Vec<String>,
}

/// Every queue of a document, for [`QueueClaimLedger::follow_edits_in_view`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimQueueView {
    pub queues: Vec<ClaimQueueRows>,
}

impl ClaimQueueView {
    /// One unnamed queue with no struck rows.
    pub fn single(rows: &[String]) -> Self {
        Self {
            queues: vec![ClaimQueueRows {
                queue_id: None,
                rows: rows.to_vec(),
                struck: Vec::new(),
            }],
        }
    }

    /// Every queue component of `content`, by resolved queue id. `None` when
    /// the queue graph does not parse (the same condition under which
    /// [`crate::queue_continuation::live_queue_head_rows`] is `None`).
    pub fn from_content(content: &str) -> Option<Self> {
        use crate::document_queue::{self, QueueEntry};
        let components = agent_doc_element::element::parse(content).ok()?;
        let graph = crate::queue_graph::QueueGraph::parse(content, &components).ok()?;
        let queues = graph
            .blocks()
            .iter()
            .map(|block| ClaimQueueRows {
                queue_id: Some(block.id.clone()),
                rows: document_queue::prompts(&block.entries)
                    .into_iter()
                    .map(|prompt| prompt.text.trim().to_string())
                    .collect(),
                struck: block
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        QueueEntry::Completed(prompt) => {
                            Some(crate::queue_consume::struck_row_work_text(&prompt.text))
                        }
                        _ => None,
                    })
                    .filter(|text| !text.is_empty())
                    .collect(),
            })
            .collect();
        Some(Self { queues })
    }

    /// Every queue's live rows, in document order.
    pub fn rows(&self) -> Vec<String> {
        self.queues
            .iter()
            .flat_map(|queue| queue.rows.iter().cloned())
            .collect()
    }
}

/// What [`QueueClaimLedger::follow_edits_in_view`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimFollowOutcome {
    /// `(previous item_text, new item_text)` per claim that followed an edit.
    pub followed: Vec<(String, String)>,
    /// `(live row text it held, struck predecessor it was restored to)` per
    /// claim that had followed off a struck row (`#claimfollowstruck`).
    pub unfollowed_struck: Vec<(String, String)>,
}

/// How many earlier texts a claim remembers in `followed_from`.
const FOLLOWED_FROM_LIMIT: usize = 8;

impl QueueClaim {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        self.state != QueueClaimState::Reported && now_secs >= self.expires_at_secs
    }

    pub fn blocks_dispatch(&self, now_secs: u64) -> bool {
        self.state.blocks_dispatch() && !self.is_expired(now_secs)
    }
}

/// The harness session a claim was made from (`#claimownerliveness`). Every
/// field is optional: each one only adds a liveness signal when present.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimOwnerSession {
    /// Which harness `session_id` came from (`claude`, `codex`). Session ids
    /// are only compared within one harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// The harness session id (`CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`).
    /// A subagent inherits its coordinator's, so a claim a subagent makes is
    /// still bound to the coordinator session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The tmux pane the harness runs in (`TMUX_PANE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    /// The harness process (`CLAUDE_PID`). It survives `/clear`; it does not
    /// survive the harness exiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

impl ClaimOwnerSession {
    /// `None` when no field carries a value, so a claim made outside any
    /// harness stays unrecorded instead of looking recorded-but-empty.
    pub fn non_empty(self) -> Option<Self> {
        (self.session_id.is_some() || self.pane.is_some() || self.pid.is_some()).then_some(self)
    }
}

/// What the caller can observe about a claim's owner session.
#[derive(Debug, Clone, Copy, Default)]
pub struct OwnerSessionObservation<'a> {
    /// The calling process's own harness session (the coordinator, or one of
    /// its subagents).
    pub caller: Option<&'a ClaimOwnerSession>,
    /// Whether the recorded owner pid is alive; `None` when no pid is recorded
    /// or it could not be probed.
    pub owner_pid_alive: Option<bool>,
    /// The session id the claim's pane's harness most recently reported
    /// through a turn hook, with the time it was observed; `None` when unknown.
    pub pane_session: Option<(&'a str, u64)>,
}

/// Why a claim's owner session is known to have ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerSessionEndReason {
    /// The recorded harness process exited.
    OwnerProcessExited,
    /// The claim's pane now runs a different harness session (`/clear`,
    /// restart, or another session resumed there).
    SessionReplaced,
}

impl OwnerSessionEndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnerProcessExited => "owner_process_exited",
            Self::SessionReplaced => "session_replaced",
        }
    }

    /// Operator-facing phrase.
    pub fn describe(self) -> &'static str {
        match self {
            Self::OwnerProcessExited => "the claiming coordinator process exited",
            Self::SessionReplaced => {
                "the claiming coordinator session was replaced in its pane (`/clear` or restart)"
            }
        }
    }
}

/// Liveness of a claim's owner session (`#claimownerliveness`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOwnerLiveness {
    /// No owner session was recorded; judge the claim by expiry/closure only.
    Unrecorded,
    /// Nothing the caller observes contradicts the owner session being alive.
    Live,
    /// The owner session is gone. The claim still holds the head until its
    /// TTL, but nothing proves its work is in flight.
    Ended(OwnerSessionEndReason),
}

/// Judge `claim`'s owner session against `observation`. Pure.
pub fn owner_session_liveness(
    claim: &QueueClaim,
    observation: OwnerSessionObservation<'_>,
) -> ClaimOwnerLiveness {
    let Some(owner) = claim.owner_session.as_ref() else {
        return ClaimOwnerLiveness::Unrecorded;
    };
    if observation.owner_pid_alive == Some(false) {
        return ClaimOwnerLiveness::Ended(OwnerSessionEndReason::OwnerProcessExited);
    }
    if let (Some(owner_pane), Some(owner_session)) =
        (owner.pane.as_deref(), owner.session_id.as_deref())
    {
        // The caller sits in the claim's pane under another session of the same
        // harness: one harness runs per pane, so the owner session is gone.
        if let Some(caller) = observation.caller
            && caller.pane.as_deref() == Some(owner_pane)
            && caller.harness == owner.harness
            && caller
                .session_id
                .as_deref()
                .is_some_and(|session| session != owner_session)
        {
            return ClaimOwnerLiveness::Ended(OwnerSessionEndReason::SessionReplaced);
        }
        // The pane's harness reported a different session after the claim was
        // made. Only Claude hook payloads are known to carry the same id as
        // `CLAUDE_CODE_SESSION_ID`, so other harnesses skip this signal.
        if owner.harness.as_deref() == Some("claude")
            && let Some((pane_session, observed_at)) = observation.pane_session
            && observed_at >= claim.claimed_at_secs
            && pane_session != owner_session
        {
            return ClaimOwnerLiveness::Ended(OwnerSessionEndReason::SessionReplaced);
        }
    }
    ClaimOwnerLiveness::Live
}

/// The document's claim ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueClaimLedger {
    #[serde(default)]
    pub claims: Vec<QueueClaim>,
    /// Exact attempts explicitly cancelled by operator row removal. Receipts
    /// are audit/generation evidence only and never count as live claims.
    #[serde(default)]
    pub cancellations: Vec<QueueClaimCancellationReceipt>,
}

/// Why [`QueueClaimLedger::prune_detailed`] dropped a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimPruneCause {
    /// Its TTL ran out.
    Expired,
    /// No live head carries its line any more (answered, reaped, or deleted),
    /// after following any operator edit of the line.
    Closed,
}

impl ClaimPruneCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expired => "expired",
            Self::Closed => "closed",
        }
    }
}

/// Outcome of [`QueueClaimLedger::claim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOutcome {
    Created,
    /// The same owner re-claimed the item; its TTL was refreshed.
    Refreshed,
    /// A lost/expired attempt was replaced by a new generation.
    Reassigned,
    /// A different owner still holds a live attempt. No mutation occurred.
    HeldByOther,
}

/// A stale or invalid attempt-scoped lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueClaimTransitionRefused {
    Missing,
    MissingGeneration {
        current: u64,
    },
    StaleGeneration {
        expected: u64,
        current: u64,
    },
    NotRunning {
        generation: u64,
    },
    Terminal {
        generation: u64,
        state: QueueClaimState,
    },
}

impl std::fmt::Display for QueueClaimTransitionRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "queue claim attempt is missing"),
            Self::MissingGeneration { current } => write!(
                f,
                "queue claim generation is required (current generation {current})"
            ),
            Self::StaleGeneration { expected, current } => write!(
                f,
                "stale queue claim generation {expected}; current generation is {current}"
            ),
            Self::NotRunning { generation } => write!(
                f,
                "queue claim generation {generation} has no acknowledged worker; run `queue start` after a successful dispatch"
            ),
            Self::Terminal { generation, state } => write!(
                f,
                "queue claim generation {generation} is terminal ({state:?})"
            ),
        }
    }
}

impl std::error::Error for QueueClaimTransitionRefused {}

/// Why a concrete worker could not be bound to a claim attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueClaimStartRefused {
    Missing,
    EmptyWorker,
    Expired {
        holder: String,
        expired_at_secs: u64,
    },
    OtherOwner {
        holder: String,
    },
    StaleAttempt {
        detail: String,
    },
    Terminal {
        generation: u64,
        state: QueueClaimState,
    },
    AttemptAlreadyBound {
        worker_id: String,
    },
    WorkerAlreadyBound {
        worker_id: String,
        item_text: String,
        generation: u64,
    },
}

impl QueueClaimStartRefused {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::EmptyWorker => "empty_worker",
            Self::Expired { .. } => "expired",
            Self::OtherOwner { .. } => "other_owner",
            Self::StaleAttempt { .. } => "stale_attempt",
            Self::Terminal { .. } => "terminal",
            Self::AttemptAlreadyBound { .. } => "attempt_already_bound",
            Self::WorkerAlreadyBound { .. } => "worker_already_bound",
        }
    }
}

impl std::fmt::Display for QueueClaimStartRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "queue claim attempt is missing"),
            Self::EmptyWorker => write!(f, "queue worker identity must not be empty"),
            Self::Expired {
                holder,
                expired_at_secs,
            } => write!(
                f,
                "queue claim held by {holder} expired at {expired_at_secs}"
            ),
            Self::OtherOwner { holder } => {
                write!(f, "queue claim is held by another owner ({holder})")
            }
            Self::StaleAttempt { detail } => write!(f, "{detail}"),
            Self::Terminal { generation, state } => write!(
                f,
                "queue claim generation {generation} is terminal ({state:?})"
            ),
            Self::AttemptAlreadyBound { worker_id } => write!(
                f,
                "queue claim attempt is already bound to worker {worker_id:?}"
            ),
            Self::WorkerAlreadyBound {
                worker_id,
                item_text,
                generation,
            } => write!(
                f,
                "worker {worker_id:?} is already bound to queue item {item_text:?} generation {generation}"
            ),
        }
    }
}

impl std::error::Error for QueueClaimStartRefused {}

fn validate_generation(
    claim: &QueueClaim,
    expected_generation: Option<u64>,
) -> Result<(), QueueClaimTransitionRefused> {
    match expected_generation {
        Some(expected) if expected == claim.generation => Ok(()),
        Some(expected) => Err(QueueClaimTransitionRefused::StaleGeneration {
            expected,
            current: claim.generation,
        }),
        None if claim.generation == 0 => Ok(()),
        None => Err(QueueClaimTransitionRefused::MissingGeneration {
            current: claim.generation,
        }),
    }
}

/// `queue_document_state.state_kind` of the per-document claim ledger row.
pub const QUEUE_CLAIMS_STATE_KIND: &str = "queue_claims";

impl QueueClaimLedger {
    /// Decode a stored ledger payload and re-key it under the current identity
    /// rule. Shared by every reader of the `queue_claims` state row so the
    /// controller and the one-shot CLI boundaries judge claims identically.
    pub fn from_state_payload(payload_json: &str) -> anyhow::Result<Self> {
        let mut ledger: QueueClaimLedger = serde_json::from_str(payload_json)
            .map_err(|err| anyhow::anyhow!("parse queue claim ledger: {err}"))?;
        // Legacy rows predate generation/state. Keep generation zero so an
        // in-flight rolling-upgrade transition can still identify that one
        // legacy attempt; the next actual re-claim advances it to generation 1.
        // `#claimdispatchidentity`: claims stored under an older identity rule
        // are compared under the current one.
        ledger.rekey();
        // A pre-`queue start` binary used `--refresh` as implicit running
        // evidence and therefore persisted no concrete worker binding. Do not
        // keep projecting that ambiguous state as 🚧: fail closed to 🟡 until
        // the coordinator binds the worker explicitly. Conversely, a durable
        // binding is sufficient evidence to repair a stale Claimed label.
        for claim in &mut ledger.claims {
            match (claim.state, claim.worker.is_some()) {
                (QueueClaimState::Running, false) => claim.state = QueueClaimState::Claimed,
                (QueueClaimState::Claimed, true) => claim.state = QueueClaimState::Running,
                _ => {}
            }
        }
        // Within one document queue the worker binding is injective. Refuse a
        // corrupted/stale projection rather than choosing which attempt owns a
        // worker and advancing both.
        let mut workers: HashMap<&str, (&str, u64)> = HashMap::new();
        for claim in &ledger.claims {
            let Some(binding) = claim.worker.as_ref() else {
                continue;
            };
            if claim.state == QueueClaimState::Lost {
                continue;
            }
            if let Some((item_text, generation)) =
                workers.insert(&binding.worker_id, (&claim.item_text, claim.generation))
            {
                anyhow::bail!(
                    "queue worker {:?} is bound to both {:?} generation {} and {:?} generation {}",
                    binding.worker_id,
                    item_text,
                    generation,
                    claim.item_text,
                    claim.generation,
                );
            }
        }
        Ok(ledger)
    }

    /// Live queue-head texts of `content` that an active claim holds
    /// (`#deferstrike`). The ledger first follows note edits against the live
    /// heads in memory, exactly as the CLI readers do, so a head the operator
    /// annotated keeps its claim.
    pub fn claimed_live_head_texts(&self, now_secs: u64, content: &str) -> Vec<String> {
        if self.claims.is_empty() {
            return Vec::new();
        }
        let Some(heads) = crate::queue_continuation::live_queue_head_texts(content) else {
            return Vec::new();
        };
        let mut ledger = self.clone();
        if let Some(view) = ClaimQueueView::from_content(content) {
            ledger.follow_edits_in_view(now_secs, &view);
        }
        let live = crate::queue_continuation::live_queue_head_identities(content);
        let claimed = ledger.claimed_items(now_secs, live.as_ref());
        heads
            .into_iter()
            .filter(|head| claimed.claims(head))
            .collect()
    }

    /// Re-derive every stored identity from its `item_text` with the current
    /// [`claim_identity`], merging claims that now share one identity (the
    /// later expiry wins). A ledger written before the identity rule changed
    /// (tag-sensitive free-text keys) would otherwise hold claims that match
    /// no live head and silently drop. Returns whether anything changed.
    pub fn rekey(&mut self) -> bool {
        let mut changed = false;
        let mut merged: Vec<QueueClaim> = Vec::with_capacity(self.claims.len());
        for mut claim in std::mem::take(&mut self.claims) {
            let identity = claim_identity(&claim.item_text);
            if identity != claim.identity {
                claim.identity = identity;
                changed = true;
            }
            match merged
                .iter_mut()
                .find(|kept| kept.identity == claim.identity)
            {
                Some(kept) => {
                    changed = true;
                    if claim.expires_at_secs > kept.expires_at_secs {
                        *kept = claim;
                    }
                }
                None => merged.push(claim),
            }
        }
        self.claims = merged;
        for receipt in &mut self.cancellations {
            let identity = claim_identity(&receipt.claim.item_text);
            if identity != receipt.claim.identity {
                receipt.claim.identity = identity;
                changed = true;
            }
        }
        changed
    }

    fn cancellation_generation_floor(&self, identity: &QueueItemIdentity) -> u64 {
        self.cancellations
            .iter()
            .filter(|receipt| &receipt.claim.identity == identity)
            .map(|receipt| receipt.claim.generation)
            .max()
            .unwrap_or(0)
    }

    /// Claim `item` for `owner` until `now_secs + ttl_secs`.
    pub fn claim(&mut self, item: &str, owner: &str, now_secs: u64, ttl_secs: u64) -> ClaimOutcome {
        self.claim_in_session(item, owner, now_secs, ttl_secs, None)
    }

    /// [`Self::claim`] bound to the claiming coordinator session
    /// (`#claimownerliveness`).
    pub fn claim_in_session(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
        owner_session: Option<ClaimOwnerSession>,
    ) -> ClaimOutcome {
        self.claim_in_session_with_generation_floor(
            item,
            owner,
            now_secs,
            ttl_secs,
            owner_session,
            0,
        )
    }

    /// [`Self::claim_in_session`] with a durable generation floor supplied by
    /// an external attempt ledger. This keeps a struck/removed/re-added row
    /// from reusing an old generation after its live claim has been pruned.
    pub fn claim_in_session_with_generation_floor(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
        owner_session: Option<ClaimOwnerSession>,
        generation_floor: u64,
    ) -> ClaimOutcome {
        let identity = claim_identity(item);
        let generation_floor = generation_floor.max(self.cancellation_generation_floor(&identity));
        let expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
        let mut fresh = QueueClaim {
            identity: identity.clone(),
            item_text: item.trim().to_string(),
            owner: owner.trim().to_string(),
            generation: 1,
            state: QueueClaimState::Claimed,
            worker: None,
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: now_secs,
            expires_at_secs,
            owner_session,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
        };
        match self
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
        {
            Some(existing)
                if existing.owner == fresh.owner && existing.blocks_dispatch(now_secs) =>
            {
                // Idempotent same-owner claim. It is the same worker attempt;
                // only explicit `--refresh` supplies running evidence.
                existing.expires_at_secs = expires_at_secs;
                if fresh.owner_session.is_some() {
                    existing.owner_session = fresh.owner_session;
                }
                ClaimOutcome::Refreshed
            }
            Some(existing) if existing.blocks_dispatch(now_secs) => {
                // Never steal a live attempt. Callers turn this typed outcome
                // into a usage error naming the current generation/owner.
                ClaimOutcome::HeldByOther
            }
            Some(existing) => {
                let outcome = ClaimOutcome::Reassigned;
                fresh.generation = existing
                    .generation
                    .max(generation_floor)
                    .saturating_add(1)
                    .max(1);
                // Re-claim after lost/expired handling keeps line anchoring but
                // starts a new fenced attempt.
                fresh.anchor = existing.anchor.take();
                fresh.followed_from = std::mem::take(&mut existing.followed_from);
                *existing = fresh;
                outcome
            }
            None => {
                fresh.generation = generation_floor.saturating_add(1).max(1);
                self.claims.push(fresh);
                ClaimOutcome::Created
            }
        }
    }

    /// Tombstone one exact claim generation after authoritative operator
    /// removal. Repeating the same cancellation is idempotent; a stale
    /// generation can never cancel a replacement attempt.
    pub fn cancel_attempt(
        &mut self,
        item: &str,
        expected_generation: Option<u64>,
        cancelled_at_secs: u64,
    ) -> Result<QueueClaimCancellationReceipt, QueueClaimTransitionRefused> {
        let identity = claim_identity(item);
        if let Some(existing) = self.cancellations.iter().find(|receipt| {
            receipt.claim.identity == identity
                && expected_generation == Some(receipt.claim.generation)
        }) {
            return Ok(existing.clone());
        }
        let Some(index) = self
            .claims
            .iter()
            .position(|claim| claim.identity == identity)
        else {
            return Err(QueueClaimTransitionRefused::Missing);
        };
        validate_generation(&self.claims[index], expected_generation)?;
        let claim = self.claims.remove(index);
        let completion_key = claim.worker.as_ref().and_then(|worker| {
            completion_attestation_key(
                &claim.identity,
                &claim.owner,
                claim.generation,
                claim.claimed_at_secs,
                &worker.worker_id,
            )
            .ok()
        });
        Ok(self.record_cancellation(claim, cancelled_at_secs, completion_key))
    }

    /// Record cancellation for an exact attempt reconstructed from a durable
    /// reported-release receipt after the live claim was already removed.
    pub fn record_cancellation(
        &mut self,
        claim: QueueClaim,
        cancelled_at_secs: u64,
        completion_key: Option<String>,
    ) -> QueueClaimCancellationReceipt {
        if let Some(existing) = self.cancellations.iter().find(|receipt| {
            receipt.claim.identity == claim.identity && receipt.claim.generation == claim.generation
        }) {
            return existing.clone();
        }
        let receipt = QueueClaimCancellationReceipt {
            claim,
            cancelled_at_secs,
            completion_key,
        };
        self.cancellations.push(receipt.clone());
        receipt
    }

    /// Bind one concrete harness worker to one exact live claim generation.
    ///
    /// Formal contract for live attempts `a` and workers `w`:
    ///
    /// * `Running(a) => exists exactly one w: worker(a) = Some(w)`; only this
    ///   transition can create `Running`, and loading downgrades an unbound
    ///   legacy `Running` row to `Claimed`;
    /// * `worker(a) = worker(b) => a = b` (injective binding);
    /// * only `Claimed -> Running` is permitted, apart from an idempotent
    ///   replay carrying the same `(owner, generation, worker)` tuple;
    /// * a stale generation can never acknowledge a replacement attempt.
    pub fn start(
        &mut self,
        item: &str,
        owner: &str,
        worker_id: &str,
        now_secs: u64,
        ttl_secs: u64,
        expected_generation: Option<u64>,
    ) -> Result<QueueClaim, QueueClaimStartRefused> {
        let worker_id = worker_id.trim();
        if worker_id.is_empty() {
            return Err(QueueClaimStartRefused::EmptyWorker);
        }
        let identity = claim_identity(item);
        let Some(index) = self
            .claims
            .iter()
            .position(|claim| claim.identity == identity)
        else {
            return Err(QueueClaimStartRefused::Missing);
        };
        let claim = &self.claims[index];
        if claim.is_expired(now_secs) || claim.state == QueueClaimState::Lost {
            return Err(QueueClaimStartRefused::Expired {
                holder: claim.owner.clone(),
                expired_at_secs: claim.expires_at_secs,
            });
        }
        if claim.owner != owner.trim() {
            return Err(QueueClaimStartRefused::OtherOwner {
                holder: claim.owner.clone(),
            });
        }
        validate_generation(claim, expected_generation).map_err(|reason| {
            QueueClaimStartRefused::StaleAttempt {
                detail: reason.to_string(),
            }
        })?;
        if matches!(
            claim.state,
            QueueClaimState::Reported | QueueClaimState::Lost
        ) {
            return Err(QueueClaimStartRefused::Terminal {
                generation: claim.generation,
                state: claim.state,
            });
        }
        if let Some(binding) = &claim.worker {
            if binding.worker_id == worker_id {
                return Ok(claim.clone());
            }
            return Err(QueueClaimStartRefused::AttemptAlreadyBound {
                worker_id: binding.worker_id.clone(),
            });
        }
        if let Some(other) = self
            .claims
            .iter()
            .enumerate()
            .find_map(|(other_index, other)| {
                (other_index != index
                    && other.blocks_dispatch(now_secs)
                    && other
                        .worker
                        .as_ref()
                        .is_some_and(|binding| binding.worker_id == worker_id))
                .then_some(other)
            })
        {
            return Err(QueueClaimStartRefused::WorkerAlreadyBound {
                worker_id: worker_id.to_string(),
                item_text: other.item_text.clone(),
                generation: other.generation,
            });
        }
        let claim = &mut self.claims[index];
        claim.worker = Some(QueueWorkerBinding {
            worker_id: worker_id.to_string(),
            started_at_secs: now_secs,
        });
        claim.state = QueueClaimState::Running;
        claim.expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
        Ok(claim.clone())
    }

    /// Extend the TTL of `owner`'s live claim on `item` to `now_secs + ttl_secs`
    /// (`queue claim --refresh`, the coordinator's heartbeat for a long-running
    /// subagent). Unlike [`Self::claim`], it never creates or takes over a
    /// claim: an unclaimed item, an expired claim, or another owner's claim is
    /// refused, so a heartbeat that arrives after the claim lapsed cannot
    /// silently re-claim a head another worker may already have taken.
    /// `claimed_at_secs` is kept; only `expires_at_secs` moves.
    pub fn refresh(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Result<QueueClaim, QueueClaimRefreshRefused> {
        self.refresh_in_session(item, owner, now_secs, ttl_secs, None)
    }

    /// [`Self::refresh`] that also **adopts** the claim into `owner_session`
    /// (`#claimownerliveness`): a coordinator that replaced the claiming
    /// session (`/clear`) and still hears from the subagent re-binds the claim
    /// to itself, so it reads as live again. `None` keeps the recorded session.
    pub fn refresh_in_session(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
        owner_session: Option<ClaimOwnerSession>,
    ) -> Result<QueueClaim, QueueClaimRefreshRefused> {
        self.refresh_fenced_in_session(item, owner, now_secs, ttl_secs, owner_session, None)
    }

    /// Attempt-fenced refresh used by command boundaries. Generated claims
    /// require the exact generation; generation-zero legacy rows may omit it.
    pub fn refresh_attempt_in_session(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
        owner_session: Option<ClaimOwnerSession>,
        expected_generation: Option<u64>,
    ) -> Result<QueueClaim, QueueClaimRefreshRefused> {
        self.refresh_fenced_in_session(
            item,
            owner,
            now_secs,
            ttl_secs,
            owner_session,
            Some(expected_generation),
        )
    }

    fn refresh_fenced_in_session(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
        owner_session: Option<ClaimOwnerSession>,
        generation_fence: Option<Option<u64>>,
    ) -> Result<QueueClaim, QueueClaimRefreshRefused> {
        let identity = claim_identity(item);
        let owner = owner.trim();
        let refused = |reason| QueueClaimRefreshRefused {
            item: item.trim().to_string(),
            owner: owner.to_string(),
            reason,
        };
        let Some(existing) = self
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
        else {
            return Err(refused(RefreshRefusal::Unclaimed));
        };
        if existing.is_expired(now_secs) || existing.state == QueueClaimState::Lost {
            return Err(refused(RefreshRefusal::Expired {
                holder: existing.owner.clone(),
                expired_at_secs: existing.expires_at_secs,
            }));
        }
        if existing.owner != owner {
            return Err(refused(RefreshRefusal::OtherOwner {
                holder: existing.owner.clone(),
            }));
        }
        if let Some(expected_generation) = generation_fence
            && let Err(reason) = validate_generation(existing, expected_generation)
        {
            return Err(refused(RefreshRefusal::StaleAttempt {
                detail: reason.to_string(),
            }));
        }
        if existing.state == QueueClaimState::Claimed || existing.worker.is_none() {
            return Err(refused(RefreshRefusal::NotRunning));
        }
        existing.expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
        if owner_session.is_some() {
            existing.owner_session = owner_session;
        }
        Ok(existing.clone())
    }

    /// Mark the matching attempt reported. New generated claims require their
    /// generation; legacy generation-zero claims accept an omitted fence.
    pub fn report(
        &mut self,
        item: &str,
        expected_generation: Option<u64>,
    ) -> Result<QueueClaim, QueueClaimTransitionRefused> {
        let identity = claim_identity(item);
        let Some(claim) = self
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
        else {
            return Err(QueueClaimTransitionRefused::Missing);
        };
        validate_generation(claim, expected_generation)?;
        if claim.state == QueueClaimState::Lost {
            return Err(QueueClaimTransitionRefused::Terminal {
                generation: claim.generation,
                state: claim.state,
            });
        }
        if claim.state == QueueClaimState::Claimed || claim.worker.is_none() {
            return Err(QueueClaimTransitionRefused::NotRunning {
                generation: claim.generation,
            });
        }
        claim.state = QueueClaimState::Reported;
        Ok(claim.clone())
    }

    /// Release the claim on `item`. Returns the released claim, if any.
    pub fn release(&mut self, item: &str) -> Option<QueueClaim> {
        self.release_attempt(item, None, false).ok().flatten()
    }

    pub fn release_fenced(
        &mut self,
        item: &str,
        expected_generation: Option<u64>,
    ) -> Result<Option<QueueClaim>, QueueClaimTransitionRefused> {
        self.release_attempt(item, expected_generation, true)
    }

    fn release_attempt(
        &mut self,
        item: &str,
        expected_generation: Option<u64>,
        enforce_generation: bool,
    ) -> Result<Option<QueueClaim>, QueueClaimTransitionRefused> {
        let identity = claim_identity(item);
        let index = match self
            .claims
            .iter()
            .position(|claim| claim.identity == identity)
        {
            Some(index) => index,
            // `#claimfollowedit`: released by the text the worker was given,
            // after the claim followed an operator edit of its line.
            None => match self.followed_claim_index(item) {
                Some(index) => index,
                // `#claimfollowsedit`: an annotation followed by a claim
                // stored before `followed_from` existed.
                None => {
                    let continuing: Vec<usize> = self
                        .claims
                        .iter()
                        .enumerate()
                        .filter(|(_, claim)| is_edit_continuation(&identity, &claim.identity))
                        .map(|(index, _)| index)
                        .collect();
                    match continuing.as_slice() {
                        [only] => *only,
                        _ => return Ok(None),
                    }
                }
            },
        };
        if enforce_generation {
            validate_generation(&self.claims[index], expected_generation)?;
        }
        Ok(Some(self.claims.remove(index)))
    }

    /// The one claim that followed an operator edit away from `item`
    /// (`#claimfollowedit`): its `followed_from` holds a text with `item`'s
    /// identity. `None` when no claim, or more than one, did.
    fn followed_claim_index(&self, item: &str) -> Option<usize> {
        let identity = claim_identity(item);
        let followed: Vec<usize> = self
            .claims
            .iter()
            .enumerate()
            .filter(|(_, claim)| {
                claim
                    .followed_from
                    .iter()
                    .any(|text| claim_identity(text) == identity)
            })
            .map(|(index, _)| index)
            .collect();
        match followed.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The current text of the one claim that followed an operator edit away
    /// from `item` (`#claimfollowedit`), so `claim`, `--refresh` and `release`
    /// by the text a worker was handed resolve to the line it became.
    pub fn followed_item_text(&self, item: &str) -> Option<&str> {
        self.followed_claim_index(item)
            .map(|index| self.claims[index].item_text.as_str())
    }

    /// Claims that are neither expired nor closed. `live_heads` is the set of
    /// identities still present as live queue heads; `None` means the caller
    /// could not read the queue, in which case closure cannot be judged and
    /// only expiry applies.
    pub fn active(
        &self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> Vec<&QueueClaim> {
        self.claims
            .iter()
            .filter(|claim| claim.blocks_dispatch(now_secs))
            .filter(|claim| live_heads.is_none_or(|live| live.contains(&claim.identity)))
            .collect()
    }

    /// Carry claims across operator edits of their lines (`#claimfollowedit`,
    /// `#claimfollowsedit`) and re-anchor every live claim on `live_heads`.
    ///
    /// A claim is bound to its LINE, not to the line's text. Each claim's
    /// [`ClaimAnchor`] is aligned with `live_heads` by the rule mid-turn
    /// steering uses to report `previous` -> `verbatim` edits
    /// ([`crate::queue_head_alignment::pair_queue_heads`]): an unchanged or
    /// moved line pairs by identity, and an equal-cardinality replace hunk pairs
    /// still-unmatched rows at the same structural offset even when their text
    /// or `#id` changes completely. A line deleted in one place and added in
    /// another does not pair. A live (unexpired) claim
    /// whose identity is no longer a live head follows the line its anchored
    /// line(s) became, so `#gh-fix …/agent-foc/issues/272` edited into
    /// `#gh-fix …/agent-doc/issues/272` stays claimed by the same owner and is
    /// never re-offered as new dispatch work.
    ///
    /// A located claim is bound to its ROW, never to whichever row shares its
    /// text (`#claimrowmigrate`). Once its anchor locates its line(s), or
    /// exactly one candidate row carries this attempt's visible
    /// `[#owner#generation]` projection, a text-identical sibling added later
    /// never joins its slots, and an `⟦effect:N⟧` annotation of the queue
    /// moves the claim to the annotated successor of ITS row, not to the first
    /// annotated copy. `live_heads` may therefore carry the lifecycle
    /// projection: the work text is stripped here and the owner annotation is
    /// kept only as row evidence. Only an unlocated claim (no anchor yet, or
    /// no anchored line survives) holds every line with its identity; when
    /// such legacy copies are later edited apart it follows the first in queue
    /// order rather than being dropped. A descendant whose identity another
    /// live claim holds is never taken over. A claim with no anchor (stored
    /// before anchors existed) falls back to the word-boundary annotation rule
    /// ([`is_edit_continuation`]).
    ///
    /// The followed claim's `item_text` becomes the line's text (so the
    /// on-load [`Self::rekey`] keeps it) and its previous text is remembered
    /// in `followed_from`. Returns `(previous item_text, new item_text)` per
    /// followed claim.
    ///
    /// This single-list form treats `live_heads` as one unnamed queue with no
    /// struck rows; document readers use [`Self::follow_edits_in_view`], which
    /// keeps every claim inside its own queue and never carries a claim off a
    /// struck row (`#claimfollowstruck`).
    pub fn follow_edits(&mut self, now_secs: u64, live_heads: &[String]) -> Vec<(String, String)> {
        self.follow_edits_in_view(now_secs, &ClaimQueueView::single(live_heads))
            .followed
    }

    /// [`Self::follow_edits`] over every queue of a document
    /// (`#claimfollowstruck`).
    ///
    /// A claim is bound to a row of ONE queue. Each claim is aligned only
    /// against the queue that holds it: the queue where its identity is a live
    /// row, else the queue its anchor recorded ([`ClaimAnchor::queue_id`]),
    /// else (a legacy anchor without a queue id) the one queue its anchored
    /// heads or its struck row identify. A claim no queue identifies is not in
    /// view and keeps its anchor. Before this, every reader aligned the anchor
    /// with the document's *selected* queue, so once a strike drained
    /// `agent:run` the selection moved to `agent:loop` and the claim's one-row
    /// anchor paired structurally with the loop's one row; when new `agent:run`
    /// heads arrived the claim jumped again (observed 2026-10-27,
    /// `subagent:kotlinflush` gen 1 on `#gh-fix …/issues/380`).
    ///
    /// A struck row is completed work, never an edit: a claim whose identity
    /// is a struck row of any queue and no live row never follows a successor
    /// head, however the structural alignment pairs them. Its row closed; the
    /// existing strike policy then applies ([`Self::retire_struck`] ends a
    /// finished attempt, a still-running one reopens its struck row as `❔`).
    ///
    /// Durable state that already followed off a struck row is surfaced, not
    /// kept: a claim sitting on a live row whose `followed_from` names a text
    /// that is now a struck row (and no live row) is restored onto that struck
    /// predecessor, so the unrelated live row stops carrying its marker and
    /// the strike policy above judges the attempt.
    pub fn follow_edits_in_view(
        &mut self,
        now_secs: u64,
        view: &ClaimQueueView,
    ) -> ClaimFollowOutcome {
        let queue_live: Vec<Vec<QueueItemIdentity>> = view
            .queues
            .iter()
            .map(|queue| {
                queue
                    .rows
                    .iter()
                    .map(|row| {
                        claim_identity(
                            &agent_doc_document::queue_projection::strip_in_progress_marker(
                                row.trim(),
                            ),
                        )
                    })
                    .collect()
            })
            .collect();
        let queue_struck: Vec<HashSet<QueueItemIdentity>> = view
            .queues
            .iter()
            .map(|queue| {
                queue
                    .struck
                    .iter()
                    .map(|text| claim_identity(text))
                    .collect()
            })
            .collect();
        let all_live: HashSet<&QueueItemIdentity> = queue_live.iter().flatten().collect();
        let closed_by_strike = |identity: &QueueItemIdentity| {
            !all_live.contains(identity) && queue_struck.iter().any(|set| set.contains(identity))
        };

        let mut outcome = ClaimFollowOutcome::default();
        for claim in &mut self.claims {
            // Only this attempt's own follows are evidence against it: a
            // re-claim (`Reassigned`) inherits the line history but was made on
            // the row as it is, and a legacy follow is unattributable.
            if !all_live.contains(&claim.identity)
                || claim.followed_generation == 0
                || claim.followed_generation != claim.generation
            {
                continue;
            }
            let Some(position) = claim
                .followed_from
                .iter()
                .rposition(|text| closed_by_strike(&claim_identity(text)))
            else {
                continue;
            };
            let restored = claim.followed_from[position].clone();
            let current = std::mem::replace(&mut claim.item_text, restored.clone());
            claim.identity = claim_identity(&restored);
            claim.followed_from.truncate(position);
            claim.anchor = None;
            outcome.unfollowed_struck.push((current, restored));
        }

        // Which queue each claim belongs to (`None`: not in view).
        let assigned: Vec<Option<usize>> =
            self.claims
                .iter()
                .map(|claim| {
                    let anchor_queue = claim.anchor.as_ref().map(|anchor| &anchor.queue_id);
                    let live_in: Vec<usize> = (0..view.queues.len())
                        .filter(|&q| queue_live[q].contains(&claim.identity))
                        .collect();
                    if let Some(&first) = live_in.first() {
                        return Some(
                            live_in
                                .iter()
                                .copied()
                                .find(|&q| anchor_queue == Some(&view.queues[q].queue_id))
                                .unwrap_or(first),
                        );
                    }
                    if let Some(Some(queue_id)) = anchor_queue {
                        return view.queues.iter().position(|queue| {
                            queue.queue_id.as_deref() == Some(queue_id.as_str())
                        });
                    }
                    if let Some(q) =
                        (0..view.queues.len()).find(|&q| queue_struck[q].contains(&claim.identity))
                    {
                        return Some(q);
                    }
                    if view.queues.len() == 1 {
                        return Some(0);
                    }
                    let single = |matches: &dyn Fn(usize) -> bool| -> Option<usize> {
                        let found: Vec<usize> =
                            (0..view.queues.len()).filter(|&q| matches(q)).collect();
                        match found.as_slice() {
                            [only] => Some(*only),
                            _ => None,
                        }
                    };
                    if claim.identity.is_id_backed() && !claim.tracked_item_aliases.is_empty() {
                        let aliases: HashSet<QueueItemIdentity> = claim
                            .tracked_item_aliases
                            .iter()
                            .map(|alias| claim_identity(alias))
                            .collect();
                        if let Some(q) =
                            single(&|q| queue_live[q].iter().any(|id| aliases.contains(id)))
                        {
                            return Some(q);
                        }
                    }
                    match &claim.anchor {
                        Some(anchor) => {
                            let anchored: HashSet<QueueItemIdentity> = anchor
                                .heads
                                .iter()
                                .map(|head| claim_identity(head))
                                .collect();
                            single(&|q| {
                                queue_live[q].iter().any(|id| anchored.contains(id))
                                    || queue_struck[q].iter().any(|id| anchored.contains(id))
                            })
                        }
                        None => single(&|q| {
                            queue_live[q]
                                .iter()
                                .any(|id| is_edit_continuation(&claim.identity, id))
                        }),
                    }
                })
                .collect();

        let held: HashSet<QueueItemIdentity> = self
            .claims
            .iter()
            .filter(|claim| claim.blocks_dispatch(now_secs))
            .map(|claim| claim.identity.clone())
            .collect();
        for (q, queue) in view.queues.iter().enumerate() {
            let members: Vec<bool> = assigned.iter().map(|a| *a == Some(q)).collect();
            if !members.iter().any(|member| *member) {
                continue;
            }
            outcome.followed.extend(self.follow_queue_edits(
                now_secs,
                queue,
                &members,
                &held,
                &closed_by_strike,
            ));
        }
        outcome
    }

    /// The single-queue body of [`Self::follow_edits_in_view`]: aligns the
    /// claims in `members` with `queue`'s live rows and re-anchors them on it.
    fn follow_queue_edits(
        &mut self,
        now_secs: u64,
        queue: &ClaimQueueRows,
        members: &[bool],
        held: &HashSet<QueueItemIdentity>,
        closed_by_strike: &dyn Fn(&QueueItemIdentity) -> bool,
    ) -> Vec<(String, String)> {
        use crate::queue_head_alignment::{
            QueueHeadPairing, normalize_queue_head_text, pair_queue_heads,
        };
        use agent_doc_document::queue_projection::{
            QueueOwnerProjection, owner_projection, strip_in_progress_marker,
        };
        let live_heads = &queue.rows;
        // Rows may carry the binary-owned lifecycle projection. Work text and
        // identity never include it; the exact owner annotation is kept apart
        // as row evidence (`#claimrowmigrate`).
        let texts: Vec<String> = live_heads
            .iter()
            .map(|head| strip_in_progress_marker(head.trim()))
            .collect();
        let row_owners: Vec<Option<QueueOwnerProjection>> = live_heads
            .iter()
            .map(|head| owner_projection(head))
            .collect();
        let norms: Vec<String> = texts
            .iter()
            .map(|head| normalize_queue_head_text(head))
            .collect();
        let live: Vec<QueueItemIdentity> = texts.iter().map(|head| claim_identity(head)).collect();
        let families: Vec<QueueItemIdentity> = texts
            .iter()
            .map(|head| effect_family_identity(head))
            .collect();

        // Per claim: the live lines it holds, and whether they are exclusive.
        // Exclusive lines were identified as THIS claim's row (by its anchor or
        // by its exact owner/generation projection), so a text-identical
        // sibling never joins them (`#claimrowmigrate`). Only a claim whose row
        // cannot be located (no anchor yet, or no anchored line survives) falls
        // back to every line with its identity.
        let mut pairings: Vec<(&Vec<String>, QueueHeadPairing)> = Vec::new();
        let mut descendants: Vec<Vec<usize>> = Vec::with_capacity(self.claims.len());
        let mut exclusive: Vec<bool> = Vec::with_capacity(self.claims.len());
        for (claim_idx, claim) in self.claims.iter().enumerate() {
            if !members[claim_idx] || !claim.blocks_dispatch(now_secs) {
                descendants.push(Vec::new());
                exclusive.push(false);
                continue;
            }
            // Never a line another live claim holds.
            let unheld = |n: &usize| live[*n] == claim.identity || !held.contains(&live[*n]);
            let mut anchored: Vec<usize> = Vec::new();
            if let Some(anchor) = &claim.anchor {
                let pairing = match pairings
                    .iter()
                    .position(|(heads, _)| **heads == anchor.heads)
                {
                    Some(index) => &pairings[index].1,
                    None => {
                        pairings.push((&anchor.heads, pair_queue_heads(&anchor.heads, &norms)));
                        &pairings[pairings.len() - 1].1
                    }
                };
                anchored.extend(
                    anchor
                        .slots
                        .iter()
                        .filter_map(|&slot| pairing.new_index_of(slot)),
                );
            }
            anchored.retain(unheld);
            let identity_lines: Vec<usize> = live
                .iter()
                .enumerate()
                .filter(|(_, id)| **id == claim.identity)
                .map(|(index, _)| index)
                .collect();
            // The visible `[#owner#generation]` annotation is the binary's own
            // projection of this exact attempt onto its row. Among the rows
            // that could be this claim's (anchored descendants, same-identity
            // lines, and effect-annotated copies of the same authored text),
            // exactly one attested row is the row; text alignment cannot tell
            // apart rows whose text is identical, the projection can.
            let family = effect_family_identity(&claim.item_text);
            let attested: Vec<usize> = row_owners
                .iter()
                .enumerate()
                .filter(|(_, owner)| {
                    owner.as_ref().is_some_and(|owner| {
                        owner.owner == claim.owner && owner.generation == claim.generation
                    })
                })
                .map(|(index, _)| index)
                .filter(unheld)
                .filter(|n| {
                    anchored.contains(n) || identity_lines.contains(n) || families[*n] == family
                })
                .collect();
            let (mut lines, is_exclusive) = if let [only] = attested.as_slice() {
                (vec![*only], true)
            } else if !anchored.is_empty() {
                (anchored, true)
            } else {
                (identity_lines, false)
            };
            lines.retain(unheld);
            lines.sort_unstable();
            lines.dedup();
            descendants.push(lines);
            exclusive.push(is_exclusive);
        }
        drop(pairings);

        let mut proposals: Vec<(usize, usize)> = Vec::new();
        for (claim_idx, claim) in self.claims.iter().enumerate() {
            if !members[claim_idx] || !claim.blocks_dispatch(now_secs) {
                continue;
            }
            let lines = &descendants[claim_idx];
            if lines.iter().any(|&n| live[n] == claim.identity) {
                // Still on (one of) its own line(s).
                continue;
            }
            if closed_by_strike(&claim.identity) {
                // `#claimfollowstruck`: its row was struck (completed). A strike
                // is never an edit, so no successor head inherits the claim,
                // whatever the structural alignment paired it with.
                continue;
            }
            if let Some(&first) = lines.first() {
                proposals.push((claim_idx, first));
                continue;
            }
            if live.contains(&claim.identity) {
                continue;
            }
            if claim.identity.is_id_backed() {
                // Backlog reconciliation can retire an id-backed mirror while
                // leaving the exact operator-facing tracked-item prose as a
                // free-text queue head. This is not a fuzzy id/free-text edit:
                // the alias was captured from that id's tracked item when the
                // claim was created, and it must identify one unheld live row.
                let candidates: Vec<usize> = live
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| !id.is_id_backed() && !held.contains(id))
                    .filter(|(_, id)| {
                        claim
                            .tracked_item_aliases
                            .iter()
                            .any(|alias| claim_identity(alias) == **id)
                    })
                    .map(|(idx, _)| idx)
                    .collect();
                if let [only] = candidates.as_slice() {
                    proposals.push((claim_idx, *only));
                }
                continue;
            }
            if claim.anchor.is_some() {
                // Anchored, and no line descends from it: the line closed.
                continue;
            }
            let candidates: Vec<usize> = live
                .iter()
                .enumerate()
                .filter(|(_, id)| !held.contains(id))
                .filter(|(_, id)| is_edit_continuation(&claim.identity, id))
                .map(|(idx, _)| idx)
                .collect();
            if let [only] = candidates.as_slice() {
                proposals.push((claim_idx, *only));
            }
        }
        let mut followed = Vec::new();
        for (claim_idx, head_idx) in &proposals {
            // Two orphaned claims landing on one line: never guess.
            if proposals
                .iter()
                .filter(|(_, h)| live[*h] == live[*head_idx])
                .count()
                != 1
            {
                continue;
            }
            let text = texts[*head_idx].trim().to_string();
            let claim = &mut self.claims[*claim_idx];
            let previous = std::mem::replace(&mut claim.item_text, text.clone());
            claim.identity = live[*head_idx].clone();
            if !claim.followed_from.contains(&previous) {
                claim.followed_from.push(previous.clone());
                let excess = claim
                    .followed_from
                    .len()
                    .saturating_sub(FOLLOWED_FROM_LIMIT);
                claim.followed_from.drain(..excess);
            }
            claim.followed_generation = claim.generation;
            followed.push((previous, text));
        }

        // Re-anchor every live claim whose line is in view, so the next
        // alignment starts from this snapshot. A claim whose line is not in
        // view (a lagging disk read, or a closed line) keeps its anchor. An
        // exclusive claim keeps exactly its located lines; only an unlocated
        // claim holds every line with its identity.
        for (claim_idx, claim) in self.claims.iter_mut().enumerate() {
            if !members[claim_idx] || claim.is_expired(now_secs) || !live.contains(&claim.identity)
            {
                continue;
            }
            let mut slots = descendants[claim_idx].clone();
            if !exclusive[claim_idx] {
                slots.extend(
                    live.iter()
                        .enumerate()
                        .filter(|(_, id)| **id == claim.identity)
                        .map(|(index, _)| index),
                );
            }
            slots.sort_unstable();
            slots.dedup();
            claim.anchor = Some(ClaimAnchor {
                queue_id: queue.queue_id.clone(),
                heads: norms.clone(),
                slots,
            });
        }
        followed
    }

    /// Carry live claims across free-text promotion (`#freetextqueue`). Each
    /// `(source_head, id)` pair is an exact promotion receipt: free-text
    /// admission minted backlog `#id` from `source_head` and replaced the head
    /// with `do [#id]`. A live claim whose identity is exactly
    /// `claim_identity(source_head)` is re-keyed onto the `#id` head (same
    /// owner, generation, worker and expiry), so the promoted head stays claimed
    /// instead of resurfacing as unclaimed dispatch work. Moving rather than
    /// copying preserves the one-attempt/one-worker invariant; a retried exact
    /// promotion is an idempotent no-op. An `#id` head with any distinct live
    /// attempt is never taken over, even when its owner label happens to match.
    /// Returns
    /// `(source_head, id, owner)` per transferred claim.
    pub fn transfer_promoted(
        &mut self,
        now_secs: u64,
        promotions: &[(String, String)],
    ) -> Vec<(String, String, String)> {
        let mut transferred = Vec::new();
        for (source, id) in promotions {
            let id = id.trim().trim_start_matches('#').to_ascii_lowercase();
            if id.is_empty() {
                continue;
            }
            let source_identity = claim_identity(source);
            let Some(source_index) = self.claims.iter().position(|claim| {
                claim.identity == source_identity && claim.blocks_dispatch(now_secs)
            }) else {
                continue;
            };
            let source_claim = self.claims[source_index].clone();
            let target_text = format!("do [#{id}]");
            let target_identity = claim_identity(&target_text);
            if target_identity == source_identity {
                continue;
            }
            let target_index = self
                .claims
                .iter()
                .position(|claim| claim.identity == target_identity);
            match target_index {
                Some(index) if self.claims[index].blocks_dispatch(now_secs) => {
                    continue;
                }
                Some(index) => {
                    let existing = &mut self.claims[index];
                    existing.owner = source_claim.owner.clone();
                    existing.generation = source_claim.generation;
                    existing.state = source_claim.state;
                    existing.worker = source_claim.worker.clone();
                    existing.owner_session = source_claim.owner_session.clone();
                    existing.item_text = target_text;
                    existing.expires_at_secs =
                        existing.expires_at_secs.max(source_claim.expires_at_secs);
                    self.claims.remove(source_index);
                }
                None => {
                    let source = &mut self.claims[source_index];
                    if !source.followed_from.contains(&source.item_text) {
                        source.followed_from.push(source.item_text.clone());
                    }
                    source.followed_generation = source.generation;
                    source.identity = target_identity;
                    source.item_text = target_text;
                    source.anchor = None;
                }
            }
            transferred.push((source.clone(), id, source_claim.owner));
        }
        transferred
    }

    /// Owner of every active claim, by identity (after any
    /// [`Self::follow_edits`] the caller applied).
    pub fn owners(
        &self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> std::collections::HashMap<QueueItemIdentity, String> {
        self.active(now_secs, live_heads)
            .into_iter()
            .map(|claim| (claim.identity.clone(), claim.owner.clone()))
            .collect()
    }

    /// Drop expired and closed claims. Returns how many were removed.
    pub fn prune(
        &mut self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> usize {
        self.prune_detailed(now_secs, live_heads).len()
    }

    /// [`Self::prune`], returning each dropped claim and why.
    pub fn prune_detailed(
        &mut self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> Vec<(QueueClaim, ClaimPruneCause)> {
        let mut pruned = Vec::new();
        let mut kept = Vec::with_capacity(self.claims.len());
        for mut claim in std::mem::take(&mut self.claims) {
            if claim.state != QueueClaimState::Lost && claim.is_expired(now_secs) {
                claim.state = QueueClaimState::Lost;
                pruned.push((claim.clone(), ClaimPruneCause::Expired));
                kept.push(claim);
            } else if live_heads.is_some_and(|live| !live.contains(&claim.identity)) {
                pruned.push((claim, ClaimPruneCause::Closed));
            } else {
                kept.push(claim);
            }
        }
        self.claims = kept;
        pruned
    }

    /// End the finished attempts of rows a strike closed (`#strikeretires`),
    /// at the strike's own successor rather than at the next closeout prune.
    ///
    /// A claim is dropped when its identity is in `struck`, has no live row
    /// in `live` (a mirror copy or duplicate keeps its attempt), and its
    /// expiry-adjusted state agrees with the strike: `Reported` or `Lost`
    /// ([`struck_row_reopened_by_claim`] is false). An attempt that is still
    /// dispatching or running contradicts the strike and stays, so its worker
    /// keeps its fence and the struck row reopens as `❔`. Without this, a
    /// `Reported` attempt outlived every non-closeout strike and projected
    /// `☑️` onto a later row with the same text. Returns the dropped claims,
    /// with their expiry-adjusted state.
    pub fn retire_struck(
        &mut self,
        now_secs: u64,
        struck: &HashSet<QueueItemIdentity>,
        live: &HashSet<QueueItemIdentity>,
    ) -> Vec<QueueClaim> {
        let mut retired = Vec::new();
        let mut kept = Vec::with_capacity(self.claims.len());
        for claim in std::mem::take(&mut self.claims) {
            let mut adjusted = claim.clone();
            if adjusted.is_expired(now_secs) {
                adjusted.state = QueueClaimState::Lost;
            }
            if struck.contains(&claim.identity)
                && !live.contains(&claim.identity)
                && !struck_row_reopened_by_claim(Some(&adjusted))
            {
                retired.push(adjusted);
            } else {
                kept.push(claim);
            }
        }
        self.claims = kept;
        retired
    }

    /// The claimed identities a drainability computation must skip.
    pub fn claimed_items(
        &self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> ClaimedQueueItems {
        ClaimedQueueItems(
            self.active(now_secs, live_heads)
                .into_iter()
                .map(|claim| claim.identity.clone())
                .collect(),
        )
    }
}

/// The identity a claim on `item` keys on: the shared queue-head work
/// identity ([`queue_head_identity`]). Every claim comparison — the ledger,
/// [`ClaimedQueueItems::claims`], live-head closure, dispatch membership —
/// goes through this one function.
pub fn claim_identity(item: &str) -> QueueItemIdentity {
    queue_head_identity(item.trim())
}

/// The claim identity of `item`'s authored work with any trailing
/// `⟦effect:N⟧` suffix removed: equal for a row, its effect-annotated
/// successor, and its text-identical siblings (`#claimrowmigrate`).
fn effect_family_identity(item: &str) -> QueueItemIdentity {
    claim_identity(&crate::document_queue::strip_queue_effect_annotation(
        item.trim(),
    ))
}

/// Binary attestation key for one exact generated claim attempt and its
/// immutable worker binding. Every producer and verifier uses this function so
/// a receipt cannot become authoritative merely by carrying a nonempty token.
pub fn completion_attestation_key(
    identity: &QueueItemIdentity,
    owner: &str,
    generation: u64,
    claimed_at_secs: u64,
    worker_id: &str,
) -> Result<String, serde_json::Error> {
    let identity = serde_json::to_string(identity)?;
    Ok(agent_doc_hash::content_hash(&format!(
        "{identity}\n{owner}\n{generation}\n{claimed_at_secs}\n{worker_id}"
    )))
}

/// The stable CLI handle for a queue claim. Only a structurally id-backed head
/// collapses to `#id`; an id mentioned later in free text remains verbatim so
/// dispatch, admission, and closeout agree on the line's identity (GH #182).
pub fn claim_item_handle(item: &str) -> String {
    match claim_identity(item) {
        QueueItemIdentity::Id(id) => format!("#{id}"),
        QueueItemIdentity::FreeText(_) => item.trim().to_string(),
    }
}

/// Whether `new` is the same free-text work as `old` with a note appended,
/// prepended, or removed at a word boundary (`#claimfollowsedit`): one
/// normalized text is a prefix or suffix of the other, and the join is not
/// inside a word, so `.../issues/126` -> `.../issues/126: note …` and
/// `… works` -> `… works.` continue the work while `.../issues/126` ->
/// `.../issues/1260` does not. Id-backed identities never need this (an edit
/// that keeps the id keeps the identity). This is the fallback for a claim
/// with no [`ClaimAnchor`] (stored before anchors existed); an anchored claim
/// follows any in-place edit of its line (`#claimfollowedit`). The shorter
/// side must carry at least eight alphanumerics so a stub cannot capture an
/// unrelated line.
pub fn is_edit_continuation(old: &QueueItemIdentity, new: &QueueItemIdentity) -> bool {
    let (QueueItemIdentity::FreeText(old), QueueItemIdentity::FreeText(new)) = (old, new) else {
        return false;
    };
    if old == new {
        return false;
    }
    let (short, long) = if old.len() <= new.len() {
        (old.as_str(), new.as_str())
    } else {
        (new.as_str(), old.as_str())
    };
    if short.chars().filter(|ch| ch.is_alphanumeric()).count() < 8 {
        return false;
    }
    let not_word = |ch: Option<char>| ch.is_none_or(|ch| !ch.is_alphanumeric());
    if let Some(rest) = long.strip_prefix(short)
        && (not_word(rest.chars().next()) || not_word(short.chars().last()))
    {
        return true;
    }
    if let Some(rest) = long.strip_suffix(short)
        && (not_word(rest.chars().last()) || not_word(short.chars().next()))
    {
        return true;
    }
    false
}

/// Tracked ids a queue line references as `[#id]` or `do #id`, lowercased, in
/// first-seen order. Preset tags (`#subagents`, `#gh-fix`) are not references,
/// so `#subagents do [#a]` references exactly `a`.
pub fn referenced_queue_ids(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let is_id_char = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_');
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !id.is_empty() && id.chars().all(is_id_char) && !out.iter().any(|seen| seen == id) {
            out.push(id.to_string());
        }
    };
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        match after.find(']') {
            Some(end) => {
                push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    for pair in words.windows(2) {
        if pair[0] == "do"
            && let Some(id) = pair[1].strip_prefix('#')
        {
            let id: String = id.chars().take_while(|ch| is_id_char(*ch)).collect();
            push(&id);
        }
    }
    out
}

/// Why [`QueueClaimLedger::refresh`] refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshRefusal {
    /// No claim on the item at all.
    Unclaimed,
    /// The claim's TTL already lapsed; the head is back in the eligible set.
    Expired {
        holder: String,
        expired_at_secs: u64,
    },
    /// A different owner holds the live claim.
    OtherOwner { holder: String },
    /// The heartbeat belongs to an older attempt or omitted the required
    /// generation fence.
    StaleAttempt { detail: String },
    /// Dispatch has not been positively acknowledged with `queue start`.
    NotRunning,
}

impl RefreshRefusal {
    /// Stable short name for ops-log lines.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unclaimed => "unclaimed",
            Self::Expired { .. } => "expired",
            Self::OtherOwner { .. } => "other_owner",
            Self::StaleAttempt { .. } => "stale_attempt",
            Self::NotRunning => "not_running",
        }
    }
}

/// A `queue claim --refresh` that has no live claim of its owner to extend.
/// An operator/agent usage error with a specific remedy, not an Agent Doc
/// turn failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueClaimRefreshRefused {
    pub item: String,
    pub owner: String,
    pub reason: RefreshRefusal,
}

impl std::fmt::Display for QueueClaimRefreshRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            RefreshRefusal::Unclaimed => write!(
                f,
                "cannot refresh {:?}: it is not claimed. Claim it first with `queue claim` \
                 (without --refresh).",
                self.item
            ),
            RefreshRefusal::Expired {
                holder,
                expired_at_secs,
            } => write!(
                f,
                "cannot refresh {:?}: the claim held by {holder} expired at {expired_at_secs}. \
                 The head is eligible for dispatch again; claim it anew (without --refresh) only \
                 if no other worker has taken it.",
                self.item
            ),
            RefreshRefusal::OtherOwner { holder } => write!(
                f,
                "cannot refresh {:?} for {}: the live claim is held by {holder}.",
                self.item, self.owner
            ),
            RefreshRefusal::StaleAttempt { detail } => write!(
                f,
                "cannot refresh {:?} for {}: {detail}.",
                self.item, self.owner
            ),
            RefreshRefusal::NotRunning => write!(
                f,
                "cannot refresh {:?} for {}: no concrete worker has acknowledged the attempt. Run `queue start` after a successful dispatch.",
                self.item, self.owner
            ),
        }
    }
}

impl std::error::Error for QueueClaimRefreshRefused {}

/// A `queue claim` / `queue release` `--item` that names no live queue head
/// (or names several). An operator/agent usage error with a specific remedy,
/// not an Agent Doc turn failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueClaimMiss {
    pub item: String,
    pub live_heads: Vec<String>,
    /// Several live heads reference the `#id`; empty for a plain miss.
    pub ambiguous: Vec<String>,
}

impl std::fmt::Display for QueueClaimMiss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quote = |items: &[String]| {
            items
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if self.ambiguous.is_empty() {
            write!(
                f,
                "no live queue head matches {:?}; live heads: {}. Pass `--item` as a head's `#id` \
                 or its exact queue line text.",
                self.item,
                if self.live_heads.is_empty() {
                    "(none)".to_string()
                } else {
                    quote(&self.live_heads)
                }
            )
        } else {
            write!(
                f,
                "{:?} matches several live queue heads: {}. Pass `--item` as the exact queue line \
                 text.",
                self.item,
                quote(&self.ambiguous)
            )
        }
    }
}

impl std::error::Error for QueueClaimMiss {}

/// Resolve a claim `--item` to the live queue head it names.
///
/// The item matches a head by claim identity (the exact line, marker-
/// invariant, or a bare `#id` for an id-backed head). When that misses and the
/// item is an `#id`, it matches the single live head that REFERENCES the id,
/// so `#a` claims a `#subagents do [#a]` head whose identity is its full text.
pub fn resolve_claim_target(item: &str, live_heads: &[String]) -> Result<String, QueueClaimMiss> {
    let identity = claim_identity(item);
    if let Some(head) = live_heads
        .iter()
        .find(|head| claim_identity(head) == identity)
    {
        return Ok(head.clone());
    }
    // `#multilinequeuehead`: a multi-line head (a title line plus a pasted
    // fenced block) is named by its title line. The item resolves only when
    // exactly one live multi-line head has that first line; several heads with
    // the same title are ambiguous and must be named by their full text.
    if !item.trim().contains('\n') {
        let by_title: Vec<String> = live_heads
            .iter()
            .filter(|head| {
                let head = head.trim();
                head.contains('\n')
                    && head
                        .lines()
                        .next()
                        .is_some_and(|title| claim_identity(title) == identity)
            })
            .cloned()
            .collect();
        match by_title.len() {
            1 => return Ok(by_title[0].clone()),
            0 => {}
            _ => {
                return Err(QueueClaimMiss {
                    item: item.trim().to_string(),
                    live_heads: live_heads.to_vec(),
                    ambiguous: by_title,
                });
            }
        }
    }
    // `#claimfollowsedit`: the item names a head the operator has since
    // annotated; resolve to the single head that continues it.
    let continuing: Vec<&String> = live_heads
        .iter()
        .filter(|head| is_edit_continuation(&identity, &claim_identity(head)))
        .collect();
    match continuing.as_slice() {
        [only] => return Ok((*only).clone()),
        [] => {}
        _ => {
            return Err(QueueClaimMiss {
                item: item.trim().to_string(),
                live_heads: live_heads.to_vec(),
                ambiguous: continuing.into_iter().cloned().collect(),
            });
        }
    }
    if let QueueItemIdentity::Id(id) = &identity {
        let referencing: Vec<String> = live_heads
            .iter()
            .filter(|head| referenced_queue_ids(head).iter().any(|r| r == id))
            .cloned()
            .collect();
        match referencing.len() {
            1 => return Ok(referencing[0].clone()),
            0 => {}
            _ => {
                return Err(QueueClaimMiss {
                    item: item.trim().to_string(),
                    live_heads: live_heads.to_vec(),
                    ambiguous: referencing,
                });
            }
        }
    }
    Err(QueueClaimMiss {
        item: item.trim().to_string(),
        live_heads: live_heads.to_vec(),
        ambiguous: Vec::new(),
    })
}

/// Which document view a claim target resolved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimHeadSource {
    /// The on-disk document.
    Disk,
    /// The live editor / CRDT authority text — the view preflight computes
    /// `queue_subagent_dispatch` from — while the editor's save to disk is
    /// still pending (e.g. a deferred native save).
    EditorAuthority,
}

impl ClaimHeadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::EditorAuthority => "editor_authority",
        }
    }
}

/// [`resolve_claim_target`] across the two views of the document (GH #124
/// ask 5). Preflight computes `queue_subagent_dispatch` from the editor
/// authority's current text; `queue claim` used to resolve against disk only,
/// so a head the operator typed but the editor had not yet saved was offered
/// for dispatch and then refused as "no live queue head". When an authority
/// cut exists it is the only live view: stale disk additions must not remain
/// claimable after the authority deletes them. Disk is authoritative only when
/// no live authority cut is available.
pub fn resolve_claim_target_in_views(
    item: &str,
    disk_heads: &[String],
    authority_heads: Option<&[String]>,
) -> Result<(String, ClaimHeadSource), QueueClaimMiss> {
    if let Some(authority_heads) = authority_heads {
        return resolve_claim_target(item, authority_heads)
            .map(|target| (target, ClaimHeadSource::EditorAuthority));
    }
    resolve_claim_target(item, disk_heads).map(|target| (target, ClaimHeadSource::Disk))
}

/// A set of claimed queue items, consulted by the drainability filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimedQueueItems(HashSet<QueueItemIdentity>);

impl ClaimedQueueItems {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_identities(identities: impl IntoIterator<Item = QueueItemIdentity>) -> Self {
        Self(identities.into_iter().collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// These claims plus `heads` (by identity): the in-session exclusion set
    /// once the queue-level subagents attribute holds heads back.
    pub fn with_heads<'a>(mut self, heads: impl IntoIterator<Item = &'a String>) -> Self {
        self.0
            .extend(heads.into_iter().map(|head| claim_identity(head)));
        self
    }

    /// Whether the queue head `prompt_text` is claimed.
    pub fn claims(&self, prompt_text: &str) -> bool {
        !self.0.is_empty() && self.0.contains(&claim_identity(prompt_text))
    }

    /// Whether this set holds a claimed identity `earlier` did not
    /// (GH #319 claim visibility): a claim landed between two reads of the ledger.
    /// A claim that released or expired in between is not a gain.
    pub fn gained_since(&self, earlier: &ClaimedQueueItems) -> bool {
        self.0.iter().any(|identity| !earlier.0.contains(identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(harness: &str, id: &str, pane: &str, pid: u32) -> ClaimOwnerSession {
        ClaimOwnerSession {
            harness: Some(harness.to_string()),
            session_id: Some(id.to_string()),
            pane: Some(pane.to_string()),
            pid: Some(pid),
        }
    }

    /// `#claimownerliveness` (haiven-dev tasks/sdk.md, 2026-10-11): a claim
    /// made by the pre-`/clear` coordinator session in pane %180 reads as
    /// ended once the pane runs another session — seen by the caller itself
    /// or by the pane's newest harness hook — and as ended when the harness
    /// process exits. The same session (including its own subagents, which
    /// inherit the session id) reads as live; a claim without a recorded
    /// session stays unrecorded.
    #[test]
    fn owner_session_liveness_detects_cleared_replaced_and_exited_sessions() {
        let mut ledger = QueueClaimLedger::default();
        let before_clear = session("claude", "pre-clear", "%180", 4242);
        ledger.claim_in_session(
            "do [#cligenport]",
            "subagent:cligoshell-then-cligenport",
            100,
            7200,
            Some(before_clear.clone()),
        );
        let claim = ledger.claims[0].clone();
        assert_eq!(claim.owner_session.as_ref(), Some(&before_clear));

        let live = |obs| owner_session_liveness(&claim, obs);
        // Same session, or a subagent of it: live.
        assert_eq!(
            live(OwnerSessionObservation {
                caller: Some(&before_clear),
                owner_pid_alive: Some(true),
                pane_session: Some(("pre-clear", 150)),
            }),
            ClaimOwnerLiveness::Live
        );
        // The post-`/clear` coordinator in the same pane: replaced.
        let after_clear = session("claude", "post-clear", "%180", 4242);
        assert_eq!(
            live(OwnerSessionObservation {
                caller: Some(&after_clear),
                owner_pid_alive: Some(true),
                pane_session: None,
            }),
            ClaimOwnerLiveness::Ended(OwnerSessionEndReason::SessionReplaced)
        );
        // A caller outside the pane (a hook, another document's coordinator)
        // learns it from the pane's harness hook observed after the claim.
        let elsewhere = session("claude", "other-doc", "%7", 999);
        assert_eq!(
            live(OwnerSessionObservation {
                caller: Some(&elsewhere),
                owner_pid_alive: Some(true),
                pane_session: Some(("post-clear", 200)),
            }),
            ClaimOwnerLiveness::Ended(OwnerSessionEndReason::SessionReplaced)
        );
        // A pane observation older than the claim proves nothing.
        assert_eq!(
            live(OwnerSessionObservation {
                caller: None,
                owner_pid_alive: None,
                pane_session: Some(("older-session", 99)),
            }),
            ClaimOwnerLiveness::Live
        );
        // The harness process is gone.
        assert_eq!(
            live(OwnerSessionObservation {
                caller: None,
                owner_pid_alive: Some(false),
                pane_session: None,
            }),
            ClaimOwnerLiveness::Ended(OwnerSessionEndReason::OwnerProcessExited)
        );
        // Session ids are only compared within one harness.
        let codex_owner = QueueClaim {
            owner_session: Some(session("codex", "thread-a", "%180", 4242)),
            ..claim.clone()
        };
        assert_eq!(
            owner_session_liveness(
                &codex_owner,
                OwnerSessionObservation {
                    caller: None,
                    owner_pid_alive: Some(true),
                    pane_session: Some(("claude-session", 200)),
                }
            ),
            ClaimOwnerLiveness::Live
        );
        // No recorded owner session: judged as before.
        let mut legacy = QueueClaimLedger::default();
        legacy.claim("do [#a]", "subagent:a", 100, 600);
        assert_eq!(
            owner_session_liveness(
                &legacy.claims[0],
                OwnerSessionObservation {
                    caller: Some(&after_clear),
                    owner_pid_alive: None,
                    pane_session: None,
                }
            ),
            ClaimOwnerLiveness::Unrecorded
        );
        assert_eq!(ClaimOwnerSession::default().non_empty(), None);
    }

    /// Adoption: the post-`/clear` coordinator refreshes the claim under the
    /// same owner and its own session; the claim is live again for it. A
    /// refresh without a session keeps the recorded one.
    #[test]
    fn refresh_in_session_adopts_the_claim_into_the_caller_session() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim_in_session(
            "do [#a]",
            "subagent:a",
            100,
            600,
            Some(session("claude", "pre-clear", "%180", 1)),
        );
        ledger
            .start("#a", "subagent:a", "worker:a", 101, 600, Some(1))
            .unwrap();
        let after_clear = session("claude", "post-clear", "%180", 1);
        let adopted = ledger
            .refresh_in_session("#a", "subagent:a", 200, 600, Some(after_clear.clone()))
            .unwrap();
        assert_eq!(adopted.owner_session.as_ref(), Some(&after_clear));
        assert_eq!(adopted.expires_at_secs, 800);
        assert_eq!(
            owner_session_liveness(
                &adopted,
                OwnerSessionObservation {
                    caller: Some(&after_clear),
                    owner_pid_alive: Some(true),
                    pane_session: Some(("post-clear", 200)),
                }
            ),
            ClaimOwnerLiveness::Live
        );
        let kept = ledger.refresh("#a", "subagent:a", 300, 600).unwrap();
        assert_eq!(kept.owner_session.as_ref(), Some(&after_clear));
        // A stored ledger without the field still loads.
        let legacy = QueueClaimLedger::from_state_payload(
            r#"{"claims":[{"identity":{"kind":"id","key":"a"},"item_text":"do [#a]","owner":"subagent:a","claimed_at_secs":1,"expires_at_secs":2}]}"#,
        )
        .unwrap();
        assert_eq!(legacy.claims[0].owner_session, None);
    }

    /// `#freetextqueue`: a claimed free-text head promoted into `do [#id]`
    /// keeps its claim under the new id (2026-10-10: the claimed
    /// "Run Agent Doc on frontend.md crashed …" head became
    /// `do [#runfrontendcrashed]` and resurfaced as unclaimed dispatch work).
    #[test]
    fn transfer_promoted_carries_claim_to_the_minted_id_head() {
        let head = "Run Agent Doc on frontend.md crashed with a panic in the route";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(head, "subagent:frontendcrash", 100, 600);

        let moved =
            ledger.transfer_promoted(200, &[(head.to_string(), "RunFrontendCrashed".to_string())]);
        assert_eq!(
            moved,
            vec![(
                head.to_string(),
                "runfrontendcrashed".to_string(),
                "subagent:frontendcrash".to_string()
            )]
        );

        let live: HashSet<QueueItemIdentity> =
            [claim_identity("🚧 do [#runfrontendcrashed]")].into();
        let owners = ledger.owners(200, Some(&live));
        assert_eq!(
            owners.get(&claim_identity("do [#runfrontendcrashed]")),
            Some(&"subagent:frontendcrash".to_string())
        );
        assert!(
            ledger
                .claimed_items(200, Some(&live))
                .claims("do [#runfrontendcrashed]")
        );
        let target = ledger
            .claims
            .iter()
            .find(|claim| claim.identity == claim_identity("[#runfrontendcrashed]"))
            .unwrap();
        assert_eq!(
            target.expires_at_secs, 700,
            "expiry is inherited, not renewed"
        );

        // Idempotent on retry, and survives the on-load re-key.
        let before = ledger.clone();
        ledger.transfer_promoted(200, &[(head.to_string(), "runfrontendcrashed".to_string())]);
        assert_eq!(ledger, before);
        assert!(!ledger.rekey());
    }

    #[test]
    fn transfer_promoted_requires_exact_source_identity_and_live_claim() {
        let head = "Run Agent Doc on frontend.md crashed with a panic in the route";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(head, "subagent:frontendcrash", 100, 600);

        // A different head (even one containing the claimed text) is not lineage.
        assert!(
            ledger
                .transfer_promoted(
                    200,
                    &[(format!("{head} and also the backend"), "other".to_string())]
                )
                .is_empty()
        );
        // An expired claim is not carried.
        assert!(
            ledger
                .transfer_promoted(800, &[(head.to_string(), "late".to_string())])
                .is_empty()
        );
        assert_eq!(ledger.claims.len(), 1);
    }

    #[test]
    fn transfer_promoted_never_takes_over_a_distinct_live_id_attempt() {
        let head = "Run Agent Doc on frontend.md crashed with a panic in the route";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(head, "subagent:a", 100, 600);
        ledger.claim("do [#x]", "subagent:b", 100, 600);
        assert!(
            ledger
                .transfer_promoted(200, &[(head.to_string(), "x".to_string())])
                .is_empty()
        );
        let live: HashSet<QueueItemIdentity> = [claim_identity("do [#x]")].into();
        assert_eq!(
            ledger
                .owners(200, Some(&live))
                .get(&claim_identity("do [#x]")),
            Some(&"subagent:b".to_string())
        );

        let mut same_owner = QueueClaimLedger::default();
        same_owner.claim(head, "subagent:a", 100, 600);
        same_owner.claim("do [#x]", "subagent:a", 100, 600);
        assert!(
            same_owner
                .transfer_promoted(200, &[(head.to_string(), "x".to_string())])
                .is_empty(),
            "an owner label is not proof that two live attempts share lineage"
        );
        assert_eq!(same_owner.claims.len(), 2);
    }

    #[test]
    fn identity_is_marker_invariant_and_id_backed() {
        assert_eq!(claim_identity("🚧 do [#a]"), claim_identity("[#a]"));
        assert_eq!(
            claim_identity("📌 #gh-fix https://x/issues/109"),
            claim_identity("#gh-fix https://x/issues/109")
        );
        assert_ne!(
            claim_identity("#gh-fix https://x/issues/109"),
            claim_identity("#gh-fix https://x/issues/110")
        );
    }

    #[test]
    fn claim_refresh_fences_live_takeover_then_reclaims_lost_generation() {
        let mut ledger = QueueClaimLedger::default();
        assert_eq!(
            ledger.claim("do [#a]", "subagent:a", 100, 10),
            ClaimOutcome::Created
        );
        assert_eq!(
            ledger.claim("🚧 do [#a]", "subagent:a", 105, 10),
            ClaimOutcome::Refreshed
        );
        assert_eq!(ledger.claims[0].expires_at_secs, 115);
        assert_eq!(
            ledger.claim("[#a]", "subagent:b", 106, 10),
            ClaimOutcome::HeldByOther
        );
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.claims[0].owner, "subagent:a");
        assert_eq!(ledger.claims[0].generation, 1);
        assert_eq!(
            ledger.claim("[#a]", "subagent:b", 116, 10),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims[0].generation, 2);
        assert_eq!(ledger.release("do [#a]").unwrap().owner, "subagent:b");
        assert!(ledger.release("do [#a]").is_none());
    }

    #[test]
    fn refresh_extends_only_the_same_owners_live_claim() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("do [#a]", "subagent:a", 100, 10);
        ledger
            .start("#a", "subagent:a", "worker:a", 101, 10, Some(1))
            .unwrap();

        // Same owner, live claim: the TTL moves, claimed_at does not.
        let refreshed = ledger.refresh("🚧 [#a]", "subagent:a", 105, 50).unwrap();
        assert_eq!(refreshed.expires_at_secs, 155);
        assert_eq!(refreshed.claimed_at_secs, 100);
        assert_eq!(ledger.claims[0].expires_at_secs, 155);

        // Another owner is refused and the claim is untouched.
        let other = ledger
            .refresh("do [#a]", "subagent:b", 106, 50)
            .unwrap_err();
        assert_eq!(
            other.reason,
            RefreshRefusal::OtherOwner {
                holder: "subagent:a".to_string()
            }
        );
        assert_eq!(ledger.claims[0].owner, "subagent:a");
        assert_eq!(ledger.claims[0].expires_at_secs, 155);

        // An expired claim is refused, never revived.
        let expired = ledger
            .refresh("do [#a]", "subagent:a", 155, 50)
            .unwrap_err();
        assert_eq!(expired.reason.kind(), "expired");
        assert_eq!(ledger.claims[0].expires_at_secs, 155);
        assert!(!ledger.claimed_items(155, None).claims("do [#a]"));

        // An unclaimed item is refused and no claim is created.
        let unclaimed = ledger
            .refresh("do [#b]", "subagent:a", 106, 50)
            .unwrap_err();
        assert_eq!(unclaimed.reason, RefreshRefusal::Unclaimed);
        assert_eq!(ledger.claims.len(), 1);
    }

    #[test]
    fn hash_id_resolves_a_preset_prefixed_head() {
        let heads = vec![
            "#gh-fix https://x/issues/110".to_string(),
            "#subagents do [#preflightdeadline]".to_string(),
        ];
        assert_eq!(
            resolve_claim_target("#preflightdeadline", &heads).unwrap(),
            "#subagents do [#preflightdeadline]"
        );
        assert_eq!(
            resolve_claim_target("#subagents do [#preflightdeadline]", &heads).unwrap(),
            "#subagents do [#preflightdeadline]"
        );
        let miss = resolve_claim_target("#nosuch", &heads).unwrap_err();
        let message = miss.to_string();
        assert!(
            message.starts_with("no live queue head matches \"#nosuch\"; live heads: "),
            "{message}"
        );
        assert!(
            message.contains("#subagents do [#preflightdeadline]"),
            "{message}"
        );
        // `#subagents do [#a]` now keys on `a` itself, so `#a` resolves it
        // directly; ambiguity needs two heads that only REFERENCE the id.
        assert_eq!(
            resolve_claim_target("#a", &["#subagents do [#a]".to_string()]).unwrap(),
            "#subagents do [#a]"
        );
        let twins = vec![
            "review do [#a] later".to_string(),
            "then also do [#a] again".to_string(),
        ];
        assert!(
            !resolve_claim_target("#a", &twins)
                .unwrap_err()
                .ambiguous
                .is_empty()
        );
        assert_eq!(
            referenced_queue_ids("#subagents do [#A] and do #b-2"),
            vec!["a", "b-2"]
        );
    }

    #[test]
    fn claim_handle_uses_only_the_structural_head_identity() {
        assert_eq!(claim_item_handle("do [#work] with notes"), "#work");
        assert_eq!(claim_item_handle("[#work] verify it"), "#work");
        assert_eq!(
            claim_item_handle("verify [#work] yourself"),
            "verify [#work] yourself"
        );
    }

    /// `#claimdispatchidentity` (live 2026-10-03): the operator edited the
    /// claimed `#subagents: <url>` into `#subagents: #gh-fix <url>` while the
    /// subagent worked. The claim must follow the edit (still claimed, still a
    /// live head, never pruned as closed), and a retarget to a different URL
    /// or id must not inherit it.
    #[test]
    fn claim_follows_a_tag_edit_but_not_a_retarget() {
        let url = "https://github.com/btakita/agent-doc/issues/120";
        let claimed_line = format!("#subagents: {url}");
        let edited_line = format!("#subagents: #gh-fix {url}");
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(&claimed_line, "subagent:gh120", 100, 1000);

        let live_after_edit: HashSet<_> = [claim_identity(&edited_line)].into_iter().collect();
        let claimed = ledger.claimed_items(105, Some(&live_after_edit));
        assert!(
            claimed.claims(&edited_line),
            "the edited line keeps its claim"
        );
        assert!(claimed.claims(&format!("🚧 {edited_line}")));
        assert_eq!(
            ledger.prune(105, Some(&live_after_edit)),
            0,
            "an edited claimed line is not a closed item"
        );
        assert_eq!(
            resolve_claim_target(&claimed_line, std::slice::from_ref(&edited_line)).unwrap(),
            edited_line,
            "release/refresh by the original text still finds the edited head"
        );

        let retarget = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/121";
        let live_after_retarget: HashSet<_> = [claim_identity(retarget)].into_iter().collect();
        assert!(
            !ledger
                .claimed_items(105, Some(&live_after_retarget))
                .claims(retarget),
            "a different issue is a different task"
        );
        assert_eq!(ledger.prune(105, Some(&live_after_retarget)), 1);

        let mut ids = QueueClaimLedger::default();
        ids.claim("do [#a]", "subagent:a", 100, 1000);
        let items = ids.claimed_items(105, None);
        assert!(items.claims("#subagents do [#a]"));
        assert!(items.claims("#subagents: #gh-fix do [#a]"));
        assert!(
            !items.claims("#subagents do [#b]"),
            "a different id is a retarget"
        );
    }

    /// A ledger stored under the old tag-sensitive key is re-keyed on load, so
    /// a claim taken before the upgrade still covers the edited head.
    #[test]
    fn rekey_migrates_tag_sensitive_free_text_claims() {
        let url = "https://x/issues/120";
        let mut ledger = QueueClaimLedger {
            claims: vec![
                QueueClaim {
                    identity: QueueItemIdentity::FreeText(format!("#subagents: {url}")),
                    item_text: format!("#subagents: {url}"),
                    owner: "subagent:a".into(),
                    generation: 0,
                    state: QueueClaimState::Claimed,
                    worker: None,
                    dispatch_policy: None,
                    tracked_item_aliases: Vec::new(),
                    claimed_at_secs: 1,
                    expires_at_secs: 50,
                    owner_session: None,
                    anchor: None,
                    followed_from: Vec::new(),
                    followed_generation: 0,
                },
                QueueClaim {
                    identity: QueueItemIdentity::FreeText(format!("#gh-fix {url}")),
                    item_text: format!("#gh-fix {url}"),
                    owner: "subagent:b".into(),
                    generation: 0,
                    state: QueueClaimState::Claimed,
                    worker: None,
                    dispatch_policy: None,
                    tracked_item_aliases: Vec::new(),
                    claimed_at_secs: 2,
                    expires_at_secs: 90,
                    owner_session: None,
                    anchor: None,
                    followed_from: Vec::new(),
                    followed_generation: 0,
                },
            ],
            cancellations: Vec::new(),
        };
        assert!(ledger.rekey());
        assert_eq!(ledger.claims.len(), 1, "one work identity, one claim");
        assert_eq!(ledger.claims[0].owner, "subagent:b");
        assert!(
            ledger
                .claimed_items(10, None)
                .claims(&format!("#subagents: #gh-fix {url}"))
        );
        assert!(!ledger.rekey(), "rekey is idempotent");
    }

    /// GH #124 ask 5: a head present in the editor authority (what dispatch
    /// saw) but not yet saved to disk is still claimable.
    #[test]
    fn claim_target_resolves_against_editor_authority_when_available() {
        let disk = vec!["do [#restartideload]".to_string()];
        let authority = vec![
            "do [#restartideload]".to_string(),
            "#bug: The tmux panes are swapped.".to_string(),
        ];
        assert_eq!(
            resolve_claim_target_in_views(
                "#bug: The tmux panes are swapped.",
                &disk,
                Some(&authority)
            )
            .unwrap(),
            (
                "#bug: The tmux panes are swapped.".to_string(),
                ClaimHeadSource::EditorAuthority
            )
        );
        assert_eq!(
            resolve_claim_target_in_views("#restartideload", &disk, Some(&authority))
                .unwrap()
                .1,
            ClaimHeadSource::EditorAuthority
        );
        assert!(
            resolve_claim_target_in_views("#bug: The tmux panes are swapped.", &disk, None)
                .is_err()
        );
        let miss = resolve_claim_target_in_views("#nosuch", &disk, Some(&authority)).unwrap_err();
        assert!(
            miss.live_heads
                .contains(&"#bug: The tmux panes are swapped.".to_string()),
            "{miss}"
        );
    }

    #[test]
    fn claim_target_rejects_a_stale_disk_head_deleted_from_editor_authority() {
        let disk = vec!["#gh-issue https://example.test/issues/296".to_string()];
        let authority = vec!["do [#still-live]".to_string()];

        let miss = resolve_claim_target_in_views(
            "#gh-issue https://example.test/issues/296",
            &disk,
            Some(&authority),
        )
        .unwrap_err();

        assert_eq!(miss.live_heads, authority);
        assert!(miss.ambiguous.is_empty());
    }

    #[test]
    fn expiry_and_closure_end_a_claim() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("do [#a]", "subagent:a", 100, 10);
        ledger.claim("free text item", "subagent:b", 100, 1000);
        assert!(ledger.claimed_items(105, None).claims("do [#a]"));
        assert!(
            !ledger.claimed_items(110, None).claims("do [#a]"),
            "an expired claim must restore drainability"
        );
        let live: HashSet<_> = [claim_identity("do [#a]")].into_iter().collect();
        assert!(
            !ledger
                .claimed_items(105, Some(&live))
                .claims("free text item"),
            "a claim on an item no longer in the queue is closed"
        );
        assert_eq!(ledger.prune(110, Some(&live)), 2);
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Lost);
        assert_eq!(ledger.claims[0].item_text, "do [#a]");
    }

    /// `#strikeretires`: a strike ends the struck row's finished attempt
    /// (`Reported`, or `Lost` by expiry) at once; a still-dispatching or
    /// running attempt, and an attempt whose identity keeps a live row, stay.
    #[test]
    fn retire_struck_ends_only_finished_attempts_without_a_live_row() {
        let mut ledger = reported_release_ledger(100);
        ledger.claim("running item", "subagent:run", 100, 3600);
        let generation = ledger.claims[1].generation;
        ledger
            .start(
                "running item",
                "subagent:run",
                "worker:run",
                100,
                3600,
                Some(generation),
            )
            .unwrap();
        ledger.claim("dispatching item", "subagent:dispatch", 100, 3600);
        ledger.claim("expired item", "subagent:expired", 100, 10);
        ledger.claim("mirrored item", "subagent:mirror", 100, 3600);
        let generation = ledger.claims[4].generation;
        ledger
            .start(
                "mirrored item",
                "subagent:mirror",
                "worker:mirror",
                100,
                3600,
                Some(generation),
            )
            .unwrap();
        ledger.report("mirrored item", Some(generation)).unwrap();
        ledger.claim("unstruck item", "subagent:other", 100, 10);

        let struck: HashSet<_> = [
            "release + publish",
            "running item",
            "dispatching item",
            "expired item",
            "mirrored item",
        ]
        .into_iter()
        .map(claim_identity)
        .collect();
        let live: HashSet<_> = [claim_identity("mirrored item")].into_iter().collect();
        let retired = ledger.retire_struck(200, &struck, &live);

        let retired_states: Vec<_> = retired
            .iter()
            .map(|claim| (claim.item_text.as_str(), claim.state))
            .collect();
        assert_eq!(
            retired_states,
            vec![
                ("release + publish", QueueClaimState::Reported),
                ("expired item", QueueClaimState::Lost),
            ]
        );
        let kept: Vec<_> = ledger
            .claims
            .iter()
            .map(|claim| claim.item_text.as_str())
            .collect();
        assert_eq!(
            kept,
            vec![
                "running item",
                "dispatching item",
                "mirrored item",
                "unstruck item"
            ],
            "a live attempt, a mirrored identity, and an unstruck row keep their claims"
        );
    }

    #[test]
    fn generation_fences_late_heartbeat_report_and_release() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("work", "subagent:a", 100, 10);
        assert_eq!(ledger.claims[0].generation, 1);
        ledger
            .start("work", "subagent:a", "worker:a", 100, 10, Some(1))
            .unwrap();
        assert_eq!(
            ledger
                .refresh_attempt_in_session("work", "subagent:a", 101, 10, None, Some(1))
                .unwrap()
                .state,
            QueueClaimState::Running
        );
        ledger.prune(111, Some(&[claim_identity("work")].into_iter().collect()));
        assert_eq!(ledger.claims[0].state, QueueClaimState::Lost);
        assert_eq!(
            ledger.claim("work", "subagent:b", 112, 10),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims[0].generation, 2);
        ledger
            .start("work", "subagent:b", "worker:b", 112, 10, Some(2))
            .unwrap();
        assert!(matches!(
            ledger.report("work", Some(1)),
            Err(QueueClaimTransitionRefused::StaleGeneration { current: 2, .. })
        ));
        assert!(matches!(
            ledger.release_fenced("work", Some(1)),
            Err(QueueClaimTransitionRefused::StaleGeneration { current: 2, .. })
        ));
        assert_eq!(ledger.claims[0].owner, "subagent:b");
        assert_eq!(
            ledger.report("work", Some(2)).unwrap().state,
            QueueClaimState::Reported
        );
        assert!(ledger.claims[0].blocks_dispatch(10_000));
    }

    #[test]
    fn cancellation_tombstone_fences_stale_completion_and_advances_readd_generation() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("work", "subagent:a", 100, 100);
        ledger
            .start("work", "subagent:a", "worker:a", 100, 100, Some(1))
            .unwrap();
        ledger.report("work", Some(1)).unwrap();

        let receipt = ledger.cancel_attempt("work", Some(1), 105).unwrap();
        assert_eq!(receipt.claim.generation, 1);
        assert_eq!(receipt.claim.state, QueueClaimState::Reported);
        assert_eq!(receipt.cancelled_at_secs, 105);
        assert!(receipt.completion_key.is_some());
        assert!(ledger.claims.is_empty());
        assert_eq!(ledger.cancellations, vec![receipt.clone()]);
        assert_eq!(
            ledger.cancel_attempt("work", Some(1), 106).unwrap(),
            receipt,
            "replayed removal is idempotent"
        );

        assert_eq!(
            ledger.claim("work", "subagent:b", 110, 100),
            ClaimOutcome::Created
        );
        assert_eq!(ledger.claims[0].generation, 2);
        assert!(matches!(
            ledger.cancel_attempt("work", Some(1), 111),
            Ok(old) if old.claim.generation == 1
        ));
        assert_eq!(ledger.claims[0].generation, 2);
        assert!(matches!(
            ledger.report("work", Some(1)),
            Err(QueueClaimTransitionRefused::StaleGeneration { current: 2, .. })
        ));
    }

    #[test]
    fn worker_acknowledgement_is_injective_generation_fenced_and_fail_closed() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("work a", "subagent:a", 100, 10);
        ledger.claim("work b", "subagent:b", 100, 10);

        assert_eq!(ledger.claims[0].state, QueueClaimState::Claimed);
        assert!(ledger.claims[0].worker.is_none());
        assert!(matches!(
            ledger.refresh_attempt_in_session("work a", "subagent:a", 101, 10, None, Some(1)),
            Err(QueueClaimRefreshRefused {
                reason: RefreshRefusal::NotRunning,
                ..
            })
        ));
        assert!(matches!(
            ledger.report("work a", Some(1)),
            Err(QueueClaimTransitionRefused::NotRunning { generation: 1 })
        ));

        let running = ledger
            .start("work a", "subagent:a", "worker:one", 101, 10, Some(1))
            .unwrap();
        assert_eq!(running.state, QueueClaimState::Running);
        assert_eq!(
            running
                .worker
                .as_ref()
                .map(|binding| binding.worker_id.as_str()),
            Some("worker:one")
        );
        assert_eq!(
            ledger
                .start("work a", "subagent:a", "worker:one", 102, 10, Some(1))
                .unwrap(),
            running,
            "the exact acknowledgement replay is idempotent"
        );
        assert!(matches!(
            ledger.start("work a", "subagent:a", "worker:two", 102, 10, Some(1)),
            Err(QueueClaimStartRefused::AttemptAlreadyBound { .. })
        ));
        assert!(matches!(
            ledger.start("work b", "subagent:b", "worker:one", 102, 10, Some(1)),
            Err(QueueClaimStartRefused::WorkerAlreadyBound { .. })
        ));

        ledger.prune(
            111,
            Some(
                &[claim_identity("work a"), claim_identity("work b")]
                    .into_iter()
                    .collect(),
            ),
        );
        assert_eq!(
            ledger.claim("work a", "subagent:new", 112, 10),
            ClaimOutcome::Reassigned
        );
        assert!(matches!(
            ledger.start("work a", "subagent:a", "worker:late", 112, 10, Some(1)),
            Err(QueueClaimStartRefused::OtherOwner { .. })
                | Err(QueueClaimStartRefused::StaleAttempt { .. })
        ));
        let replacement = ledger
            .claims
            .iter()
            .find(|claim| claim.item_text == "work a")
            .unwrap();
        assert_eq!(replacement.generation, 2);
        assert_eq!(replacement.state, QueueClaimState::Claimed);
        assert!(replacement.worker.is_none());
    }

    #[test]
    fn stored_running_without_worker_downgrades_and_duplicate_worker_fails_closed() {
        let legacy_running = r#"{"claims":[{"identity":{"kind":"free_text","key":"work"},"item_text":"work","owner":"subagent:a","generation":1,"state":"running","claimed_at_secs":1,"expires_at_secs":50}]}"#;
        let ledger = QueueClaimLedger::from_state_payload(legacy_running).unwrap();
        assert_eq!(ledger.claims[0].state, QueueClaimState::Claimed);

        let duplicate = r#"{"claims":[{"identity":{"kind":"free_text","key":"a"},"item_text":"a","owner":"subagent:a","generation":1,"state":"running","worker":{"worker_id":"worker:same","started_at_secs":2},"claimed_at_secs":1,"expires_at_secs":50},{"identity":{"kind":"free_text","key":"b"},"item_text":"b","owner":"subagent:b","generation":1,"state":"running","worker":{"worker_id":"worker:same","started_at_secs":2},"claimed_at_secs":1,"expires_at_secs":50}]}"#;
        assert!(QueueClaimLedger::from_state_payload(duplicate).is_err());
    }

    #[test]
    fn legacy_claim_json_defaults_to_generation_zero_claimed() {
        let payload = r#"{"claims":[{"identity":{"kind":"free_text","key":"work"},"item_text":"work","owner":"subagent:a","claimed_at_secs":1,"expires_at_secs":50}]}"#;
        let ledger = QueueClaimLedger::from_state_payload(payload).unwrap();
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.claims[0].generation, 0);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Claimed);
        assert!(ledger.claims[0].blocks_dispatch(2));
    }

    /// `#claimfollowsedit` (#steerworks): the 2026-10-04 agent-doc-bugs.md
    /// shapes. A claimed `#gh-fix <url>` annotated with `: note …`, then given
    /// a trailing period, keeps ONE claim with its owner; the coordinator never
    /// re-claims by the new text.
    #[test]
    fn a_claim_follows_operator_annotations_of_its_head() {
        let url = "https://github.com/btakita/agent-doc/issues/126";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(&format!("#gh-fix {url}"), "subagent:ghfix126", 100, 3600);

        let annotated = format!("#gh-fix {url}: note this is in a Coder environment");
        let followed = ledger.follow_edits(200, std::slice::from_ref(&annotated));
        assert_eq!(followed.len(), 1);
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.claims[0].identity, claim_identity(&annotated));
        assert_eq!(ledger.claims[0].owner, "subagent:ghfix126");
        // The on-load re-key keeps the followed identity.
        assert!(!ledger.rekey());

        let period = format!("{annotated}.");
        ledger.follow_edits(300, std::slice::from_ref(&period));
        assert_eq!(ledger.claims[0].identity, claim_identity(&period));
        let live: HashSet<_> = [claim_identity(&period)].into_iter().collect();
        assert!(ledger.claimed_items(300, Some(&live)).claims(&period));
        assert_eq!(
            ledger
                .owners(300, Some(&live))
                .get(&claim_identity(&period)),
            Some(&"subagent:ghfix126".to_string())
        );
        // Released by the text the worker was originally given.
        assert!(ledger.release(&format!("#gh-fix {url}")).is_some());
        assert!(ledger.claims.is_empty());
    }

    #[test]
    fn a_claim_does_not_follow_a_retarget_or_an_ambiguous_edit() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("#gh-fix https://x.test/issues/126", "subagent:a", 100, 3600);
        // A different issue number is a different task.
        assert!(
            ledger
                .follow_edits(200, &["#gh-fix https://x.test/issues/1260".to_string()])
                .is_empty()
        );
        // Two heads both continue the claim: never guess.
        assert!(
            ledger
                .follow_edits(
                    200,
                    &[
                        "#gh-fix https://x.test/issues/126: note a".to_string(),
                        "#gh-fix https://x.test/issues/126: note b".to_string(),
                    ],
                )
                .is_empty()
        );
        // The claimed head is still live: an extra annotated line is new work.
        assert!(
            ledger
                .follow_edits(
                    200,
                    &[
                        "#gh-fix https://x.test/issues/126".to_string(),
                        "#gh-fix https://x.test/issues/126: also".to_string(),
                    ],
                )
                .is_empty()
        );
        // An expired claim never follows.
        assert!(
            ledger
                .follow_edits(
                    10_000,
                    &["#gh-fix https://x.test/issues/126: note".to_string()]
                )
                .is_empty()
        );
    }

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    /// `#claimfollowedit` (agent-doc-bugs.md, 2026-10-15): the queue held two
    /// copies of `#gh-fix …/agent-foc/issues/272` (a duplication bug) when the
    /// coordinator claimed it. The operator removed the duplicate block, then
    /// corrected the line to `…/agent-doc/issues/272`. Keyed by text, the claim
    /// matched no live head, steering offered the edited line as new subagent
    /// work, and closeout logged `queue_claim_prune pruned=1`. The `#270` line
    /// "kept" its claim only because that claim had been made on the other
    /// copy, whose text already read `agent-doc`. A claim binds to its line:
    /// the in-place edit carries it.
    #[test]
    fn a_claim_follows_an_in_place_edit_of_its_line_through_a_duplicate_cleanup() {
        let foc = "#gh-fix https://github.com/btakita/agent-foc/issues/272";
        let fixed = "#gh-fix https://github.com/btakita/agent-doc/issues/272";
        let doubled = strings(&[
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            foc,
            "release + publish",
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            foc,
            "release + publish",
        ]);
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(foc, "subagent:gh272", 100, 3600);
        assert!(ledger.follow_edits(100, &doubled).is_empty());
        let anchor = ledger.claims[0]
            .anchor
            .clone()
            .expect("anchored on its lines");
        assert_eq!(anchor.slots, vec![1, 4], "both identical lines are held");

        // The operator deletes the duplicate block: the claim still holds.
        let single = strings(&[
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            foc,
            "release + publish",
        ]);
        assert!(ledger.follow_edits(110, &single).is_empty());
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![1]);

        // ...then corrects the URL in place.
        let corrected = strings(&[
            "#gh-fix https://github.com/btakita/agent-doc/issues/271",
            fixed,
            "release + publish",
        ]);
        let followed = ledger.follow_edits(120, &corrected);
        assert_eq!(followed, vec![(foc.to_string(), fixed.to_string())]);
        assert_eq!(ledger.claims[0].identity, claim_identity(fixed));
        assert_eq!(ledger.claims[0].owner, "subagent:gh272");
        assert_eq!(ledger.claims[0].followed_from, vec![foc.to_string()]);
        let live: HashSet<_> = corrected.iter().map(|head| claim_identity(head)).collect();
        assert!(ledger.claimed_items(120, Some(&live)).claims(fixed));
        assert_eq!(ledger.prune(120, Some(&live)), 0, "never pruned as closed");

        // The worker still names it by the text it was handed.
        assert_eq!(ledger.followed_item_text(foc), Some(fixed));
        assert!(
            ledger.release(foc).is_some(),
            "released by the original text"
        );
        assert!(ledger.claims.is_empty());
    }

    /// Two identical claimed lines are one claim. Editing one copy leaves the
    /// claim on the other; once that copy goes too, the claim follows the
    /// edited copy instead of closing.
    #[test]
    fn a_duplicate_claimed_line_keeps_the_claim_until_every_copy_is_gone() {
        let foc = "#gh-fix https://x.test/agent-foc/issues/272";
        let fixed = "#gh-fix https://x.test/agent-doc/issues/272";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(foc, "subagent:gh272", 100, 3600);
        ledger.follow_edits(100, &strings(&[foc, "inline", foc]));

        let one_edited = strings(&[fixed, "inline", foc]);
        assert!(ledger.follow_edits(110, &one_edited).is_empty());
        assert_eq!(ledger.claims[0].identity, claim_identity(foc));
        assert_eq!(
            ledger.claims[0].anchor.as_ref().unwrap().slots,
            vec![0, 2],
            "the edited copy stays one of the claim's lines"
        );

        let other_deleted = strings(&[fixed, "inline"]);
        assert_eq!(
            ledger.follow_edits(120, &other_deleted),
            vec![(foc.to_string(), fixed.to_string())]
        );
        assert_eq!(ledger.claims[0].identity, claim_identity(fixed));
    }

    /// A reported claim on `release + publish`, rendered as the binary
    /// projects it: `☑️ [#<owner>#<generation>] <text>`.
    fn reported_release_row(ledger: &QueueClaimLedger, text: &str) -> String {
        use agent_doc_document::queue_projection::{
            QueueLifecycleMarker, QueueOwnerProjection, apply_lifecycle_marker,
            apply_owner_projection,
        };
        let claim = &ledger.claims[0];
        apply_owner_projection(
            &apply_lifecycle_marker(text, QueueLifecycleMarker::Reported),
            Some(&QueueOwnerProjection {
                owner: claim.owner.clone(),
                generation: claim.generation,
            }),
        )
    }

    fn reported_release_ledger(now: u64) -> QueueClaimLedger {
        let mut ledger = QueueClaimLedger::default();
        let owner = "subagent:release_publish_patch";
        ledger.claim("release + publish", owner, now, 3600);
        let generation = ledger.claims[0].generation;
        ledger
            .start(
                "release + publish",
                owner,
                "worker:release",
                now,
                3600,
                Some(generation),
            )
            .unwrap();
        ledger
            .report("release + publish", Some(generation))
            .unwrap();
        assert_eq!(ledger.claims[0].state, QueueClaimState::Reported);
        ledger
    }

    /// `#claimrowmigrate` (agent-doc-bugs.md, 2026-10-26): a 52h-old reported
    /// `release + publish` claim sat on its row when the operator restored a
    /// text-identical `release + publish` row ABOVE it. Every ledger read then
    /// widened the claim's anchor onto the restored copy (one identity, two
    /// slots), and when preflight annotated the pair `⟦effect:1⟧`/`⟦effect:2⟧`
    /// the legacy migration followed "the first descendant": the claim jumped
    /// onto the restored row and its own row became `📥`. A located claim is
    /// bound to its row; a text-identical sibling never joins its slots.
    #[test]
    fn a_located_claim_never_widens_onto_a_restored_identical_sibling() {
        let now = 100;
        let mut ledger = reported_release_ledger(now);
        let claimed = reported_release_row(&ledger, "release + publish");
        ledger.follow_edits(
            now,
            &strings(&["#gh-fix https://x.test/issues/1", &claimed]),
        );
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![1]);

        // 52 hours later the operator restores an identical, unclaimed row above.
        let later = now + 52 * 3600;
        let restored = strings(&[
            "release + publish",
            "#gh-fix https://x.test/issues/1",
            &claimed,
        ]);
        assert!(ledger.follow_edits(later, &restored).is_empty());
        assert_eq!(
            ledger.claims[0].anchor.as_ref().unwrap().slots,
            vec![2],
            "the restored copy is not this claim's row"
        );

        // Preflight annotates the duplicate pair in queue order.
        let annotated = strings(&[
            "release + publish ⟦effect:1⟧",
            "#gh-fix https://x.test/issues/1",
            &reported_release_row(&ledger, "release + publish ⟦effect:2⟧"),
        ]);
        let followed = ledger.follow_edits(later + 1, &annotated);
        assert_eq!(
            followed,
            vec![(
                "release + publish".to_string(),
                "release + publish ⟦effect:2⟧".to_string()
            )]
        );
        let claim = &ledger.claims[0];
        assert_eq!(
            claim.identity,
            claim_identity("release + publish ⟦effect:2⟧")
        );
        assert_eq!(claim.owner, "subagent:release_publish_patch");
        assert_eq!(claim.state, QueueClaimState::Reported);
        assert_eq!(claim.anchor.as_ref().unwrap().slots, vec![2]);
        let live: HashSet<_> = annotated
            .iter()
            .map(|head| {
                claim_identity(
                    &agent_doc_document::queue_projection::strip_in_progress_marker(head),
                )
            })
            .collect();
        let claimed_items = ledger.claimed_items(later + 1, Some(&live));
        assert!(claimed_items.claims("release + publish ⟦effect:2⟧"));
        assert!(
            !claimed_items.claims("release + publish ⟦effect:1⟧"),
            "the restored sibling never inherits the reported claim"
        );
    }

    /// `#claimrowmigrate`: the same incident when the restore and the effect
    /// annotation land in ONE queue cut, with the restored row directly above
    /// the claimed one. Text alignment alone cannot tell the two copies apart
    /// (both annotated rows contain the claimed text), so the claim follows the
    /// row carrying its own `[#owner#generation]` projection.
    #[test]
    fn an_effect_annotation_follows_the_row_carrying_the_claims_own_projection() {
        let now = 100;
        let mut ledger = reported_release_ledger(now);
        let claimed = reported_release_row(&ledger, "release + publish");
        ledger.follow_edits(now, &strings(&["inline", &claimed]));
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![1]);

        let annotated = strings(&[
            "inline",
            "release + publish ⟦effect:1⟧",
            &reported_release_row(&ledger, "release + publish ⟦effect:2⟧"),
        ]);
        let followed = ledger.follow_edits(now + 1, &annotated);
        assert_eq!(
            followed,
            vec![(
                "release + publish".to_string(),
                "release + publish ⟦effect:2⟧".to_string()
            )]
        );
        assert_eq!(
            ledger.claims[0].identity,
            claim_identity("release + publish ⟦effect:2⟧")
        );
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![2]);

        // A further read of the same cut is stable.
        assert!(ledger.follow_edits(now + 2, &annotated).is_empty());
        assert_eq!(
            ledger.claims[0].identity,
            claim_identity("release + publish ⟦effect:2⟧")
        );
    }

    /// `#claimrowmigrate`: a legacy claim that legitimately anchored every
    /// identical copy migrates onto the copy carrying its projection, and an
    /// owner annotation for a DIFFERENT generation is not evidence.
    #[test]
    fn a_legacy_multi_slot_claim_migrates_onto_its_projected_row_only() {
        let now = 100;
        let mut ledger = reported_release_ledger(now);
        ledger.follow_edits(now, &strings(&["release + publish", "release + publish"]));
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![0, 1]);

        let generation = ledger.claims[0].generation;
        let stale = agent_doc_document::queue_projection::apply_owner_projection(
            "release + publish ⟦effect:1⟧",
            Some(
                &agent_doc_document::queue_projection::QueueOwnerProjection {
                    owner: ledger.claims[0].owner.clone(),
                    generation: generation + 7,
                },
            ),
        );
        let annotated = vec![
            stale,
            reported_release_row(&ledger, "release + publish ⟦effect:2⟧"),
        ];
        ledger.follow_edits(now + 1, &annotated);
        assert_eq!(
            ledger.claims[0].identity,
            claim_identity("release + publish ⟦effect:2⟧")
        );
        assert_eq!(ledger.claims[0].anchor.as_ref().unwrap().slots, vec![1]);
    }

    /// A deleted claimed line closes its claim, while an in-place rewrite keeps
    /// the attempt. A similar line added in another place never inherits it.
    #[test]
    fn a_claim_follows_any_in_place_rewrite_but_not_a_distant_add() {
        let line = "#gh-fix https://x.test/issues/7";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(line, "subagent:gh7", 100, 3600);
        ledger.follow_edits(100, &strings(&["inline", line, "release"]));

        let deleted = strings(&["inline", "release"]);
        assert!(ledger.follow_edits(110, &deleted).is_empty());
        let live: HashSet<_> = deleted.iter().map(|head| claim_identity(head)).collect();
        let pruned = ledger.clone().prune_detailed(110, Some(&live));
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].1, ClaimPruneCause::Closed);

        let elsewhere = strings(&["#gh-fix https://x.test/issues/8", "inline", "release"]);
        assert!(ledger.follow_edits(110, &elsewhere).is_empty());

        let mut ids = QueueClaimLedger::default();
        ids.claim("do [#a]", "subagent:a", 100, 3600);
        ids.follow_edits(100, &strings(&["inline", "do [#a]"]));
        assert_eq!(
            ids.follow_edits(110, &strings(&["inline", "do [#b]"])),
            vec![("do [#a]".to_string(), "do [#b]".to_string())],
            "an in-place id change steers the existing worker"
        );
        assert_eq!(ids.claims[0].owner, "subagent:a");
        assert_eq!(ids.claims[0].item_text, "do [#b]");

        let rewritten = "replace this with a wholly different request";
        assert_eq!(
            ids.follow_edits(120, &strings(&["inline", rewritten])),
            vec![("do [#b]".to_string(), rewritten.to_string())]
        );
        assert_eq!(ids.claims[0].owner, "subagent:a");
        assert_eq!(ids.claims[0].item_text, rewritten);
    }

    /// `#claimtrackedalias` (sdk-demo.md, 2026-10-21): a running worker held
    /// `do [#rebaseexistingproject]`, then backlog reconciliation retired the
    /// id-backed mirror while leaving the tracked item's exact question as a
    /// free-text queue head. The claim-time tracked-item receipt supplies the
    /// explicit identity bridge without weakening generic id/free-text edits.
    #[test]
    fn an_id_claim_follows_its_unique_exact_tracked_item_alias() {
        let id_head = "do [#rebaseexistingproject]";
        let question = "Why can't we just rebase the existing Project source code into the aggregate PR branch with the fixes?";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(id_head, "subagent:project_rebase_review", 100, 3600);
        ledger.claims[0].tracked_item_aliases = vec![question.to_string()];
        ledger.follow_edits(100, &strings(&["before", id_head, "after"]));

        let live = strings(&["before", question, "after"]);
        assert_eq!(
            ledger.follow_edits(110, &live),
            vec![(id_head.to_string(), question.to_string())]
        );
        assert_eq!(ledger.claims[0].identity, claim_identity(question));
        assert_eq!(ledger.claims[0].item_text, question);
        assert_eq!(ledger.claims[0].followed_from, vec![id_head.to_string()]);

        let live_identities: HashSet<_> = live.iter().map(|head| claim_identity(head)).collect();
        assert_eq!(ledger.prune(110, Some(&live_identities)), 0);
        assert!(
            ledger
                .claimed_items(110, Some(&live_identities))
                .claims(question)
        );
    }

    #[test]
    fn an_id_claim_does_not_guess_between_duplicate_tracked_item_aliases() {
        let question = "Why can't we rebase the project?";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("do [#rebase]", "subagent:rebase", 100, 3600);
        ledger.claims[0].tracked_item_aliases = vec![question.to_string()];
        ledger.follow_edits(100, &strings(&["do [#rebase]"]));

        assert!(
            ledger
                .follow_edits(110, &strings(&[question, question]))
                .is_empty(),
            "duplicate exact aliases are ambiguous and must not inherit ownership"
        );
        assert_eq!(ledger.claims[0].identity, claim_identity("do [#rebase]"));
    }

    /// An edited line another live claim already holds is never taken over,
    /// and an expired claim never follows.
    #[test]
    fn a_claim_never_follows_onto_another_claims_line_or_after_expiry() {
        let a = "#gh-fix https://x.test/issues/10";
        let b = "#gh-fix https://x.test/issues/11";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(a, "subagent:a", 100, 3600);
        ledger.claim(b, "subagent:b", 100, 3600);
        ledger.follow_edits(100, &strings(&[a, b]));
        // `a` edited into `b`'s text: that line is `b`'s claim, not `a`'s.
        assert!(
            ledger
                .clone()
                .follow_edits(110, &strings(&[b, b]))
                .is_empty()
        );
        // The operator deletes `a` and the similar `b` stays: `a` closes.
        assert!(ledger.follow_edits(110, &strings(&[b])).is_empty());
        assert_eq!(ledger.claims[0].identity, claim_identity(a));

        let mut expired = QueueClaimLedger::default();
        expired.claim(a, "subagent:a", 100, 10);
        expired.follow_edits(100, &strings(&[a]));
        assert!(
            expired
                .follow_edits(500, &strings(&["#gh-fix https://x.test/issues/12"]))
                .is_empty()
        );
    }

    #[test]
    fn edit_continuation_needs_a_word_boundary_and_substance() {
        let id = |text: &str| claim_identity(text);
        assert!(is_edit_continuation(
            &id("Ensure the real-time reactive steering works."),
            &id("Ensure the real-time reactive steering works. When do you get the signal?")
        ));
        assert!(is_edit_continuation(
            &id("note this is in a Coder environment"),
            &id("note this is in a Coder environment.")
        ));
        assert!(!is_edit_continuation(&id("fix it"), &id("fix it now")));
        assert!(!is_edit_continuation(&id("do [#a]"), &id("do [#a] now")));
        assert!(!is_edit_continuation(
            &id("https://x.test/issues/12"),
            &id("https://x.test/issues/126")
        ));
    }

    #[test]
    fn claim_target_resolves_the_annotated_head_from_the_original_text() {
        let heads = vec!["#gh-fix https://x.test/issues/126: note coder".to_string()];
        assert_eq!(
            resolve_claim_target("#gh-fix https://x.test/issues/126", &heads).unwrap(),
            heads[0]
        );
    }

    /// `#queueeffectidentity`: a bare family name cannot silently select the
    /// first of several effects, while either exact annotation is claimable.
    #[test]
    fn duplicate_effect_annotations_are_exact_claim_targets() {
        let heads = vec![
            "release + publish ⟦effect:1⟧".to_string(),
            "release + publish ⟦effect:2⟧".to_string(),
        ];
        let miss = resolve_claim_target("release + publish", &heads).unwrap_err();
        assert_eq!(miss.ambiguous, heads);
        assert_eq!(
            resolve_claim_target("release + publish ⟦effect:2⟧", &heads).unwrap(),
            heads[1]
        );
        assert_ne!(claim_identity(&heads[0]), claim_identity(&heads[1]));
    }

    /// `#multilinequeuehead`: `queue claim` / `queue brief --item` name a
    /// multi-line head by its title line, unambiguously.
    #[test]
    fn multilinequeuehead_title_line_resolves_the_multiline_head() {
        use crate::document_queue::multiline_pasted_fixture::{BETA_HEAD, QUEUE_BODY};
        let doc = format!("<!-- agent:queue -->\n{QUEUE_BODY}<!-- /agent:queue -->\n");
        let heads = crate::queue_heads::active_queue_heads(&doc);
        assert_eq!(
            resolve_claim_target("beta.md issue", &heads).unwrap(),
            BETA_HEAD
        );
        assert!(
            resolve_claim_target("alpha.md issue", &heads)
                .unwrap()
                .contains("alpha.md issue\n```")
        );
        // The full head text still resolves to itself.
        assert_eq!(resolve_claim_target(BETA_HEAD, &heads).unwrap(), BETA_HEAD);

        let twin = vec![
            "bug\n```\nlog a\n```".to_string(),
            "bug\n```\nlog b\n```".to_string(),
        ];
        let miss = resolve_claim_target("bug", &twin).unwrap_err();
        assert_eq!(miss.ambiguous, twin);
    }

    #[test]
    fn bare_paragraph_after_list_item_resolves_as_its_own_claim_target() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- Does the queue split free text after a list item?\n",
            "The terminal windows closed because a scope was left behind.\n",
            "> diagnostic detail\n",
            "<!-- /agent:queue -->\n",
        );
        let heads = crate::queue_heads::active_queue_heads(doc);
        assert_eq!(heads.len(), 2, "{heads:?}");
        assert_eq!(
            resolve_claim_target(
                "The terminal windows closed because a scope was left behind.",
                &heads,
            )
            .unwrap(),
            "The terminal windows closed because a scope was left behind.\n> diagnostic detail"
        );
    }

    #[test]
    fn lifecycle_detector_self_heals_only_projection_drift() {
        use agent_doc_document::queue_projection::QueueLifecycleMarker;
        let mut ledger = QueueClaimLedger::default();
        assert_eq!(
            ledger.claim("fix queue", "subagent:q", 100, 600),
            ClaimOutcome::Created
        );
        let claim = &ledger.claims[0];

        let drift = diagnose_lifecycle(claim, None, 101, 5).unwrap();
        assert_eq!(drift.invariant, QueueLifecycleInvariant::ProjectionDrift);
        assert_eq!(drift.expected, QueueLifecycleMarker::Claimed);
        assert_eq!(drift.worker_liveness, WorkerLivenessEvidence::Unknown);
        assert_eq!(
            drift.correction,
            QueueInvariantCorrection::ProjectExpectedMarker
        );

        let missing =
            diagnose_lifecycle(claim, Some(QueueLifecycleMarker::Claimed), 106, 5).unwrap();
        assert_eq!(
            missing.invariant,
            QueueLifecycleInvariant::DispatchEvidenceMissing
        );
        assert_eq!(missing.worker_liveness, WorkerLivenessEvidence::Unknown);
        assert_eq!(missing.correction, QueueInvariantCorrection::None);
        assert!(!dispatch_evidence_missing(claim, 104, 5));
        assert!(dispatch_evidence_missing(claim, 105, 5));
        assert!(
            dispatch_evidence_missing(claim, 106, 5),
            "projection drift must not hide the independent coordinator obligation"
        );
    }

    #[test]
    fn generation_fenced_heartbeat_is_positive_worker_evidence() {
        use agent_doc_document::queue_projection::QueueLifecycleMarker;
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("fix queue", "subagent:q", 100, 600);
        ledger
            .start("fix queue", "subagent:q", "worker:q", 101, 600, Some(1))
            .unwrap();
        ledger.refresh("fix queue", "subagent:q", 102, 600).unwrap();
        let claim = &ledger.claims[0];
        assert_eq!(claim.state, QueueClaimState::Running);
        assert!(diagnose_lifecycle(claim, Some(QueueLifecycleMarker::Running), 500, 5).is_none());
    }

    /// GH #319 claim visibility: a later ledger read gained a claim only when it
    /// holds an identity the earlier read did not. A claim released or expired
    /// between the reads is not a gain, so it never blocks a dispatch.
    #[test]
    fn claimed_items_gained_since_reports_only_new_identities() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("#upgrade", "coordinator:x", 100, 600);
        let earlier = ledger.claimed_items(100, None);
        assert!(!ledger.claimed_items(100, None).gained_since(&earlier));

        ledger.claim("fix queue", "subagent:q", 101, 600);
        let later = ledger.claimed_items(101, None);
        assert!(later.gained_since(&earlier), "a new claim is a gain");
        assert!(
            !earlier.gained_since(&later),
            "a missing claim is not a gain"
        );

        ledger.release("#upgrade");
        assert!(
            !ledger.claimed_items(102, None).gained_since(&later),
            "a released claim is not a gain"
        );
        assert!(
            !ClaimedQueueItems::none().gained_since(&ClaimedQueueItems::none()),
            "two empty reads gained nothing"
        );
    }

    /// A session document with an `agent:run` queue and an `agent:loop`
    /// queue, the shape of agent-doc-bugs.md on 2026-10-27.
    fn run_loop_doc(run: &[&str], loop_rows: &[&str]) -> String {
        let rows =
            |items: &[&str]| -> String { items.iter().map(|item| format!("- {item}\n")).collect() };
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Run <!-- agent:run subagents go -->\n{}<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop interval=\"1h\" -->\n{}<!-- /agent:loop -->\n",
            rows(run),
            rows(loop_rows),
        )
    }

    fn view_of(content: &str) -> ClaimQueueView {
        let view = ClaimQueueView::from_content(content).expect("queue graph parses");
        assert_eq!(view.queues.len(), 2, "{view:?}");
        view
    }

    const KOTLINFLUSH: &str = "do [#kotlinflushprep]";
    const LOOP_WATCH: &str = "Watch https://github.com/btakita/agent-doc/issues for new issues. \
Add new issues into the backlog + queue except issue #40 + #45.";
    const GH380: &str = "#gh-fix https://github.com/btakita/agent-doc/issues/380";
    const GH381: &str = "#gh-fix https://github.com/btakita/agent-doc/issues/381";

    fn running_kotlinflush(now: u64) -> QueueClaimLedger {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(KOTLINFLUSH, "subagent:kotlinflush", now, 3600);
        let generation = ledger.claims[0].generation;
        ledger
            .start(
                KOTLINFLUSH,
                "subagent:kotlinflush",
                "a200708c59007b127",
                now,
                3600,
                Some(generation),
            )
            .unwrap();
        ledger
    }

    /// `#claimfollowstruck` (agent-doc-bugs.md, 2026-10-27): the claim for
    /// `subagent:kotlinflush` gen 1, dispatched for `do [#kotlinflushprep]`,
    /// moved onto the `agent:loop` watch row once `#kotlinflushprep` was
    /// struck (the strike drained `agent:run`, so the document's selected
    /// queue became `agent:loop`, and a one-row anchor paired structurally
    /// with its one row), then onto the `#gh-fix …/issues/380` row when new
    /// `agent:run` heads arrived. #380 showed `🚧` with no worker and was never
    /// dispatched. A strike ends the row's claim; no successor head, in the
    /// same queue or a sibling queue, inherits it.
    #[test]
    fn a_struck_row_claim_never_follows_onto_the_next_head_in_any_queue() {
        let now = 1_000;
        let mut ledger = running_kotlinflush(now);
        let claimed = run_loop_doc(&[KOTLINFLUSH], &[LOOP_WATCH]);
        let outcome = ledger.follow_edits_in_view(now, &view_of(&claimed));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        let run_id = view_of(&claimed).queues[0].queue_id.clone();
        assert!(run_id.is_some());
        assert_eq!(
            ledger.claims[0].anchor.as_ref().unwrap().queue_id,
            run_id,
            "the anchor names the queue that holds the claimed row"
        );

        // The coordinator strikes the finished row: `agent:run` is drained.
        let struck_text = format!("~~{KOTLINFLUSH}~~");
        let struck = run_loop_doc(&[&struck_text], &[LOOP_WATCH]);
        let outcome = ledger.follow_edits_in_view(now + 1, &view_of(&struck));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        assert_eq!(ledger.claims[0].identity, claim_identity(KOTLINFLUSH));

        // The loop pass adds new issues to `agent:run`.
        let next = run_loop_doc(&[&struck_text, GH380, GH381], &[LOOP_WATCH]);
        let outcome = ledger.follow_edits_in_view(now + 2, &view_of(&next));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        let claim = &ledger.claims[0];
        assert_eq!(claim.identity, claim_identity(KOTLINFLUSH));
        assert_eq!(claim.item_text, KOTLINFLUSH);
        assert!(claim.followed_from.is_empty(), "{:?}", claim.followed_from);
        let live: HashSet<_> = view_of(&next)
            .rows()
            .iter()
            .map(|row| claim_identity(row))
            .collect();
        let claimed_items = ledger.claimed_items(now + 2, Some(&live));
        for head in [GH380, GH381, LOOP_WATCH] {
            assert!(!claimed_items.claims(head), "{head} must stay dispatchable");
        }
        // The running attempt contradicts the strike: its own row reopens `❔`.
        assert!(struck_row_reopened_by_claim(Some(claim)));
    }

    /// `#claimfollowstruck`: the same-queue form. The claimed row is struck
    /// and a new head is added in the same successor, so the one-row anchor
    /// and the one live row form an equal-cardinality replace hunk, which the
    /// structural fallback pairs as an in-place edit. A strike is completion,
    /// not an edit: the new head stays unclaimed.
    #[test]
    fn strike_then_next_head_in_the_same_queue_does_not_inherit_the_claim() {
        let now = 2_000;
        let mut ledger = running_kotlinflush(now);
        ledger.follow_edits_in_view(now, &view_of(&run_loop_doc(&[KOTLINFLUSH], &[])));
        let struck_text = format!("~~{KOTLINFLUSH}~~");
        let successor = run_loop_doc(&[&struck_text, GH380], &[]);
        let outcome = ledger.follow_edits_in_view(now + 1, &view_of(&successor));
        assert!(outcome.followed.is_empty(), "{outcome:?}");
        assert_eq!(ledger.claims[0].identity, claim_identity(KOTLINFLUSH));

        // A finished attempt is then retired by the strike policy.
        let generation = ledger.claims[0].generation;
        ledger.report(KOTLINFLUSH, Some(generation)).unwrap();
        let struck: HashSet<_> = [claim_identity(KOTLINFLUSH)].into_iter().collect();
        let live: HashSet<_> = [claim_identity(GH380)].into_iter().collect();
        assert_eq!(ledger.retire_struck(now + 2, &struck, &live).len(), 1);
        assert!(ledger.claims.is_empty());
    }

    /// `#claimfollowstruck` keeps `#claimfollowedit`: an in-place edit of a
    /// claimed row still carries the claim, and stays inside its queue while a
    /// sibling queue's row sits at the same structural offset.
    #[test]
    fn an_in_place_edit_still_follows_within_its_own_queue() {
        let now = 3_000;
        let original = "#gh-fix https://github.com/btakita/agent-foc/issues/272";
        let edited = "#gh-fix https://github.com/btakita/agent-doc/issues/272";
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(original, "subagent:gh272", now, 3600);
        ledger.follow_edits_in_view(now, &view_of(&run_loop_doc(&[original], &[LOOP_WATCH])));
        let outcome =
            ledger.follow_edits_in_view(now + 1, &view_of(&run_loop_doc(&[edited], &[LOOP_WATCH])));
        assert_eq!(
            outcome.followed,
            vec![(original.to_string(), edited.to_string())]
        );
        assert_eq!(ledger.claims[0].identity, claim_identity(edited));

        // The row moves out of view of its queue (a drained `agent:run` while
        // the loop is selected): the claim keeps its anchor, never the loop row.
        let drained = run_loop_doc(&[], &[LOOP_WATCH]);
        let outcome = ledger.follow_edits_in_view(now + 2, &view_of(&drained));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        assert_eq!(ledger.claims[0].identity, claim_identity(edited));
    }

    /// `#claimfollowstruck` repair: a ledger that already followed off a
    /// struck row (the 2026-10-27 durable state: on `#gh-fix …/380`,
    /// `followed_from` = [`do [#kotlinflushprep]`, the loop watch row], a
    /// legacy anchor without a queue id) is restored onto its struck row, so
    /// #380 is dispatchable again and the strike policy judges the attempt.
    #[test]
    fn a_claim_that_already_followed_off_a_struck_row_is_restored_onto_it() {
        let now = 4_000;
        let mut ledger = running_kotlinflush(now);
        {
            let claim = &mut ledger.claims[0];
            claim.identity = claim_identity(GH380);
            claim.item_text = GH380.to_string();
            claim.followed_from = vec![KOTLINFLUSH.to_string(), LOOP_WATCH.to_string()];
            claim.followed_generation = claim.generation;
            claim.anchor = Some(ClaimAnchor {
                queue_id: None,
                heads: strings(&[GH380, GH381]),
                slots: vec![0],
            });
        }
        let struck_text = format!("~~{KOTLINFLUSH}~~");
        let content = run_loop_doc(&[&struck_text, GH380, GH381], &[LOOP_WATCH]);
        let outcome = ledger.follow_edits_in_view(now + 1, &view_of(&content));
        assert_eq!(
            outcome.unfollowed_struck,
            vec![(GH380.to_string(), KOTLINFLUSH.to_string())]
        );
        assert!(outcome.followed.is_empty(), "{outcome:?}");
        let claim = &ledger.claims[0];
        assert_eq!(claim.identity, claim_identity(KOTLINFLUSH));
        assert_eq!(claim.item_text, KOTLINFLUSH);
        assert!(claim.followed_from.is_empty());
        assert!(struck_row_reopened_by_claim(Some(claim)));
        let live: HashSet<_> = view_of(&content)
            .rows()
            .iter()
            .map(|row| claim_identity(row))
            .collect();
        assert!(!ledger.claimed_items(now + 1, Some(&live)).claims(GH380));

        // Stable: the next read neither re-follows nor re-reports.
        let outcome = ledger.follow_edits_in_view(now + 2, &view_of(&content));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        assert_eq!(ledger.claims[0].identity, claim_identity(KOTLINFLUSH));
    }

    /// `#claimfollowstruck` repair is fenced to the attempt that followed.
    /// After the mis-follow the coordinator re-claimed `#gh-fix …/380` for a
    /// new worker (`Reassigned`, gen 2): that attempt inherits the line
    /// history but was made on #380 as it is, so it is never yanked back onto
    /// the struck row. A legacy follow (no recorded generation) is likewise
    /// unattributable and left alone.
    #[test]
    fn a_reclaimed_or_legacy_attempt_is_not_restored_onto_a_struck_predecessor() {
        let now = 5_000;
        let mut ledger = running_kotlinflush(now);
        {
            let claim = &mut ledger.claims[0];
            claim.identity = claim_identity(GH380);
            claim.item_text = GH380.to_string();
            claim.followed_from = vec![KOTLINFLUSH.to_string(), LOOP_WATCH.to_string()];
            claim.followed_generation = claim.generation;
        }
        // The gen-1 attempt expires (its owner session ended); gen 2 is claimed.
        let later = now + 3601;
        assert_eq!(
            ledger.claim(GH380, "subagent:gh380", later, 3600),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims[0].generation, 2);
        let now = later;
        let struck_text = format!("~~{KOTLINFLUSH}~~");
        let content = run_loop_doc(&[&struck_text, GH380, GH381], &[LOOP_WATCH]);
        let mut legacy = ledger.clone();
        let outcome = ledger.follow_edits_in_view(now + 2, &view_of(&content));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        assert_eq!(ledger.claims[0].identity, claim_identity(GH380));
        assert_eq!(ledger.claims[0].owner, "subagent:gh380");

        legacy.claims[0].followed_generation = 0;
        legacy.claims[0].generation = 1;
        let outcome = legacy.follow_edits_in_view(now + 2, &view_of(&content));
        assert_eq!(outcome, ClaimFollowOutcome::default());
        assert_eq!(legacy.claims[0].identity, claim_identity(GH380));
    }
}
