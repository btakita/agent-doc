//! Pure workflow doctor policy.
//!
//! Orchestration gathers logs and other evidence. This module owns durable
//! classification rules that turn raw workflow evidence into stable doctor
//! marker names.

pub fn classify_ops_marker(line: &str) -> Option<&'static str> {
    if line.contains("failed_stage=queue_paused")
        && (line.contains("reason=#qchurn")
            || line.contains("stale host supervisor")
            || line.contains("supervisor_binary_stale"))
    {
        return Some("stale_queue_pause");
    }
    if line.contains("supervisor_binary_stale") || line.contains("stale host supervisor") {
        return Some("stale_supervisor");
    }
    if line.contains("retry_on_current_generation") || line.contains("supervisor_restart_redirect")
    {
        return Some("retry_on_current_generation");
    }
    if line.contains("stale_generation") || line.contains("stale generation") {
        return Some("stale_generation_block");
    }
    if line.contains("File Cache Conflict")
        || line.contains("ipc_proof_insufficient")
        || line.contains("editor_convergence_ack_mismatch")
        || line.contains("editor_convergence_no_ack")
        || line.contains("live_prompt_drift_after_preflight")
    {
        return Some("editor_convergence_failure");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_marker_classifies_queue_pause_causes() {
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused reason=#qchurn"),
            Some("stale_queue_pause")
        );
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused stale host supervisor"),
            Some("stale_queue_pause")
        );
        assert_eq!(
            classify_ops_marker("failed_stage=queue_paused supervisor_binary_stale"),
            Some("stale_queue_pause")
        );
    }

    #[test]
    fn ops_marker_classifies_stale_supervisor_without_queue_pause() {
        assert_eq!(
            classify_ops_marker("supervisor_binary_stale path=/tmp/agent-doc"),
            Some("stale_supervisor")
        );
        assert_eq!(
            classify_ops_marker("stale host supervisor generation=7"),
            Some("stale_supervisor")
        );
    }

    #[test]
    fn ops_marker_classifies_generation_redirect_and_blocks() {
        assert_eq!(
            classify_ops_marker("retry_on_current_generation supervisor restarted"),
            Some("retry_on_current_generation")
        );
        assert_eq!(
            classify_ops_marker("supervisor_restart_redirect generation=9"),
            Some("retry_on_current_generation")
        );
        assert_eq!(
            classify_ops_marker("blocked stale_generation current=4"),
            Some("stale_generation_block")
        );
        assert_eq!(
            classify_ops_marker("blocked stale generation current=4"),
            Some("stale_generation_block")
        );
    }

    #[test]
    fn ops_marker_classifies_editor_convergence_failures() {
        for line in [
            "File Cache Conflict",
            "ipc_proof_insufficient",
            "editor_convergence_ack_mismatch",
            "editor_convergence_no_ack",
            "live_prompt_drift_after_preflight",
        ] {
            assert_eq!(
                classify_ops_marker(line),
                Some("editor_convergence_failure"),
                "{line}"
            );
        }
    }

    #[test]
    fn ops_marker_ignores_unrelated_lines() {
        assert_eq!(classify_ops_marker("plain ops log line"), None);
    }
}
