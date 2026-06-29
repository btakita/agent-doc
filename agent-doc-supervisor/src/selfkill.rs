//! Pure supervisor self-kill policy.
//!
//! The orchestration crate owns sentinel files, `/proc` reads, and process
//! signalling. This module owns the side-effect-free decisions used by those
//! adapters.

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
pub fn start_route_owned_doc_from_args(args: &[String]) -> Option<PathBuf> {
    if !args.iter().any(|arg| arg.ends_with("agent-doc")) {
        return None;
    }
    if !args.iter().any(|arg| arg == "start") || !args.iter().any(|arg| arg == "--route-owned") {
        return None;
    }
    let mut seen_start = false;
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if !seen_start {
            if arg == "start" {
                seen_start = true;
            }
            continue;
        }
        if arg == "--route-owned-reap-policy" {
            // Long form `--flag VALUE`; `--flag=VALUE` carries its own `=` and is
            // already filtered by the `--` prefix check below.
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return Some(PathBuf::from(arg));
    }
    None
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
