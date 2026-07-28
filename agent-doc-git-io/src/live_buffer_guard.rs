use anyhow::Result;
use std::path::Path;

/// The refusal an agent reads when the commit defers on relay convergence.
///
/// This deferral is **not** a failed closeout: the capture is already durable
/// and the retained intent commits on its own moments later — `session-check`
/// routinely reports `committed` seconds after this text is printed. But the
/// text used to state only the fact, and a bare `Error:` with no next step is
/// what makes an agent invent recovery. Observed on 2026-07-26 across two
/// concurrent sessions, which escalated through `admin recycle`, `reload-lib`,
/// and repeated `write --commit` — each of which perturbs the capture being
/// waited on, so the invented recovery is worse than doing nothing.
///
/// Same lesson as `#closeoutwaitchurn`: a fail-closed path must say what to run,
/// and say what NOT to run. Kept as one function so all three call sites give
/// the same instruction rather than drifting into three dialects.
pub fn crdt_relay_pending_refusal(file: &Path) -> String {
    format!(
        "editor is the current authority for {}, but CRDT relay convergence is still pending; \
         disk is a non-authoritative replica and was not used as commit authority. \
         The capture is already durable and the same intent commits itself once delivery \
         converges — this is a deferral, not a lost response. Run `agent-doc session-check {}` \
         once to observe the terminal state; do NOT re-send the response, force disk, \
         `admin recycle`, or `admin reload-lib`, all of which disturb the capture being awaited.",
        file.display(),
        file.display(),
    )
}

pub trait LiveBufferGuardEffects {
    fn commit_barrier_ready(&self, file: &Path) -> bool;
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
    let commit_barrier_ready = effects.commit_barrier_ready(file);
    let commit_allowed = match closeout_commit_allowed(commit_barrier_ready) {
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
    effects.log_op(
        file,
        &format!(
            "commit_blocked_crdt_relay_pending file={} basis={} source=crdt_relay decision={}",
            file.display(),
            basis,
            if cfg!(feature = "adstatechart") {
                "adstatechart"
            } else {
                "fail_closed_fallback"
            },
        ),
    );
    effects.log_live_buffer_guard_blocked(file);
    anyhow::bail!("{}", crdt_relay_pending_refusal(file));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct WedgedFinalizeSimWorld {
        barrier_ready: Cell<bool>,
        commits: Cell<usize>,
        logs: RefCell<Vec<String>>,
    }

    impl WedgedFinalizeSimWorld {
        fn wedged() -> Self {
            Self {
                barrier_ready: Cell::new(false),
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
        fn commit_barrier_ready(&self, _file: &Path) -> bool {
            self.barrier_ready.get()
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }

        fn log_live_buffer_guard_blocked(&self, _file: &Path) {}
    }

    /// A fail-closed path that states only a fact is what makes an agent invent
    /// recovery, and every recovery it invents here perturbs the capture being
    /// awaited. So the refusal has to carry both halves: the one command to run,
    /// and the ones not to.
    #[test]
    fn the_deferral_names_the_next_step_and_forbids_the_invented_ones() {
        let message = crdt_relay_pending_refusal(Path::new("plan.md"));

        assert!(
            message.contains("agent-doc session-check plan.md"),
            "the refusal must name the exact command that observes the outcome: {message}"
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
