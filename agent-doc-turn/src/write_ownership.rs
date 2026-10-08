//! One ownership predicate shared by every "your write is retained" refusal.
//!
//! Three places tell an agent that a write is retained and forbid recovery, and
//! they live in three crates:
//!
//! | Site | Crate |
//! |---|---|
//! | `crdt_relay_pending_refusal` | `agent-doc-git-io` |
//! | authority/disk divergence INTERRUPT | `agent-doc-session-check-io` |
//! | `await_editor_replica_no_disk_write` | `agent-doc-document-realtime-io` |
//!
//! Until 0.35.123 none of them asked whether anything actually owned the write.
//! All three fired on `tasks/agent-doc/agent-doc-bugs2.md` on 2026-08-03 with the
//! newest cycle `committed` hours earlier and no capture retained, and each told
//! the session to stand down; every one was recovered by hand with
//! `agent-doc commit`. 0.35.123 fixed the `session-check` site only, so an agent
//! that reaches either of the other two first still obeys the wrong instruction.
//!
//! `crdt_relay_pending_refusal`'s own doc comment records the trap: it was made
//! one function "so all three call sites give the same instruction rather than
//! drifting into three dialects." Consolidating the *wording* did not stop the
//! *predicate* from diverging. So the predicate is the shared thing here, and the
//! wording is derived from it.
//!
//! The "do NOT re-send / force disk / `admin recycle` / `admin reload-lib`"
//! guidance stays intact wherever a real owner is found — it was written for a
//! real 2026-07-26 incident in which two sessions invented recoveries that each
//! perturbed the capture being awaited. Only the *unowned* case gains a remedy.
//!
//! This is the interim predicate for `#percellconverge`. Per-component ownership
//! replaces `cycle_open`/`retained_capture` with "does anything own **the
//! components this turn wrote**"; the call sites do not change when it does.

/// Whether anything durable owns a retained write.
///
/// Deliberately **not** keyed on editor attachment: `TransportProjection`'s
/// `editor_generation` is a one-way latch (`#editormodelmissing`) that reports an
/// editor as attached forever once one has ever attached, so a predicate that
/// trusted it would inherit the latch and answer "owned" for every document that
/// has ever been opened in an editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedWriteOwnership {
    /// A cycle is open for this document (`preflight_started`,
    /// `response_captured`, or `write_applied`).
    pub cycle_open: bool,
    /// A response capture is retained for this document.
    ///
    /// This is also ownership of the terminal commit. A captured-finalize
    /// worker resumes the exact same response after editor delivery converges;
    /// `write_applied` does not transfer that ownership to the calling agent.
    pub retained_capture: bool,
    /// The open cycle is specifically at `write_applied`
    /// (`#ownershipverdictdiverges`).
    ///
    /// Not every open phase is self-committing. At `write_applied` the response
    /// write has ALREADY landed and only the terminal commit is outstanding.
    /// When no response capture remains, `agent-doc commit` is the recovery;
    /// when a capture remains, its captured-finalize worker still owns that
    /// exact commit and manual recovery would race it.
    pub write_applied: bool,
    /// A non-response projection continuation durably owns the current
    /// authority/disk divergence.
    ///
    /// This includes retained document-write delivery and Compact Exchange's
    /// content-bearing continuation. Neither requires an open response cycle or
    /// response capture, but both have a controller state edge that will resume
    /// the exact operation. Omitting this bit lets a partial editor-native save
    /// look like a fresh unanswered edit once the response cycle is committed.
    pub retained_projection: bool,
    /// The divergence is an unanswered edit to typed components this turn's
    /// write never produced — a queue strike, a backlog add — that the disk
    /// projection does not have yet (`#strandedremedydeadlock`).
    ///
    /// This is what distinguishes "the agent's write is stranded" from "an
    /// edit is sitting in the live buffer waiting for the next cycle". Both
    /// look identical to an ownership check — no cycle, no capture — but only
    /// the first is recoverable by committing.
    ///
    /// Deliberately **not** called an *operator* edit. The sites that set this
    /// compare components; none of them can prove who authored the difference,
    /// and the common non-operator case is an earlier `agent-doc write` whose
    /// own commit refused. Claiming authorship the predicate cannot establish
    /// is how a diagnostic starts lying — observed the same day this flag
    /// shipped, when a `--backlog-add-after` left exactly this drift and the
    /// message called it an operator edit.
    pub unanswered_edit: bool,
    /// Nothing is running that can deliver the retained capture's next state
    /// edge (`#capturedresumeunowned`).
    ///
    /// A retained capture is only self-completing because a captured-finalize
    /// resume worker re-drives it on the next controller document-state edge,
    /// and the only drivers are the supervisor idle watch and the Codex `Stop`
    /// hook. With neither present the resume is edge-triggered with no edge
    /// source: observed 2026-09-11 on `src/haiven-dev/tasks/backend.md`, whose
    /// cycle sat at `response_captured` across turns while the controller
    /// logged `controller_orphan_drain_dispatch ... reason=no_supervisor_idle_watch`
    /// and `delivery_converged=true`. `Deferred`'s promise — "the same intent
    /// commits itself once delivery converges" — was false, and every guard
    /// repeated it, so the document had no next move at all.
    ///
    /// Only a site that has actually looked for the driver may set this; the
    /// default is `false`, which keeps the conservative Deferred reading and
    /// the 2026-07-26 "do NOT invent a recovery" guidance intact.
    pub capture_resume_unowned: bool,
    /// The editor endpoint that would have to converge this write answered its
    /// delivery receipt with an explicit REJECTION (GH #131).
    ///
    /// Every holder above — an open cycle, a retained capture, a retained
    /// delivery projection — completes by waiting for the editor's delivery
    /// projection to converge. An endpoint that returned
    /// `{"type":"receipt","status":"rejected"}` has answered that it will NOT,
    /// so those holders hold nothing: the "it commits itself once delivery
    /// converges" promise is false and the do-NOT list forbids the only moves
    /// that work. Observed on 0.35.449 on `tasks/agent-doc/agent-doc.ad.md`:
    /// a JetBrains backend running deleted plugin jars rejected every receipt,
    /// and the retained intents converged the instant it exited.
    ///
    /// `#idlerevisionreactive`: this is the fourth outcome, "the endpoint
    /// answered NO", and it must not collapse into "not yet converged". Only a
    /// site that has read the durable rejection record may set it; the default
    /// `false` keeps the conservative reading.
    pub delivery_rejected: bool,
    /// The refusing endpoint has crossed the unregister threshold AND the
    /// document authority currently reports zero live editor replicas (GH
    /// #144).
    ///
    /// Neither observation is sufficient alone: a rejection below the
    /// threshold still names an endpoint to recover, while a transient
    /// `live_editors=0` observation can retain editor authority through the
    /// reliable open-file projection. Together they prove that the delivery
    /// route has been removed and no replica can emit the state edge the
    /// retained write is waiting for. Any cycle/capture/projection bits then
    /// describe durable work, not a live holder, so the write is stranded and
    /// `agent-doc commit` is the recovery.
    pub editor_route_unowned: bool,
    /// The editor that holds this document is not serving its replica
    /// (GH #131, `#replicaunservedremedy`).
    ///
    /// Observed directly: the live authority reports the editor attached while
    /// its replica is not registered. Every holder above completes through
    /// that replica, and nothing the agent can wait for re-registers it —
    /// `agent-doc admin reload-lib` does. Before this bit existed the retained
    /// refusal answered `Deferred` and forbade `admin reload-lib`, while
    /// `session-check`'s integrity gate prescribed `admin reload-lib` for the
    /// same document in the same session, and that command was the one that
    /// worked. Both texts now read this one fact.
    ///
    /// Only a site that has actually observed the replica may set it; the
    /// default `false` keeps the conservative reading.
    pub replica_unserved: bool,
}

impl RetainedWriteOwnership {
    /// The shape a site reports when it has not looked. Kept explicit so a site
    /// cannot silently claim ownership it never proved: unproven reads as
    /// stranded, and the remedy names a recovery instead of forbidding one.
    pub const UNOWNED: Self = Self {
        cycle_open: false,
        retained_capture: false,
        write_applied: false,
        retained_projection: false,
        unanswered_edit: false,
        capture_resume_unowned: false,
        delivery_rejected: false,
        editor_route_unowned: false,
        replica_unserved: false,
    };

    pub const fn new(cycle_open: bool, retained_capture: bool) -> Self {
        Self {
            cycle_open,
            retained_capture,
            write_applied: false,
            retained_projection: false,
            unanswered_edit: false,
            capture_resume_unowned: false,
            delivery_rejected: false,
            editor_route_unowned: false,
            replica_unserved: false,
        }
    }

    /// [`Self::new`] for a caller that knows the open cycle's phase.
    pub const fn new_with_phase(
        cycle_open: bool,
        retained_capture: bool,
        write_applied: bool,
    ) -> Self {
        Self {
            cycle_open,
            retained_capture,
            write_applied,
            retained_projection: false,
            unanswered_edit: false,
            capture_resume_unowned: false,
            delivery_rejected: false,
            editor_route_unowned: false,
            replica_unserved: false,
        }
    }

    /// Refine ownership with a durable non-response projection continuation.
    pub const fn with_retained_projection(mut self, retained_projection: bool) -> Self {
        self.retained_projection |= retained_projection;
        self
    }

    /// Record that the diverging components are an edit this turn's write did
    /// not produce. Only a site that has actually compared the components may
    /// set it; the default is `false`, so a site that has not looked keeps the
    /// conservative retained-write reading.
    pub const fn with_unanswered_edit(mut self, unanswered_edit: bool) -> Self {
        self.unanswered_edit = unanswered_edit;
        self
    }

    /// Record that nothing is running that can deliver this capture's next
    /// state edge. Only a site that has actually looked for the resume driver
    /// may set it; the default keeps the Deferred reading.
    pub const fn with_capture_resume_unowned(mut self, capture_resume_unowned: bool) -> Self {
        self.capture_resume_unowned |= capture_resume_unowned;
        self
    }

    /// Record that the editor endpoint explicitly rejected this document's
    /// delivery receipt (GH #131). Only a site that has read the durable
    /// rejection record may set it.
    pub const fn with_delivery_rejected(mut self, delivery_rejected: bool) -> Self {
        self.delivery_rejected |= delivery_rejected;
        self
    }

