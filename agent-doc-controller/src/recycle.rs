//! Pure controller recycle policy.

use std::time::{Duration, Instant};

pub fn recycle_debounce_decision(
    wants_recycle_and_idle: bool,
    stale_since: Option<Instant>,
    now: Instant,
    grace: Duration,
) -> (bool, Option<Instant>) {
    match (wants_recycle_and_idle, stale_since) {
        (false, _) => (false, None),
        (true, None) => (false, Some(now)),
        (true, Some(since)) => (now.duration_since(since) >= grace, Some(since)),
    }
}

/// A project-controller image may begin a two-phase recycle while RPCs and a
/// durable harness dispatch remain open.
///
/// The harness child is owned by the route-owned supervisor, not by the project
/// controller. Dispatch/cycle state is durable in `state.db`, so treating the
/// whole harness turn as controller activity only keeps stale controller code
/// alive and delays retained-write recovery. The only launch exclusion is a
/// non-stable handoff. Promotion redirects new RPCs to the replacement, and the
/// predecessor exits only after its already-accepted RPCs drain.
pub fn controller_recycle_safe_to_handoff(handoff_stable: bool) -> bool {
    handoff_stable
}

/// Reason an install fan-out attaches to its `recycle` request.
pub const INSTALL_FANOUT_RECYCLE_REASON: &str = "install_fanout";

/// An install fan-out `recycle` is redundant when the controller is provably
/// already executing the installed binary — typically because it self-detected
/// the stale binary and restarted onto the new build between `binary-install`
/// and the fan-out. Launching a second handoff then buys nothing and holds every
/// RPC in `Preparing` for the whole successor wait (GH: fpe Stop hook overran its
/// 45s budget behind exactly that handoff). Unknown identities are not proof, so
/// they still recycle.
pub fn install_fanout_recycle_is_redundant(
    reason: Option<&str>,
    recorded: Option<&crate::status::ControllerBinaryIdentity>,
    current: Option<&crate::status::ControllerBinaryIdentity>,
) -> bool {
    reason == Some(INSTALL_FANOUT_RECYCLE_REASON)
        && crate::status::controller_binary_identity_matches(recorded, current)
}

/// Explicit force and protocol-skew recovery skip the normal recycle debounce.
/// Neither case may interrupt an RPC; promotion and predecessor drain own that
/// proof.
pub fn controller_recycle_is_urgent(recycle_forced: bool, protocol_skew_urgent: bool) -> bool {
    recycle_forced || protocol_skew_urgent
}

/// `#recycleidleonly`: a ROUTINE stale-binary recycle must wait for a real turn
/// boundary.
///
/// `execve` is supposed to preserve the live child and its tmux pane, but its
/// documented fallback is a clean exit + child restart, and that fallback tears
/// the pane down: observed live, pane `%3` vanished mid-turn and came back with
/// `history_size=5` against a 50000-line limit, so the operator lost the entire
/// visible session (`boundary=safe_intra_turn via=execve_preserve_child`).
/// `safe_intra_turn` is a truthful claim about DOCUMENT safety, not pane safety.
///
/// A pending queue head is NOT a licence to recycle mid-turn: `head_pending`
/// only bypasses the idle-grace *debounce* (an inter-queue-item recycle should
/// not wait out the grace window), and a genuine inter-queue-item boundary is
/// already a `turn_boundary`. Gating solely on `turn_boundary` is what closes
/// the gap with the `#wd40` / `#staleloop-recycle-restart` yield protocol.
///
/// Non-routine recycles (wedged supervisor, explicit admin, stale editor
/// delivery — i.e. `RecycleImmediate`) are never deferred here: the alternative
/// to recycling them mid-turn is staying wedged forever.
pub fn routine_stale_recycle_deferred_intra_turn(
    routine_stale_recycle: bool,
    turn_boundary: bool,
) -> bool {
    routine_stale_recycle && !turn_boundary
}

