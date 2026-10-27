use agent_doc_turn::commit_barrier::{CommitBarrierRefusal, CommitBarrierVerdict};
use agent_doc_turn::write_ownership::{RetainedWriteOwnership, retained_write_remedy};
use anyhow::Result;
use std::path::Path;

/// The refusal an agent reads when the commit defers on relay convergence.
///
/// This deferral is **not** a failed closeout *when something owns the write*:
/// the capture is durable and the retained intent commits on its own moments
/// later. But the text used to state only the fact, and a bare `Error:` with no
/// next step is what makes an agent invent recovery. Observed on 2026-07-26
/// across two concurrent sessions, which escalated through `admin recycle`,
/// `reload-lib`, and repeated `write --commit` — each of which perturbs the
/// capture being waited on, so the invented recovery is worse than doing nothing.
///
/// Same lesson as `#closeoutwaitchurn`: a fail-closed path must say what to run,
/// and say what NOT to run. Kept as one function so all three call sites give
/// the same instruction rather than drifting into three dialects.
///
/// One function was not enough (`#percellconverge`). The *wording* was shared;
/// the *predicate* was not, and this site kept promising a deferral
/// unconditionally. Observed three times on 2026-08-03 on
/// `tasks/agent-doc/agent-doc-bugs2.md`: this refusal printed while
/// `session-check` — carrying the 0.35.123 ownership fix — said the same state
/// was stranded, and `session-check` was right every time. An agent that reached
/// this text first waited on an edge that could not fire. So the caller must now
/// prove ownership, and the remedy is derived from that proof rather than
/// asserted.
pub fn crdt_relay_pending_refusal(file: &Path, ownership: RetainedWriteOwnership) -> String {
    format!(
        "editor is the current authority for {}, but CRDT relay convergence is still pending; \
         disk is a non-authoritative replica and was not used as commit authority. {}.",
        file.display(),
        retained_write_remedy(ownership, &file.display().to_string()),
    )
}

/// `#clobberfollowups` (2): the refusal for a commit whose barrier is ready
/// while the editor's VISIBLE buffer holds text the canonical lacks
/// (`state=editor_holds_unmerged_visible_text`). The remedy is derived from the
/// same ownership predicate every other retained-write reader uses.
pub fn editor_holds_unmerged_visible_text_refusal(
    file: &Path,
    ownership: RetainedWriteOwnership,
) -> String {
    format!(
        "editor is the current authority for {}, and its visible buffer holds text the canonical \
         does not (state={}); committing the canonical now would publish a cut without that \
         text, so the commit was refused and nothing was written. {}.",
        file.display(),
        agent_doc_turn::write_ownership::EDITOR_HOLDS_UNMERGED_VISIBLE_TEXT_STATE,
        retained_write_remedy(
            ownership.with_editor_holds_unmerged_visible_text(true),
            &file.display().to_string()
        ),
    )
}

/// `#inflighttypinghold` (3): the refusal for a commit the relay barrier
/// refused because a live editor replica is quarantined. It used to print
/// [`crdt_relay_pending_refusal`], whose "wait for convergence" remedy is wrong
/// here: the editor holds operator text the canonical lacks.
pub fn quarantined_replica_refusal(file: &Path) -> String {
    format!(
        "editor is the current authority for {}, and the commit was refused because a live \
         editor replica is quarantined; committing the canonical now would publish a cut \
         without that editor's text. {}.",
        file.display(),
        agent_doc_turn::write_ownership::quarantined_replica_commit_remedy(
            &file.display().to_string()
        ),
    )
}

/// The refusal for a commit the relay barrier refused, by its reason. Every
/// CLI refusal site renders this one function, so a quarantined replica is
/// named the same way wherever the commit was attempted.
pub fn commit_barrier_refusal(
    file: &Path,
    refusal: CommitBarrierRefusal,
    ownership: impl FnOnce() -> RetainedWriteOwnership,
) -> String {
    match refusal {
        CommitBarrierRefusal::QuarantinedReplica => quarantined_replica_refusal(file),
        CommitBarrierRefusal::ConvergencePending => crdt_relay_pending_refusal(file, ownership()),
    }
}

