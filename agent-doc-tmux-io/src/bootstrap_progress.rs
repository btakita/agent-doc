//! Opt-in phase progress for editor-facing bootstrap commands
//! (`tmux start`, `tmux ensure`), `#starttimeoutload`.
//!
//! An editor sets [`AGENT_DOC_PROGRESS_ENV`] to `1` and reads
//! `[progress] phase=<name>` lines from stderr, so its wait can extend while
//! the command advances and a timeout can name the phase it stalled in. Unset,
//! nothing is printed: a person or another caller sees the same output as
//! before.
//!
//! It lives here, below `agent-doc-session-registry-io`, so a bounded registry
//! lock wait (`#registrylockbound`) can report a heartbeat while it waits
//! instead of going silent until the editor gives up.

use std::io::Write;

/// The environment variable that turns progress reporting on.
pub const AGENT_DOC_PROGRESS_ENV: &str = "AGENT_DOC_PROGRESS";

/// The stderr prefix of one progress line (`[progress] phase=<name>`).
pub const PROGRESS_LINE_PREFIX: &str = "[progress] phase=";

/// Whether this process reports bootstrap phases (`AGENT_DOC_PROGRESS=1`).
pub fn progress_reporting_enabled() -> bool {
    progress_reporting_enabled_from(std::env::var(AGENT_DOC_PROGRESS_ENV).ok().as_deref())
}

/// Pure form of [`progress_reporting_enabled`].
pub fn progress_reporting_enabled_from(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.trim(), "1" | "true" | "yes"))
}

/// The progress line for `phase`.
pub fn progress_line(phase: &str) -> String {
    format!("{PROGRESS_LINE_PREFIX}{phase}")
}

/// Report that a bootstrap command entered (or is still in) `phase`. A no-op
/// unless [`progress_reporting_enabled`]; never fails the command.
pub fn report_progress_phase(phase: &str) {
    if progress_reporting_enabled() {
        let mut stderr = std::io::stderr().lock();
        if let Err(error) =
            writeln!(stderr, "{}", progress_line(phase)).and_then(|()| stderr.flush())
        {
            // stderr itself failed; there is nowhere better to report it, and a
            // progress line must never fail the command it describes.
            drop(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reporting_is_opt_in_and_lines_are_parseable() {
        assert!(!progress_reporting_enabled_from(None));
        assert!(!progress_reporting_enabled_from(Some("")));
        assert!(!progress_reporting_enabled_from(Some("0")));
        assert!(progress_reporting_enabled_from(Some("1")));
        assert!(progress_reporting_enabled_from(Some(" true ")));
        assert_eq!(
            progress_line("registry_lookup"),
            "[progress] phase=registry_lookup"
        );
        assert_eq!(
            progress_line("reused").strip_prefix(PROGRESS_LINE_PREFIX),
            Some("reused")
        );
    }
}
