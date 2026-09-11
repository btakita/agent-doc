//! Pure supervisor self-kill policy.
//!
//! The supervisor I/O crate owns sentinel files, `/proc` reads, and process
//! signalling. This module owns the side-effect-free decisions used by that
//! adapter.

use std::path::PathBuf;
use std::time::Duration;

/// `#supkill-a` — should the supervisor honor a graceful self-kill *now*?
///
/// A healthy live turn is never interrupted: the request is honored only at a
/// turn boundary. A wedged supervisor that never reaches a turn boundary is
/// handled by the external force-kill decision instead.
pub fn supervisor_self_kill_action(requested: bool, turn_boundary: bool) -> bool {
    requested && turn_boundary
}

/// `#supkill-b` — should the external driver escalate to a force-kill now?
///
/// The supervisor must still be alive, and the graceful request must be at least
/// `grace` old.
pub fn supervisor_force_kill_decision(
    requested_elapsed: Duration,
    alive: bool,
    grace: Duration,
) -> bool {
    alive && requested_elapsed >= grace
}

/// `#supkill` — parse a `start --route-owned <FILE>` supervisor cmdline.
///
/// Returns the owned document path, possibly relative to the process cwd.
/// Returns `None` for any other process. Skips the value of
/// `--route-owned-reap-policy` so it cannot be mistaken for the positional
/// document.
/// The document a live `agent-doc start --route-owned` supervisor serves.
///
/// `#supervisoridlewatchmissing`: this used to walk positionally from `start`,
/// skipping `-`-prefixed tokens and one hand-listed `--flag VALUE` pair
/// (`--route-owned-reap-policy`). Every later value-taking flag then made it
/// return that flag's VALUE as the document. `--route-owned-start-purpose
/// layout-provision` is how agent-doc launches its own layout-provision
/// supervisors, so the parser answered `layout-provision` for essentially every
/// live supervisor, `supervisor_doc_canonical` failed to canonicalize it, and
/// `supervisor_pid_for_doc` reported NO supervisor for documents that had one.
/// Downstream that reads as "no idle watch": the controller falls back to
/// `controller_orphan_drain_dispatch ... reason=no_supervisor_idle_watch`, and
/// the captured-finalize resume — whose only drivers are that idle watch and
/// the Codex `Stop` hook — has no driver at all. Observed 2026-09-11 with
/// twelve live supervisors running and `supervisor_pid_for_doc` finding none.
///
/// Keying on the `.md` token instead makes it flag-order- and flag-set-
/// independent, and matches
/// `agent_doc_controller::command_line::start_supervisor_document_from_args`,
/// which already parsed these same command lines correctly. That crate depends
/// on this one, so the shared predicate cannot live there; an agreement test in
/// `agent-doc-controller` keeps the two from drifting again.
pub fn start_route_owned_doc_from_args(args: &[String]) -> Option<PathBuf> {
    if !args.iter().any(|arg| arg.ends_with("agent-doc")) {
        return None;
    }
    if !args.iter().any(|arg| arg == "start") || !args.iter().any(|arg| arg == "--route-owned") {
        return None;
    }
    let start_idx = args.iter().position(|arg| arg == "start")?;
    args[start_idx + 1..]
        .iter()
        .find(|arg| {
            arg.trim_matches(|c| c == '"' || c == '\'')
                .ends_with(".md")
        })
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_kill_action_is_idle_gated() {
        assert!(supervisor_self_kill_action(true, true));
        assert!(!supervisor_self_kill_action(true, false));
        assert!(!supervisor_self_kill_action(false, true));
        assert!(!supervisor_self_kill_action(false, false));
    }

    #[test]
    fn force_kill_decision_waits_for_grace_then_escalates() {
        let grace = Duration::from_secs(10);

        assert!(!supervisor_force_kill_decision(
            Duration::from_secs(5),
            true,
            grace
        ));
        assert!(!supervisor_force_kill_decision(
            grace - Duration::from_millis(1),
            true,
            grace
        ));

        assert!(supervisor_force_kill_decision(grace, true, grace));
        assert!(supervisor_force_kill_decision(
            Duration::from_secs(50),
            true,
            grace
        ));

        assert!(!supervisor_force_kill_decision(
            Duration::from_secs(50),
            false,
            grace
        ));
    }

    #[test]
    fn parses_start_route_owned_doc_positional() {
        let args = vec![
            "/home/u/.cargo/bin/agent-doc".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
            "tasks/agent-doc/agent-doc-bugs2.md".to_string(),
        ];
        assert_eq!(
            start_route_owned_doc_from_args(&args),
            Some(PathBuf::from("tasks/agent-doc/agent-doc-bugs2.md"))
        );
    }

    #[test]
    fn parses_doc_before_route_owned_flag() {
        let args = vec![
            "agent-doc".to_string(),
            "start".to_string(),
            "plan.md".to_string(),
            "--route-owned".to_string(),
        ];
        assert_eq!(
            start_route_owned_doc_from_args(&args),
            Some(PathBuf::from("plan.md"))
        );
    }

    #[test]
    fn skips_reap_policy_flag_value() {
        let args = vec![
            "agent-doc".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
            "--route-owned-reap-policy".to_string(),
            "auto".to_string(),
            "doc.md".to_string(),
        ];

        assert_eq!(
            start_route_owned_doc_from_args(&args),
            Some(PathBuf::from("doc.md"))
        );
    }

    /// `#supervisoridlewatchmissing`: the exact command lines of the live
    /// supervisors observed 2026-09-11, all twelve of which the positional
    /// parser answered `layout-provision` for.
    #[test]
    fn a_value_taking_flag_after_start_is_never_mistaken_for_the_document() {
        let layout_provision = "/home/u/.cargo/bin/agent-doc start --route-owned \
             --route-owned-reap-policy keep-alive \
             --route-owned-start-purpose layout-provision tasks/agent-doc/agent-doc-bugs.md";
        assert_eq!(
            start_route_owned_doc_from_args(&split(layout_provision)),
            Some(PathBuf::from("tasks/agent-doc/agent-doc-bugs.md")),
        );

        let resumed = "/home/u/.cargo/bin/agent-doc start --route-owned \
             --route-owned-reap-policy keep-alive \
             --route-owned-start-purpose layout-provision --resume -- tasks/backend.md";
        assert_eq!(
            start_route_owned_doc_from_args(&split(resumed)),
            Some(PathBuf::from("tasks/backend.md")),
        );

        // A flag whose value the parser has never heard of must not matter:
        // that open-endedness is the whole point of keying on the document.
        let unknown_flag = "agent-doc start --route-owned --harness codex \
             --some-future-flag some-future-value tasks/plan.md";
        assert_eq!(
            start_route_owned_doc_from_args(&split(unknown_flag)),
            Some(PathBuf::from("tasks/plan.md")),
        );
    }

    fn split(command_line: &str) -> Vec<String> {
        command_line
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn rejects_non_supervisor_cmdlines() {
        let controller = vec![
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/p".to_string(),
        ];
        assert_eq!(start_route_owned_doc_from_args(&controller), None);

        let foreground = vec![
            "agent-doc".to_string(),
            "start".to_string(),
            "doc.md".to_string(),
        ];
        assert_eq!(start_route_owned_doc_from_args(&foreground), None);

        let other = vec![
            "vim".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
        ];
        assert_eq!(start_route_owned_doc_from_args(&other), None);
    }
}
