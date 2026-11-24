//! Preflight's reads of the controller's per-revision `repair` and
//! `stuck_cycle_detection` verdicts (`#q-1d2a127f32aa`), and the controller-side
//! refresh that computes the stuck verdict.
//!
//! See [`agent_doc_controller_io::preflight_revision_cache`] for the cache.
//! Preflight asks the running controller once per phase. A clean repair
//! verdict (plus the cheap ops-log commit-missing check, which is not part of
//! the projection) skips `enforce_cycle_completion`; a quiet stuck verdict for
//! the current HEAD skips the stuck sweep. Anything else, including no
//! controller, runs the phase inline as before.
//!
//! `AGENT_DOC_PREFLIGHT_REVISION_CACHE=off` forces the inline phases.

use std::path::Path;
use std::time::Instant;

use agent_doc_controller_io::preflight_revision_cache::{
    PreflightRevisionVerdicts, StuckSweepVerdict, closeout_revision, record_stuck_verdict,
};
use anyhow::Result;

/// `off` makes preflight ignore the controller's verdicts.
pub const PREFLIGHT_REVISION_CACHE_ENV: &str = "AGENT_DOC_PREFLIGHT_REVISION_CACHE";

fn cache_disabled() -> bool {
    std::env::var(PREFLIGHT_REVISION_CACHE_ENV)
        .is_ok_and(|value| value.trim().eq_ignore_ascii_case("off"))
}

fn one_line(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .replace('\n', " | ")
        .chars()
        .take(200)
        .collect()
}

/// Ask the running controller for `file`'s verdicts. `None` when the cache is
/// disabled or no controller answers (the caller runs its phase inline).
pub fn fetch_verdicts(file: &Path, phase: &str) -> Option<PreflightRevisionVerdicts> {
    fetch_verdicts_with(file, phase, |file| {
        agent_doc_controller_io::project_controller::request_preflight_revision_verdicts(file)
    })
}

/// [`fetch_verdicts`] with the controller read injected.
pub fn fetch_verdicts_with(
    file: &Path,
    phase: &str,
    read: impl FnOnce(&Path) -> Result<PreflightRevisionVerdicts>,
) -> Option<PreflightRevisionVerdicts> {
    if cache_disabled() {
        return None;
    }
    let started = Instant::now();
    match read(file) {
        Ok(verdicts) => Some(verdicts),
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_revision_cache_unavailable file={} phase={phase} elapsed_ms={} error={} (#q-1d2a127f32aa)",
                    file.display(),
                    started.elapsed().as_millis(),
                    one_line(&error),
                ),
            );
            None
        }
    }
}

/// Whether preflight's cycle-completion recovery may be skipped: the
/// controller's projection is clean and the ops log records no response write
/// whose commit is missing (that evidence lives outside the projection).
pub fn repair_served_by_cache(file: &Path, verdicts: Option<&PreflightRevisionVerdicts>) -> bool {
    let Some(verdicts) = verdicts else {
        return false;
    };
    let commit_missing = agent_doc_ops_log_io::detect_write_completed_commit_missing(file);
    let served = verdicts.repair_clean() && matches!(commit_missing, Ok(None));
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "preflight_revision_cache_repair file={} served={served} cycle_open={:?} pending_write={} pending_journal={} pending_compact={} commit_missing={} revision={} (#q-1d2a127f32aa)",
            file.display(),
            verdicts.cycle_open,
            verdicts.pending_document_write,
            verdicts.pending_write_journal,
            verdicts.pending_compact_projection,
            match &commit_missing {
                Ok(None) => "none",
                Ok(Some(_)) => "found",
                Err(_) => "unreadable",
            },
            short(&verdicts.closeout_revision),
        ),
    );
    served
}

/// Whether the stuck sweep may be skipped: the controller recorded a quiet
/// verdict for the current closeout revision against the current HEAD.
pub fn stuck_served_by_cache(file: &Path, verdicts: Option<&PreflightRevisionVerdicts>) -> bool {
    let Some(verdicts) = verdicts else {
        return false;
    };
    let head = head_oid(file);
    let served = head
        .as_deref()
        .is_some_and(|head| verdicts.stuck_quiet_at(head));
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "preflight_revision_cache_stuck file={} served={served} cached={} revision={} (#q-1d2a127f32aa)",
            file.display(),
            match &verdicts.stuck {
                None => "miss".to_string(),
                Some(stuck) => format!(
                    "quiet={}@{}{}",
                    stuck.quiet,
                    short(&stuck.head_oid),
                    if head.as_deref() == Some(stuck.head_oid.as_str()) {
                        ""
                    } else {
                        ":head_moved"
                    }
                ),
            },
            short(&verdicts.closeout_revision),
        ),
    );
    served
}