    /// Record that the rejecting editor route is currently unregistered and
    /// document authority has no live editor replica (GH #144).
    pub const fn with_editor_route_unowned(mut self, editor_route_unowned: bool) -> Self {
        self.editor_route_unowned |= editor_route_unowned;
        self
    }

    /// Record that the editor holding this document is observed not serving its
    /// replica (GH #131). Only a site that has observed the replica may set it.
    pub const fn with_replica_unserved(mut self, replica_unserved: bool) -> Self {
        self.replica_unserved |= replica_unserved;
        self
    }

    /// Refine ownership with a response capture proven by the current caller.
    ///
    /// Some guards already hold the loaded capture. Keeping that evidence is
    /// stronger than re-reading a sidecar that can race capture settlement.
    pub const fn with_retained_capture(mut self, retained_capture: bool) -> Self {
        self.retained_capture |= retained_capture;
        self
    }

    pub const fn verdict(self) -> RetainedWriteVerdict {
        // A retained capture owns the whole closeout, including the terminal
        // commit after `write_applied`. This must be checked before phase: the
        // captured-finalize worker is waiting on the same editor state edge and
        // a manual `commit` would race it. Only an *uncaptured* write-applied
        // cycle needs the manual terminal-commit recovery.
        if self.editor_route_unowned {
            // GH #144: the retained bits say work is durable, but the endpoint
            // that could advance it has been removed and authority has no live
            // replica. Waiting cannot fire a state edge. This is the stranded
            // shape and commit is deliberately the named recovery.
            RetainedWriteVerdict::Stranded
        } else if self.retained_capture && self.capture_resume_unowned && !self.retained_projection {
            // `#capturedresumeunowned`: the capture is durable, but the worker
            // that would re-drive it is not running. Waiting is the one thing
            // that cannot work, so this must not read as Deferred.
            RetainedWriteVerdict::CaptureResumeUnowned
        } else if self.delivery_rejected
            && (self.retained_capture || self.retained_projection || self.cycle_open)
        {
            // GH #131: every holder here completes by waiting for the editor's
            // delivery projection to converge, and the endpoint answered NO.
            // Nothing that can fire will fire, so this must not read as
            // Deferred — and it is not Stranded either, because the response
            // is durable and must not be re-sent. The recovery is removing the
            // rejecting endpoint.
            RetainedWriteVerdict::DeliveryRejected
        } else if self.replica_unserved {
            // GH #131 (`#replicaunservedremedy`): whatever holds the write —
            // or nothing — it can only complete through the replica the editor
            // is not serving. Waiting cannot end it and committing is refused
            // by the same integrity gate, so the one terminating move is the
            // replica recovery the integrity gate names.
            RetainedWriteVerdict::ReplicaUnserved
        } else if self.retained_capture || self.retained_projection {
            RetainedWriteVerdict::Deferred
        } else if self.write_applied {
            RetainedWriteVerdict::AwaitingTerminalCommit
        } else if self.cycle_open {
            RetainedWriteVerdict::Deferred
        } else if self.unanswered_edit {
            // Refines the unowned case only. With a durable holder the write
            // is genuinely deferred and the pending edit rides along with it;
            // it is the *unowned* shape the two readings collide on.
            RetainedWriteVerdict::UnansweredEditPending
        } else {
            RetainedWriteVerdict::Stranded
        }
    }

    pub const fn is_stranded(self) -> bool {
        matches!(self.verdict(), RetainedWriteVerdict::Stranded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedWriteVerdict {
    /// Something durable holds the write, so a state edge will fire and the
    /// intent commits itself. Waiting is correct and every invented recovery
    /// perturbs the capture being awaited.
    Deferred,
    /// Nothing holds the write. No state edge can fire, so waiting never
    /// commits it — the visible edits are stranded, not deferred.
    Stranded,
    /// The response write already landed, no response capture remains to own
    /// closeout, and only the terminal commit is outstanding
    /// (`#ownershipverdictdiverges`).
    ///
    /// Between the other two: the response is NOT lost, so re-sending it would
    /// duplicate work — but nothing is going to finish it either, so waiting is
    /// equally wrong. `agent-doc commit` is the one command that advances it.
    /// A retained capture instead yields [`Self::Deferred`], because its
    /// captured-finalize worker still owns this boundary.
    AwaitingTerminalCommit,
    /// A response capture is durable and nothing is running that can deliver
    /// its next state edge (`#capturedresumeunowned`).
    ///
    /// Between [`Self::Deferred`] and [`Self::Stranded`]: the response is NOT
    /// lost and must not be re-sent — the exact capture is still the thing to
    /// finish — but no worker will pick it up, so waiting never ends. Resuming
    /// that same capture on demand is the recovery.
    CaptureResumeUnowned,
    /// Nothing owns a write because there is no write — the divergence is an
    /// unanswered edit to typed components this turn's write never produced
    /// (`#strandedremedydeadlock`).
    ///
    /// Indistinguishable from [`Self::Stranded`] by ownership alone, and the
    /// opposite instruction: committing a fresh unanswered edit would swallow
    /// the next turn's prompt, so every commit path refuses it on purpose.
    /// Answering it — running the document again — is what resolves it.
    UnansweredEditPending,
    /// The only holder of the write waits on an editor endpoint that explicitly
    /// REJECTED the delivery receipt (GH #131).
    ///
    /// Between [`Self::Deferred`] and [`Self::Stranded`]: the response is NOT
    /// lost and must not be re-sent, but the convergence every holder waits on
    /// is being refused, so waiting never ends. Restarting or reloading the
    /// rejecting editor is the recovery; once it is gone or re-registered the
    /// retained intent converges through document authority on its own.
    DeliveryRejected,
    /// The editor that holds the document is not serving its replica, so no
    /// delivery to it can converge (GH #131, `#replicaunservedremedy`).
    ///
    /// The terminating recovery is the one `session-check`'s integrity gate
    /// names for the same observation — [`editor_replica_recovery`] — which is
    /// why this verdict and that gate derive the instruction from one owner
    /// instead of one forbidding what the other prescribes.
    ReplicaUnserved,
}

impl RetainedWriteVerdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deferred => "deferred",
            Self::CaptureResumeUnowned => "capture_resume_unowned",
            Self::Stranded => "stranded",
            Self::AwaitingTerminalCommit => "awaiting_terminal_commit",
            Self::UnansweredEditPending => "unanswered_edit_pending",
            Self::DeliveryRejected => "delivery_rejected",
            Self::ReplicaUnserved => "replica_unserved",
        }
    }

    /// Whether `agent-doc commit <FILE>` is the recovery this verdict names.
    ///
    /// `#strandedremedydeadlock`: naming a command is a promise that the command
    /// runs. `commit`'s already-current path used to hold an *independent*
    /// predicate — any non-exchange typed-component drift without a
    /// binary-owned retained-target proof was a terminal refusal — so the two
    /// commands could and did contradict each other. Observed 2026-08-09 on
    /// `tasks/agent-doc/agent-doc-bugs2.md`: `session-check` returned the
    /// `Stranded` verdict whose remedy is "run `agent-doc commit <FILE>`", and
    /// that exact command answered "refusing to close as already committed …
    /// typed-component drift without an exact binary-owned retained-target
    /// proof". An agent obeying the instruction faithfully had no next move,
    /// and the live queue drift it described could never be committed by
    /// anything.
    ///
    /// So the refusal is derived from the verdict rather than re-decided:
    /// `commit` reconciles exactly when the remedy sends the agent to it.
    /// `Deferred` is the one verdict that does not — something durable holds
    /// the write and commits it itself, and the 2026-07-26 "do NOT invent a
    /// recovery" guidance still governs there.
    pub const fn commit_is_the_named_recovery(self) -> bool {
        match self {
            Self::Deferred
            | Self::CaptureResumeUnowned
            | Self::UnansweredEditPending
            | Self::DeliveryRejected
            | Self::ReplicaUnserved => false,
            Self::Stranded | Self::AwaitingTerminalCommit => true,
        }
    }
}

/// Which graph owns the retained projection a refusal is describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedProjectionOwnership {
    /// A response write has entered closeout, so its cycle/capture facts decide
    /// whether the caller waits or performs the terminal commit.
    ResponseWrite(RetainedWriteOwnership),
    /// A mutation has not run yet. Its retained cut is only a canonical base;
    /// response-cycle commit commands cannot own or release it.
    PrewriteMutation,
}

/// The marker every retained-write refusal carries in its rendered message.
///
/// `#retaineddeferisnotafailure`: the refusal is emitted by
/// `agent-doc-document-realtime-io`, but the crates that must *classify* it sit
/// on the other side of a dependency edge (and, for the ops-log `reason_head`
/// and the harness hooks, on the other side of a process boundary), so a typed
/// `downcast_ref` cannot reach it. Stamping one token from one constructor makes
/// the classification structural rather than remembered.
///
/// It lives here rather than in the emitting crate because every consumer
/// already depends on `agent-doc-turn` for the remedy wording, and the two must
/// not be reachable independently: a site that can render the remedy must also
/// be able to recognize the refusal that carries it.
pub const AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN: &str =
    "recovery=await_editor_replica_no_disk_write";

/// The additional marker carried ONLY by a retained write whose editor replica
/// is live and whose delivery projection has simply not converged yet.
///
/// The class token above spans three refusals, and they are NOT equally safe to
/// continue past. Two of them — an attached editor with no registered replica,
/// and editor sync pending — mean the editor authority is **unreachable**, which
/// is exactly the shape closeout must fail closed on so it never writes behind
/// an active listener. Only this one has a live replica that will converge on
/// its own, which is what makes deferring idempotent bookkeeping past it safe.
///
/// Told apart by a token rather than by the wording of the refusal for the same
/// reason the class carries one: a prose needle is a rule that holds only while
/// every author remembers it, and the wedge this exists to prevent was caused by
/// exactly that.
pub const RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN: &str = "retained=delivery_projection_pending";

/// Whether a rendered error is a retained-write refusal.
///
/// True means the write **reached the editor authority and was retained** — the
/// CRDT accepted it, disk was deliberately not touched, and only the secondary
/// snapshot/commit boundary is waiting on a delivery projection. It is NOT a
/// failed write, and a caller whose own work is idempotent bookkeeping must not
/// escalate it into a failure of the surrounding operation.
///
/// Matching prose instead of this token is how the same class stayed unhandled
/// in two crates at once: `agent-doc-repair-command-io` listed three phrases and
/// a retained refusal matched none of them (`#retainconv`), and preflight's
/// pending-maintenance defer listed two more and missed it as well — which
/// failed turn admission outright, so `/agent-doc <FILE>` produced no cycle
/// contract and the session could not start.
pub fn is_retained_write_refusal(message: &str) -> bool {
    message.contains(AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN)
}

