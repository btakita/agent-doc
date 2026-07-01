//! JetBrains plugin-owner stale sidecar cleanup policy.
//!
//! Orchestration owns hook firing plus filesystem/process adapters. This module
//! owns the filename vocabulary and pure dead-pid cleanup decisions for
//! JetBrains consumer patch files and live-buffer sidecars.

use std::path::{Path, PathBuf};

/// `#fccreap`: project-local directory holding per-instance JetBrains consumer
/// patch files.
pub fn jetbrains_consumer_patches_dir(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join("patches")
}

/// `#lbreap`: project-local directory holding live-buffer sidecars.
pub fn jetbrains_live_buffer_sidecar_dir(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join("live-buffer")
}

/// `#fccreap`: parse the IntelliJ-plugin consumer pid out of a per-instance
/// patch filename.
///
/// The JetBrains plugin registers a per-instance consumer id
/// `jetbrains-<pid>-<uuid>` (see
/// `editors/jetbrains/.../TypingTracker.kt`), and per-instance patch files land
/// in `.agent-doc/patches/` named `<doc_hash>.jetbrains-<pid>-<uuid>.json`.
///
/// Returns `Some(pid)` only for filenames that contain the literal `.jetbrains-`
/// marker followed by a run of ASCII digits (the pid). Returns `None` for the
/// base `<hash>.json`, `.vscode` variants, or any name where the pid cannot be
/// parsed; callers must treat `None` as "do not reap".
pub fn jetbrains_consumer_pid(filename: &str) -> Option<u32> {
    if !filename.ends_with(".json") {
        return None;
    }
    let after_marker = filename.split(".jetbrains-").nth(1)?;
    let digits: String = after_marker
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &after_marker[digits.len()..];
    if !rest.starts_with('-') {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// `#lbreap`: parse the owning pid from a live-buffer sidecar filename that
/// embeds a `jetbrains-<pid>-<uuid>` editor id (`<stem>.jetbrains-<pid>-<uuid>`).
/// Returns `None` for the legacy no-editor-id sidecar (`<stem>`) and any
/// non-JetBrains editor id, which are never reaped.
pub fn jetbrains_live_buffer_pid(filename: &str) -> Option<u32> {
    let idx = filename.find("jetbrains-")?;
    let rest = &filename[idx + "jetbrains-".len()..];
    let pid_str = rest.split('-').next()?;
    if pid_str.is_empty() || !pid_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid_str.parse::<u32>().ok()
}

/// Decide whether a JetBrains consumer patch file should be reaped.
///
/// Only files matching `*.jetbrains-<digits>-<uuid>.json` whose pid is not live
/// and is not this process's pid are eligible. Non-matching names, base
/// `<hash>.json` files, `.vscode` variants, and malformed pids are skipped.
pub fn should_reap_jetbrains_consumer_file(
    filename: &str,
    self_pid: u32,
    is_pid_live: impl Fn(u32) -> bool,
) -> bool {
    let Some(pid) = jetbrains_consumer_pid(filename) else {
        return false;
    };
    pid != self_pid && !is_pid_live(pid)
}

/// Decide whether a JetBrains live-buffer sidecar should be reaped.
///
/// Only regular sidecar files with an embedded `jetbrains-<pid>` whose pid is
/// not live and is not this process's pid are eligible. Legacy no-editor-id
/// sidecars and non-JetBrains ids are skipped.
pub fn should_reap_jetbrains_live_buffer_sidecar(
    filename: &str,
    is_regular_file: bool,
    self_pid: u32,
    is_pid_live: impl Fn(u32) -> bool,
) -> bool {
    if !is_regular_file {
        return false;
    }
    let Some(pid) = jetbrains_live_buffer_pid(filename) else {
        return false;
    };
    pid != self_pid && !is_pid_live(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn cleanup_paths_are_project_local() {
        let root = Path::new("/tmp/project");

        assert_eq!(
            jetbrains_consumer_patches_dir(root),
            Path::new("/tmp/project/.agent-doc/patches")
        );
        assert_eq!(
            jetbrains_live_buffer_sidecar_dir(root),
            Path::new("/tmp/project/.agent-doc/live-buffer")
        );
    }

    #[test]
    fn jetbrains_consumer_pid_parses_pid_and_rejects_non_matching() {
        assert_eq!(
            jetbrains_consumer_pid("abc123.jetbrains-12345-1f2e3d4c-uuid.json"),
            Some(12345)
        );
        assert_eq!(jetbrains_consumer_pid("abc123.json"), None);
        assert_eq!(jetbrains_consumer_pid("abc123.vscode-99-uuid.json"), None);
        assert_eq!(jetbrains_consumer_pid("abc123.jetbrains-12345.json"), None);
        assert_eq!(jetbrains_consumer_pid("abc123.jetbrains--uuid.json"), None);
        assert_eq!(
            jetbrains_consumer_pid("abc123.jetbrains-12345-uuid.txt"),
            None
        );
    }

    #[test]
    fn consumer_cleanup_policy_reaps_only_dead_pid_consumer_files() {
        let self_pid = 999;
        let live_pid = |pid| pid == 333;

        assert!(should_reap_jetbrains_consumer_file(
            "h1.jetbrains-111-aaaa.json",
            self_pid,
            live_pid
        ));
        assert!(!should_reap_jetbrains_consumer_file(
            "h3.jetbrains-333-cccc.json",
            self_pid,
            live_pid
        ));
        assert!(!should_reap_jetbrains_consumer_file(
            "h9.jetbrains-999-self.json",
            self_pid,
            |_| false
        ));
        assert!(!should_reap_jetbrains_consumer_file(
            "h4.json",
            self_pid,
            |_| false
        ));
        assert!(!should_reap_jetbrains_consumer_file(
            "h5.vscode-111-aaaa.json",
            self_pid,
            |_| false
        ));
    }

    #[test]
    fn jetbrains_live_buffer_pid_parses_pid_from_sidecar_name() {
        assert_eq!(
            jetbrains_live_buffer_pid("130a18479c134e03.jetbrains-3873180-bf4c9d87-uuid"),
            Some(3873180)
        );
        assert_eq!(jetbrains_live_buffer_pid("130a18479c134e03"), None);
        assert_eq!(jetbrains_live_buffer_pid("stem.vscode-99-uuid"), None);
        assert_eq!(jetbrains_live_buffer_pid("stem.jetbrains--uuid"), None);
    }

    #[test]
    fn live_buffer_cleanup_policy_reaps_only_dead_regular_jetbrains_sidecars() {
        let self_pid = 999;
        let live_pid = |pid| pid == 333;

        assert!(should_reap_jetbrains_live_buffer_sidecar(
            "s1.jetbrains-111-aaaa",
            true,
            self_pid,
            live_pid
        ));
        assert!(!should_reap_jetbrains_live_buffer_sidecar(
            "s3.jetbrains-333-cccc",
            true,
            self_pid,
            live_pid
        ));
        assert!(!should_reap_jetbrains_live_buffer_sidecar(
            "s9.jetbrains-999-self",
            true,
            self_pid,
            |_| false
        ));
        assert!(!should_reap_jetbrains_live_buffer_sidecar(
            "s1.jetbrains-111-aaaa",
            false,
            self_pid,
            |_| false
        ));
        assert!(!should_reap_jetbrains_live_buffer_sidecar(
            "s4",
            true,
            self_pid,
            |_| false
        ));
        assert!(!should_reap_jetbrains_live_buffer_sidecar(
            "s5.vscode-111-aaaa",
            true,
            self_pid,
            |_| false
        ));
    }
}