fn short(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

fn head_oid(file: &Path) -> Option<String> {
    let dir = file.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    agent_doc_git_io::revision::rev_parse(dir, "HEAD")
        .ok()
        .flatten()
}

/// Controller-side refresh (the per-document worker of
/// `agent_doc_controller_io::document_maintenance::preflight_revision_cache`):
/// compute the stuck-sweep verdict for the live closeout projection against
/// the current HEAD and record it under the projection's revision.
pub fn refresh_preflight_revision_cache(file: &Path) -> Result<()> {
    let started = Instant::now();
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(head) = head_oid(&canonical) else {
        return Ok(());
    };
    let closeout = agent_doc_cycle_state_io::load_document_closeout_projection(&canonical)?;
    let revision = closeout_revision(closeout.as_ref());
    let quiet = match closeout.as_ref() {
        None => true,
        Some(closeout) => {
            agent_doc_flow_io::closeout::stuck_sweep_is_quiet(&canonical, closeout, &head)?
        }
    };
    record_stuck_verdict(
        &agent_doc_hash::document_id_for_path(&canonical),
        &revision,
        StuckSweepVerdict {
            head_oid: head.clone(),
            quiet,
        },
    );
    agent_doc_ops_log_io::log_op(
        &canonical,
        &format!(
            "preflight_revision_cache_refreshed file={} revision={} head={} stuck_quiet={quiet} elapsed_ms={} (#q-1d2a127f32aa)",
            canonical.display(),
            short(&revision),
            short(&head),
            started.elapsed().as_millis(),
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdicts(cycle_open: Option<bool>) -> PreflightRevisionVerdicts {
        PreflightRevisionVerdicts {
            closeout_revision: "rev".to_string(),
            cycle_open,
            pending_document_write: false,
            pending_write_journal: false,
            pending_compact_projection: false,
            stuck: None,
        }
    }

    fn doc_in_git_repo(dir: &Path) -> std::path::PathBuf {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("task.md");
        std::fs::write(&doc, "# Task\n").unwrap();
        run(&["init", "-q"]);
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "add",
            "task.md",
        ]);
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "init",
        ]);
        doc
    }

    #[test]
    fn a_clean_projection_serves_the_repair_phase() {
        let dir = tempfile::tempdir().unwrap();
        let doc = doc_in_git_repo(dir.path());
        assert!(repair_served_by_cache(&doc, Some(&verdicts(Some(false)))));
        assert!(!repair_served_by_cache(&doc, Some(&verdicts(Some(true)))));
        assert!(!repair_served_by_cache(&doc, Some(&verdicts(None))));
        assert!(
            !repair_served_by_cache(&doc, None),
            "no controller: run inline"
        );
    }

    #[test]
    fn a_missing_commit_in_the_ops_log_is_never_served_from_the_projection() {
        let dir = tempfile::tempdir().unwrap();
        let doc = doc_in_git_repo(dir.path());
        agent_doc_ops_log_io::log_op(
            &doc,
            &format!(
                "ipc_write_consumed file={}",
                doc.canonicalize().unwrap().display()
            ),
        );
        assert!(
            agent_doc_ops_log_io::detect_write_completed_commit_missing(&doc)
                .unwrap()
                .is_some(),
            "fixture: the ops log records a write whose commit is missing"
        );
        assert!(!repair_served_by_cache(&doc, Some(&verdicts(Some(false)))));
    }

    #[test]
    fn a_quiet_stuck_verdict_serves_only_its_own_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = doc_in_git_repo(dir.path());
        let head = head_oid(&doc).expect("head");
        let mut current = verdicts(Some(false));
        current.stuck = Some(StuckSweepVerdict {
            head_oid: head.clone(),
            quiet: true,
        });
        assert!(stuck_served_by_cache(&doc, Some(&current)));

        current.stuck = Some(StuckSweepVerdict {
            head_oid: "0".repeat(40),
            quiet: true,
        });
        assert!(!stuck_served_by_cache(&doc, Some(&current)), "HEAD moved");

        current.stuck = Some(StuckSweepVerdict {
            head_oid: head,
            quiet: false,
        });
        assert!(!stuck_served_by_cache(&doc, Some(&current)), "not quiet");
        assert!(!stuck_served_by_cache(&doc, None));
    }

    #[test]
    fn an_unavailable_controller_reads_no_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let doc = doc_in_git_repo(dir.path());
        assert!(
            fetch_verdicts_with(&doc, "repair", |_| anyhow::bail!("no project controller"))
                .is_none()
        );
        assert_eq!(
            fetch_verdicts_with(&doc, "repair", |_| Ok(verdicts(Some(false)))),
            Some(verdicts(Some(false)))
        );
    }

    #[test]
    fn a_refresh_without_a_projection_records_a_quiet_verdict_for_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = doc_in_git_repo(dir.path());
        refresh_preflight_revision_cache(&doc).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let cached = agent_doc_controller_io::preflight_revision_cache::cached_stuck_verdict(
            &agent_doc_hash::document_id_for_path(&canonical),
            &closeout_revision(None),
        )
        .expect("recorded");
        assert!(cached.quiet);
        assert_eq!(Some(cached.head_oid), head_oid(&doc));
    }
}
