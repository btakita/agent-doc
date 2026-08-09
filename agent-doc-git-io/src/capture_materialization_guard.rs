use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::Result;
use std::path::Path;

pub struct ActiveCaptureMaterialization {
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    pub terminal: bool,
}

pub trait CaptureMaterializationGuardEffects {
    fn load_active_capture(&self, file: &Path) -> Result<Option<ActiveCaptureMaterialization>>;
    /// Whether anything durable owns a retained write for this document.
    ///
    /// `#commitwritecommitdeadlock`: this guard used to name
    /// `agent-doc write --commit <FILE>` unconditionally. When the cycle is at
    /// `write_applied`, that command answers with the `AwaitingTerminalCommit`
    /// remedy — which names `agent-doc commit <FILE>`, the command whose guard
    /// this is. Two commands, one state, each pointing at the other.
    /// Implementors call `agent_doc_capture_io::retained_write_ownership`.
    fn retained_write_ownership(
        &self,
        file: &Path,
    ) -> agent_doc_turn::write_ownership::RetainedWriteOwnership;
    fn response_materialized_in_referenced_compact_archive(
        &self,
        file: &Path,
        response_body: &str,
        commit_surface: &str,
    ) -> bool;
    fn log_op(&self, file: &Path, message: &str);
    fn log_missing_capture_guard(&self, file: &Path);
}

pub fn ensure_active_capture_materialized_for_head_current_noop(
    effects: &impl CaptureMaterializationGuardEffects,
    file: &Path,
    snapshot_content: Option<&str>,
    head_doc: Option<&str>,
) -> Result<()> {
    ensure_active_capture_materialized_for_commit(
        effects,
        file,
        snapshot_content.or(head_doc),
        "head_current",
    )
}

