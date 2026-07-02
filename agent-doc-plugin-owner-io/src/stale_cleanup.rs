//! JetBrains plugin-owner stale sidecar cleanup IO.

use std::path::Path;

use agent_doc_plugin_owner::stale_cleanup::{
    jetbrains_consumer_patches_dir, jetbrains_live_buffer_sidecar_dir,
    should_reap_jetbrains_consumer_file, should_reap_jetbrains_live_buffer_sidecar,
};

/// Counts from one JetBrains stale-sidecar closeout pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JetBrainsStaleReapCounts {
    pub consumer_patches: usize,
    pub live_buffers: usize,
}

/// Resolve the project root for a session document and reap stale JetBrains
/// patch/live-buffer sidecars. Missing project roots are a no-op.
pub fn reap_stale_jetbrains_for_file(file: &Path) -> JetBrainsStaleReapCounts {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return JetBrainsStaleReapCounts::default();
    };
    JetBrainsStaleReapCounts {
        consumer_patches: reap_stale_jetbrains_consumers(&project_root),
        live_buffers: reap_stale_jetbrains_live_buffers(&project_root),
    }
}

/// `#fccreap`: best-effort reap of stale dead-PID IntelliJ plugin consumer patch
/// files from `<project_root>/.agent-doc/patches/`.
///
/// Removes only files whose pid is provably dead according to the focused
/// plugin-owner liveness predicate. Directory/read/remove errors degrade to
/// stderr warnings; hook closeout must never fail because a reap could not run.
pub fn reap_stale_jetbrains_consumers(project_root: &Path) -> usize {
    let patches_dir = jetbrains_consumer_patches_dir(project_root);
    reap_stale_jetbrains_consumers_with(
        &patches_dir,
        agent_doc_plugin_owner::plugin_owner_pid_is_live,
    )
}

/// `#fccreap`: testable core of [`reap_stale_jetbrains_consumers`]. Takes an
/// injectable liveness predicate so unit tests avoid real process probes.
pub fn reap_stale_jetbrains_consumers_with(
    patches_dir: &Path,
    is_pid_live: impl Fn(u32) -> bool,
) -> usize {
    let entries = match std::fs::read_dir(patches_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(err) => {
            eprintln!(
                "[plugin-owner-io] jetbrains consumer reap: cannot read {}: {err}",
                patches_dir.display()
            );
            return 0;
        }
    };
    let self_pid = std::process::id();
    let mut reaped = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("[plugin-owner-io] jetbrains consumer reap: dir entry error: {err}");
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !should_reap_jetbrains_consumer_file(name, self_pid, &is_pid_live) {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(()) => reaped += 1,
            Err(err) => {
                eprintln!(
                    "[plugin-owner-io] jetbrains consumer reap: failed to remove {}: {err}",
                    path.display()
                );
            }
        }
    }
    reaped
}

/// `#lbreap`: best-effort reap of stale dead-PID IntelliJ live-buffer sidecars
/// from `<project_root>/.agent-doc/live-buffer/`.
///
/// Mirrors [`reap_stale_jetbrains_consumers`] for live-buffer sidecars. Removes
/// only regular sidecar files whose embedded JetBrains pid is provably dead.
pub fn reap_stale_jetbrains_live_buffers(project_root: &Path) -> usize {
    let dir = jetbrains_live_buffer_sidecar_dir(project_root);
    reap_stale_jetbrains_live_buffers_with(&dir, agent_doc_plugin_owner::plugin_owner_pid_is_live)
}

/// `#lbreap`: testable core of [`reap_stale_jetbrains_live_buffers`].
pub fn reap_stale_jetbrains_live_buffers_with(
    dir: &Path,
    is_pid_live: impl Fn(u32) -> bool,
) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(err) => {
            eprintln!(
                "[plugin-owner-io] live-buffer reap: cannot read {}: {err}",
                dir.display()
            );
            return 0;
        }
    };
    let self_pid = std::process::id();
    let mut reaped = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("[plugin-owner-io] live-buffer reap: dir entry error: {err}");
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let path = entry.path();
        if !should_reap_jetbrains_live_buffer_sidecar(name, path.is_file(), self_pid, &is_pid_live)
        {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => reaped += 1,
            Err(err) => {
                eprintln!(
                    "[plugin-owner-io] live-buffer reap: failed to remove {}: {err}",
                    path.display()
                );
            }
        }
    }
    reaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reap_removes_only_dead_pid_consumer_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let patches = tmp.path().join("patches");
        std::fs::create_dir_all(&patches).unwrap();

        let dead_a = patches.join("h1.jetbrains-111-aaaa.json");
        let dead_b = patches.join("h2.jetbrains-222-bbbb.json");
        let alive = patches.join("h3.jetbrains-333-cccc.json");
        let base = patches.join("h4.json");
        let unrelated = patches.join("notes.txt");
        for p in [&dead_a, &dead_b, &alive, &base, &unrelated] {
            std::fs::write(p, "{}").unwrap();
        }

        let reaped = reap_stale_jetbrains_consumers_with(&patches, |pid| pid == 333);

        assert_eq!(reaped, 2);
        assert!(!dead_a.exists(), "dead-pid file A should be reaped");
        assert!(!dead_b.exists(), "dead-pid file B should be reaped");
        assert!(alive.exists(), "alive-pid file should survive");
        assert!(base.exists(), "base patch file should survive");
        assert!(unrelated.exists(), "unrelated file should survive");
    }

    #[test]
    fn reap_removes_only_dead_pid_live_buffer_sidecars() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("live-buffer");
        std::fs::create_dir_all(&dir).unwrap();

        let dead_a = dir.join("s1.jetbrains-111-aaaa");
        let dead_b = dir.join("s2.jetbrains-222-bbbb");
        let alive = dir.join("s3.jetbrains-333-cccc");
        let legacy = dir.join("s4");
        let vscode = dir.join("s5.vscode-9-dddd");
        for p in [&dead_a, &dead_b, &alive, &legacy, &vscode] {
            std::fs::write(p, "{}").unwrap();
        }

        let reaped = reap_stale_jetbrains_live_buffers_with(&dir, |pid| pid == 333);

        assert_eq!(reaped, 2);
        assert!(!dead_a.exists(), "dead-pid sidecar A should be reaped");
        assert!(!dead_b.exists(), "dead-pid sidecar B should be reaped");
        assert!(alive.exists(), "alive-pid sidecar should survive");
        assert!(legacy.exists(), "legacy no-id sidecar must never be reaped");
        assert!(
            vscode.exists(),
            "non-jetbrains sidecar must never be reaped"
        );
    }

    #[test]
    fn reap_live_buffers_is_noop_on_missing_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("live-buffer");
        assert_eq!(
            reap_stale_jetbrains_live_buffers_with(&missing, |_| false),
            0
        );
    }

    #[test]
    fn reap_is_noop_on_empty_or_missing_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("patches");
        assert_eq!(reap_stale_jetbrains_consumers_with(&missing, |_| false), 0);

        std::fs::create_dir_all(&missing).unwrap();
        assert_eq!(reap_stale_jetbrains_consumers_with(&missing, |_| false), 0);
    }
}