pub trait LiveBufferGuardEffects {
    /// The relay commit barrier's verdict for `file`, with its refusal reason.
    fn commit_barrier(&self, file: &Path) -> CommitBarrierVerdict;
    /// Whether the editor holding `file` keeps visible text the canonical
    /// lacks (the durable exhausted replica-rebuild record still describes the
    /// document). Implementors read
    /// `agent_doc_document_realtime_io::editor_holds_unmerged_visible_text`.
    fn editor_holds_unmerged_visible_text(&self, file: &Path) -> bool;
    /// Whether anything durable owns a write this guard would retain.
    ///
    /// The guard cannot read `state.db` itself — `agent-doc-git-io` sits below
    /// the capture/cycle crates — and it must not guess: an unproven site that
    /// assumes a holder is precisely the defect. Implementors call
    /// `agent_doc_capture_io::retained_write_ownership`.
    fn retained_write_ownership(&self, file: &Path) -> RetainedWriteOwnership;
    fn log_op(&self, file: &Path, message: &str);
    fn log_live_buffer_guard_blocked(&self, file: &Path);
}

#[cfg(feature = "adstatechart")]
fn closeout_commit_allowed(commit_barrier_ready: bool) -> Result<bool, String> {
    agent_doc_state_backbone::adstatechart::closeout_commit_transition(commit_barrier_ready)
}

#[cfg(not(feature = "adstatechart"))]
fn closeout_commit_allowed(commit_barrier_ready: bool) -> Result<bool, String> {
    // The chart is deliberately optional so narrow consumers can compile it
    // out. Preserve the previous guard as a fail-closed fallback: only a proven
    // ready relay barrier can commit.
    Ok(commit_barrier_ready)
}

