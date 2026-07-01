//! Pure supervisor idle-watch policy and messages.
//!
//! This module does not poll panes, run tmux, mutate files, or write ops logs.
//! Orchestration projects runtime facts into these helpers and performs the
//! effects itself.

use std::path::Path;

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
