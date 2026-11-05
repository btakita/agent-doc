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

/// Stable token stamped on [`RetainedCloseoutPending`] so hooks and scripts
/// that only see text can tell a pending retained closeout from a failure.
pub const RETAINED_CLOSEOUT_PENDING_TOKEN: &str = "status=retained_closeout_pending";

/// `#respondfalseerror`: the cycle this command captured is still open, its
/// response is durably captured, and the command failed only because the
/// controller's derived state projection had not activated yet
/// (`awaits_projection` is the caller's reading of the
/// `recovery=await_live_projection_then_retry_controller_commit` token).
///
/// 2026-11-04, agent-doc-bugs.md, cycles 1793834587613, 1793835295040 and
/// 1793836980789: closeout's waiting-on-you refresh hit that refusal right
/// after its own write landed, `respond` exited non-zero with the dogfood fix
/// prompt, and the retained closeout worker committed the same cycle two
/// seconds later. That captured cycle is a closeout the worker owns, not a lost
/// response. Returns the cycle id the worker must close.
pub fn retained_capture_awaits_projection(
    before: Option<&(String, bool)>,
    after: Option<&CycleState>,
    awaits_projection: bool,
) -> Option<String> {
    if !awaits_projection {
        return None;
    }
    let after = after?;
    if !matches!(
        after.phase,
        CyclePhase::ResponseCaptured | CyclePhase::WriteApplied
    ) || after.response_sha256.is_none()
        || after.capture_id.is_none()
    {
        return None;
    }
    let captured_by_this_command = match before {
        None => true,
        Some((cycle_id, was_open)) => *cycle_id != after.cycle_id || *was_open,
    };
    captured_by_this_command.then(|| after.cycle_id.clone())
}

/// A `respond` whose captured response is retained for the closeout worker,
/// which had not committed it when the bounded wait ended. NOT a failure: the
/// response is durable and nothing must be re-sent.
#[derive(Debug)]
pub struct RetainedCloseoutPending {
    pub file: String,
    pub cycle_id: String,
    pub waited_ms: u128,
    /// The original refusal, verbatim, so token classifiers still see it.
    pub refusal: String,
}

impl std::fmt::Display for RetainedCloseoutPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{RETAINED_CLOSEOUT_PENDING_TOKEN}] cycle `{}` of {} is captured and retained for the \
             closeout worker, which had not committed it after {}ms. The controller's derived state \
             projection was still activating. This is a pending closeout, not a failure: the \
             response is durable. Do NOT re-answer, resubmit, force disk, or `admin recycle`; run \
             `agent-doc session-check {}` to observe the commit. Refusal: {}",
            self.cycle_id, self.file, self.waited_ms, self.file, self.refusal,
        )
    }
}

impl std::error::Error for RetainedCloseoutPending {}

/// Whether `err` is a [`RetainedCloseoutPending`] deferral.
pub fn retained_closeout_pending(err: &anyhow::Error) -> Option<&RetainedCloseoutPending> {
    err.downcast_ref::<RetainedCloseoutPending>()
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

    #[test]
    fn only_this_commands_open_capture_awaiting_projection_is_retained() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        crate::start_preflight(&doc, Some("body\n"), Some("body\n")).unwrap();
        let before = observed(crate::load(&doc).unwrap().as_ref());
        let preflight = crate::load(&doc).unwrap();
        assert_eq!(
            retained_capture_awaits_projection(before.as_ref(), preflight.as_ref(), true),
            None,
            "no captured response means nothing is retained for the worker"
        );
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
        let cycle_id = captured.as_ref().unwrap().cycle_id.clone();
        assert_eq!(
            retained_capture_awaits_projection(before.as_ref(), captured.as_ref(), true),
            Some(cycle_id),
        );
        assert_eq!(
            retained_capture_awaits_projection(before.as_ref(), captured.as_ref(), false),
            None,
            "a refusal without the await-projection recovery stays a failure"
        );
        crate::mark_committed(&doc, "commit_success", Some("body\n"), Some("body\n")).unwrap();
        let committed = crate::load(&doc).unwrap();
        assert_eq!(
            retained_capture_awaits_projection(before.as_ref(), committed.as_ref(), true),
            None,
            "a committed cycle is the committed-closeout case, not a pending one"
        );
    }

    #[test]
    fn retained_closeout_pending_is_recognized_through_context() {
        let err = anyhow::Error::new(RetainedCloseoutPending {
            file: "doc.md".to_string(),
            cycle_id: "cycle-1".to_string(),
            waited_ms: 30_000,
            refusal: "recovery=await_live_projection_then_retry_controller_commit".to_string(),
        });
        let rendered = format!("{err:#}");
        assert!(rendered.contains(RETAINED_CLOSEOUT_PENDING_TOKEN));
        assert!(rendered.contains("recovery=await_live_projection_then_retry_controller_commit"));
        assert!(retained_closeout_pending(&err).is_some());
        assert!(retained_closeout_pending(&anyhow::anyhow!("commit refused")).is_none());
    }
}