/// Whether a rendered error is the one retained-write refusal that a live
/// editor replica will resolve on its own.
///
/// Requires both tokens, so the narrow case cannot drift out of the class it is
/// a subset of.
pub fn is_retained_delivery_projection_pending(message: &str) -> bool {
    is_retained_write_refusal(message)
        && message.contains(RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN)
}

/// The one instruction for an editor that holds a document but is not serving
/// its replica (GH #131, `#replicaunservedremedy`).
///
/// Two texts used to author it independently and disagree: `session-check`'s
/// attached-editor integrity gate prescribed `agent-doc admin reload-lib`, and
/// the retained-write deferral forbade it, about the same document in the same
/// session — and the forbidden command was the one that worked. Both now read
/// this function: the integrity gate prefixes the holder it observed, and the
/// [`RetainedWriteVerdict::ReplicaUnserved`] remedy appends it.
pub fn editor_replica_recovery() -> &'static str {
    "Run `agent-doc admin reload-lib` so the editor re-registers its replica, then retry the \
     same command; restart that editor only if the retry is refused again (#84)."
}

/// When `admin reload-lib` is sanctioned, stated once for every remedy that
/// otherwise tells the agent to wait (GH #131).
///
/// The deferral's do-NOT list exists because sessions invented recoveries that
/// perturbed a capture a live replica was about to converge. It must not
/// outlaw the one recovery the integrity gate prescribes for a replica that is
/// NOT being served, or the two instructions form a loop with no exit.
pub const EDITOR_REPLICA_RELOAD_SANCTION: &str = "`admin reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports \
     that the editor holding the document is not serving its replica, and then its remedy \
     governs";

/// The remedy every retained-projection refusal appends, derived from one owner.
pub fn retained_projection_remedy(ownership: RetainedProjectionOwnership, file: &str) -> String {
    match ownership {
        RetainedProjectionOwnership::ResponseWrite(ownership) => {
            retained_write_remedy_inner(ownership, file)
        }
        RetainedProjectionOwnership::PrewriteMutation => format!(
            "The canonical pre-write base remains owned by the document model. Retry the same \
             mutation after the editor replica observes that base; do NOT run `agent-doc commit \
             {file}`, `agent-doc write --commit {file}`, or force disk because no response-cycle \
             write owns this projection"
        ),
    }
}

/// The remedy every retained response-write refusal appends, derived from one predicate.
///
/// `file` is the document path as the caller displays it; it is interpolated
/// into the commands so an agent can copy them verbatim.
pub fn retained_write_remedy(ownership: RetainedWriteOwnership, file: &str) -> String {
    retained_projection_remedy(RetainedProjectionOwnership::ResponseWrite(ownership), file)
}

fn retained_write_remedy_inner(ownership: RetainedWriteOwnership, file: &str) -> String {
    match ownership.verdict() {
        RetainedWriteVerdict::Deferred => format!(
            "The retained capture or projection is already durable and this exact retained \
             intent commits itself once delivery converges — this is a deferral, not a lost \
             response. Wait for its controller-owned terminal state edge; do not use the \
             document's current-cycle status as proof because queue continuation can advance \
             to a different cycle. Do NOT re-send the response, force disk, or `admin recycle`, \
             which disturb the capture being awaited. Do not reach for `admin reload-lib` on \
             your own either: {}",
            EDITOR_REPLICA_RELOAD_SANCTION
        ),
        RetainedWriteVerdict::CaptureResumeUnowned => format!(
            "The response capture is DURABLE but UNOWNED: no supervisor idle watch and no \
             harness stop hook is running for this document, so the captured-finalize resume \
             has no state edge source and waiting will never commit it. Do NOT re-send the \
             response, force disk, or `admin recycle` — the exact capture is still the thing to \
             finish. Resume that same capture from the pane that OWNS this session: \
             `agent-doc repair --resume-capture {file}`"
        ),
        RetainedWriteVerdict::DeliveryRejected => format!(
            "The registered editor endpoint REJECTED the delivery receipt (`IPC receipt \
             rejected`) — it answered NO, so the delivery projection this write is waiting on \
             will never converge and waiting will not commit it. The response is durable and NOT \
             lost: do NOT re-send it or force disk. Recover by removing the rejecting endpoint: \
             restart or reload the editor that has {file} open (a backend still running deleted \
             plugin jars after an update is the observed cause). Once that endpoint is gone or re-registered, the retained intent \
             converges through document authority on its own; then run `agent-doc session-check \
             {file}` and follow the recovery it names"
        ),
        RetainedWriteVerdict::ReplicaUnserved => format!(
            "The editor that has {file} open holds the document but is not serving its replica, \
             so no delivery to it can converge: waiting will not commit anything and a manual \
             commit is refused by the same integrity gate. Any retained write is durable \
             and NOT lost — do NOT re-send it, and do NOT force disk (that editor's buffer may \
             hold unsaved text). {} Then run `agent-doc session-check {file}` and follow the \
             recovery it names",
            editor_replica_recovery()
        ),
        RetainedWriteVerdict::Stranded => {
            let evidence = if ownership.editor_route_unowned {
                "The refusing editor endpoint is UNREGISTERED and document authority reports \
                 ZERO live editor replicas, so no durable holder can emit another state edge"
            } else {
                "NO cycle is open and NO response capture is retained, so nothing owns this write \
                 and no state edge will fire"
            };
            format!(
                "{evidence} — the visible edits are STRANDED, not deferred, and waiting will not \
                 commit them. Run `agent-doc commit {file}`; when a live document actor exists \
                 the controller routes that commit under the actor's explicit pane identity, so \
                 do not claim or move the live session. Use `agent-doc write --commit {file}` \
                 from the owning pane only if an unwritten response body remains"
            )
        }
        RetainedWriteVerdict::AwaitingTerminalCommit => format!(
            "The response write ALREADY LANDED and only the terminal commit is outstanding — \
             this is neither a lost response nor a self-completing deferral, and \
             `agent-doc session-check {file}` will report the cycle INTERRUPTED at \
             `write_applied`. Run `agent-doc commit {file}`; a live controller routes the \
             terminal commit under the document actor's explicit pane identity. Do NOT re-send the response (the body is already \
             durable and would duplicate), force disk, `admin recycle`, or `admin reload-lib`"
        ),
        RetainedWriteVerdict::UnansweredEditPending => format!(
            "NO write is retained — the response is already committed and the divergence is an \
             UNANSWERED DOCUMENT EDIT to typed components this turn's write never produced (a \
             queue or backlog line the live buffer has and the disk projection does not). It is \
             either operator steering or an earlier `agent-doc write` whose own commit refused; \
             this check compares components and cannot tell which, so it does not guess. Either \
             way it is not a failed closeout. Answer it: run `agent-doc {file}` to open the next \
             cycle, which reads that edit as its prompt and commits it. Do NOT run `agent-doc \
             commit {file}` — every commit path refuses a fresh unanswered edit on purpose, \
             because committing it would swallow the next turn's prompt — and do NOT force disk, \
             which clobbers the live edits"
        ),
    }
}

/// Whether a tracked-work-only repair (`agent-doc write <FILE> ... --pending-only
/// --commit`) can complete in the current state (GH #131, `#trackedrepairterminates`).
///
/// `session-check` names that command as the repair for an unrecorded `--done`.
/// On 0.35.453 the command answered with the retained-write deferral, whose
/// remedy is "run `session-check`" — and `session-check` named the same command
/// again. A remedy that names a command the write path refuses is the
/// `#strandedremedydeadlock` defect in a new place, so admission is derived from
/// the same predicate the write path's refusal is, rather than re-decided.
///
/// The write path's delivery refusal always carries proof that its own
/// projection is retained, so the question is asked of the ownership exactly as
/// that refusal will see it. [`RetainedWriteVerdict::Deferred`] is admissible:
/// the write absorbs the retained mutation into its pending-only commit
/// continuation instead of refusing ([`pending_only_retention`]). Only the
/// verdicts whose convergence the editor is refusing are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedWorkRepairAdmission {
    /// The repair command completes (or is absorbed) now.
    Admissible,
    /// The repair command would be refused until the editor serves its
    /// replica again; the carried verdict names that recovery.
    RecoverEditorFirst(RetainedWriteVerdict),
}

pub const fn tracked_work_repair_admission(
    ownership: RetainedWriteOwnership,
) -> TrackedWorkRepairAdmission {
    match ownership.with_retained_projection(true).verdict() {
        verdict @ (RetainedWriteVerdict::ReplicaUnserved
        | RetainedWriteVerdict::DeliveryRejected) => {
            TrackedWorkRepairAdmission::RecoverEditorFirst(verdict)
        }
        _ => TrackedWorkRepairAdmission::Admissible,
    }
}

/// The instruction a guard renders for a tracked-work-only repair, derived from
/// [`tracked_work_repair_admission`]: the command itself when it can complete,
/// otherwise the editor recovery FIRST and the command after it, so following
/// the text in order terminates.
pub fn tracked_work_repair_instruction(
    ownership: RetainedWriteOwnership,
    file: &str,
    repair: &str,
) -> String {
    match tracked_work_repair_admission(ownership) {
        TrackedWorkRepairAdmission::Admissible => format!("`{repair}`"),
        TrackedWorkRepairAdmission::RecoverEditorFirst(_) => format!(
            "`{repair}` AFTER recovering the editor — the write path refuses it until then. {}",
            retained_write_remedy(ownership.with_retained_projection(true), file)
        ),
    }
}

/// What a tracked-work-only write does when its own mutation envelope was
/// retained by the editor delivery projection (GH #131, `#trackedrepairterminates`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOnlyRetention {
    /// The mutation is durable in the editor authority, the response it sits
    /// beside is already committed, and the exact target is recorded as the
    /// pending-only commit continuation that `session-check` resumes once
    /// delivery converges. Nothing would be gained by refusing: re-running the
    /// write cannot add anything, so the write reports the retention and exits
    /// successfully.
    Absorbed,
    /// The retained intent already settled before the synchronous error
    /// handler inspected it. The refusal token proves this was a converging
    /// delivery projection, but the caller must still prove delivery and the
    /// tracked-work landing before it continues to commit.
    AwaitSettledDelivery,
    /// Not proven absorbable: keep the refusal and its derived remedy.
    Refused,
}

