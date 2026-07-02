//! Local-model lease registry process adapters.

use std::path::Path;

/// Outcome of one best-effort local-model lease reap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapOutcome {
    /// Reap ran and exited 0. Carries stdout (JSON) for diagnostics.
    Reaped(String),
    /// This file has no tsift project root, so there is nothing to reap.
    SkippedNoProjectRoot,
    /// No `.tsift/gpu-lease.json`; tsift local-model leasing is not in use here.
    SkippedNoRegistry,
    /// `tsift` binary could not be spawned.
    SpawnFailed(String),
    /// Reap exited non-zero. Carries status code and stderr.
    NonZeroExit(Option<i32>, String),
}

/// `#kgleasereap`: best-effort automatic reclamation of crashed-session GPU
/// leases from closeout.
///
/// Runs `tsift local-model lease reap --unload-empty --json` under the resolved
/// project root when a lease registry is present. Failures are returned for
/// inspectability and logged here, but callers should treat this as best-effort.
pub fn reap_local_model_leases(file: &Path) -> ReapOutcome {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return ReapOutcome::SkippedNoProjectRoot;
    };
    if !project_root
        .join(agent_doc_lease::DEFAULT_LOCAL_MODEL_LEASE_REGISTRY_RELATIVE)
        .exists()
    {
        return ReapOutcome::SkippedNoRegistry;
    }
    let args = agent_doc_lease::local_model_reap_command_args(None, None);
    let mut cmd = std::process::Command::new("tsift");
    cmd.args(&args).current_dir(&project_root).arg("--json");
    match cmd.output() {
        Ok(output) if output.status.success() => {
            eprintln!("[hooks] tsift lease reap ok for {}", file.display());
            ReapOutcome::Reaped(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            eprintln!(
                "[hooks] tsift lease reap exited with code {:?}: {}",
                output.status.code(),
                stderr.trim()
            );
            ReapOutcome::NonZeroExit(output.status.code(), stderr)
        }
        Err(err) => {
            eprintln!("[hooks] tsift lease reap failed to spawn: {err}");
            ReapOutcome::SpawnFailed(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reap_skips_without_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();

        assert_eq!(
            reap_local_model_leases(&doc),
            ReapOutcome::SkippedNoRegistry
        );
    }
}