/// Why an `execve_preserve_child` hot-reload must not run right now.
///
/// `#reexecdeadchild`: the in-place reexec hands the CURRENT harness child to
/// the replacement image, which adopts it and resumes reaping it. That handoff
/// is only sound while the child is alive and nothing has already decided to
/// replace it. Observed live on `tasks/sdk.md` (2026-10-03T02:27:41Z): an
/// operator "Clear Session Context" (`session_clear delivery=supervisor_restart_fresh`)
/// SIGTERMed the child, the host loop reaped it (`exit_code=143`), and in the
/// window before the host loop stopped the idle watch an install-fanout recycle
/// fired `supervisor_binary_stale_self_recycled ... child_pid=899775` with the
/// already-reaped PID. The new image adopted a PID it could never wait on
/// (`try_wait failed: No child processes`), synthesized exit 1, lost the fresh
/// restart the clear asked for, and the operator saw the harness "crash" twice
/// (`claude exited with code 1. Restarting in 2s...`) before a fresh relaunch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReexecPreserveChildRefusal {
    /// No child PID is published (between generations, or never spawned).
    NoLiveChild,
    /// A stop or a child-replacing restart is pending: the current host loop
    /// owns that kill + relaunch, and an exec would silently drop the request.
    ChildReplacementPending,
    /// The published child already exited (zombie) or was already reaped.
    ChildExited,
}

impl ReexecPreserveChildRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoLiveChild => "no_live_child",
            Self::ChildReplacementPending => "child_replacement_pending",
            Self::ChildExited => "child_exited",
        }
    }
}

impl std::fmt::Display for ReexecPreserveChildRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Observed facts the `execve_preserve_child` handoff depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReexecPreserveChildFacts {
    pub child_pid_published: bool,
    pub child_exited: bool,
    pub stop_requested: bool,
    pub restart_requested: bool,
    /// The pending restart is itself served by the in-place reexec
    /// (`restart_reexec`), so it is not a child replacement.
    pub restart_served_by_reexec: bool,
}

/// `#reexecdeadchild`: decide whether an in-place reexec may preserve the
/// current child. `None` means the handoff is sound.
pub fn reexec_preserve_child_refusal(
    facts: ReexecPreserveChildFacts,
) -> Option<ReexecPreserveChildRefusal> {
    if !facts.child_pid_published {
        return Some(ReexecPreserveChildRefusal::NoLiveChild);
    }
    if facts.child_exited {
        return Some(ReexecPreserveChildRefusal::ChildExited);
    }
    if facts.stop_requested || (facts.restart_requested && !facts.restart_served_by_reexec) {
        return Some(ReexecPreserveChildRefusal::ChildReplacementPending);
    }
    None
}

/// `#installworktreecontrollers`: observed evidence that somebody is actually
/// using a project root right now.
///
/// An install fan-out (`make install` / `install-full` -> `lib-install`) walks
/// `/proc` for every `controller serve` process. Subagent worktrees and dev roots
/// that nobody has open used to receive a recycle (a two-phase handoff that
/// SPAWNS a replacement) or a `reliable_sync_status` RPC through
/// `connect_or_launch` (which SPAWNS a controller when the socket does not
/// answer). Both kept idle controllers alive forever — one worktree reached
/// controller generation 44 in a day with zero documents and no editor.
///
/// Only evidence that is cheap to observe without the controller counts: a
/// listening PID-scoped editor socket, a live reliable-sync editor registration,
/// or an open `agent-doc start` supervisor serving a document in the root.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectRootUseEvidence {
    /// Live editor endpoints: listening PID-scoped editor sockets, or live
    /// reliable-sync registrations / allocated editor models when observed from
    /// inside the controller.
    pub live_editor_endpoints: usize,
    /// Open `agent-doc start` supervisors whose document resolves to this root.
    pub open_supervisors: usize,
}

impl ProjectRootUseEvidence {
    pub fn in_use(self) -> bool {
        self.live_editor_endpoints > 0 || self.open_supervisors > 0
    }

    pub fn as_log_fields(self) -> String {
        format!(
            "live_editor_endpoints={} open_supervisors={}",
            self.live_editor_endpoints, self.open_supervisors
        )
    }
}

/// What the install fan-out may do to a project root that has a running
/// controller process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallFanoutRootAction {
    /// Somebody uses the root: recycle its controller onto the new binary.
    Recycle,
    /// Nobody has the root open: never launch a replacement for it. A later
    /// client reaches the new binary through the ordinary lazy launch.
    SkipIdle,
}

pub fn install_fanout_root_action(evidence: ProjectRootUseEvidence) -> InstallFanoutRootAction {
    if evidence.in_use() {
        InstallFanoutRootAction::Recycle
    } else {
        InstallFanoutRootAction::SkipIdle
    }
}

