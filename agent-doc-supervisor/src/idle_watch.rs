//! Pure supervisor idle-watch policy and messages.
//!
//! This module does not poll panes, run tmux, mutate files, or write ops logs.
//! Orchestration projects runtime facts into these helpers and performs the
//! effects itself.

use std::path::Path;
use std::time::Duration;

use agent_doc_state_scope::LocalProcessScope;
use lazily::{Computed, Source};

/// Scalar gate for a supervisor-owned captured-finalize resume. The effectful
/// idle watch supplies these facts. A durable captured operation already owns
/// the closeout lease, so recovery must remain live even while the harness turn
/// is blocked by its Stop hook. A pending Lazily current transition, IPC,
/// controller pressure, maintenance, and another resume worker remain the actual
/// concurrency gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedFinalizeResumeFacts {
    pub captured_operation_present: bool,
    pub actor_ready: bool,
    pub current_transition_pending: bool,
    pub ipc_inflight: u64,
    pub worker_in_flight: bool,
    pub retry_cooldown_elapsed: bool,
    pub controller_pressure_cooldown: bool,
    pub urgent_supervisor_maintenance: bool,
}

pub fn captured_finalize_resume_should_start(facts: CapturedFinalizeResumeFacts) -> bool {
    facts.captured_operation_present
        && !facts.current_transition_pending
        && facts.ipc_inflight == 0
        && !facts.worker_in_flight
        && facts.retry_cooldown_elapsed
        && !facts.controller_pressure_cooldown
        && !facts.urgent_supervisor_maintenance
}

/// Longest reason prefix a `needs_operator` diagnostic may carry.
///
/// The classifier reads an `anyhow` chain formatted with `{err:#}`, whose head is
/// code-authored context. The tail can quote document text, so the diagnostic
/// keeps the head and drops the rest — the full chain stays identified by its
/// `reason_sha256`.
const CAPTURED_FINALIZE_REASON_HEAD_CHARS: usize = 220;

/// A bounded, single-line, secret-redacted prefix of a resume failure reason.
///
/// Until 0.35.163 the `needs_operator` diagnostic logged only `reason_bytes` and
/// `reason_sha256` (`#needsoperatorstateedge`). That is enough to tell two
/// failures apart and nothing else: when the 2026-08-08 `haiven-dev.md` deadlock
/// was investigated, the reason that latched the closeout 27 times was
/// unrecoverable from any log, `state.db` row, or playback record. A hash is an
/// identity, not a diagnosis.
///
/// The prefix is the classifier's own input, so a reason that keeps landing in
/// [`crate::idle_watch`]'s default arm can be read directly and given a real
/// classification instead of the operator-required fallback.
pub fn captured_finalize_resume_reason_head(reason: &str) -> String {
    let redacted = agent_doc_secret_redact::redact(reason);
    let flattened = redacted
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '"' {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>();
    let mut head = flattened.split_whitespace().collect::<Vec<_>>().join(" ");
    if head.chars().count() > CAPTURED_FINALIZE_REASON_HEAD_CHARS {
        head = head
            .chars()
            .take(CAPTURED_FINALIZE_REASON_HEAD_CHARS)
            .collect::<String>()
            + "…";
    }
    head
}

/// The operator-facing message for a latched captured-finalize resume.
///
/// `write_ownership`'s rule applies here too: a site that stands down must name
/// the recovery rather than only announce that it stood down. The previous text
/// ("needs operator resolution; response retained without mutation") told the
/// operator a decision had been made and gave them no command to run, while
/// `session-check` was concurrently telling the agent that the controller owned
/// the next closeout. Neither side named an action, so neither side took one.
pub fn captured_finalize_resume_operator_message(file: &Path, reason_head: &str) -> String {
    format!(
        "[agent-doc] captured finalize for {} needs operator resolution; response retained without mutation. \
         Reason: {reason_head}. A later controller document-state edge retries this automatically; \
         if none arrives, recover from the pane that OWNS this session with `agent-doc write --commit {}` \
         (or `agent-doc commit {}` when the response body is already written). Do NOT use `--force-disk` \
         while an editor holds authority.",
        file.display(),
        file.display(),
        file.display(),
    )
}

/// Reactive trigger for one captured-finalize attempt.
///
/// State convergence is an input edge, not an error to retry. After an attempt
/// consumes the current edge, the graph remains quiet until either the
/// controller publishes a newer document-state edge or a genuinely failed
/// effect's backoff expires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CapturedFinalizeResumeTriggerProjection {
    operation_key: Option<String>,
    state_epoch: u64,
    consumed_state_epoch: u64,
    effect_retry_epoch: u64,
    consumed_effect_retry_epoch: u64,
    needs_operator: bool,
}

