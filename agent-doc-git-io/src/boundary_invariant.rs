use anyhow::{Context, Result};
use std::path::Path;

pub trait BoundaryInvariantEffects {
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn log_op(&self, file: &Path, message: &str);
}

/// Enforce the single-boundary invariant on the just-committed HEAD artifact.
///
/// The pre-stage snapshot collapse should already guarantee this, but a
/// previously-accreted blob is caught here and self-healed with a binary-owned
/// follow-up collapse commit. This never races the live editor: it re-collapses
/// committed content, not the editor buffer.
pub fn enforce_committed_single_boundary_invariant(
    effects: &impl BoundaryInvariantEffects,
    file: &Path,
    git_root: &Path,
    resolved: &Path,
) -> Result<()> {
    let Some(head_blob) = crate::revision::show_head(file).with_context(|| {
        format!(
            "boundary_invariant_violation phase=post_commit_inspect file={}",
            file.display()
        )
    })?
    else {
        return Ok(());
    };
    let boundary_count = head_blob
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if boundary_count <= 1 {
        return Ok(());
    }
    eprintln!(
        "[commit] boundary_invariant_violation: committed HEAD carries {} agent:boundary markers (expected 1) - self-healing collapse",
        boundary_count
    );
    effects.log_op(
        file,
        &format!(
            "boundary_invariant_violation phase=post_commit file={} committed_boundaries={}",
            file.display(),
            boundary_count
        ),
    );
    let collapsed = agent_doc_template::reposition_boundary_to_end_clean(&head_blob);
    let collapsed_count = collapsed
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if collapsed == head_blob || collapsed_count != 1 {
        anyhow::bail!(
            "boundary_invariant_violation phase=post_commit_collapse file={} \
             committed_boundaries={} collapsed_boundaries={}",
            file.display(),
            boundary_count,
            collapsed_count
        );
    }
    effects.save_snapshot(file, &collapsed).with_context(|| {
        format!(
            "boundary_invariant_violation phase=post_commit_snapshot file={}",
            file.display()
        )
    })?;
    crate::transaction::stage_and_commit_once(
        git_root,
        resolved,
        Some(collapsed.as_str()),
        "agent-doc: collapse accreted agent:boundary markers (#boundaryaccum1)",
    )
    .map_err(|err| {
        anyhow::anyhow!(
            "boundary_invariant_violation phase=post_commit_selfheal_commit file={} error={err:?}",
            file.display()
        )
    })?;

    let verified = crate::revision::show_head(file)
        .with_context(|| {
            format!(
                "boundary_invariant_violation phase=post_commit_verify file={}",
                file.display()
            )
        })?
        .with_context(|| {
            format!(
                "boundary_invariant_violation phase=post_commit_verify file={} missing_head_blob",
                file.display()
            )
        })?;
    let verified_count = verified
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    anyhow::ensure!(
        verified_count == 1,
        "boundary_invariant_violation phase=post_commit_verify file={} \
         committed_boundaries={verified_count}",
        file.display()
    );

    eprintln!("[commit] boundary_invariant self-heal collapse committed and verified");
    effects.log_op(
        file,
        &format!(
            "boundary_invariant_selfheal_committed file={} verified_boundaries={verified_count}",
            file.display()
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEffects {
        snapshot: Mutex<Option<String>>,
        logs: Mutex<Vec<String>>,
    }

    impl BoundaryInvariantEffects for RecordingEffects {
        fn save_snapshot(&self, _file: &Path, content: &str) -> Result<()> {
            *self.snapshot.lock().unwrap() = Some(content.to_string());
            Ok(())
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.lock().unwrap().push(message.to_string());
        }
    }

    fn init_repo_with_accreted_boundaries() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Agent Doc Test"],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let file = root.join("session.md");
        std::fs::write(
            &file,
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "### Re: one\n\nFirst.\n",
                "<!-- agent:boundary:stale-one -->\n",
                "### Re: two\n\nSecond.\n",
                "<!-- agent:boundary:stale-two -->\n",
                "<!-- agent:boundary:stale-three -->\n",
                "<!-- /agent:exchange -->\n",
            ),
        )
        .unwrap();
        assert!(
            Command::new("git")
                .current_dir(root)
                .args(["add", "session.md"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .current_dir(root)
                .args(["commit", "-m", "accreted", "--no-verify"])
                .status()
                .unwrap()
                .success()
        );
        (dir, file)
    }

    #[test]
    fn post_commit_self_heal_re_reads_head_and_proves_one_boundary() {
        let (dir, file) = init_repo_with_accreted_boundaries();
        let effects = RecordingEffects::default();

        enforce_committed_single_boundary_invariant(&effects, &file, dir.path(), &file).unwrap();

        let head = crate::revision::show_head(&file).unwrap().unwrap();
        assert_eq!(
            head.matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
                .count(),
            1,
            "{head}"
        );
        let snapshot = effects.snapshot.lock().unwrap().clone().unwrap();
        assert_eq!(
            snapshot
                .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
                .count(),
            1,
            "{snapshot}"
        );
        assert!(
            effects
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("verified_boundaries=1"))
        );
    }

    #[test]
    fn post_commit_self_heal_fails_loudly_when_follow_up_commit_cannot_land() {
        let (_dir, file) = init_repo_with_accreted_boundaries();
        let effects = RecordingEffects::default();
        let invalid_root = file.join("not-a-repository");

        let error =
            enforce_committed_single_boundary_invariant(&effects, &file, &invalid_root, &file)
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("phase=post_commit_selfheal_commit"),
            "{error:#}"
        );
    }
}