/// How an install-time `reload_library` fan-out may reach a project's
/// controller for its reliable-sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallFanoutControllerAccess {
    /// The root is in use (or the caller seeded it explicitly): a missing
    /// controller may be launched, as before.
    MayLaunch,
    /// The root is idle: query an already-running controller only; a missing
    /// one is skipped, never started.
    ExistingOnly,
}

pub fn install_fanout_controller_access(
    evidence: ProjectRootUseEvidence,
    seeded_by_caller: bool,
) -> InstallFanoutControllerAccess {
    if seeded_by_caller || evidence.in_use() {
        InstallFanoutControllerAccess::MayLaunch
    } else {
        InstallFanoutControllerAccess::ExistingOnly
    }
}

/// `#installworktreecontrollers`: a controller that notices its binary was
/// replaced normally launches a replacement (R1 self-handoff). For a root nobody
/// uses, that replacement is exactly the perpetual idle controller this fix
/// removes, so the controller retires instead and the next client launches the
/// installed binary lazily.
///
/// Every condition must hold: the recycle is the ROUTINE stale-binary one (an
/// operator, forced, or protocol-skew recycle always hands off); no live editor
/// or supervisor uses the root; the controller owns no route-owned documents
/// (`None` = unknown, which never retires); no RPC is in flight; and no client
/// has connected within `quiet_for_at_least`.
pub fn idle_stale_binary_controller_should_retire(
    routine_stale_binary: bool,
    evidence: ProjectRootUseEvidence,
    owned_documents: Option<usize>,
    active_clients: usize,
    quiet_for: Duration,
    quiet_for_at_least: Duration,
) -> bool {
    routine_stale_binary
        && !evidence.in_use()
        && owned_documents == Some(0)
        && active_clients == 0
        && quiet_for >= quiet_for_at_least
}

// ---------------------------------------------------------------------------
// GH #128 — a stale-binary recycle must report the image it actually re-exec'd
// into, and a stale-binary restart must not be re-requested for a pid and
// generation that already has a handoff attempt in flight or just failed.
// ---------------------------------------------------------------------------

/// How long a stale-binary restart attempt for one controller pid + generation
/// suppresses another request for the same pid + generation. The controller's
/// own self-recycle debounce is a few seconds and a handoff waits for the
/// replacement to hydrate; this window covers both so the four-requests-in-nine-
/// seconds pattern from GH #128 collapses into one attempt.
pub const STALE_RESTART_ATTEMPT_BACKOFF: Duration = Duration::from_secs(30);

/// Durable record of the most recent stale-binary restart attempt for a project
/// controller, written by whoever starts the handoff (a client's
/// `connect_or_launch` or the controller's own self-recycle).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StaleRestartAttempt {
    pub pid: Option<u32>,
    pub generation: u64,
    pub at_secs: u64,
    pub requester: String,
}

/// `Some(age_secs)` when `attempt` already targets this exact pid + generation
/// and is younger than `backoff`; the caller must then not start another
/// handoff. A different pid or generation, or an expired attempt, is `None`.
pub fn stale_restart_recently_attempted(
    attempt: Option<&StaleRestartAttempt>,
    pid: Option<u32>,
    generation: u64,
    now_secs: u64,
    backoff: Duration,
) -> Option<u64> {
    let attempt = attempt?;
    if attempt.pid != pid || attempt.generation != generation {
        return None;
    }
    let age = now_secs.saturating_sub(attempt.at_secs);
    (age < backoff.as_secs()).then_some(age)
}

/// Parse `agent-doc --version` output (`agent-doc 0.35.449`) into the version.
pub fn parse_agent_doc_version_output(stdout: &str) -> Option<String> {
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    let version = parts.next()?;
    (name.starts_with("agent-doc") && version.chars().next()?.is_ascii_digit())
        .then(|| version.to_string())
}

/// What a stale-binary self-recycle should do once it has read the version of
/// the binary it is about to launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfRecycleTargetDecision {
    /// The target is provably newer (or its version could not be read, which
    /// is fail-open): hand off.
    Launch,
    /// The resolved target is not newer than the running image — handing off
    /// would relaunch the same (or an older) build and every newer client
    /// would keep asking for a restart. Defer and say so.
    DeferTargetNotNewer,
}

