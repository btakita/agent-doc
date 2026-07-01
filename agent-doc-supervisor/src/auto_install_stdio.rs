//! Supervisor auto-install child stdio policy.
//!
//! This module decides which stdio streams an auto-install child should use. It
//! does not spawn processes or own the platform-specific `Command` wiring.

#[cfg(unix)]
use std::os::fd::RawFd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoInstallStdioStream {
    Null,
    Inherit,
    #[cfg(unix)]
    DuplicateFd(RawFd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoInstallChildStdioPlan {
    pub stdin: AutoInstallStdioStream,
    pub stdout: AutoInstallStdioStream,
    pub stderr: AutoInstallStdioStream,
}

/// `#restartstderrbleed` — route auto-install stdout+stderr to the supervisor
/// log target and deny stdin so build output cannot corrupt the agent pane.
#[cfg(unix)]
pub fn auto_install_child_stdio_plan_to_fd(target_fd: RawFd) -> AutoInstallChildStdioPlan {
    AutoInstallChildStdioPlan {
        stdin: AutoInstallStdioStream::Null,
        stdout: AutoInstallStdioStream::DuplicateFd(target_fd),
        stderr: AutoInstallStdioStream::DuplicateFd(target_fd),
    }
}

/// Default auto-install child stdio policy.
#[cfg(unix)]
pub fn auto_install_child_stdio_plan() -> AutoInstallChildStdioPlan {
    auto_install_child_stdio_plan_to_fd(2)
}

/// Non-unix has no route-owned pane fd-multiplexing model; keep build stdout
/// off parent stdout, deny stdin, and let stderr flow to logs.
#[cfg(not(unix))]
pub fn auto_install_child_stdio_plan() -> AutoInstallChildStdioPlan {
    AutoInstallChildStdioPlan {
        stdin: AutoInstallStdioStream::Null,
        stdout: AutoInstallStdioStream::Null,
        stderr: AutoInstallStdioStream::Inherit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_auto_install_stdio_duplicates_log_fd_for_output() {
        assert_eq!(
            auto_install_child_stdio_plan_to_fd(77),
            AutoInstallChildStdioPlan {
                stdin: AutoInstallStdioStream::Null,
                stdout: AutoInstallStdioStream::DuplicateFd(77),
                stderr: AutoInstallStdioStream::DuplicateFd(77),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_default_auto_install_stdio_targets_stderr_fd() {
        assert_eq!(
            auto_install_child_stdio_plan(),
            AutoInstallChildStdioPlan {
                stdin: AutoInstallStdioStream::Null,
                stdout: AutoInstallStdioStream::DuplicateFd(2),
                stderr: AutoInstallStdioStream::DuplicateFd(2),
            }
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_auto_install_stdio_discards_stdout_and_inherits_stderr() {
        assert_eq!(
            auto_install_child_stdio_plan(),
            AutoInstallChildStdioPlan {
                stdin: AutoInstallStdioStream::Null,
                stdout: AutoInstallStdioStream::Null,
                stderr: AutoInstallStdioStream::Inherit,
            }
        );
    }
}
