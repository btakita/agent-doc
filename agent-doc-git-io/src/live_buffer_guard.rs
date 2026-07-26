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

pub fn ensure_no_live_editor_buffer_ahead_of_disk(
    effects: &impl LiveBufferGuardEffects,
    file: &Path,
    _file_content: &str,
    basis: &str,
    _staged_content: Option<&str>,
) -> Result<()> {
    if effects.commit_barrier_ready(file) {
        effects.log_op(
            file,
            &format!(
                "commit_crdt_barrier_ready file={} basis={} source=crdt_relay",
                file.display(),
                basis
            ),
        );
        return Ok(());
    }
    effects.log_op(
        file,
        &format!(
            "commit_blocked_crdt_relay_pending file={} basis={} source=crdt_relay",
            file.display(),
            basis
        ),
    );
    effects.log_live_buffer_guard_blocked(file);
    anyhow::bail!("{}", crdt_relay_pending_refusal(file));
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