/// `target` carries the version read FROM the target binary, not the running
/// process's compiled-in version. Only a routine `stale_binary` recycle is
/// gated; an operator request always launches.
pub fn self_recycle_target_decision(
    reason: &str,
    recorded: Option<&crate::status::ControllerBinaryIdentity>,
    target: Option<&crate::status::ControllerBinaryIdentity>,
) -> SelfRecycleTargetDecision {
    if reason != "stale_binary" {
        return SelfRecycleTargetDecision::Launch;
    }
    match (recorded, target) {
        (Some(_), Some(_))
            if !crate::status::controller_binary_identity_is_newer(target, recorded) =>
        {
            SelfRecycleTargetDecision::DeferTargetNotNewer
        }
        _ => SelfRecycleTargetDecision::Launch,
    }
}

/// Classify a completed handoff by the identity the REPLACEMENT reported for
/// itself (its `handoff_status`), which is the image actually exec'd.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfRecycleOutcome {
    /// The replacement runs a different image than the predecessor.
    Escaped,
    /// The replacement reported the predecessor's exact identity: the recycle
    /// did not escape the stale binary and must be logged as a failure.
    SameImage,
    /// The replacement did not report an identity.
    Unknown,
}

pub fn self_recycle_outcome(
    recorded: Option<&crate::status::ControllerBinaryIdentity>,
    replacement: Option<&crate::status::ControllerBinaryIdentity>,
) -> SelfRecycleOutcome {
    match (recorded, replacement) {
        (_, None) => SelfRecycleOutcome::Unknown,
        (Some(recorded), Some(replacement)) if recorded == replacement => {
            SelfRecycleOutcome::SameImage
        }
        _ => SelfRecycleOutcome::Escaped,
    }
}

#[cfg(test)]
mod gh128_tests {
    use super::*;
    use crate::status::ControllerBinaryIdentity;
    use std::path::PathBuf;

    fn identity(version: &str, mtime: u64) -> ControllerBinaryIdentity {
        ControllerBinaryIdentity {
            path: PathBuf::from("/home/u/.local/bin/agent-doc"),
            version: version.to_string(),
            len: 70,
            modified_secs: mtime,
            modified_nanos: 0,
        }
    }

    fn attempt(pid: u32, generation: u64, at_secs: u64) -> StaleRestartAttempt {
        StaleRestartAttempt {
            pid: Some(pid),
            generation,
            at_secs,
            requester: "client".to_string(),
        }
    }

    #[test]
    fn gh128_same_pid_and_generation_inside_backoff_is_suppressed() {
        let recent = attempt(1223695, 35, 1_000);
        assert_eq!(
            stale_restart_recently_attempted(
                Some(&recent),
                Some(1223695),
                35,
                1_002,
                STALE_RESTART_ATTEMPT_BACKOFF
            ),
            Some(2)
        );
    }

    #[test]
    fn gh128_expired_or_different_target_attempt_is_not_suppressed() {
        let recent = attempt(1223695, 35, 1_000);
        let backoff = STALE_RESTART_ATTEMPT_BACKOFF;
        assert_eq!(
            stale_restart_recently_attempted(None, Some(1223695), 35, 1_002, backoff),
            None
        );
        assert_eq!(
            stale_restart_recently_attempted(Some(&recent), Some(1223695), 35, 1_030, backoff),
            None
        );
        assert_eq!(
            stale_restart_recently_attempted(Some(&recent), Some(1223695), 36, 1_002, backoff),
            None
        );
        assert_eq!(
            stale_restart_recently_attempted(Some(&recent), Some(7), 35, 1_002, backoff),
            None
        );
    }

    #[test]
    fn gh128_parses_agent_doc_version_output() {
        assert_eq!(
            parse_agent_doc_version_output("agent-doc 0.35.449\n").as_deref(),
            Some("0.35.449")
        );
        assert_eq!(parse_agent_doc_version_output(""), None);
        assert_eq!(parse_agent_doc_version_output("bash 5.2"), None);
        assert_eq!(parse_agent_doc_version_output("agent-doc"), None);
    }