/// Decide [`PendingOnlyRetention`] from facts the write path can prove.
///
/// - `message` must carry the narrow `retained=delivery_projection_pending`
///   marker: a live replica that converges on its own. A rejecting endpoint or
///   an unserved replica withholds it and stays refused, because waiting there
///   never ends.
/// - `response_committed`: no response cycle is open, so this write carries no
///   response half that a closeout still owns.
/// - `own_intent_retained`: a NEW retained write intent appeared during this
///   invocation, i.e. this envelope reached the editor authority.
/// - `continuation_recorded`: the pending-only commit continuation names that
///   intent's exact target.
pub fn pending_only_retention(
    message: &str,
    response_committed: bool,
    own_intent_retained: bool,
    continuation_recorded: bool,
) -> PendingOnlyRetention {
    if is_retained_delivery_projection_pending(message)
        && response_committed
        && own_intent_retained
        && continuation_recorded
    {
        PendingOnlyRetention::Absorbed
    } else if is_retained_delivery_projection_pending(message)
        && response_committed
        && !own_intent_retained
        && !continuation_recorded
    {
        PendingOnlyRetention::AwaitSettledDelivery
    } else {
        PendingOnlyRetention::Refused
    }
}

/// The notice an absorbed pending-only write prints instead of refusing.
pub fn pending_only_absorbed_notice(file: &str, target_hash: &str) -> String {
    format!(
        "tracked-work mutation for {file} reached the editor authority and is retained while its \
         delivery projection converges; its exact target ({target_hash}) is recorded as the \
         pending-only commit continuation, which `agent-doc session-check {file}` commits once \
         delivery converges. The mutation is recorded — do NOT re-run this write. {}",
        EDITOR_REPLICA_RELOAD_SANCTION
    )
}

/// The tracked-work mutations one closeout recorded, for provenance.
///
/// `#retainedmutdrop`: see [`recorded_tracked_work_is_unlanded`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RecordedTrackedWork<'a> {
    /// Ids this cycle recorded a `--done` / reap for.
    pub done_ids: &'a [String],
    /// Ids this cycle recorded a `--backlog-add` / `--review-add` for.
    pub added_ids: &'a [String],
    /// `#mutprovenancepreresponse`: `--done` ids this cycle REQUESTED, recorded
    /// before the response cell was published.
    ///
    /// The two fields above are written by the mutation phase, which runs AFTER
    /// the response write — so a write that fails once the response has landed
    /// records nothing, and the divergence reads as a fresh operator edit.
    pub requested_done_ids: &'a [String],
    /// Explicit add ids this cycle requested (`#mutprovenancepreresponse`).
    pub requested_added_ids: &'a [String],
    /// `#mutplanwitness`: this cycle requested at least one tracked-work
    /// mutation of ANY shape — including gates, ungates, edits, reorders,
    /// review-edits and `--status`, none of which the id witnesses above can
    /// see.
    pub requested_mutations: bool,
    /// `#mutplanwitness`: the tracked-work mutation envelope was published to
    /// the document. Recorded only after the pending-write transaction
    /// succeeds, so a retained or failed mutation write leaves it false.
    pub mutations_applied: bool,
}

/// Whether a recorded tracked-work mutation is still missing from `disk`.
///
/// `#retainedmutdrop`: session-check classified a queue/backlog divergence as an
/// unanswered operator edit on the premise that "`exchange` is where a response
/// lives, so an exchange-clean divergence in queue/backlog/status is by
/// construction not this turn's write". **That premise is false.** A closeout's
/// write produces `queue`, `backlog`, `status`, `review`, and `done` as well —
/// they are in the turn's own `write_set` — so a response that committed while
/// its tracked-work half did not produces exactly this shape. Calling it an
/// operator edit sends recovery to sweep it, and the mutations are lost while
/// the response stays committed: the delivery half of `#prmergeguardpr`.
///
/// Observed 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`: `respond`
/// reported `completed and reaped 1 item(s) atomically: projpassstart`, the
/// write was retained, `commit` landed the response, and the next preflight
/// swept the mutations — `#projpassstart` was still open and still a queue head.
///
/// The distinguishing fact is not *which* components diverge but *whether this
/// cycle's own recorded mutations are visible yet*:
///
/// - a recorded `--done` id that disk still renders as an **open** item, or
/// - a recorded add id that disk does not contain at all
///
/// is this closeout's unlanded write, not a fresh edit. When every recorded
/// mutation has landed, a queue/backlog divergence really is new, and the
/// unanswered-edit refusal stays correct — which is what keeps `commit` from
/// swallowing the operator's next prompt.
pub fn recorded_tracked_work_is_unlanded(recorded: RecordedTrackedWork<'_>, disk: &str) -> bool {
    // `#mutplanwitness`: the id witnesses below can only see `--done` and
    // explicitly-named adds. A closeout carrying ONLY gates, ungates, edits,
    // reorders, review-edits or a `--status` change has no id whose document
    // rendering changes in a way they inspect, so they reported "landed" for a
    // document that never received the mutation — and the captured-closeout
    // resume then continued straight to commit with the tracked-work half
    // dropped. This asks the question directly: the cycle recorded that it
    // wanted mutations, and never recorded that the envelope published.
    if recorded.requested_mutations && !recorded.mutations_applied {
        return true;
    }
    let normalize = |id: &str| id.trim().trim_start_matches('#').to_string();
    let done_unlanded = |ids: &[String]| {
        ids.iter()
            .any(|id| disk.contains(&format!("- [ ] [#{}]", normalize(id))))
    };
    // `#addreapwitness`: an id added AND completed in the same closeout is reaped
    // into the done archive, so it never appears in the document. Its presence
    // cannot witness the add; the done witness (not still open) already covers it.
    let completed: std::collections::HashSet<String> = recorded
        .done_ids
        .iter()
        .chain(recorded.requested_done_ids)
        .map(|id| normalize(id))
        .collect();
    let add_unlanded = |ids: &[String]| {
        ids.iter().any(|id| {
            let id = normalize(id);
            !completed.contains(&id) && !disk.contains(&format!("[#{id}]"))
        })
    };
    done_unlanded(recorded.done_ids)
        || add_unlanded(recorded.added_ids)
        // Intent counts the same as the post-hoc record: both mean "this
        // closeout asked for a mutation that is not visible yet".
        || done_unlanded(recorded.requested_done_ids)
        || add_unlanded(recorded.requested_added_ids)
    // Intent counts the same as the post-hoc record: both mean "this
    // closeout asked for a mutation that is not visible yet".
}

/// How a closeout's tracked-work mutation phase failed, once the response half
/// has already been written.
///
/// `#retainedprojexit1`: the mutation phase used to render exactly one message
/// for every failure that reached it — "the document is half-applied", plus
/// `agent-doc commit <FILE>` / `write --commit <FILE> --backlog-only` as the
/// recovery. Observed 2026-09-20 on `cycle-1789878037902`, that message was
/// doubly wrong. The failure was a retained refusal: the mutation envelope had
/// reached the editor authority and the keyed worker converged seconds later,
/// so disk already matched HEAD with BOTH halves present and a second
/// `session-check` flipped `INTERRUPTED(write_applied)` to `ok(committed)`. The
/// message asserted a half-apply that never happened, and its remedies would
/// have DOUBLE-APPLIED the tracked-work half that had already landed.
///
/// Classify from facts the caller can actually prove — the refusal token, and
/// whether this cycle's own recorded mutations are visible — rather than from
/// "the mutation phase returned `Err`". Prose needles are what let the retained
/// class stay unhandled in three crates at once (`#retainconv`,
/// `#retaineddeferisnotafailure`), so this reads the stamped token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedWorkMutationFailure {
    /// The response write was itself retained, and the tracked-work half could
    /// not be retained alongside it. Both halves are unwritten.
    RetainedWithResponse,
    /// The mutation reached the editor authority and only its delivery
    /// projection has not converged. A live replica resolves this on its own:
    /// it is a deferral, never a half-apply, and it has no resubmit remedy.
    DeferredDeliveryProjection,
    /// The response half is applied and this cycle's own recorded mutations are
    /// provably still missing from the document. This is the real half-apply.
    HalfApplied,
    /// The response half is applied, the recorded mutations are provably
    /// visible, and the failure is in the projection tail after both halves
    /// landed. Not a half-apply, so it must not carry a resubmit remedy.
    ProjectionFailedAfterLanding,
}

impl TrackedWorkMutationFailure {
    /// Whether the surrounding closeout may report success and let the keyed
    /// worker converge, instead of escalating the failure.
    pub fn is_deferral(self) -> bool {
        matches!(self, Self::DeferredDeliveryProjection)
    }
}

/// Classify a tracked-work mutation failure.
///
/// `tracked_work_unlanded` is `None` when the caller could not resolve the
/// document or this cycle's recorded mutations. Unprovable means unproven, so
/// the classification stays on the fail-closed [`TrackedWorkMutationFailure::HalfApplied`]
/// branch rather than claiming both halves landed.
pub fn classify_tracked_work_mutation_failure(
    message: &str,
    response_write_retained: bool,
    tracked_work_unlanded: Option<bool>,
) -> TrackedWorkMutationFailure {
    if response_write_retained {
        return TrackedWorkMutationFailure::RetainedWithResponse;
    }
    if is_retained_delivery_projection_pending(message) {
        return TrackedWorkMutationFailure::DeferredDeliveryProjection;
    }
    match tracked_work_unlanded {
        Some(false) => TrackedWorkMutationFailure::ProjectionFailedAfterLanding,
        Some(true) | None => TrackedWorkMutationFailure::HalfApplied,
    }
}

/// What a deferred tracked-work closeout does once its bounded convergence
/// wait ends.
///
/// `#retaineddeferwedge`: the deferral used to `return Ok(())` straight out of
/// the write command, skipping queue consumption and the commit, on the claim
/// that "the same intent commits itself once delivery converges". Nothing
/// did. Observed 2026-09-29 on `cycle-1790658555784`: delivery converged
/// within seconds (authority == disk, 21647 bytes), yet the response sat
/// uncommitted until an operator re-piped it by hand. The write command is the
/// only owner of that commit, so it must finish the cycle itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredTrackedWorkResolution {
    /// Delivery converged and this cycle's recorded mutations are visible in
    /// the authority: continue into the ordinary closeout tail and commit.
    ContinueToCommit,
    /// Not proven converged-and-landed within the wait: fail the closeout so
    /// it is not reported as success while uncommitted.
    StillRetained,
}