pub fn ensure_active_capture_materialized_for_commit(
    effects: &impl CaptureMaterializationGuardEffects,
    file: &Path,
    staged_content: Option<&str>,
    basis: &str,
) -> Result<()> {
    let capture = effects.load_active_capture(file)?;
    let response_in_commit_surface =
        capture
            .as_ref()
            .zip(staged_content)
            .is_some_and(|(capture, materialized)| {
                agent_doc_turn::response_replay::response_materialized_in_content(
                    &capture.response_body,
                    materialized,
                )
            });
    let response_in_referenced_compact_archive =
        capture
            .as_ref()
            .zip(staged_content)
            .is_some_and(|(capture, materialized)| {
                !response_in_commit_surface
                    && effects.response_materialized_in_referenced_compact_archive(
                        file,
                        &capture.response_body,
                        materialized,
                    )
            });
    let decision = agent_doc_workflow::capture::decide_capture_closeout_materialization(
        agent_doc_workflow::capture::CaptureCloseoutMaterializationEvidence {
            active_capture: capture.is_some(),
            capture_terminal: capture.as_ref().is_some_and(|capture| capture.terminal),
            commit_surface_available: staged_content.is_some(),
            response_in_commit_surface,
            response_in_referenced_compact_archive,
        },
    );
    match decision {
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::Allow(
            agent_doc_workflow::capture::CaptureCloseoutMaterializationBasis::ReferencedCompactArchive,
        ) => {
            let capture = capture.as_ref().expect("archive decision requires active capture");
            effects.log_op(
                file,
                &format!(
                    "commit_capture_materialized_in_referenced_compact_archive file={} capture_id={} response_sha256={} basis={}",
                    file.display(),
                    capture.capture_id,
                    capture.response_sha256,
                    basis,
                ),
            );
            return Ok(());
        }
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::Allow(_) => {
            return Ok(());
        }
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::BlockMissingResponse => {}
    }
    let capture = capture.expect("blocked materialization requires active capture");

    effects.log_op(
        file,
        &format!(
            "{} file={} capture_id={} response_sha256={} basis={}",
            OpsLogEvent::CommitBlockedMissingCapturedResponse,
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            basis
        ),
    );
    effects.log_missing_capture_guard(file);

    // `#commitwritecommitdeadlock`: pick the remedy from the shared ownership
    // predicate instead of always naming `write --commit`.
    //
    // Observed 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md` at
    // `cycle-1786271908581`: `respond` failed with the `AwaitingTerminalCommit`
    // remedy naming `agent-doc commit`; `commit` reached this guard and named
    // `agent-doc write --commit`; `write --commit` answered with the SAME
    // `AwaitingTerminalCommit` remedy, naming `commit` again. Three commands, a
    // closed cycle, and an agent following the instructions faithfully has no
    // move — the identical failure `#strandedremedydeadlock` fixed for the
    // `commit`/`session-check` pair, in a pair that fix did not cover.
    //
    // `commit_is_the_named_recovery` is the same predicate that resolved that
    // one: it is true exactly for the verdicts whose remedy sends the agent to
    // `agent-doc commit`. When it holds, naming `write --commit` here is what
    // closes the loop, because `write --commit` will bounce straight back. The
    // recovery that actually works in that state — proven on the incident above
    // — is the next cycle, whose preflight owns capture materialisation and
    // committed both responses without losing any text.
    let ownership = effects.retained_write_ownership(file);
    let remedy = if ownership.verdict().commit_is_the_named_recovery() {
        format!(
            "This is NOT a missed patchback: the capture is durable and the cycle is already at \
             `write_applied`, so `agent-doc write --commit {}` will answer that the write ALREADY \
             LANDED and send you back to `agent-doc commit` — a loop with no exit. Open the next \
             cycle instead: `agent-doc {}`. Preflight owns capture materialisation and recovery, \
             and no visible text is lost while it is pending",
            file.display(),
            file.display()
        )
    } else {
        format!(
            "Replay the captured response with `agent-doc write --commit {}` before marking the \
             cycle committed",
            file.display()
        )
    };
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout. {remedy}.",
        file.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::write_ownership::RetainedWriteOwnership;
    use std::cell::RefCell;

    struct Effects {
        ownership: RetainedWriteOwnership,
        logs: RefCell<Vec<String>>,
    }

    impl Effects {
        fn with(ownership: RetainedWriteOwnership) -> Self {
            Self {
                ownership,
                logs: RefCell::new(Vec::new()),
            }
        }
    }

    impl CaptureMaterializationGuardEffects for Effects {
        fn load_active_capture(
            &self,
            _file: &Path,
        ) -> Result<Option<ActiveCaptureMaterialization>> {
            Ok(Some(ActiveCaptureMaterialization {
                capture_id: "cap-1".to_string(),
                response_sha256: "sha".to_string(),
                response_body: "### Re: something — opus-5\n\nbody\n".to_string(),
                terminal: false,
            }))
        }

        fn retained_write_ownership(&self, _file: &Path) -> RetainedWriteOwnership {
            self.ownership
        }

        fn response_materialized_in_referenced_compact_archive(
            &self,
            _file: &Path,
            _response_body: &str,
            _commit_surface: &str,
        ) -> bool {
            false
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }

        fn log_missing_capture_guard(&self, _file: &Path) {}
    }

    fn blocked_error(ownership: RetainedWriteOwnership) -> String {
        let effects = Effects::with(ownership);
        let err = ensure_active_capture_materialized_for_head_current_noop(
            &effects,
            Path::new("plan.md"),
            Some("staged content without the captured response"),
            None,
        )
        .expect_err("a missing captured response must fail closed");
        format!("{err:#}")
    }

    /// `#commitwritecommitdeadlock`: at `write_applied` this guard must NOT name
    /// `write --commit`.
    ///
    /// Observed 2026-08-09 on `tasks/agent-doc/agent-doc-bugs2.md`: `respond`
    /// named `agent-doc commit`, `commit` reached this guard and named
    /// `agent-doc write --commit`, and `write --commit` answered with the same
    /// `AwaitingTerminalCommit` remedy naming `commit` again. An agent obeying
    /// the instructions faithfully has no move.
    #[test]
    fn a_write_applied_cycle_is_not_sent_to_the_command_that_sends_it_back() {
        let awaiting = RetainedWriteOwnership::new_with_phase(true, true, true);
        assert!(
            awaiting.verdict().commit_is_the_named_recovery(),
            "fixture must be a verdict whose remedy names `agent-doc commit`"
        );

        let err = blocked_error(awaiting);
        // Mentioning the command to warn against it is fine — PRESCRIBING it is
        // the deadlock. Same distinction the `#strandedremedydeadlock`
        // biconditional draws.
        assert!(
            !err.contains("Replay the captured response with `agent-doc write --commit"),
            "prescribing the command that bounces straight back is the deadlock: {err}"
        );
        assert!(
            err.contains("`agent-doc plan.md`"),
            "the remedy must name the recovery that actually works: {err}"
        );
        assert!(
            err.contains("no exit") || err.contains("loop"),
            "say why the obvious command is wrong, or the next agent will try it: {err}"
        );
    }

    /// The ordinary missed-patchback case is unchanged: nothing owns a retained
    /// write, `write --commit` really is the replay path, and it does not bounce.
    #[test]
    fn an_unowned_missed_patchback_still_names_write_commit() {
        let unowned = RetainedWriteOwnership::UNOWNED;
        // `Stranded` also routes through `commit`, so use the shape that does
        // not: something durable still holds the write.
        let deferred = RetainedWriteOwnership::new(true, false);
        assert!(!deferred.verdict().commit_is_the_named_recovery());

        let err = blocked_error(deferred);
        assert!(
            err.contains("`agent-doc write --commit plan.md`"),
            "the replay path must still be named when it works: {err}"
        );
        // And the guard still fires at all for the unowned shape.
        assert!(blocked_error(unowned).contains("captured response body is not present"));
    }
}