    #[test]
    fn gh128_stale_binary_recycle_defers_when_target_is_not_newer() {
        let running = identity("0.35.448", 100);
        assert_eq!(
            self_recycle_target_decision(
                "stale_binary",
                Some(&running),
                Some(&identity("0.35.448", 100))
            ),
            SelfRecycleTargetDecision::DeferTargetNotNewer
        );
        assert_eq!(
            self_recycle_target_decision(
                "stale_binary",
                Some(&running),
                Some(&identity("0.35.447", 200))
            ),
            SelfRecycleTargetDecision::DeferTargetNotNewer
        );
        assert_eq!(
            self_recycle_target_decision(
                "stale_binary",
                Some(&running),
                Some(&identity("0.35.449", 200))
            ),
            SelfRecycleTargetDecision::Launch
        );
        // Same-version rebuild with a later mtime is a real new image.
        assert_eq!(
            self_recycle_target_decision(
                "stale_binary",
                Some(&running),
                Some(&identity("0.35.448", 101))
            ),
            SelfRecycleTargetDecision::Launch
        );
        // Unreadable target version is fail-open; operator requests always launch.
        assert_eq!(
            self_recycle_target_decision("stale_binary", Some(&running), None),
            SelfRecycleTargetDecision::Launch
        );
        assert_eq!(
            self_recycle_target_decision(
                "operator_request",
                Some(&running),
                Some(&identity("0.35.448", 100))
            ),
            SelfRecycleTargetDecision::Launch
        );
    }