/// Decide a deferred closeout from the two facts the caller can prove.
/// Unprovable means unproven: only an observed convergence plus an observed
/// landing continues to the commit.
pub fn resolve_deferred_tracked_work(
    delivery_converged: bool,
    tracked_work_unlanded: Option<bool>,
) -> DeferredTrackedWorkResolution {
    if delivery_converged && tracked_work_unlanded == Some(false) {
        DeferredTrackedWorkResolution::ContinueToCommit
    } else {
        DeferredTrackedWorkResolution::StillRetained
    }
}

/// The message a deferred closeout renders when its convergence wait expires.
/// It names the one remedy every recovery surface agrees on: the response half
/// is in the authority but not in HEAD, which `commit` refuses and
/// `write --commit` absorbs (the response cell dedups).
pub fn deferred_tracked_work_timeout_message(file: &str, waited_secs: u64) -> String {
    format!(
        "tracked-work mutations for {file} reached the editor authority but their delivery \
         projection did not converge (or did not land) within {waited_secs}s, so this closeout \
         is NOT committed. Once the editor converges, re-pipe the same response body through \
         `agent-doc write --commit {file}` WITHOUT repeating the tracked-work flags (they are \
         already retained; the response cell dedups), then run `agent-doc session-check {file}`. \
         Do NOT force disk or `admin recycle`"
    )
}

/// The message a tracked-work mutation failure renders, derived from one owner.
///
/// `file` is the document path as the caller displays it, interpolated into the
/// commands so an agent can copy them verbatim. Only
/// [`TrackedWorkMutationFailure::HalfApplied`] names a resubmit remedy, because
/// it is the only variant whose tracked-work half is proven not to have landed.
pub fn tracked_work_mutation_failure_message(
    failure: TrackedWorkMutationFailure,
    file: &str,
) -> String {
    match failure {
        TrackedWorkMutationFailure::RetainedWithResponse => format!(
            "response target for {file} is retained; failed to retain the same closeout's \
             tracked-work mutations"
        ),
        TrackedWorkMutationFailure::DeferredDeliveryProjection => format!(
            "tracked-work mutations for {file} reached the editor authority and are retained \
             while their delivery projection converges — this is a deferral, not a lost or \
             half-applied closeout. Run `agent-doc session-check {file}` once to observe the \
             terminal state. Do NOT run `agent-doc commit {file}`, re-submit this closeout's \
             tracked-work half, force disk, or `admin recycle`: the mutations are already \
             retained, so resubmitting them double-applies and the rest disturbs the \
             projection being awaited"
        ),
        TrackedWorkMutationFailure::HalfApplied => format!(
            "response for {file} is already applied but the same closeout's tracked-work \
             mutations are not: the document is half-applied. Recover with `agent-doc commit \
             {file}` from the owning pane, or re-run the tracked-work half via `agent-doc write \
             --commit {file} --backlog-only ...`"
        ),
        TrackedWorkMutationFailure::ProjectionFailedAfterLanding => format!(
            "tracked-work mutations for {file} are already visible in the document; the failure \
             is in the projection tail AFTER both halves landed, so the cycle is NOT \
             half-applied. Run `agent-doc session-check {file}` to observe the terminal state; \
             do NOT run `agent-doc commit {file}` or re-run the tracked-work half, which would \
             double-apply mutations that already landed"
        ),
    }
}

#[cfg(test)]
mod tests {

