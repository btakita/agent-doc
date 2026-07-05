use anyhow::Result;
use std::path::Path;

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
    anyhow::bail!(
        "editor is the current authority for {}, but CRDT relay convergence is still pending; disk is a non-authoritative replica and was not used as commit authority",
        file.display()
    );
}
