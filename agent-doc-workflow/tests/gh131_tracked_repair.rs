//! GH #131 (`#trackedrepairterminates`): tracked-work guards never name a
//! repair the write path refuses in the current state.
//!
//! Lives outside `session_check.rs` so the FlowCore hot-path token budget
//! counts only production code.

use agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode;
use agent_doc_turn::write_ownership::RetainedWriteOwnership;
use agent_doc_workflow::session_check::{
    GuardResult, blocked_closeout_followup_guard_result, expect_done_or_gate_guard_result,
    pending_done_guard_result,
};

fn rendered(result: GuardResult) -> String {
    match result {
        GuardResult::None => String::new(),
        GuardResult::Warn(lines) => lines.join("\n"),
        GuardResult::Error(message) => message,
    }
}

/// GH #131 shape 1: the guard named `write --done <id> --pending-only
/// --commit`, the write refused, and its remedy sent the agent back to the
/// guard. When the write path would refuse, the guard must put the recovery
/// that unblocks it FIRST, so following the text terminates.
#[test]
fn tracked_work_guards_never_name_a_repair_the_write_path_refuses() {
    let ids = vec!["turnleasesweep".to_string()];
    let unserved = RetainedWriteOwnership::UNOWNED.with_replica_unserved(true);
    for mode in [
        PendingCaptureGuardMode::Warn,
        PendingCaptureGuardMode::Strict,
    ] {
        for result in [
            pending_done_guard_result("plan.md", &ids, mode, unserved),
            expect_done_or_gate_guard_result("plan.md", &ids, mode, unserved),
            blocked_closeout_followup_guard_result("plan.md", &ids, mode, unserved),
        ] {
            let text = rendered(result);
            assert!(
                text.contains("agent-doc admin reload-lib"),
                "names the replica recovery: {text}"
            );
            assert!(
                text.contains("--pending-only --commit"),
                "still names the repair: {text}"
            );
            assert!(
                text.contains("AFTER recovering the editor"),
                "the repair is sequenced after the recovery: {text}"
            );
        }
    }

    // Nothing blocks the write: the hint is the plain command, unchanged.
    let text = rendered(pending_done_guard_result(
        "plan.md",
        &ids,
        PendingCaptureGuardMode::Strict,
        RetainedWriteOwnership::UNOWNED,
    ));
    assert!(
        text.contains(
            "repair with `agent-doc write plan.md --done turnleasesweep --pending-only --commit`"
        ),
        "{text}"
    );
    assert!(!text.contains("reload-lib"), "{text}");
}
