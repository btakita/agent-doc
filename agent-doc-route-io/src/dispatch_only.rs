use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    DispatchOnlyProofOutcomeFacts, DispatchOnlyRecycleInflightMessageFacts,
    DispatchOnlyReopenDelivery, DispatchStartProofDecision, DispatchStartProofFacts,
    RoutedDispatchStartProof, RoutedReopenGuardReason, accepted_only_dispatch_start_log_message,
    accepted_only_dispatch_start_refusal_message,
    dispatch_only_dispatch_start_proof_required as controller_dispatch_only_dispatch_start_proof_required,
    dispatch_only_recycle_inflight_message, dispatch_only_sent_console_message,
    dispatch_only_sent_log_message, dispatch_proof_failed_event,
    routed_dispatch_start_timeout_for_binary,
};
use agent_doc_harness::HarnessConfig;

#[derive(Debug, Clone, Copy)]
pub struct DispatchOnlyBugReportFacts {
    pub elapsed: Duration,
    pub proof: RoutedDispatchStartProof,
}

pub fn wait_for_dispatch_only_recycle_inflight_settle(
    file: &Path,
    file_path: &str,
    pane: &str,
    harness_binary: &str,
) -> Result<()> {
    if !agent_doc_supervisor_io::recycle_inflight::recycle_inflight_pending(file_path) {
        return Ok(());
    }

    let started = std::time::Instant::now();
    let reason = agent_doc_supervisor_io::recycle_inflight::read_recycle_inflight(file_path)
        .map(|m| m.reason)
        .unwrap_or_else(|| "unknown".to_string());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_recycle_inflight_wait file={} pane={} harness={} reason={}",
            file.display(),
            pane,
            harness_binary,
            reason
        ),
    );
    while agent_doc_supervisor_io::recycle_inflight::recycle_inflight_pending(file_path) {
        if started.elapsed() >= agent_doc_supervisor::recycle_inflight::RECYCLE_SETTLE_WAIT {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_recycle_inflight_unsettled file={} pane={} harness={} reason={} waited_ms={}",
                    file.display(),
                    pane,
                    harness_binary,
                    reason,
                    started.elapsed().as_millis()
                ),
            );
            let file_display = file.display().to_string();
            let outcome_fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
                "wait_for_supervisor_recycle_settle",
            );
            anyhow::bail!(dispatch_only_recycle_inflight_message(
                DispatchOnlyRecycleInflightMessageFacts {
                    harness_binary,
                    pane,
                    file_display: &file_display,
                    reason: &reason,
                    outcome_fields: &outcome_fields,
                },
            ));
        }
        std::thread::sleep(agent_doc_supervisor::recycle_inflight::RECYCLE_SETTLE_POLL);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_recycle_inflight_settled file={} pane={} harness={} reason={} waited_ms={}",
            file.display(),
            pane,
            harness_binary,
            reason,
            started.elapsed().as_millis()
        ),
    );
    Ok(())
}

pub fn dispatch_only_dispatch_start_proof_required(file: &Path, harness: &HarnessConfig) -> bool {
    if harness.binary == "codex"
        && crate::dispatch_start::codex_dispatch_start_tracking_enabled(file)
    {
        return true;
    }
    controller_dispatch_only_dispatch_start_proof_required(&harness.binary)
}

pub fn require_dispatch_only_dispatch_start_proof(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
    mut report_bug: impl FnMut(DispatchOnlyBugReportFacts),
) -> Result<()> {
    let proof_required = dispatch_only_dispatch_start_proof_required(file, harness);
    let classification =
        agent_doc_controller::dispatch::classify_dispatch_start_proof(DispatchStartProofFacts {
            proof: dispatch_start,
            dispatch_start_proof_required: proof_required,
        });
    if classification.decision == DispatchStartProofDecision::Accepted {
        return Ok(());
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let file_display = file.display().to_string();
    let facts = DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: timeout.as_secs(),
    };
    agent_doc_flow_io::log_flow_event(
        file,
        dispatch_proof_failed_event(RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof),
        agent_doc_ops_log_io::log_op,
    );
    if let Err(err) = agent_doc_supervisor_io::route_submit_inflight::mark_route_submit_blocked(
        file,
        pane,
        &harness.binary,
        "accepted_without_dispatch_start_proof",
    ) {
        eprintln!(
            "[route] warning: failed to mark accepted-without-dispatch route block for {}: {err:#}",
            file.display()
        );
    }
    agent_doc_ops_log_io::log_op(file, &accepted_only_dispatch_start_log_message(facts));
    report_bug(DispatchOnlyBugReportFacts {
        elapsed: timeout,
        proof: dispatch_start,
    });
    anyhow::bail!(accepted_only_dispatch_start_refusal_message(facts));
}

