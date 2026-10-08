//! Best-effort publication of a completed closeout commit.
//!
//! The commit transaction is authoritative. This adapter runs only after that
//! transaction has completed and returns a typed advisory outcome; a remote
//! failure must never turn the committed cycle back into a failed closeout.

use std::path::Path;
use std::process::Command;

use agent_doc_frontmatter::project_config::CommitPushMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamTarget {
    pub remote: String,
    pub branch_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamObservation {
    Configured(UpstreamTarget),
    DetachedHead,
    Missing { branch: String },
    Invalid { detail: String },
    Error { phase: &'static str, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostCloseoutPushDecision {
    Disabled,
    Push(UpstreamTarget),
    Skip { reason: &'static str, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostCloseoutPushOutcome {
    Disabled,
    Pushed(UpstreamTarget),
    Skipped { reason: &'static str, detail: String },
    Rejected { target: UpstreamTarget, detail: String },
    Failed { phase: &'static str, detail: String },
}

/// Own the publication decision independently of Git process execution.
pub fn decide_post_closeout_push(
    mode: CommitPushMode,
    upstream: UpstreamObservation,
) -> PostCloseoutPushDecision {
    if mode == CommitPushMode::Off {
        return PostCloseoutPushDecision::Disabled;
    }

    match upstream {
        UpstreamObservation::Configured(target) => PostCloseoutPushDecision::Push(target),
        UpstreamObservation::DetachedHead => PostCloseoutPushDecision::Skip {
            reason: "detached_head",
            detail: "HEAD is detached; no branch upstream can be selected".to_string(),
        },
        UpstreamObservation::Missing { branch } => PostCloseoutPushDecision::Skip {
            reason: "missing_upstream",
            detail: format!("branch {branch} has no configured upstream"),
        },
        UpstreamObservation::Invalid { detail } => PostCloseoutPushDecision::Skip {
            reason: "invalid_upstream",
            detail,
        },
        UpstreamObservation::Error { phase, detail } => PostCloseoutPushDecision::Skip {
            reason: phase,
            detail,
        },
    }
}

/// Run the optional one-shot publication effect after closeout is durably
/// committed. Every non-success result is typed and advisory; callers must not
/// roll back or invalidate the committed cycle.
pub fn push_after_closeout(git_root: &Path, mode: CommitPushMode) -> PostCloseoutPushOutcome {
    if mode == CommitPushMode::Off {
        return PostCloseoutPushOutcome::Disabled;
    }

    match decide_post_closeout_push(mode, observe_upstream(git_root)) {
        PostCloseoutPushDecision::Disabled => PostCloseoutPushOutcome::Disabled,
        PostCloseoutPushDecision::Skip { reason, detail } => {
            PostCloseoutPushOutcome::Skipped { reason, detail }
        }
        PostCloseoutPushDecision::Push(target) => push_target(git_root, target),
    }
}

fn push_target(git_root: &Path, target: UpstreamTarget) -> PostCloseoutPushOutcome {
    let refspec = format!("HEAD:{}", target.branch_ref);
    let output = Command::new("git")
        .current_dir(git_root)
        .args([
            "push",
            "--porcelain",
            "--no-force",
            "--no-force-with-lease",
            "--no-force-if-includes",
            "--no-all",
            "--no-mirror",
            "--no-tags",
            "--no-follow-tags",
            "--recurse-submodules=no",
            "--",
        ])
        .arg(&target.remote)
        .arg(&refspec)
        .output();
    match output {
        Ok(output) if output.status.success() => PostCloseoutPushOutcome::Pushed(target),
        Ok(output) => PostCloseoutPushOutcome::Rejected {
            target,
            detail: agent_doc_git::render_git_process_output(&output),
        },
        Err(error) => PostCloseoutPushOutcome::Failed {
            phase: "launch",
            detail: error.to_string(),
        },
    }
}

fn observe_upstream(git_root: &Path) -> UpstreamObservation {
    let branch = match git_stdout(git_root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(Some(branch)) => branch,
        Ok(None) => return UpstreamObservation::DetachedHead,
        Err(detail) => {
            return UpstreamObservation::Error {
                phase: "observe_branch",
                detail,
            };
        }
    };

    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let remote = match git_stdout(git_root, &["config", "--get", &remote_key]) {
        Ok(Some(remote)) => remote,
        Ok(None) => return UpstreamObservation::Missing { branch },
        Err(detail) => {
            return UpstreamObservation::Error {
                phase: "observe_remote",
                detail,
            };
        }
    };
    let branch_ref = match git_stdout(git_root, &["config", "--get", &merge_key]) {
        Ok(Some(branch_ref)) => branch_ref,
        Ok(None) => return UpstreamObservation::Missing { branch },
        Err(detail) => {
            return UpstreamObservation::Error {
                phase: "observe_merge_ref",
                detail,
            };
        }
    };

    if remote.trim().is_empty() || !branch_ref.starts_with("refs/heads/") {
        return UpstreamObservation::Invalid {
            detail: format!("remote={remote:?} merge_ref={branch_ref:?}"),
        };
    }
    UpstreamObservation::Configured(UpstreamTarget { remote, branch_ref })
}

fn git_stdout(git_root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok((!value.is_empty()).then_some(value));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    Err(agent_doc_git::render_git_process_output(&output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            agent_doc_git::render_git_process_output(&output)
        );
    }

    fn git_value(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn init_repo(root: &Path) {
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        fs::write(root.join("doc.md"), "initial\n").unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", "initial"]);
    }

    fn commit(root: &Path, content: &str, message: &str) {
        fs::write(root.join("doc.md"), content).unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", message]);
    }

    #[test]
    fn policy_transition_table_is_exhaustive_for_non_push_states() {
        let target = UpstreamTarget {
            remote: "origin".into(),
            branch_ref: "refs/heads/main".into(),
        };
        assert_eq!(
            decide_post_closeout_push(
                CommitPushMode::Off,
                UpstreamObservation::Configured(target.clone())
            ),
            PostCloseoutPushDecision::Disabled
        );
        assert_eq!(
            decide_post_closeout_push(
                CommitPushMode::FfOnly,
                UpstreamObservation::Configured(target.clone())
            ),
            PostCloseoutPushDecision::Push(target)
        );
        for observation in [
            UpstreamObservation::DetachedHead,
            UpstreamObservation::Missing {
                branch: "main".into(),
            },
            UpstreamObservation::Invalid {
                detail: "bad ref".into(),
            },
            UpstreamObservation::Error {
                phase: "observe_branch",
                detail: "git unavailable".into(),
            },
        ] {
            assert!(matches!(
                decide_post_closeout_push(CommitPushMode::FfOnly, observation),
                PostCloseoutPushDecision::Skip { .. }
            ));
        }
    }

    #[test]
    fn disabled_policy_performs_no_git_observation() {
        let missing = Path::new("/definitely/not/a/repository");
        assert_eq!(
            push_after_closeout(missing, CommitPushMode::Off),
            PostCloseoutPushOutcome::Disabled
        );
    }

    #[test]
    fn ff_only_push_advances_only_the_configured_upstream_branch() {
        let remote_dir = tempfile::TempDir::new().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        let local_dir = tempfile::TempDir::new().unwrap();
        init_repo(local_dir.path());
        git(
            local_dir.path(),
            &["remote", "add", "origin", remote_dir.path().to_str().unwrap()],
        );
        git(local_dir.path(), &["push", "-u", "origin", "main"]);
        commit(local_dir.path(), "published\n", "closeout");
        let expected = git_value(local_dir.path(), &["rev-parse", "HEAD"]);

        let outcome = push_after_closeout(local_dir.path(), CommitPushMode::FfOnly);

        assert!(matches!(outcome, PostCloseoutPushOutcome::Pushed(_)));
        assert_eq!(
            git_value(remote_dir.path(), &["rev-parse", "refs/heads/main"]),
            expected
        );
    }

    #[test]
    fn rejected_non_fast_forward_is_advisory_and_never_rewrites_remote() {
        let remote_dir = tempfile::TempDir::new().unwrap();
        git(remote_dir.path(), &["init", "--bare"]);
        let local_dir = tempfile::TempDir::new().unwrap();
        init_repo(local_dir.path());
        git(
            local_dir.path(),
            &["remote", "add", "origin", remote_dir.path().to_str().unwrap()],
        );
        git(local_dir.path(), &["push", "-u", "origin", "main"]);

        let peer_dir = tempfile::TempDir::new().unwrap();
        git(
            peer_dir.path(),
            &["clone", remote_dir.path().to_str().unwrap(), "."],
        );
        git(peer_dir.path(), &["config", "user.email", "peer@example.com"]);
        git(peer_dir.path(), &["config", "user.name", "Peer"]);
        commit(peer_dir.path(), "remote-ahead\n", "peer");
        git(peer_dir.path(), &["push", "origin", "main"]);
        let remote_before = git_value(remote_dir.path(), &["rev-parse", "refs/heads/main"]);

        commit(local_dir.path(), "local-diverged\n", "closeout");
        let local_head = git_value(local_dir.path(), &["rev-parse", "HEAD"]);
        let outcome = push_after_closeout(local_dir.path(), CommitPushMode::FfOnly);

        assert!(matches!(outcome, PostCloseoutPushOutcome::Rejected { .. }));
        assert_eq!(
            git_value(remote_dir.path(), &["rev-parse", "refs/heads/main"]),
            remote_before
        );
        assert_eq!(git_value(local_dir.path(), &["rev-parse", "HEAD"]), local_head);
    }

    #[test]
    fn missing_upstream_is_an_advisory_skip() {
        let local_dir = tempfile::TempDir::new().unwrap();
        init_repo(local_dir.path());

        assert!(matches!(
            push_after_closeout(local_dir.path(), CommitPushMode::FfOnly),
            PostCloseoutPushOutcome::Skipped {
                reason: "missing_upstream",
                ..
            }
        ));
    }
}
