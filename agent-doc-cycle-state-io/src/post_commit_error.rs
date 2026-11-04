//! `#committedcloseouttimeout`: classify a `respond`/`finalize` error raised
//! after its closeout commit already landed.
//!
//! 2026-11-03, agent-doc-bugs.md, cycle-1793748547361: the closeout committed
//! (`flow=closeout stage=commit outcome=completed`), then a later
//! controller observation hit its 5s RPC deadline and the command exited
//! nonzero with `timed out after 5.0s waiting for project controller
//! response`. A terminal failure invites a re-answer of a response that is
//! already durable. A deadline on a post-commit observation is not a failed
//! closeout once this command's cycle moved to a committed terminal.

use crate::{CyclePhase, CycleState};

/// The cycle identity observed before the command ran.
pub fn observed(state: Option<&CycleState>) -> Option<(String, bool)> {
    state.map(|state| (state.cycle_id.clone(), state.is_open()))
}

/// True when `error` is only a controller deadline and the cycle reached a
/// committed terminal with a response DURING this command: it was open (or a
/// different cycle) before and is committed with a response after.
pub fn landed_commit_survives_post_commit_error(
    before: Option<&(String, bool)>,
    after: Option<&CycleState>,
    error: &anyhow::Error,
) -> bool {
    let Some(after) = after else {
        return false;
    };
    if after.phase != CyclePhase::Committed || after.response_sha256.is_none() {
        return false;
    }
    let committed_by_this_command = match before {
        None => true,
        Some((cycle_id, was_open)) => *cycle_id != after.cycle_id || *was_open,
    };
    committed_by_this_command && is_controller_deadline(error)
}

/// A controller RPC deadline, as raised by the project-controller client.
pub fn is_controller_deadline(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
            || cause
                .to_string()
                .contains("waiting for project controller response")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deadline() -> anyhow::Error {
        anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out after 5.0s waiting for project controller response",
        ))
    }

    #[test]
    fn controller_deadline_is_recognized_through_context() {
        assert!(is_controller_deadline(
            &deadline().context("post-commit cleanup")
        ));
        assert!(!is_controller_deadline(&anyhow::anyhow!("commit refused")));
    }

    #[test]
    fn only_a_commit_landed_by_this_command_survives_a_deadline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        crate::start_preflight(&doc, Some("body\n"), Some("body\n")).unwrap();
        let before = observed(crate::load(&doc).unwrap().as_ref());
        crate::mark_response_captured(
            &doc,
            "response_captured",
            Some("body\n"),
            Some("body\n"),
            "response-sha",
            None,
        )
        .unwrap();
        let captured = crate::load(&doc).unwrap();
        assert!(
            !landed_commit_survives_post_commit_error(
                before.as_ref(),
                captured.as_ref(),
                &deadline()
            ),
            "a deadline before the commit landed is a real failure"
        );
        crate::mark_committed(&doc, "commit_success", Some("body\n"), Some("body\n")).unwrap();
        let committed = crate::load(&doc).unwrap();
        assert!(landed_commit_survives_post_commit_error(
            before.as_ref(),
            committed.as_ref(),
            &deadline()
        ));
        assert!(
            !landed_commit_survives_post_commit_error(
                before.as_ref(),
                committed.as_ref(),
                &anyhow::anyhow!("session-check INTERRUPTED")
            ),
            "only a controller deadline is downgraded"
        );
        let already = observed(committed.as_ref());
        assert!(
            !landed_commit_survives_post_commit_error(
                already.as_ref(),
                committed.as_ref(),
                &deadline()
            ),
            "an older committed cycle is not this command's commit"
        );
    }
}