    #[test]
    fn gh128_replacement_reporting_predecessor_identity_is_a_same_image_failure() {
        let running = identity("0.35.448", 100);
        assert_eq!(
            self_recycle_outcome(Some(&running), Some(&running.clone())),
            SelfRecycleOutcome::SameImage
        );
        assert_eq!(
            self_recycle_outcome(Some(&running), Some(&identity("0.35.449", 200))),
            SelfRecycleOutcome::Escaped
        );
        assert_eq!(
            self_recycle_outcome(Some(&running), None),
            SelfRecycleOutcome::Unknown
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> ProjectRootUseEvidence {
        ProjectRootUseEvidence::default()
    }

    fn with_editor() -> ProjectRootUseEvidence {
        ProjectRootUseEvidence {
            live_editor_endpoints: 1,
            ..Default::default()
        }
    }

    fn with_supervisor() -> ProjectRootUseEvidence {
        ProjectRootUseEvidence {
            open_supervisors: 1,
            ..Default::default()
        }
    }

    /// `#installworktreecontrollers`: the install fan-out recycles a controller
    /// only where an editor or supervisor proves the root is in use.
    #[test]
    fn install_fanout_recycles_only_in_use_roots() {
        assert_eq!(
            install_fanout_root_action(idle()),
            InstallFanoutRootAction::SkipIdle
        );
        assert_eq!(
            install_fanout_root_action(with_editor()),
            InstallFanoutRootAction::Recycle
        );
        assert_eq!(
            install_fanout_root_action(with_supervisor()),
            InstallFanoutRootAction::Recycle
        );
    }

    #[test]
    fn install_fanout_reload_launches_only_for_in_use_or_seeded_roots() {
        assert_eq!(
            install_fanout_controller_access(idle(), false),
            InstallFanoutControllerAccess::ExistingOnly
        );
        assert_eq!(
            install_fanout_controller_access(idle(), true),
            InstallFanoutControllerAccess::MayLaunch
        );
        assert_eq!(
            install_fanout_controller_access(with_editor(), false),
            InstallFanoutControllerAccess::MayLaunch
        );
        assert_eq!(
            install_fanout_controller_access(with_supervisor(), false),
            InstallFanoutControllerAccess::MayLaunch
        );
    }

    #[test]
    fn idle_stale_binary_controller_retires_only_when_every_idle_proof_holds() {
        let quiet = Duration::from_secs(60);
        let retire = |routine, evidence, docs, clients, quiet_for| {
            idle_stale_binary_controller_should_retire(
                routine, evidence, docs, clients, quiet_for, quiet,
            )
        };
        assert!(retire(true, idle(), Some(0), 0, quiet));
        // Operator / forced / skew recycles always hand off.
        assert!(!retire(false, idle(), Some(0), 0, quiet));
        // Somebody uses the root.
        assert!(!retire(true, with_editor(), Some(0), 0, quiet));
        assert!(!retire(true, with_supervisor(), Some(0), 0, quiet));
        // The controller owns documents, or ownership is unknown.
        assert!(!retire(true, idle(), Some(1), 0, quiet));
        assert!(!retire(true, idle(), None, 0, quiet));
        // An RPC is in flight, or a client connected recently.
        assert!(!retire(true, idle(), Some(0), 1, quiet));
        assert!(!retire(
            true,
            idle(),
            Some(0),
            0,
            quiet - Duration::from_secs(1)
        ));
    }

    fn live_child() -> ReexecPreserveChildFacts {
        ReexecPreserveChildFacts {
            child_pid_published: true,
            ..Default::default()
        }
    }

    /// `#reexecdeadchild` regression: the sdk.md clear crash. A clear's fresh
    /// restart SIGTERMed and reaped the child; the recycle must refuse to hand
    /// the dead PID across `execve`.
    #[test]
    fn reexec_refuses_to_preserve_an_exited_or_replaced_child() {
        assert_eq!(reexec_preserve_child_refusal(live_child()), None);
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts::default()),
            Some(ReexecPreserveChildRefusal::NoLiveChild)
        );
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                child_exited: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildExited)
        );
        // The operator clear: `restart mode=fresh` is pending, child not yet dead.
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                restart_requested: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildReplacementPending)
        );
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                stop_requested: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildReplacementPending)
        );
        // A restart routed to the in-place reexec keeps the child by design.
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                restart_requested: true,
                restart_served_by_reexec: true,
                ..live_child()
            }),
            None
        );
    }

    fn identity(modified_secs: u64) -> crate::status::ControllerBinaryIdentity {
        crate::status::ControllerBinaryIdentity {
            path: "/bin/agent-doc".into(),
            version: "0.35.428".into(),
            len: 10,
            modified_secs,
            modified_nanos: 0,
        }
    }

    /// A controller that already restarted onto the installed build declines the
    /// install fan-out's recycle instead of launching a redundant handoff.
    #[test]
    fn install_fanout_recycle_is_redundant_only_for_a_proven_current_binary() {
        let current = identity(2);
        let reason = Some(INSTALL_FANOUT_RECYCLE_REASON);
        assert!(install_fanout_recycle_is_redundant(
            reason,
            Some(&current),
            Some(&current)
        ));
        // Same version, different build: still a real recycle.
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            Some(&identity(1)),
            Some(&current)
        ));
        // Unknown identity is not proof.
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            None,
            Some(&current)
        ));
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            Some(&current),
            None
        ));
        // An explicit operator recycle always recycles.
        assert!(!install_fanout_recycle_is_redundant(
            None,
            Some(&current),
            Some(&current)
        ));
        assert!(!install_fanout_recycle_is_redundant(
            Some("operator_request"),
            Some(&current),
            Some(&current)
        ));
    }

    #[test]
    fn debounce_requires_continuous_idle_grace() {
        let grace = Duration::from_secs(5);
        let t0 = Instant::now();

        assert_eq!(
            recycle_debounce_decision(false, Some(t0), t0, grace),
            (false, None)
        );

        let (do_recycle, since) = recycle_debounce_decision(true, None, t0, grace);
        assert!(!do_recycle);
        assert_eq!(since, Some(t0));

        let t_mid = t0 + Duration::from_secs(2);
        assert_eq!(
            recycle_debounce_decision(true, since, t_mid, grace),
            (false, Some(t0))
        );

        let t_late = t0 + Duration::from_secs(6);
        assert_eq!(
            recycle_debounce_decision(true, since, t_late, grace),
            (true, Some(t0))
        );
        assert_eq!(
            recycle_debounce_decision(false, since, t_late, grace),
            (false, None)
        );
    }

    #[test]
    fn routine_stale_recycle_waits_for_a_turn_boundary() {
        // At a turn boundary the routine recycle proceeds.
        assert!(!routine_stale_recycle_deferred_intra_turn(true, true));
        // Mid-turn it defers — this is the pane-destroying `boundary=safe_intra_turn`
        // case the operator hit (#eqmv / #recycleidleonly).
        assert!(routine_stale_recycle_deferred_intra_turn(true, false));
        // Non-routine (RecycleImmediate: wedge / admin / stale editor delivery)
        // is never deferred, boundary or not.
        assert!(!routine_stale_recycle_deferred_intra_turn(false, false));
        assert!(!routine_stale_recycle_deferred_intra_turn(false, true));
    }

    #[test]
    fn controller_recycle_is_safe_midturn_only_between_rpcs_and_outside_handoff() {
        // Active RPCs drain on the predecessor after promotion. A durable harness
        // dispatch may likewise remain open: it is supervisor-owned and survives.
        assert!(controller_recycle_safe_to_handoff(true));
        assert!(!controller_recycle_safe_to_handoff(false));
    }

    #[test]
    fn forced_or_protocol_skew_recycle_skips_the_debounce() {
        assert!(controller_recycle_is_urgent(true, false));
        assert!(controller_recycle_is_urgent(false, true));
        assert!(controller_recycle_is_urgent(true, true));
        assert!(!controller_recycle_is_urgent(false, false));
    }
}