pub fn ensure_no_live_editor_buffer_ahead_of_disk(
    effects: &impl LiveBufferGuardEffects,
    file: &Path,
    _file_content: &str,
    basis: &str,
    _staged_content: Option<&str>,
) -> Result<()> {
    let barrier = effects.commit_barrier(file);
    let commit_allowed = match closeout_commit_allowed(barrier.is_ready()) {
        Ok(allowed) => allowed,
        Err(err) => {
            effects.log_op(
                file,
                &format!(
                    "commit_adstatechart_unavailable file={} basis={} error={err}",
                    file.display(),
                    basis,
                ),
            );
            false
        }
    };
    if commit_allowed && effects.editor_holds_unmerged_visible_text(file) {
        let ownership = effects.retained_write_ownership(file);
        effects.log_op(
            file,
            &format!(
                "commit_blocked_editor_holds_unmerged_visible_text file={} basis={} state={} ownership={} (#clobberfollowups)",
                file.display(),
                basis,
                agent_doc_turn::write_ownership::EDITOR_HOLDS_UNMERGED_VISIBLE_TEXT_STATE,
                ownership
                    .with_editor_holds_unmerged_visible_text(true)
                    .verdict()
                    .as_str(),
            ),
        );
        effects.log_live_buffer_guard_blocked(file);
        anyhow::bail!(
            "{}",
            editor_holds_unmerged_visible_text_refusal(file, ownership)
        );
    }
    if commit_allowed {
        effects.log_op(
            file,
            &format!(
                "commit_crdt_barrier_ready file={} basis={} source=crdt_relay decision={}",
                file.display(),
                basis,
                if cfg!(feature = "adstatechart") {
                    "adstatechart"
                } else {
                    "fail_closed_fallback"
                },
            ),
        );
        return Ok(());
    }
    if barrier.refusal() == Some(CommitBarrierRefusal::QuarantinedReplica) {
        effects.log_op(
            file,
            &format!(
                "commit_blocked_crdt_quarantined_replica file={} basis={} source=crdt_relay state={} (#inflighttypinghold)",
                file.display(),
                basis,
                agent_doc_turn::write_ownership::QUARANTINED_REPLICA_COMMIT_STATE,
            ),
        );
        effects.log_live_buffer_guard_blocked(file);
        anyhow::bail!("{}", quarantined_replica_refusal(file));
    }
    let ownership = effects.retained_write_ownership(file);
    effects.log_op(
        file,
        &format!(
            "commit_blocked_crdt_relay_pending file={} basis={} source=crdt_relay decision={} ownership={}",
            file.display(),
            basis,
            if cfg!(feature = "adstatechart") {
                "adstatechart"
            } else {
                "fail_closed_fallback"
            },
            ownership.verdict().as_str(),
        ),
    );
    effects.log_live_buffer_guard_blocked(file);
    anyhow::bail!("{}", crdt_relay_pending_refusal(file, ownership));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct WedgedFinalizeSimWorld {
        barrier_ready: Cell<bool>,
        barrier_refusal: Cell<CommitBarrierRefusal>,
        unmerged_visible_text: Cell<bool>,
        ownership: Cell<RetainedWriteOwnership>,
        commits: Cell<usize>,
        logs: RefCell<Vec<String>>,
    }

    impl WedgedFinalizeSimWorld {
        fn wedged() -> Self {
            Self {
                barrier_ready: Cell::new(false),
                barrier_refusal: Cell::new(CommitBarrierRefusal::ConvergencePending),
                unmerged_visible_text: Cell::new(false),
                // An open cycle owns the retained write: the historical shape
                // this SimWorld was written for.
                ownership: Cell::new(RetainedWriteOwnership::new(true, true)),
                commits: Cell::new(0),
                logs: RefCell::new(Vec::new()),
            }
        }

        fn direct_commit(&self) -> Result<()> {
            ensure_no_live_editor_buffer_ahead_of_disk(
                self,
                Path::new("plan.md"),
                "stale disk",
                "pre_stage",
                Some("stale disk"),
            )?;
            self.commits.set(self.commits.get() + 1);
            Ok(())
        }
    }

    impl LiveBufferGuardEffects for WedgedFinalizeSimWorld {
        fn commit_barrier(&self, _file: &Path) -> CommitBarrierVerdict {
            if self.barrier_ready.get() {
                CommitBarrierVerdict::Ready
            } else {
                CommitBarrierVerdict::Refused(self.barrier_refusal.get())
            }
        }

        fn editor_holds_unmerged_visible_text(&self, _file: &Path) -> bool {
            self.unmerged_visible_text.get()
        }

        fn retained_write_ownership(&self, _file: &Path) -> RetainedWriteOwnership {
            self.ownership.get()
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }

        fn log_live_buffer_guard_blocked(&self, _file: &Path) {}
    }

    /// `#clobberfollowups` (2): a ready relay barrier is not enough while the
    /// editor's visible buffer holds text the canonical lacks. The commit is
    /// refused with the operator-text remedy, and nothing is committed.
    #[test]
    fn clobberfollowups_commit_refuses_while_the_editor_holds_unmerged_visible_text() {
        let world = WedgedFinalizeSimWorld::wedged();
        world.barrier_ready.set(true);
        world.unmerged_visible_text.set(true);
        let err = world.direct_commit().unwrap_err().to_string();
        assert_eq!(world.commits.get(), 0, "no commit may seal the canonical");
        assert!(
            err.contains("state=editor_holds_unmerged_visible_text")
                && err.contains("nothing was written"),
            "{err}"
        );
        assert!(
            err.contains("Do NOT re-send it, force disk"),
            "the remedy must rule out the recoveries that discard operator text: {err}"
        );
        assert!(
            world
                .logs
                .borrow()
                .iter()
                .any(|line| line.contains("commit_blocked_editor_holds_unmerged_visible_text")),
            "{:?}",
            world.logs.borrow()
        );

        world.unmerged_visible_text.set(false);
        world.direct_commit().unwrap();
        assert_eq!(world.commits.get(), 1);
    }

    /// `#inflighttypinghold` (3): a commit refused over a quarantined live
    /// replica names the quarantine and its remedy, not the generic
    /// convergence wait, and commits nothing.
    #[test]
    fn inflighttypinghold_commit_refused_over_a_quarantined_replica_names_the_quarantine() {
        let world = WedgedFinalizeSimWorld::wedged();
        world
            .barrier_refusal
            .set(CommitBarrierRefusal::QuarantinedReplica);
        let err = world.direct_commit().unwrap_err().to_string();
        assert_eq!(world.commits.get(), 0, "no commit may seal the canonical");
        assert!(
            err.contains("state=crdt_commit_barrier_refused_quarantined_replica")
                && err.contains("replica is quarantined"),
            "{err}"
        );
        assert!(
            !err.contains("convergence is still pending"),
            "the generic convergence wait is the wrong remedy here: {err}"
        );
        for forbidden in ["Do NOT force disk", "re-send the response"] {
            assert!(
                err.contains(forbidden),
                "must rule out `{forbidden}`: {err}"
            );
        }
        assert!(
            world
                .logs
                .borrow()
                .iter()
                .any(|line| line.contains("commit_blocked_crdt_quarantined_replica")),
            "{:?}",
            world.logs.borrow()
        );
        // Every CLI site renders the same text through the one dispatcher.
        assert_eq!(
            commit_barrier_refusal(
                Path::new("plan.md"),
                CommitBarrierRefusal::QuarantinedReplica,
                || unreachable!("a quarantine refusal never reads write ownership"),
            ),
            err
        );

        // The generic refusal keeps the convergence text.
        world
            .barrier_refusal
            .set(CommitBarrierRefusal::ConvergencePending);
        let err = world.direct_commit().unwrap_err().to_string();
        assert!(err.contains("convergence is still pending"), "{err}");
        assert_eq!(world.commits.get(), 0);
    }

    /// A fail-closed path that states only a fact is what makes an agent invent
    /// recovery, and every recovery it invents here perturbs the capture being
    /// awaited. So the refusal has to carry both halves: the keyed owner edge
    /// to await, and the actions not to take.
    #[test]
    fn the_owned_deferral_names_the_next_step_and_forbids_the_invented_ones() {
        let message = crdt_relay_pending_refusal(
            Path::new("plan.md"),
            RetainedWriteOwnership::new(true, true),
        );

        assert!(
            message.contains("this exact retained intent")
                && message.contains("controller-owned terminal state edge"),
            "the refusal must keep observation with the keyed owner: {message}"
        );
        assert!(
            !message.contains("Run `agent-doc session-check plan.md`"),
            "a document-wide status check can observe a successor cycle: {message}"
        );
        assert!(
            message.contains("deferral, not a lost response"),
            "an agent that reads this as a lost response re-sends it: {message}"
        );
        for invented in ["force disk", "admin recycle", "admin reload-lib", "re-send"] {
            assert!(
                message.contains(invented),
                "must explicitly rule out `{invented}`, which sessions reached for on 2026-07-26: {message}"
            );
        }
    }

    /// The 2026-08-03 shape, observed three times on `agent-doc-bugs2.md`: the
    /// mutation applied, this refusal printed, and `session-check` said the same
    /// state was stranded. Each was recovered by hand with `agent-doc commit`.
    /// Whichever text the agent reached first decided whether the work survived,
    /// so the two paths must now agree.
    #[test]
    fn an_unowned_write_is_reported_stranded_here_exactly_as_session_check_reports_it() {
        let message =
            crdt_relay_pending_refusal(Path::new("plan.md"), RetainedWriteOwnership::UNOWNED);

        assert!(
            message.contains("STRANDED, not deferred"),
            "the write path must stop contradicting session-check: {message}"
        );
        assert!(
            message.contains("agent-doc commit plan.md"),
            "the unowned case must name the recovery that actually worked: {message}"
        );
        assert!(
            !message.contains("deferral, not a lost response"),
            "nothing owns this write, so the deferral promise is false: {message}"
        );
        assert!(
            !message.contains("do NOT"),
            "forbidding recovery on an unowned write is what stranded the edits: {message}"
        );
    }

    /// The blocked-commit log line must carry the verdict, or an operator
    /// reading `ops.log` after the fact cannot tell which of the two states the
    /// refusal was reporting.
    #[test]
    fn the_blocked_commit_log_records_which_verdict_it_printed() {
        let world = WedgedFinalizeSimWorld::wedged();
        world.ownership.set(RetainedWriteOwnership::UNOWNED);

        world
            .direct_commit()
            .expect_err("a pending relay barrier still rejects the direct commit");

        assert!(
            world
                .logs
                .borrow()
                .iter()
                .any(|line| line.contains("commit_blocked_crdt_relay_pending")
                    && line.contains("ownership=stranded")),
            "logs: {:?}",
            world.logs.borrow()
        );
    }

    /// Phase E rung 3 SimWorld: a finalize wedge may fall through to the direct
    /// commit path repeatedly, but every cycle still crosses the production
    /// chart/fallback decision and stale disk is never committed.
    #[test]
    fn n_wedged_finalize_direct_commit_cycles_never_commit_stale_disk() {
        const CYCLES: usize = 64;
        let world = WedgedFinalizeSimWorld::wedged();

        for _ in 0..CYCLES {
            let err = world
                .direct_commit()
                .expect_err("editor-ahead relay barrier must reject direct commit");
            assert!(
                err.to_string()
                    .contains("relay convergence is still pending")
            );
        }
        assert_eq!(world.commits.get(), 0);

        world.barrier_ready.set(true);
        world
            .direct_commit()
            .expect("a converged relay cut may take the commit edge");
        assert_eq!(world.commits.get(), 1);
        assert!(world.logs.borrow().iter().any(|line| {
            line.contains(if cfg!(feature = "adstatechart") {
                "decision=adstatechart"
            } else {
                "decision=fail_closed_fallback"
            })
        }));
    }
}