pub fn dispatch_only_sent_log_message_for(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_log_message(DispatchOnlyProofOutcomeFacts {
        file_display: &file_display,
        pane,
        harness_binary: &harness.binary,
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
        .as_secs(),
    })
}

pub fn dispatch_only_sent_console_message_for(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_console_message(DispatchOnlyProofOutcomeFacts {
        file_display: &file_display,
        pane,
        harness_binary: &harness.binary,
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
        .as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_only_codex_requires_start_proof_when_hooks_are_visible() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(dispatch_only_dispatch_start_proof_required(
            &doc,
            &HarnessConfig::codex()
        ));
        let err = require_dispatch_only_dispatch_start_proof(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .expect_err("visible Codex hooks make accepted-only delivery insufficient");
        let message = err.to_string();
        assert!(
            message.contains("only pane-input acceptance proof was available"),
            "{message}"
        );
    }

    #[test]
    fn dispatch_only_codex_accepts_enter_delivery_without_visible_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(!dispatch_only_dispatch_start_proof_required(
            &doc,
            &HarnessConfig::codex()
        ));
        require_dispatch_only_dispatch_start_proof(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .expect(
            "Codex without hook tracking may accept text+Enter delivery for dispatch-only reroutes",
        );

        let message = dispatch_only_sent_log_message_for(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_submit_proof_gate_accepts_enter_delivery_without_codex_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        for harness in [
            HarnessConfig::codex(),
            HarnessConfig::opencode(),
            HarnessConfig::claude(),
        ] {
            require_dispatch_only_dispatch_start_proof(
                &doc,
                "%4",
                &harness,
                DispatchOnlyReopenDelivery::DirectPaneSubmit,
                RoutedDispatchStartProof::CommandAcceptedOnly,
                |_| {},
            )
            .expect("accepted-only delivery remains an explicit success path for this harness");
        }
    }

    #[test]
    fn dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed() {
        let err = require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::DispatchStartUnproven,
            |_| {},
        )
        .expect_err("tracked dispatch-start timeouts must not report route success");

        let message = format!("{err:#}");
        assert!(
            message.contains("only pane-input acceptance proof"),
            "{message}"
        );
        assert!(
            message.contains("no dispatch-start proof was recorded"),
            "{message}"
        );
    }

    #[test]
    fn dispatch_only_sent_log_marks_claude_accepted_only_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/robert-ross.md"),
            "%7",
            &HarnessConfig::claude(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );

        assert!(message.contains("harness=claude"), "{message}");
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_sent_log_marks_opencode_accepted_only_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );

        assert!(message.contains("harness=opencode"), "{message}");
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_sent_log_marks_opencode_pane_state_dispatch_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::PaneStateChanged,
        );

        assert!(message.contains("harness=opencode"), "{message}");
        assert!(message.contains("proof=pane_state_changed"), "{message}");
        assert!(message.contains("proof_scope=dispatch_start"), "{message}");
    }

    #[test]
    fn dispatch_only_opencode_accepted_only_proof_is_successful_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_opencode_pane_state_proof_is_successful_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::PaneStateChanged,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_claude_accepted_only_proof_remains_accepted_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/robert-ross.md"),
            "%7",
            &HarnessConfig::claude(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_sent_log_marks_codex_hook_proof_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%1",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::HookPromptMatched,
        );

        assert!(message.contains("harness=codex"), "{message}");
        assert!(message.contains("proof=consumed"), "{message}");
        assert!(message.contains("proof_scope=dispatch_start"), "{message}");
    }
}