    /// `#addreapwitness`: 2026-09-29, `--backlog-add "id=a ..." --done a` reaped
    /// `#a` into the done archive in the same closeout. The add witness looked
    /// for `[#a]` in the document, never found it, and the deferred closeout
    /// timed out after 30s with `unlanded=Some(true)` on a converged document.
    #[test]
    fn an_id_added_and_completed_in_one_closeout_is_witnessed_by_its_absence() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let reaped = "<!-- agent:backlog -->\n- [ ] [#c] still open\n<!-- /agent:backlog -->\n";
        let record = |done: &'static [String], added: &'static [String]| RecordedTrackedWork {
            done_ids: done,
            added_ids: added,
            requested_done_ids: &[],
            requested_added_ids: &[],
            requested_mutations: true,
            mutations_applied: true,
        };
        let ids: &'static [String] = Box::leak(ids.into_boxed_slice());
        assert!(!recorded_tracked_work_is_unlanded(record(ids, ids), reaped));
        // Not reaped yet: still open, so the done half has not landed.
        let open = "- [ ] [#a] x\n- [ ] [#b] y\n";
        assert!(recorded_tracked_work_is_unlanded(record(ids, ids), open));
        // A plain add (not completed) still needs its row in the document.
        assert!(recorded_tracked_work_is_unlanded(record(&[], ids), reaped));
    }
    use super::*;

    /// `#retaineddeferwedge`: a deferral continues to its commit only on an
    /// observed convergence AND an observed landing.
    #[test]
    fn deferred_tracked_work_continues_only_when_converged_and_landed() {
        use DeferredTrackedWorkResolution::*;
        assert_eq!(
            resolve_deferred_tracked_work(true, Some(false)),
            ContinueToCommit
        );
        assert_eq!(
            resolve_deferred_tracked_work(true, Some(true)),
            StillRetained
        );
        assert_eq!(resolve_deferred_tracked_work(true, None), StillRetained);
        assert_eq!(
            resolve_deferred_tracked_work(false, Some(false)),
            StillRetained
        );
        let message = deferred_tracked_work_timeout_message("doc.md", 30);
        assert!(message.contains("NOT committed"), "{message}");
        assert!(
            message.contains("agent-doc write --commit doc.md"),
            "{message}"
        );
        assert!(!message.contains("agent-doc commit doc.md"), "{message}");
    }

    /// `#retainedprojexit1`: the closeout's mutation phase must never call a
    /// retained delivery projection a half-apply.
    ///
    /// Observed 2026-09-20 on `cycle-1789878037902`: `respond` printed
    /// `[pending] completed and reaped 1 item(s)` twice, then exited 1 with
    /// "the document is half-applied" — while disk already matched HEAD at
    /// `16d6ab2981` with the response section AND the done-archive move both
    /// present, and a second `session-check` flipped
    /// `INTERRUPTED(write_applied)` to `ok(committed, commit_success)`. Nothing
    /// was half-applied, and both remedies the message named would have
    /// double-applied the tracked-work half.
    #[test]
    fn a_retained_delivery_projection_is_a_deferral_not_a_half_apply() {
        let retained = format!(
            "document write retained: {AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN} \
             {RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN}"
        );

        let failure = classify_tracked_work_mutation_failure(&retained, false, Some(true));

        assert_eq!(
            failure,
            TrackedWorkMutationFailure::DeferredDeliveryProjection,
            "a live replica converges on its own; the token says so"
        );
        assert!(
            failure.is_deferral(),
            "a deferral must let the closeout report success instead of exiting 1"
        );
    }

    /// The remedy half of the same defect: a deferral must name only the
    /// await-and-observe path.
    #[test]
    fn a_deferral_message_names_no_resubmit_remedy() {
        let rendered = tracked_work_mutation_failure_message(
            TrackedWorkMutationFailure::DeferredDeliveryProjection,
            "plan.md",
        );

        assert!(
            rendered.contains("agent-doc session-check plan.md"),
            "the deferral must name the observe path: {rendered}"
        );
        assert!(
            rendered.contains("deferral, not a lost or half-applied closeout"),
            "the deferral must say plainly that nothing is half-applied: {rendered}"
        );
        for double_applying in [
            "Recover with `agent-doc commit",
            "re-run the tracked-work half via `agent-doc write --commit plan.md --backlog-only",
        ] {
            assert!(
                !rendered.contains(double_applying),
                "a deferral must not name `{double_applying}` — the tracked-work half \
                 is already retained, so resubmitting it double-applies: {rendered}"
            );
        }
    }

    /// The other half of `#retainedprojexit1`: half-applied framing is reserved
    /// for a cycle PROVEN half-applied, and unprovable stays fail-closed.
    #[test]
    fn half_applied_framing_requires_proof_that_the_mutations_are_unlanded() {
        let generic = "pending/status write failed: disk projection rejected";

        assert_eq!(
            classify_tracked_work_mutation_failure(generic, false, Some(true)),
            TrackedWorkMutationFailure::HalfApplied,
            "recorded mutations still missing from the document IS the half-apply"
        );
        assert_eq!(
            classify_tracked_work_mutation_failure(generic, false, None),
            TrackedWorkMutationFailure::HalfApplied,
            "unprovable is not proof of landing; stay on the fail-closed branch"
        );
        assert_eq!(
            classify_tracked_work_mutation_failure(generic, false, Some(false)),
            TrackedWorkMutationFailure::ProjectionFailedAfterLanding,
            "mutations that are provably visible are not a half-apply"
        );
        assert_eq!(
            classify_tracked_work_mutation_failure(generic, true, Some(true)),
            TrackedWorkMutationFailure::RetainedWithResponse,
            "a retained response write keeps its own framing"
        );
    }

    /// Only the proven half-apply may print a resubmit remedy; every other
    /// variant would double-apply a mutation that already landed.
    #[test]
    fn only_a_proven_half_apply_names_a_resubmit_remedy() {
        for (failure, may_resubmit) in [
            (TrackedWorkMutationFailure::HalfApplied, true),
            (
                TrackedWorkMutationFailure::DeferredDeliveryProjection,
                false,
            ),
            (
                TrackedWorkMutationFailure::ProjectionFailedAfterLanding,
                false,
            ),
            (TrackedWorkMutationFailure::RetainedWithResponse, false),
        ] {
            let rendered = tracked_work_mutation_failure_message(failure, "plan.md");
            assert_eq!(
                rendered.contains("--backlog-only"),
                may_resubmit,
                "{failure:?} rendered the wrong remedy class: {rendered}"
            );
        }
    }

    /// `#retainedprojexit1`: the classifier is only worth having if the
    /// closeout that used to render one message for every failure consults it.
    ///
    /// The state that makes it fire — a retained refusal from the mutation
    /// phase mid-closeout — is not reachable from a unit test, so guard the
    /// wiring structurally, the same way `#retainedmutdrop` guards its
    /// provenance sites below.
    #[test]
    fn the_closeout_mutation_phase_consults_the_shared_classifier() {
        let source = include_str!("../../agent-doc-write-runtime-io/src/lib.rs");

        assert!(
            source.contains("classify_tracked_work_mutation_failure"),
            "the closeout mutation phase must classify its failure through the \
             shared classifier; deciding it locally is what let a retained \
             deferral be reported as a half-apply"
        );
        assert!(
            source.contains("tracked_work_mutation_failure_message"),
            "the closeout mutation phase must render its message from the shared \
             owner so a remedy cannot drift back onto a deferral"
        );
        assert!(
            source.contains("recorded_tracked_work_unlanded_now"),
            "the closeout mutation phase must prove whether the mutations landed \
             before any message calls the document half-applied"
        );
        assert!(
            !source.contains("the document is half-applied. Recover with"),
            "the closeout mutation phase must not re-author the half-applied \
             wording — that is how it stayed attached to every failure"
        );
    }

    /// `#retainedmutdrop`: the predicate is only worth having if both deciding
    /// sites actually consult it.
    ///
    /// The pure predicate below is mutation-checked, but removing the *wiring*
    /// at either call site reddened nothing — the whole defect was two sites
    /// each deciding provenance on their own, so an untested wiring is the
    /// exact regression shape to guard. Both sites are single and named; the
    /// state that makes them fire (a retained write mid-closeout) is not
    /// reachable from a unit test, so guard them structurally — the same reason
    /// `#preflightprojpass` guards its entry points this way.
    #[test]
    fn both_provenance_sites_consult_the_shared_predicate() {
        for (label, source) in [
            (
                "session-check",
                include_str!("../../agent-doc-session-check-io/src/command.rs"),
            ),
            (
                "commit",
                include_str!("../../agent-doc-commit-io/src/lib.rs"),
            ),
            // `#mutplanwitness`: the captured-closeout resume decides whether to
            // replay the tracked-work half from the same predicate. It is a
            // third deciding site, so it belongs in this guard too.
            (
                "captured-finalize-resume",
                include_str!("../../agent-doc-repair-command-io/src/lib.rs"),
            ),
        ] {
            assert!(
                source.contains("recorded_tracked_work_is_unlanded"),
                "`{label}` must decide unanswered-edit provenance with the shared \
                 predicate; deciding it locally is what let a closeout's mutations \
                 be swept as an operator edit"
            );
            assert!(
                !source.contains(".with_unanswered_edit(true)"),
                "`{label}` must not assert an unanswered edit unconditionally — \
                 that is the false premise this fix removed"
            );
        }
    }

    /// `#mutplanwitness`: a closeout whose only mutation is a gate has no id the
    /// text witnesses can see.
    ///
    /// Observed as the residual gap in `#deferredmutdrop` (0.35.363): the
    /// captured-closeout resume is gated on this predicate, so a plan carrying
    /// only `--backlog-gate` / `--backlog-ungate` / `--backlog-edit` /
    /// `--backlog-reorder` / `--review-edit` / `--status` resumed with the
    /// response and dropped its tracked-work half, because every id witness
    /// reported "landed" for a document that never received the mutation.
    #[test]
    fn a_mutation_only_closeout_with_no_done_or_add_id_is_still_unlanded() {
        // The gate moved `#gateonly` out of the active backlog, but the document
        // under inspection is byte-identical to its pre-cycle state — and it
        // names no id this cycle recorded as done or added.
        let disk = "- [ ] [#gateonly] still open in the live backlog\n";

        assert!(
            recorded_tracked_work_is_unlanded(
                RecordedTrackedWork {
                    done_ids: &[],
                    added_ids: &[],
                    requested_done_ids: &[],
                    requested_added_ids: &[],
                    requested_mutations: true,
                    mutations_applied: false,
                },
                disk,
            ),
            "a requested tracked-work mutation that never published is this \
             closeout's unlanded write"
        );

        // Once the envelope published, the same document is not this cycle's
        // unlanded write — which is what keeps `commit` refusing to swallow a
        // genuine operator edit.
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &[],
                added_ids: &[],
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: true,
                mutations_applied: true,
            },
            disk,
        ));

        // A cycle that requested nothing is unaffected in either direction.
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &[],
                added_ids: &[],
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));
    }

    /// `#retainedmutdrop`: a `--done` whose item is still open on disk, or an
    /// add that is not on disk at all, is this closeout's unlanded write — not
    /// an operator edit for recovery to sweep.
    #[test]
    fn an_unlanded_recorded_mutation_is_not_an_operator_edit() {
        let done = vec!["projpassstart".to_string()];
        let added = vec!["relayresolveblind".to_string()];

        // The exact shape observed: the --done item is still open and the added
        // item never arrived.
        let disk = "- [ ] [#projpassstart] measure the start resume path\n";
        assert!(recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &done,
                added_ids: &added,
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));

        // The `#id` spelling must not change the answer.
        let hashed = vec!["#projpassstart".to_string()];
        assert!(recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &hashed,
                added_ids: &[],
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));
    }

    /// `#mutprovenancepreresponse`: intent alone proves ownership when the
    /// post-hoc record never got written.
    ///
    /// `done_ids` / `added_ids` are recorded by the mutation phase, which runs
    /// AFTER the response write. Observed 2026-08-09 closing
    /// `#hooktriggerunresolved`: `respond` hit the pre-write delivery barrier,
    /// the response reached HEAD, the `--done` never applied and was never
    /// recorded — so this predicate correctly saw nothing, and the divergence
    /// was classified as a fresh operator edit with the queue head unstruck.
    #[test]
    fn requested_intent_alone_proves_the_divergence_is_ours() {
        let requested = vec!["hooktriggerunresolved".to_string()];
        // Exactly the observed shape: the post-hoc record is EMPTY because the
        // mutation phase never ran, and the item is still open on disk.
        let disk = "- [ ] [#hooktriggerunresolved] FIXED in 0.35.219\n";
        assert!(recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &[],
                added_ids: &[],
                requested_done_ids: &requested,
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));

        // A requested ADD that never arrived is ours too.
        let requested_add = vec!["neverlanded".to_string()];
        assert!(recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &[],
                added_ids: &[],
                requested_done_ids: &[],
                requested_added_ids: &requested_add,
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));

        // And once the intent HAS landed, it stops claiming the divergence —
        // otherwise every later operator edit would read as ours forever.
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &[],
                added_ids: &[],
                requested_done_ids: &requested,
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            "- [x] [#hooktriggerunresolved] FIXED in 0.35.219\n",
        ));
    }

    /// The intent must be recorded BEFORE the response is published, or it does
    /// not solve the problem it exists for.
    ///
    /// The whole defect is that provenance written after the response write is
    /// absent exactly when the write fails post-landing. A unit test cannot
    /// reach that state, and ordering is invisible to a behavioural test that
    /// succeeds — so guard the position structurally.
    #[test]
    fn the_intent_is_recorded_before_the_response_write() {
        let source = include_str!("../../agent-doc-write-runtime-io/src/lib.rs");
        let record = source
            .find("record_requested_tracked_work(")
            .expect("the write path must record tracked-work intent");
        let write = source
            .find("let write_result = if options.is_ipc {")
            .expect("the response write site moved or was renamed");
        assert!(
            record < write,
            "tracked-work intent must be recorded BEFORE the response cell is \
             published; recorded at byte {record}, response write at {write}"
        );
    }

    /// The other half, and the one that keeps `commit` from swallowing the
    /// operator's next prompt: once every recorded mutation is visible, a
    /// queue/backlog divergence really is a fresh edit.
    #[test]
    fn a_fully_landed_closeout_leaves_divergence_to_the_operator() {
        let done = vec!["projpassstart".to_string()];
        let added = vec!["relayresolveblind".to_string()];
        // The done item was reaped away; the added item is present.
        let disk = "- [ ] [#relayresolveblind] attribute the crdt_relay resolves\n";
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &done,
                added_ids: &added,
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            disk,
        ));

        // A gated (not open) item also counts as landed — `--done` that became
        // `[/]` is not the untouched `[ ]` this predicate looks for.
        let gated = "- [/] [#projpassstart] x\n- [ ] [#relayresolveblind] y\n";
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork {
                done_ids: &done,
                added_ids: &added,
                requested_done_ids: &[],
                requested_added_ids: &[],
                requested_mutations: false,
                mutations_applied: false,
            },
            gated,
        ));
    }

    /// A closeout that recorded nothing can never claim the divergence.
    #[test]
    fn no_recorded_mutations_never_claims_the_divergence() {
        assert!(!recorded_tracked_work_is_unlanded(
            RecordedTrackedWork::default(),
            "- [ ] [#anything] a live operator edit\n",
        ));
    }

    #[test]
    fn only_a_durable_holder_earns_the_deferral_claim() {
        assert_eq!(
            RetainedWriteOwnership::new(true, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(false, true).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(true, true).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(false, false).verdict(),
            RetainedWriteVerdict::Stranded,
            "the 2026-08-03 shape: newest cycle committed hours earlier, zero retained captures"
        );
        assert_eq!(
            RetainedWriteOwnership::UNOWNED
                .with_unanswered_edit(true)
                .with_retained_projection(true)
                .verdict(),
            RetainedWriteVerdict::Deferred,
            "a compact continuation owns partial projection drift before it can be an unanswered edit"
        );
    }

    /// A site that has not looked must not inherit the deferral promise. This is
    /// the whole failure mode: the promise was unconditional, so every site made
    /// it for free.
    #[test]
    fn an_unproven_site_reads_as_stranded() {
        assert!(RetainedWriteOwnership::UNOWNED.is_stranded());
    }

    /// The owned case keeps the 2026-07-26 safety boundary: two sessions
    /// invented recoveries that each perturbed the capture being awaited. Its
    /// observation instruction remains keyed so queue continuation cannot
    /// switch the subject to a successor cycle.
    #[test]
    fn the_owned_remedy_still_forbids_every_invented_recovery() {
        let remedy = retained_write_remedy(RetainedWriteOwnership::new(true, false), "plan.md");

        assert!(remedy.contains("this exact retained intent"));
        assert!(remedy.contains("controller-owned terminal state edge"));
        assert!(!remedy.contains("Run `agent-doc session-check plan.md`"));
        assert!(remedy.contains("deferral, not a lost response"));
        for invented in ["force disk", "admin recycle", "admin reload-lib", "re-send"] {
            assert!(
                remedy.contains(invented),
                "must explicitly rule out `{invented}`: {remedy}"
            );
        }
    }

    #[test]
    fn prewrite_projection_remedy_retries_the_mutation_not_response_closeout() {
        let remedy =
            retained_projection_remedy(RetainedProjectionOwnership::PrewriteMutation, "plan.md");

        assert!(remedy.contains("Retry the same mutation"));
        assert!(remedy.contains("no response-cycle write owns"));
        assert!(remedy.contains("do NOT run `agent-doc commit plan.md`"));
        assert!(!remedy.contains("Recover from the pane"));
        assert!(!remedy.contains("STRANDED"));
    }

    /// `#ownershipverdictdiverges`: an uncaptured `write_applied` cycle is not
    /// self-committing. A retained capture is different: its captured-finalize
    /// worker owns the terminal commit and wakes on editor convergence.
    #[test]
    fn uncaptured_write_applied_is_awaiting_a_terminal_commit() {
        let applied = RetainedWriteOwnership::new_with_phase(true, false, true);
        assert_eq!(
            applied.verdict(),
            RetainedWriteVerdict::AwaitingTerminalCommit,
            "an uncaptured write_applied cycle has no binary owner left"
        );
        assert!(!applied.is_stranded(), "the response body IS durable");

        // The other open phases are unchanged: a state edge really does still fire.
        assert_eq!(
            RetainedWriteOwnership::new_with_phase(true, false, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new_with_phase(false, true, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
    }

    /// Regression for the editor-save race: the response reached canonical
    /// editor authority, native-save projection lagged, and the capture worker
    /// remained active. The refusal must not tell the model to race that worker
    /// with a second `agent-doc commit` invocation.
    #[test]
    fn captured_write_applied_remains_binary_owned() {
        let captured = RetainedWriteOwnership::new_with_phase(true, true, true);
        assert_eq!(captured.verdict(), RetainedWriteVerdict::Deferred);

        let remedy = retained_write_remedy(captured, "plan.md");
        assert!(remedy.contains("this exact retained intent"));
        assert!(remedy.contains("controller-owned terminal state edge"));
        assert!(!remedy.contains("Run `agent-doc session-check plan.md`"));
        assert!(remedy.contains("commits itself"));
        assert!(
            !remedy.contains("Finish it from the pane"),
            "manual commit recovery races captured-finalize ownership: {remedy}"
        );
        assert!(
            !captured.verdict().commit_is_the_named_recovery(),
            "the captured-finalize worker, not the model, owns terminal commit"
        );
    }

    /// The remedy must name the command that actually recovers. Naming only
    /// `session-check` — an OBSERVATION — is what left the agent with an
    /// accurate diagnosis and no way to act on it.
    #[test]
    fn the_awaiting_commit_remedy_names_agent_doc_commit() {
        let remedy = retained_write_remedy(
            RetainedWriteOwnership::new_with_phase(true, false, true),
            "plan.md",
        );

        assert!(remedy.contains("agent-doc commit plan.md"));
        assert!(remedy.contains("ALREADY LANDED"));
        assert!(
            !remedy.contains("commits itself"),
            "promising a self-commit is the defect being fixed: {remedy}"
        );
        assert!(
            !remedy.contains("deferral, not a lost response"),
            "must not reuse the deferred verdict's phrase: {remedy}"
        );
        // Re-sending is still forbidden: the body is durable, so a re-send
        // duplicates rather than recovers.
        for invented in ["re-send", "force disk", "admin recycle", "admin reload-lib"] {
            assert!(
                remedy.contains(invented),
                "must rule out `{invented}`: {remedy}"
            );
        }
    }

    /// `#strandedremedydeadlock`: the deadlock was two predicates disagreeing
    /// about one state — the remedy sent the agent to `agent-doc commit`, and
    /// `commit` refused. Naming the command and accepting it are now the SAME
    /// fact, so they cannot drift apart again: whatever `retained_write_remedy`
    /// prints, `commit_is_the_named_recovery` must agree with, for every
    /// verdict.
    #[test]
    fn commit_accepts_exactly_the_verdicts_whose_remedy_names_it() {
        for verdict in [
            RetainedWriteVerdict::Deferred,
            RetainedWriteVerdict::Stranded,
            RetainedWriteVerdict::AwaitingTerminalCommit,
            RetainedWriteVerdict::CaptureResumeUnowned,
            RetainedWriteVerdict::UnansweredEditPending,
            RetainedWriteVerdict::DeliveryRejected,
            RetainedWriteVerdict::ReplicaUnserved,
        ] {
            let ownership = match verdict {
                RetainedWriteVerdict::Deferred => RetainedWriteOwnership::new(true, false),
                RetainedWriteVerdict::Stranded => RetainedWriteOwnership::UNOWNED,
                RetainedWriteVerdict::AwaitingTerminalCommit => {
                    RetainedWriteOwnership::new_with_phase(true, false, true)
                }
                RetainedWriteVerdict::CaptureResumeUnowned => {
                    RetainedWriteOwnership::new(true, true).with_capture_resume_unowned(true)
                }
                RetainedWriteVerdict::UnansweredEditPending => {
                    RetainedWriteOwnership::UNOWNED.with_unanswered_edit(true)
                }
                RetainedWriteVerdict::DeliveryRejected => {
                    RetainedWriteOwnership::new(true, true).with_delivery_rejected(true)
                }
                RetainedWriteVerdict::ReplicaUnserved => {
                    RetainedWriteOwnership::new(true, false).with_replica_unserved(true)
                }
            };
            assert_eq!(ownership.verdict(), verdict, "fixture builds {verdict:?}");

            let remedy = retained_write_remedy(ownership, "plan.md");
            let names_commit = remedy.contains("`agent-doc commit plan.md`");
            let forbids_commit = remedy.contains("Do NOT run `agent-doc commit plan.md`");
            assert_eq!(
                names_commit && !forbids_commit,
                verdict.commit_is_the_named_recovery(),
                "{verdict:?}: the remedy and the commit-side predicate must not disagree: {remedy}"
            );
        }
    }

    /// `#capturedresumeunowned`: a durable capture with no resume driver is the
    /// one shape where "wait, it commits itself" is provably false. Observed
    /// 2026-09-11 on `src/haiven-dev/tasks/backend.md`, whose cycle sat at
    /// `response_captured` across turns while the controller logged
    /// `reason=no_supervisor_idle_watch` and `delivery_converged=true`.
    #[test]
    fn a_capture_with_no_resume_driver_is_not_deferred() {
        assert_eq!(
            RetainedWriteOwnership::new(true, true)
                .with_capture_resume_unowned(true)
                .verdict(),
            RetainedWriteVerdict::CaptureResumeUnowned,
        );
        // The same facts with a driver running keep the deferral they earn.
        assert_eq!(
            RetainedWriteOwnership::new(true, true).verdict(),
            RetainedWriteVerdict::Deferred,
        );
        // A durable non-response projection continuation still owns the write,
        // so an absent captured-finalize driver does not strand it.
        assert_eq!(
            RetainedWriteOwnership::new(true, true)
                .with_capture_resume_unowned(true)
                .with_retained_projection(true)
                .verdict(),
            RetainedWriteVerdict::Deferred,
        );
        // The flag says nothing when no capture is retained.
        assert_eq!(
            RetainedWriteOwnership::UNOWNED
                .with_capture_resume_unowned(true)
                .verdict(),
            RetainedWriteVerdict::Stranded,
        );
    }

    /// The remedy must name a command that actually runs, and must still refuse
    /// the recoveries that perturb the capture being awaited: re-sending the
    /// response is wrong here even though waiting is also wrong.
    #[test]
    fn the_unowned_capture_remedy_names_the_resume_without_licensing_a_re_send() {
        let remedy = retained_write_remedy(
            RetainedWriteOwnership::new(true, true).with_capture_resume_unowned(true),
            "plan.md",
        );

        assert!(remedy.contains("agent-doc repair --resume-capture plan.md"));
        assert!(remedy.contains("DURABLE but UNOWNED"));
        for invented in ["re-send", "force disk", "admin recycle"] {
            assert!(
                remedy.contains(invented),
                "must still rule out `{invented}`: {remedy}"
            );
        }
        assert!(
            !remedy.contains("commits itself"),
            "the deferral promise is the false claim being removed: {remedy}"
        );
    }
    /// The unowned case must name a recovery rather than forbid one. A session
    /// that obeys the owned wording here waits forever on an edge that cannot
    /// fire, which is exactly what happened three times on 2026-08-03.
    #[test]
    fn the_unowned_remedy_names_the_recovery_instead_of_forbidding_it() {
        let remedy = retained_write_remedy(RetainedWriteOwnership::UNOWNED, "plan.md");

        assert!(remedy.contains("STRANDED, not deferred"));
        assert!(remedy.contains("agent-doc commit plan.md"));
        assert!(remedy.contains("agent-doc write --commit plan.md"));
        assert!(
            !remedy.contains("do NOT"),
            "forbidding recovery is the defect being fixed: {remedy}"
        );
        assert!(
            !remedy.contains("deferral, not a lost response"),
            "the deferral promise must be earned, not asserted: {remedy}"
        );
    }

    /// GH #131: a retained write whose only holders wait on an editor endpoint
    /// that answered NO is NOT owned. Every holder shape (open cycle, retained
    /// capture, retained delivery projection) must yield the rejected verdict,
    /// and a rejection with nothing to hold still reads as stranded.
    #[test]
    fn a_holder_waiting_on_a_rejecting_endpoint_is_not_deferred() {
        for owned in [
            RetainedWriteOwnership::new(true, false),
            RetainedWriteOwnership::new(false, true),
            RetainedWriteOwnership::new(true, true),
            RetainedWriteOwnership::UNOWNED.with_retained_projection(true),
            RetainedWriteOwnership::new_with_phase(true, true, true).with_retained_projection(true),
        ] {
            assert_eq!(owned.verdict(), RetainedWriteVerdict::Deferred, "{owned:?}");
            let rejected = owned.with_delivery_rejected(true);
            assert_eq!(
                rejected.verdict(),
                RetainedWriteVerdict::DeliveryRejected,
                "{rejected:?} must not read as owned"
            );
            assert!(!rejected.verdict().commit_is_the_named_recovery());
        }
        // Nothing held at all: the rejection does not invent a holder.
        assert_eq!(
            RetainedWriteOwnership::UNOWNED
                .with_delivery_rejected(true)
                .verdict(),
            RetainedWriteVerdict::Stranded,
        );
        // Not looked == not rejected: the default keeps the deferral.
        assert_eq!(
            RetainedWriteOwnership::new(true, true)
                .with_delivery_rejected(false)
                .verdict(),
            RetainedWriteVerdict::Deferred,
        );
    }

    /// GH #131: the rejected remedy names a real recovery (remove the rejecting
    /// endpoint) and does NOT carry the owned case's blanket prohibition on
    /// `admin recycle` / `admin reload-lib`, nor its "commits itself" promise.
    #[test]
    fn the_rejected_remedy_names_a_recovery_instead_of_forbidding_every_one() {
        let remedy = retained_write_remedy(
            RetainedWriteOwnership::new(true, true)
                .with_retained_projection(true)
                .with_delivery_rejected(true),
            "plan.md",
        );
        assert!(remedy.contains("REJECTED the delivery receipt"), "{remedy}");
        assert!(remedy.contains("restart or reload the editor"), "{remedy}");
        assert!(remedy.contains("agent-doc session-check plan.md"), "{remedy}");
        assert!(remedy.contains("re-send"), "the response is durable: {remedy}");
        for forbidden_claim in [
            "deferral, not a lost response",
            "commits itself",
            "`admin recycle`, or `admin reload-lib`",
        ] {
            assert!(
                !remedy.contains(forbidden_claim),
                "rejected remedy must not carry `{forbidden_claim}`: {remedy}"
            );
        }
    }

    /// GH #144: once the refusing endpoint is actually unregistered and the
    /// authority reports no live replica, the durable cycle/capture/projection
    /// facts no longer prove a holder. Waiting is impossible; commit is the
    /// recovery the same predicate must both print and admit.
    #[test]
    fn an_unregistered_editor_route_with_zero_live_replicas_is_stranded() {
        let ownership = RetainedWriteOwnership::new(true, true)
            .with_retained_projection(true)
            .with_delivery_rejected(true)
            .with_editor_route_unowned(true);

        assert_eq!(ownership.verdict(), RetainedWriteVerdict::Stranded);
        assert!(ownership.verdict().commit_is_the_named_recovery());
        let remedy = retained_write_remedy(ownership, "plan.md");
        assert!(remedy.contains("UNREGISTERED"), "{remedy}");
        assert!(remedy.contains("ZERO live editor replicas"), "{remedy}");
        assert!(remedy.contains("`agent-doc commit plan.md`"), "{remedy}");
        assert!(!remedy.contains("restart or reload the editor"), "{remedy}");
        assert!(!remedy.contains("commits itself"), "{remedy}");
    }

    /// Every input combination, so a new verdict branch cannot quietly
    /// reintroduce the contradiction.
    fn every_ownership() -> Vec<RetainedWriteOwnership> {
        (0u16..512)
            .map(|bits| {
                let bit = |n: u16| bits & (1 << n) != 0;
                RetainedWriteOwnership::new_with_phase(bit(0), bit(1), bit(2))
                    .with_retained_projection(bit(3))
                    .with_unanswered_edit(bit(4))
                    .with_capture_resume_unowned(bit(5))
                    .with_delivery_rejected(bit(6))
                    .with_editor_route_unowned(bit(7))
                    .with_replica_unserved(bit(8))
            })
            .collect()
    }

    /// GH #131 shape 2: `session-check`'s integrity gate prescribed `admin
    /// reload-lib` for an editor not serving its replica, while the retained
    /// write's deferral forbade it for the same document. An observed unserved
    /// replica now yields a verdict whose remedy IS the integrity gate's
    /// recovery, derived from the one function both render.
    #[test]
    fn an_unserved_replica_names_the_integrity_gate_recovery() {
        let owned = RetainedWriteOwnership::new(false, false)
            .with_retained_projection(true)
            .with_replica_unserved(true);
        assert_eq!(owned.verdict(), RetainedWriteVerdict::ReplicaUnserved);
        let remedy = retained_write_remedy(owned, "plan.md");
        assert!(remedy.contains(editor_replica_recovery()), "{remedy}");
        assert!(remedy.contains("agent-doc admin reload-lib"), "{remedy}");
        assert!(remedy.contains("agent-doc session-check plan.md"), "{remedy}");
        assert!(remedy.contains("re-send"), "retained work is durable: {remedy}");
        assert!(
            !remedy.contains("deferral, not a lost response"),
            "an unserved replica never converges on its own: {remedy}"
        );
        assert!(
            !owned.verdict().commit_is_the_named_recovery(),
            "the integrity gate refuses commit for the same observation"
        );
    }

    /// GH #131 shape 2, the other half: no remedy may forbid `admin reload-lib`
    /// categorically, because the integrity gate prescribes it. The deferral
    /// keeps forbidding invented recoveries and states when reload-lib is
    /// sanctioned instead.
    #[test]
    fn no_remedy_forbids_the_recovery_the_integrity_gate_prescribes() {
        for ownership in every_ownership() {
            let verdict = ownership.verdict();
            let remedy = retained_write_remedy(ownership, "plan.md");
            if matches!(
                verdict,
                RetainedWriteVerdict::Deferred | RetainedWriteVerdict::ReplicaUnserved
            ) {
                assert!(
                    !remedy.contains("`admin recycle`, or `admin reload-lib`"),
                    "{verdict:?} must not forbid reload-lib outright: {remedy}"
                );
            }
            if verdict == RetainedWriteVerdict::Deferred {
                assert!(remedy.contains(EDITOR_REPLICA_RELOAD_SANCTION), "{remedy}");
                for invented in ["re-send", "force disk", "admin recycle"] {
                    assert!(remedy.contains(invented), "{invented}: {remedy}");
                }
            }
            if ownership.replica_unserved {
                assert!(
                    verdict != RetainedWriteVerdict::Deferred,
                    "an observed unserved replica must never read as a self-completing \
                     deferral: {ownership:?}"
                );
            }
        }
    }

    /// `#retainedobservationrace`: a durable keyed capture can commit and let
    /// auto-queue advance before an operator runs a document-wide status
    /// command. That command then describes the successor cycle rather than
    /// the retained intent, so the deferral must keep observation with its
    /// keyed controller owner.
    #[test]
    fn deferred_remedy_observes_the_exact_intent_through_its_owner() {
        let owned = RetainedWriteOwnership::new_with_phase(true, true, false)
            .with_retained_projection(true);
        assert_eq!(owned.verdict(), RetainedWriteVerdict::Deferred);
        let remedy = retained_write_remedy(owned, "plan.md");
        assert!(remedy.contains("this exact retained intent"), "{remedy}");
        assert!(
            remedy.contains("controller-owned terminal state edge"),
            "{remedy}"
        );
        assert!(remedy.contains("different cycle"), "{remedy}");
        assert!(
            !remedy.contains("Run `agent-doc session-check plan.md`"),
            "an unkeyed current-cycle check can observe a successor: {remedy}"
        );
    }

    /// GH #131 shape 1: `session-check` named `write --done <id> --pending-only
    /// --commit`, and that command refused. The repair is admissible exactly
    /// when the write path would not refuse it, asked of the same predicate.
    #[test]
    fn a_tracked_work_repair_is_named_only_when_the_write_path_can_complete_it() {
        let repair = "agent-doc write plan.md --done x --pending-only --commit";
        for ownership in every_ownership() {
            let write_path_verdict = ownership.with_retained_projection(true).verdict();
            let admission = tracked_work_repair_admission(ownership);
            let instruction = tracked_work_repair_instruction(ownership, "plan.md", repair);
            assert!(instruction.contains(repair), "{instruction}");
            match write_path_verdict {
                RetainedWriteVerdict::ReplicaUnserved | RetainedWriteVerdict::DeliveryRejected => {
                    assert_eq!(
                        admission,
                        TrackedWorkRepairAdmission::RecoverEditorFirst(write_path_verdict)
                    );
                    assert!(instruction.contains("AFTER recovering the editor"), "{instruction}");
                }
                _ => {
                    assert_eq!(admission, TrackedWorkRepairAdmission::Admissible);
                    assert_eq!(instruction, format!("`{repair}`"));
                }
            }
        }
        // The reported shape: a live replica still converging is admissible,
        // because the write absorbs that retention instead of refusing.
        assert_eq!(
            tracked_work_repair_admission(RetainedWriteOwnership::UNOWNED),
            TrackedWorkRepairAdmission::Admissible
        );
        let unserved = RetainedWriteOwnership::UNOWNED.with_replica_unserved(true);
        assert!(
            tracked_work_repair_instruction(unserved, "plan.md", repair)
                .contains("agent-doc admin reload-lib")
        );
    }

    /// GH #131 shape 1, the write path: a pending-only mutation retained by a
    /// converging delivery projection beside an already-committed response is
    /// absorbed, and every unproven fact keeps the refusal.
    #[test]
    fn a_pending_only_retention_is_absorbed_only_when_every_fact_is_proven() {
        let pending = format!(
            "retained [{AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN}] [{RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN}]"
        );
        let unserved = format!("retained [{AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN}]");
        assert_eq!(
            pending_only_retention(&pending, true, true, true),
            PendingOnlyRetention::Absorbed
        );
        for (message, committed, own, recorded) in [
            (unserved.as_str(), true, true, true),
            (pending.as_str(), false, true, true),
            (pending.as_str(), true, false, true),
            (pending.as_str(), true, true, false),
            ("unrelated failure", true, true, true),
        ] {
            assert_eq!(
                pending_only_retention(message, committed, own, recorded),
                PendingOnlyRetention::Refused,
                "{message} committed={committed} own={own} recorded={recorded}"
            );
        }
        let notice = pending_only_absorbed_notice("plan.md", "abc");
        assert!(notice.contains("agent-doc session-check plan.md"), "{notice}");
        assert!(!notice.contains("deferral, not a lost response"), "{notice}");
    }

    #[test]
    fn a_pending_only_retention_that_settled_before_inspection_is_awaited() {
        let pending = format!(
            "visible write deferred [{AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN}] \
             [{RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN}]"
        );
        assert_eq!(
            pending_only_retention(&pending, true, false, false),
            PendingOnlyRetention::AwaitSettledDelivery,
        );
    }
}