impl CapturedFinalizeResumeTriggerProjection {
    fn ready(&self) -> bool {
        self.operation_key.is_some()
            && !self.needs_operator
            && (self.state_epoch > self.consumed_state_epoch
                || self.effect_retry_epoch > self.consumed_effect_retry_epoch)
    }
}

/// Process-scoped Lazily graph separating document-state wakeups from effect
/// retries for captured finalize.
pub struct CapturedFinalizeResumeTriggers {
    scope: LocalProcessScope,
    projection: Source<CapturedFinalizeResumeTriggerProjection>,
    ready: Computed<bool>,
}

impl Default for CapturedFinalizeResumeTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl CapturedFinalizeResumeTriggers {
    pub fn new() -> Self {
        let scope = LocalProcessScope::new();
        let projection = scope
            .ctx()
            .source(CapturedFinalizeResumeTriggerProjection::default());
        let projection_for_ready = projection;
        let ready = scope
            .ctx()
            .computed(move |ctx| ctx.get(&projection_for_ready).ready());
        Self {
            scope,
            projection,
            ready,
        }
    }

    /// Observe the durable identity currently eligible for recovery. A newly
    /// observed operation carries its own initial state edge.
    pub fn observe_operation(&self, operation_key: Option<String>) {
        let mut projection = self.scope.ctx().get(&self.projection);
        if projection.operation_key == operation_key {
            return;
        }
        projection.operation_key = operation_key;
        projection.needs_operator = false;
        projection.state_epoch = projection.state_epoch.saturating_add(1);
        projection.consumed_state_epoch = projection.state_epoch.saturating_sub(1);
        projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Publish a controller-owned document-state edge.
    ///
    /// A state edge **retires** a prior `needs_operator` verdict
    /// (`#needsoperatorstateedge`). That verdict means "this effect failed on the
    /// document state as it was", not "this capture is unrecoverable forever":
    /// the classifier's default arm produces it for any error string it does not
    /// recognize, so an unfamiliar transient failure latched exactly like a real
    /// operator conflict. Because `needs_operator` also suppressed the state
    /// edge, no later controller transition could ever re-arm the attempt, and
    /// the only thing that clears the flag otherwise is a *different* operation
    /// key — which a retained capture never gets.
    ///
    /// Observed 2026-08-08 on `src/haiven-dev/tasks/haiven-dev.md`: the resume
    /// latched at 03:51:25, the controller published `ResumeSettledDelivery` with
    /// `proof=exact_target` at 03:55:48 (`delivery_converged=true`,
    /// `editor_converged=true`, `inflight=0`, `actor_idle=true`), and the
    /// supervisor ignored it. The cycle stayed open with `unmet=committed`
    /// forever while `session-check` told the agent the controller owned the next
    /// closeout — a deadlock by mutual deferral.
    ///
    /// This does not reintroduce blind retries: `observe_effect_retry_due` is
    /// still gated on `!needs_operator` by the caller, so only a real
    /// controller-published document transition can re-arm the attempt, and
    /// obeying that edge feeds the same observation stream that clears it
    /// (`#idlerevisionreactive`).
    pub fn observe_state_edge(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.state_epoch = projection.state_epoch.saturating_add(1);
        projection.needs_operator = false;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Publish expiry of a backoff for a failed effect. This is intentionally a
    /// different Source edge from document convergence.
    pub fn observe_effect_retry_due(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.effect_retry_epoch = projection.effect_retry_epoch.saturating_add(1);
        self.scope.ctx().set(&self.projection, projection);
    }

    pub fn consume_attempt(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.consumed_state_epoch = projection.state_epoch;
        projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Record that the last attempt failed on evidence a blind retry cannot
    /// change. Suppresses the effect-retry edge only; see
    /// [`Self::observe_state_edge`] for why a document transition retires it.
    pub fn require_operator(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.needs_operator = true;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Whether the last attempt latched an operator-required verdict that no
    /// document-state edge has retired yet.
    pub fn needs_operator(&self) -> bool {
        self.scope.ctx().get(&self.projection).needs_operator
    }

    pub fn ready(&self) -> bool {
        self.scope.ctx().get(&self.ready)
    }
}

/// Lazily-owned state for one queue-continuation edge.
///
/// A queue head is state, not a command to recursively invoke `agent-doc`.
/// The supervisor observes the head identity, derives whether an effect is due,
/// and consumes that edge only after the harness proves dispatch admission.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct QueueContinuationProjection {
    head_key: Option<String>,
    state_epoch: u64,
    consumed_state_epoch: u64,
    effect_retry_epoch: u64,
    consumed_effect_retry_epoch: u64,
    dispatch_effect_in_flight: bool,
}

impl QueueContinuationProjection {
    fn ready(&self) -> bool {
        self.head_key.is_some()
            && !self.dispatch_effect_in_flight
            && (self.state_epoch > self.consumed_state_epoch
                || self.effect_retry_epoch > self.consumed_effect_retry_epoch)
    }
}

/// Process-scoped Lazily graph for an owned pane's queue continuation.
///
/// Document/head observations and failed-effect retries are intentionally
/// separate source edges. Merely writing text into a pane does not consume the
/// document edge; a proven dispatch start does.
pub struct QueueContinuationTriggers {
    scope: LocalProcessScope,
    projection: Source<QueueContinuationProjection>,
    ready: Computed<bool>,
}

impl Default for QueueContinuationTriggers {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueContinuationTriggers {
    pub fn new() -> Self {
        let scope = LocalProcessScope::new();
        let projection = scope.ctx().source(QueueContinuationProjection::default());
        let projection_for_ready = projection;
        let ready = scope
            .ctx()
            .computed(move |ctx| ctx.get(&projection_for_ready).ready());
        Self {
            scope,
            projection,
            ready,
        }
    }

    /// Observe the current queue-head identity. Clearing and later re-adding the
    /// same text creates a new state edge because the intervening `None` is
    /// itself observed.
    pub fn observe_head(&self, head_key: Option<String>) {
        let mut projection = self.scope.ctx().get(&self.projection);
        if projection.head_key == head_key {
            return;
        }
        projection.head_key = head_key;
        projection.state_epoch = projection.state_epoch.saturating_add(1);
        projection.dispatch_effect_in_flight = false;
        if projection.head_key.is_none() {
            projection.consumed_state_epoch = projection.state_epoch;
            projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        }
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Mark the tmux/PTY delivery effect as started without claiming that the
    /// harness accepted it.
    pub fn begin_dispatch_effect(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.dispatch_effect_in_flight = true;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// Consume the current state edge only after dispatch-start proof.
    pub fn observe_dispatch_started(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.dispatch_effect_in_flight = false;
        projection.consumed_state_epoch = projection.state_epoch;
        projection.consumed_effect_retry_epoch = projection.effect_retry_epoch;
        self.scope.ctx().set(&self.projection, projection);
    }

    /// A failed boundary effect rearms independently of document state.
    pub fn observe_effect_failed(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.dispatch_effect_in_flight = false;
        projection.effect_retry_epoch = projection.effect_retry_epoch.saturating_add(1);
        self.scope.ctx().set(&self.projection, projection);
    }

    /// (`#strandeddraftresubmit`) The effect finished but nothing could observe
    /// whether it started a turn.
    ///
    /// Distinct from [`Self::observe_effect_failed`] on purpose: bumping the
    /// retry epoch is a claim that a blind retry is warranted, and a retry here
    /// means writing a SECOND trigger into a composer that may already hold the
    /// first one. Releasing the in-flight flag without that bump keeps the
    /// un-consumed document edge live, so the next tick re-observes the pane and
    /// reaches the pending-payload resubmit instead of re-injecting
    /// (`#idlerevisionreactive`).
    pub fn observe_effect_unobservable(&self) {
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.dispatch_effect_in_flight = false;
        self.scope.ctx().set(&self.projection, projection);
    }

    pub fn ready(&self) -> bool {
        self.scope.ctx().get(&self.ready)
    }
}

/// Whether an idle-queue dispatch proved that it started a turn.
///
/// `Unobservable` is deliberately distinct from `Unproven`: "the controller
/// projected no admission and the pane told us nothing" and "the trigger is
/// visibly still sitting in the composer" are different facts, and only the
/// second one is evidence that nothing was submitted (`#idlerevisionreactive`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueDispatchStartObservation {
    Proven,
    Unproven,
    Unobservable,
}

impl QueueDispatchStartObservation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Unproven => "unproven",
            Self::Unobservable => "unobservable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueDispatchStartFacts {
    /// Whether this payload needs dispatch-start proof at all. A slash command
    /// or a non-pane delivery has no admission projection to advance.
    pub proof_required: bool,
    /// Whether the controller's turn-admission projection advanced past the
    /// pre-dispatch baseline.
    pub admission_advanced: bool,
    /// Whether the pane could be captured after the send.
    pub pane_observed: bool,
    /// Whether the harness shows a busy cue. Accept-only evidence: a pane that
    /// is working right after a send from a proven-idle prompt is running our
    /// turn, which is why the auto-trigger path already races this against the
    /// admission RPC (`#autotriggeradmissionstall`).
    pub pane_busy: bool,
    /// Whether the payload is still visible unsubmitted in the current input.
    /// This is the ONLY positive evidence that the dispatch did not start.
    pub payload_still_pending: bool,
}

/// (`#strandeddraftresubmit`) Classify an idle-queue dispatch from the admission
/// projection plus one post-send pane observation.
///
/// The live failure this replaces: a null `turn_admission_projection`
/// (`controller_response_missing_data command=turn_admission_projection raw data
/// null ok true`) was read as `dispatch_start_unproven`, which bumped the
/// effect-retry epoch and rearmed the drain. A legitimately no-op turn never
/// projects an admission at all, so that inverted "could not observe" into "was
/// not submitted" and turned it into a fresh injection on top of whatever was
/// already in the composer.
pub const fn classify_queue_dispatch_start_observation(
    facts: QueueDispatchStartFacts,
) -> QueueDispatchStartObservation {
    if !facts.proof_required
        || facts.admission_advanced
        // Accept-only pane proof.
        || (facts.pane_observed && facts.pane_busy)
    {
        QueueDispatchStartObservation::Proven
    } else if facts.pane_observed && facts.payload_still_pending {
        QueueDispatchStartObservation::Unproven
    } else {
        QueueDispatchStartObservation::Unobservable
    }
}

/// Render the continuation delivered to an already-owned pane.
///
/// `#qcontprose`: this is the harness's own reopen trigger, not prose.
///
/// `#runpromptverbose`: it is the *harness-native* trigger, not a hardcoded bare
/// one. `specs/07-session-tmux-commands.md` has always specified the split —
/// "Claude/OpenCode drains inject the normal slash-command harness reopen, and
/// Codex drains inject the bare `agent-doc <FILE>` reopen" — and
/// `idle_queue_drain_payload_keeps_trigger_for_non_codex_harnesses` asserts
/// exactly that against `HarnessConfig::trigger_command`. This function ignored
/// the harness and hardcoded the bare form, so every Claude/OpenCode drain
/// silently violated both. `agent-doc <file>` does get admitted by the
/// `UserPromptSubmit` hook, which is why the divergence was invisible — but only
/// `/agent-doc <file>` also loads the skill deterministically, which matters in a
/// freshly-`/clear`ed pane where the agent has no prior workflow context. Codex
/// keeps the bare form because that is its harness-native entrypoint.
///
/// It used to render a five-line paragraph ("Agent Doc queue state advanced for
/// … do not invoke `agent-doc` recursively … Execute the current active queue
/// head: …"), on the theory that the bare trigger would recurse into the
/// session that already owns the document. It does not: in an owned pane the
/// harness is the process reading input, so `agent-doc <file>` is a *prompt*
/// that the harness `UserPromptSubmit` hook admits and answers with an
/// in-binary preflight — the same session continues. The route path has always
/// dispatched exactly this trigger into live owned panes
/// (`route_dispatch_drain_plain_trigger_pass_through`), and
/// `agent_doc_queue::idle_drain`'s own contract test already asserted the
/// payload is `agent-doc <file>`.
///
/// The paragraph was also redundant: naming the active head told the agent
/// nothing preflight does not already select and hand it as
/// `selected_queue_prompts`. Its only effects were a ~280-byte injection into
/// the operator's console and two dispatch paths that disagreed about what a
/// queue continuation looks like.
/// `#qcontabspath`: the path is absolutized because this string is now
/// **executed**, not merely read.
///
/// The paragraph this replaced embedded the same path descriptively, so a
/// relative one was harmless. As a trigger it is resolved against the **pane's**
/// working directory, which is not guaranteed to equal the supervisor's — the
/// pane's shell can be anywhere, including inside a submodule. Observed
/// immediately after `#qcontprose` shipped: the supervisor dispatched
/// `agent-doc tasks/agent-doc/agent-doc-bugs2.md`, the pane's cwd had moved to
/// `src/agent-doc`, the path did not resolve, and the harness admission hook
/// produced no cycle contract at all — a silently dead queue continuation.
///
/// `std::path::absolute` is lexical plus the supervisor's cwd, which is exactly
/// the directory the relative `file` was expressed against. Doing it here rather
/// than at the call site makes the guarantee unconditional: a caller cannot
/// forget it, and every dispatch path emits the same absolute form the route
/// path already sends.
pub fn owned_pane_queue_continuation_prompt(
    file: &Path,
    harness: &agent_doc_harness::HarnessConfig,
) -> String {
    let path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    harness.trigger_command(&path.to_string_lossy())
}

/// Exponential retry capped at one attempt per 30 seconds. Transient editor or
/// controller recovery can therefore complete unattended without recreating the
/// high-frequency finalize/backpressure loop it is intended to heal.
pub fn captured_finalize_resume_retry_delay(attempt: u32) -> Duration {
    const BASE_SECS: u64 = 2;
    const MAX_SECS: u64 = 30;
    let shift = attempt.saturating_sub(1).min(4);
    Duration::from_secs((BASE_SECS << shift).min(MAX_SECS))
}

/// `#supinstallfeedback` phases of the supervisor dogfood auto-install, used to
/// build the user-visible owned-pane status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorAutoInstallPhase {
    Started,
    Succeeded,
    Failed,
}

/// Build the owned-pane status message for an auto-install phase. The `Started`
/// line explicitly tells the operator not to press Enter because the visible
/// keepalive prompt can make the rebuild look like it is waiting on a keypress.
pub fn supervisor_auto_install_pane_message(phase: SupervisorAutoInstallPhase) -> &'static str {
    match phase {
        SupervisorAutoInstallPhase::Started => {
            "agent-doc: rebuilding the freshly-committed binary (~1 min) — do NOT press Enter; the supervisor auto-restarts when the build finishes"
        }
        SupervisorAutoInstallPhase::Succeeded => {
            "agent-doc: rebuild complete — recycling onto the fresh binary"
        }
        SupervisorAutoInstallPhase::Failed => {
            "agent-doc: auto-install failed — run the dogfood refresh to rebuild; staying on the current binary"
        }
    }
}

/// `#qstallguard` Layer C: should the supervisor idle-watch skip dispatch on a
/// queue under an accepted `admin queue pause`?
///
/// A pause suppresses unattended reinjection floods, not all draining. Skip only
/// when an in-session `/loop` owns the drain or there is no drainable head. With
/// no loop owner and a drainable head, the supervisor may perform the bounded
/// failsafe drain.
pub fn paused_idle_watch_should_skip(
    paused: bool,
    has_drainable_head: bool,
    loop_owner_lease_fresh: bool,
) -> bool {
    if !paused {
        return false;
    }
    loop_owner_lease_fresh || !has_drainable_head
}

/// Select the submit-mode diagnostic for an idle-queue dispatch from scalar
/// supervisor facts.
pub fn idle_queue_submit_mode(has_inject_pane: bool, harness_binary: &str) -> &'static str {
    if has_inject_pane {
        agent_doc_tmux_commands::tmux_submit_mode_for_harness(harness_binary)
    } else {
        "pty_cr"
    }
}

pub fn idle_queue_context_reset_ops_log_message(
    file: &Path,
    harness_binary: &str,
    clear_cmd: &str,
    target: &str,
    active_head: &str,
    reason: &str,
) -> String {
    format!(
        "idle_queue_watch_context_reset file={} harness={} cmd={:?} target={} head_bytes={} head_sha256={} reason={:?}",
        file.display(),
        harness_binary,
        clear_cmd,
        target,
        active_head.len(),
        agent_doc_hash::content_hash(active_head),
        reason,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof_required_facts() -> QueueDispatchStartFacts {
        QueueDispatchStartFacts {
            proof_required: true,
            admission_advanced: false,
            pane_observed: true,
            pane_busy: false,
            payload_still_pending: false,
        }
    }

    #[test]
    fn queue_dispatch_start_proof_is_not_required_without_a_pane_admission_edge() {
        assert_eq!(
            classify_queue_dispatch_start_observation(QueueDispatchStartFacts {
                proof_required: false,
                ..proof_required_facts()
            }),
            QueueDispatchStartObservation::Proven
        );
    }

    #[test]
    fn queue_dispatch_start_is_proven_by_an_advanced_admission_or_a_busy_pane() {
        assert_eq!(
            classify_queue_dispatch_start_observation(QueueDispatchStartFacts {
                admission_advanced: true,
                ..proof_required_facts()
            }),
            QueueDispatchStartObservation::Proven
        );
        // Accept-only pane proof: a harness working right after the send is
        // running our turn (`#autotriggeradmissionstall`).
        assert_eq!(
            classify_queue_dispatch_start_observation(QueueDispatchStartFacts {
                pane_busy: true,
                ..proof_required_facts()
            }),
            QueueDispatchStartObservation::Proven
        );
    }

    /// `#strandeddraftresubmit`: only a visibly pending payload proves the
    /// dispatch did not start. Every other unproven shape is UNOBSERVABLE, so a
    /// null admission projection can never rearm a blind retry that appends a
    /// second trigger to the first (`#idlerevisionreactive`).
    #[test]
    fn queue_dispatch_start_separates_unobservable_from_unproven() {
        assert_eq!(
            classify_queue_dispatch_start_observation(QueueDispatchStartFacts {
                payload_still_pending: true,
                ..proof_required_facts()
            }),
            QueueDispatchStartObservation::Unproven
        );
        // Controller projected nothing and the pane shows an idle, empty
        // composer: this is the legitimate no-op turn shape as much as it is a
        // strand, so it must not be called unproven.
        assert_eq!(
            classify_queue_dispatch_start_observation(proof_required_facts()),
            QueueDispatchStartObservation::Unobservable
        );
        // Pane could not be captured at all.
        assert_eq!(
            classify_queue_dispatch_start_observation(QueueDispatchStartFacts {
                pane_observed: false,
                payload_still_pending: true,
                ..proof_required_facts()
            }),
            QueueDispatchStartObservation::Unobservable
        );
    }

    /// The load-bearing difference between the two non-proven closeouts: a
    /// failed effect rearms a retry, an unobservable one must not.
    #[test]
    fn unobservable_dispatch_release_does_not_rearm_a_blind_retry() {
        let triggers = QueueContinuationTriggers::new();
        triggers.observe_head(Some("do [#head]".to_string()));
        assert!(triggers.ready());

        triggers.begin_dispatch_effect();
        assert!(!triggers.ready());
        triggers.observe_effect_unobservable();
        let after_unobservable = triggers.scope.ctx().get(&triggers.projection);
        assert_eq!(
            after_unobservable.effect_retry_epoch, 0,
            "an unobservable dispatch must not bump the effect-retry epoch"
        );
        assert!(
            triggers.ready(),
            "the un-consumed document edge keeps the next tick re-observing"
        );

        triggers.begin_dispatch_effect();
        triggers.observe_effect_failed();
        assert_eq!(
            triggers
                .scope
                .ctx()
                .get(&triggers.projection)
                .effect_retry_epoch,
            1,
            "a proven-failed dispatch still rearms"
        );
    }

    fn ready_resume_facts() -> CapturedFinalizeResumeFacts {
        CapturedFinalizeResumeFacts {
            captured_operation_present: true,
            actor_ready: true,
            current_transition_pending: false,
            ipc_inflight: 0,
            worker_in_flight: false,
            retry_cooldown_elapsed: true,
            controller_pressure_cooldown: false,
            urgent_supervisor_maintenance: false,
        }
    }

    #[test]
    fn captured_finalize_resume_requires_a_quiet_single_flight_boundary() {
        assert!(captured_finalize_resume_should_start(ready_resume_facts()));
        assert!(captured_finalize_resume_should_start(
            CapturedFinalizeResumeFacts {
                actor_ready: false,
                ..ready_resume_facts()
            }
        ));
        for blocked in [
            CapturedFinalizeResumeFacts {
                current_transition_pending: true,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                ipc_inflight: 1,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                worker_in_flight: true,
                ..ready_resume_facts()
            },
            CapturedFinalizeResumeFacts {
                controller_pressure_cooldown: true,
                ..ready_resume_facts()
            },
        ] {
            assert!(!captured_finalize_resume_should_start(blocked));
        }
    }

    #[test]
    fn captured_finalize_resume_backoff_is_bounded() {
        assert_eq!(
            captured_finalize_resume_retry_delay(1),
            Duration::from_secs(2)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(2),
            Duration::from_secs(4)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(5),
            Duration::from_secs(30)
        );
        assert_eq!(
            captured_finalize_resume_retry_delay(99),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn captured_finalize_resume_waits_for_a_new_state_edge_instead_of_retrying() {
        let triggers = CapturedFinalizeResumeTriggers::new();
        triggers.observe_operation(Some("cycle:capture:response".to_string()));
        assert!(triggers.ready());
        triggers.consume_attempt();
        assert!(!triggers.ready(), "a consumed state edge must stay quiet");

        triggers.observe_state_edge();
        assert!(
            triggers.ready(),
            "a new controller state edge rearms recovery"
        );
        triggers.consume_attempt();
        assert!(!triggers.ready());

        triggers.observe_effect_retry_due();
        assert!(
            triggers.ready(),
            "effect failure backoff is an explicit, separate trigger"
        );
    }

    /// `#needsoperatorstateedge` — the 2026-08-08 `haiven-dev.md` deadlock.
    ///
    /// An operator-required verdict must suppress the blind effect-retry timer
    /// and nothing else. A controller-published document transition is new
    /// evidence, so it retires the verdict and re-arms the attempt; otherwise the
    /// only thing that ever clears the flag is a different operation key, which a
    /// retained capture never gets, and the cycle stays open forever.
    #[test]
    fn a_state_edge_retires_an_operator_required_verdict() {
        let triggers = CapturedFinalizeResumeTriggers::new();
        triggers.observe_operation(Some("cycle:capture:response".to_string()));
        triggers.consume_attempt();

        triggers.require_operator();
        assert!(triggers.needs_operator());
        triggers.observe_effect_retry_due();
        assert!(
            !triggers.ready(),
            "a blind backoff must not retry an operator-required verdict"
        );

        triggers.observe_state_edge();
        assert!(
            !triggers.needs_operator(),
            "a document transition is new evidence and retires the verdict"
        );
        assert!(
            triggers.ready(),
            "the controller's settled-delivery edge must re-arm the resume"
        );
    }

    /// A hash identifies a failure; it never explains one. The diagnostic must
    /// carry readable, bounded, single-line evidence (`#needsoperatorstateedge`).
    #[test]
    fn the_reason_head_is_bounded_single_line_and_readable() {
        let head = captured_finalize_resume_reason_head(
            "closeout blocked\n  by: visible user-authored content diverged\tfrom the \"captured\" baseline",
        );

        assert!(head.contains("closeout blocked by: visible user-authored content diverged"));
        assert!(
            !head.contains('\n') && !head.contains('\t') && !head.contains('"'),
            "must stay one unquoted ops-log field: {head}"
        );

        let long = captured_finalize_resume_reason_head(&"é".repeat(4096));
        assert_eq!(
            long.chars().count(),
            CAPTURED_FINALIZE_REASON_HEAD_CHARS + 1,
            "truncation must land on a char boundary and mark the elision"
        );
    }

    /// The stand-down message must name the recovery, exactly like
    /// `agent_doc_turn::write_ownership`'s unowned remedy. Announcing a decision
    /// without an action is what let both sides of the 2026-08-08 deadlock wait
    /// on each other.
    #[test]
    fn the_operator_message_names_the_recovery_and_the_automatic_retry() {
        let message = captured_finalize_resume_operator_message(
            Path::new("tasks/haiven-dev.md"),
            "editor authority conflict",
        );

        assert!(message.contains("editor authority conflict"));
        assert!(message.contains("agent-doc write --commit tasks/haiven-dev.md"));
        assert!(message.contains("agent-doc commit tasks/haiven-dev.md"));
        assert!(
            message.contains("controller document-state edge retries this automatically"),
            "the operator must know a state edge re-arms it: {message}"
        );
        assert!(
            message.contains("Do NOT use `--force-disk`"),
            "the live-editor guard stays: {message}"
        );
    }

    #[test]
    fn queue_continuation_is_a_state_edge_consumed_only_by_dispatch_start() {
        let triggers = QueueContinuationTriggers::new();
        triggers.observe_head(Some("head:a".to_string()));
        assert!(triggers.ready());

        triggers.begin_dispatch_effect();
        assert!(!triggers.ready(), "one delivery effect owns the edge");

        triggers.observe_effect_failed();
        assert!(triggers.ready(), "effect failure is a separate retry edge");

        triggers.begin_dispatch_effect();
        triggers.observe_dispatch_started();
        assert!(!triggers.ready(), "dispatch-start proof consumes the edge");
        triggers.observe_head(Some("head:a".to_string()));
        assert!(!triggers.ready(), "polling identical state must stay quiet");
    }

    #[test]
    fn queue_continuation_rearms_for_a_new_or_reenqueued_head() {
        let triggers = QueueContinuationTriggers::new();
        triggers.observe_head(Some("head:a".to_string()));
        triggers.begin_dispatch_effect();
        triggers.observe_dispatch_started();

        triggers.observe_head(Some("head:b".to_string()));
        assert!(triggers.ready(), "a new head is a new state edge");
        triggers.begin_dispatch_effect();
        triggers.observe_dispatch_started();

        triggers.observe_head(None);
        assert!(!triggers.ready());
        triggers.observe_head(Some("head:b".to_string()));
        assert!(triggers.ready(), "clear then re-enqueue must rearm");
    }

    #[test]
    fn owner_pane_continuation_is_the_plain_trigger_not_prose() {
        // #qcontprose: the operator sees this text in their console. It must be
        // the same one-line trigger the route path dispatches, not a paragraph
        // restating what preflight already selects.
        for harness in [
            agent_doc_harness::HarnessConfig::claude(),
            agent_doc_harness::HarnessConfig::codex(),
            agent_doc_harness::HarnessConfig::opencode(),
        ] {
            let prompt = owned_pane_queue_continuation_prompt(
                Path::new("/repo/tasks/sampleorders.md"),
                &harness,
            );

            assert!(!prompt.contains('\n'), "a queue continuation is one line");
            assert!(
                prompt.ends_with(" /repo/tasks/sampleorders.md"),
                "the continuation dispatches the document: {prompt}"
            );
            for prose in [
                "queue state advanced",
                "do not invoke",
                "recursively",
                "Execute the current active queue head",
                "Persist and finalize",
            ] {
                assert!(
                    !prompt.contains(prose),
                    "queue continuation must not carry prose: {prose}"
                );
            }
        }
    }

    /// `#runpromptverbose`: the continuation is the HARNESS-NATIVE trigger.
    ///
    /// `specs/07-session-tmux-commands.md` specifies "Claude/OpenCode drains
    /// inject the normal slash-command harness reopen, and Codex drains inject the
    /// bare `agent-doc <FILE>` reopen". This function used to hardcode the bare
    /// form for every harness, silently violating that split — and the bare form
    /// is admitted by the hook, so nothing failed loudly.
    #[test]
    fn owner_pane_continuation_uses_the_harness_native_trigger() {
        let doc = Path::new("/repo/tasks/sampleorders.md");

        assert_eq!(
            owned_pane_queue_continuation_prompt(
                doc,
                &agent_doc_harness::HarnessConfig::claude()
            ),
            "/agent-doc /repo/tasks/sampleorders.md",
            "Claude Code takes the slash-command reopen so a cleared pane loads the skill"
        );
        assert_eq!(
            owned_pane_queue_continuation_prompt(
                doc,
                &agent_doc_harness::HarnessConfig::opencode()
            ),
            "/agent-doc /repo/tasks/sampleorders.md",
            "OpenCode takes the slash-command reopen too"
        );
        assert_eq!(
            owned_pane_queue_continuation_prompt(
                doc,
                &agent_doc_harness::HarnessConfig::codex()
            ),
            "agent-doc /repo/tasks/sampleorders.md",
            "Codex's harness-native entrypoint IS the bare trigger"
        );
    }

    #[test]
    fn owner_pane_continuation_absolutizes_the_trigger_path() {
        // #qcontabspath: this string is executed in the pane, whose cwd is not
        // guaranteed to be the supervisor's. A relative path silently produced
        // no cycle contract at all — the continuation just died.
        let prompt = owned_pane_queue_continuation_prompt(
            Path::new("tasks/sampleorders.md"),
            &agent_doc_harness::HarnessConfig::claude(),
        );
        let dispatched = prompt
            .strip_prefix("/agent-doc ")
            .expect("continuation is the harness trigger");

        assert!(
            Path::new(dispatched).is_absolute(),
            "a dispatched trigger path must not depend on the pane's cwd: {prompt}"
        );
        assert!(
            dispatched.ends_with("tasks/sampleorders.md"),
            "absolutizing must preserve the document: {prompt}"
        );
    }

    #[test]
    fn supervisor_auto_install_pane_message_started_warns_against_keypress() {
        let started = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Started);
        assert!(started.contains("rebuild"), "must mention the rebuild");
        assert!(
            started.contains("do NOT press Enter"),
            "must warn against the misleading keepalive keypress"
        );
        assert!(
            started.contains("auto-restart"),
            "must promise the supervisor restarts itself"
        );

        let ok = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Succeeded);
        let fail = supervisor_auto_install_pane_message(SupervisorAutoInstallPhase::Failed);
        assert!(ok.contains("complete") && ok.contains("recycling"));
        assert!(fail.contains("failed") && fail.contains("current binary"));
        assert_ne!(started, ok);
        assert_ne!(ok, fail);
    }

    #[test]
    fn paused_failsafe_drains_only_when_no_loop_owner_holds_a_drainable_head() {
        assert!(
            paused_idle_watch_should_skip(true, true, true),
            "loop owner present -> defer (skip)"
        );
        assert!(
            paused_idle_watch_should_skip(true, false, false),
            "no drainable head -> skip"
        );
        assert!(
            !paused_idle_watch_should_skip(true, true, false),
            "paused + drainable + no loop owner -> drain"
        );
        assert!(!paused_idle_watch_should_skip(false, true, false));
        assert!(!paused_idle_watch_should_skip(false, false, true));
    }

    #[test]
    fn idle_queue_submit_mode_uses_enter_for_codex_owner_pane() {
        assert_eq!(idle_queue_submit_mode(true, "codex"), "tmux_text_enter");
    }

    #[test]
    fn idle_queue_submit_mode_uses_pty_cr_without_owner_pane() {
        assert_eq!(idle_queue_submit_mode(false, "codex"), "pty_cr");
    }

    #[test]
    fn context_reset_ops_log_message_keeps_stable_fields() {
        let active_head = "agent:queue item";
        let message = idle_queue_context_reset_ops_log_message(
            Path::new("plan.md"),
            "codex",
            "/clear",
            "%1",
            active_head,
            "fresh context",
        );

        assert_eq!(
            message,
            format!(
                "idle_queue_watch_context_reset file=plan.md harness=codex cmd=\"/clear\" target=%1 head_bytes=16 head_sha256={} reason=\"fresh context\"",
                agent_doc_hash::content_hash(active_head)
            )
        );
    }
}
