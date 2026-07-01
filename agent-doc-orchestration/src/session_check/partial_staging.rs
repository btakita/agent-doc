use super::*;
use agent_doc_git::parse_porcelain_path;

pub(crate) fn check_partial_closeout_state_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };

    let content = std::fs::read_to_string(file)?;
    let open_backlog_ids = agent_doc_document::tracked_work_projection::open_backlog_ids(&content);
    let candidates = match agent_doc_turn::closeout_signal::partial_closeout_state_decision(
        agent_doc_turn::closeout_signal::PartialCloseoutStateEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.state
                == agent_doc_workflow::capture::CaptureState::Committed,
            response_body: &capture.response_body,
            directed_ids: &state.expect_done_or_gate_ids,
            pending_done_ids: &state.pending_done_ids,
            reaped_pending_ids: &state.reaped_pending_ids,
            open_backlog_ids: &open_backlog_ids,
        },
    ) {
        agent_doc_turn::closeout_signal::PartialCloseoutStateDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::PartialCloseoutStateDecision::Warn { candidate_ids } => {
            candidate_ids
        }
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "partial_closeout_state_guard_fired file={} candidates={}",
            file.display(),
            candidates.join(",")
        ),
    );

    Ok(agent_doc_workflow::session_check::partial_closeout_state_guard_result(&candidates))
}

#[derive(Debug, Clone)]
pub(crate) struct PartialStagingFinding {
    repo: std::path::PathBuf,
    committed_paths: Vec<String>,
    dirty_paths: Vec<String>,
    literals: Vec<String>,
}

/// `#partial-staging-closeout-guard`: a manual repo commit can accidentally
/// stage only the source half of a source+test change. Local verification then
/// passes against the dirty worktree while CI sees only the partial commit.
/// This guard is WARN-only and narrow: it requires a latest-commit source/test
/// path relationship plus overlapping changed string literals in tracked
/// uncommitted or staged companion changes.
pub(crate) fn check_partial_staging_closeout_guard(file: &Path) -> Result<GuardResult> {
    let findings = partial_staging_closeout_findings(file)?;
    if findings.is_empty() {
        return Ok(GuardResult::None);
    }

    for finding in findings.iter().take(3) {
        crate::ops_log::log_op(
            file,
            &format!(
                "partial_staging_closeout_guard_fired file={} repo={} committed_paths={} dirty_paths={} literals={}",
                file.display(),
                finding.repo.display(),
                finding.committed_paths.join(","),
                finding.dirty_paths.join(","),
                finding.literals.join("|")
            ),
        );
    }

    let workflow_findings = findings
        .iter()
        .map(
            |finding| agent_doc_workflow::session_check::PartialStagingCloseoutGuardFinding {
                repo: finding.repo.display().to_string(),
                committed_paths: finding.committed_paths.clone(),
                dirty_paths: finding.dirty_paths.clone(),
                literals: finding.literals.clone(),
            },
        )
        .collect::<Vec<_>>();

    Ok(
        agent_doc_workflow::session_check::partial_staging_closeout_guard_result(
            &workflow_findings,
        ),
    )
}

pub(crate) fn partial_staging_closeout_findings(file: &Path) -> Result<Vec<PartialStagingFinding>> {
    let mut findings = Vec::new();
    for repo in partial_staging_candidate_repos(file)? {
        if let Some(finding) = partial_staging_finding_for_repo(&repo)? {
            findings.push(finding);
        }
    }
    Ok(findings)
}

pub(crate) fn partial_staging_candidate_repos(file: &Path) -> Result<Vec<std::path::PathBuf>> {
    let start = if file.is_dir() {
        file
    } else {
        file.parent().unwrap_or_else(|| Path::new("."))
    };
    let Some(root) = git_toplevel(start)? else {
        return Ok(Vec::new());
    };

    let mut repos = vec![root.clone()];
    if let Some(status) = git_stdout(
        &root,
        &["status", "--porcelain=v1", "--ignore-submodules=none"],
    )? {
        for line in status.lines() {
            let Some(rel) = parse_porcelain_path(line) else {
                continue;
            };
            let candidate = root.join(rel);
            if !candidate.is_dir() {
                continue;
            }
            if let Some(subroot) = git_toplevel(&candidate)?
                && subroot != root
            {
                repos.push(subroot);
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}

pub(crate) fn partial_staging_finding_for_repo(
    repo: &Path,
) -> Result<Option<PartialStagingFinding>> {
    if git_stdout(repo, &["rev-parse", "--verify", "HEAD^"])?.is_none() {
        return Ok(None);
    }

    let committed_paths = git_name_lines(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=ACMRT",
            "HEAD^",
            "HEAD",
        ],
    )?;

    let mut dirty_paths = git_name_lines(repo, &["diff", "--name-only", "--diff-filter=ACMRT"])?;
    dirty_paths.extend(git_name_lines(
        repo,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMRT"],
    )?);

    let committed_diff =
        git_stdout(repo, &["diff", "--unified=0", "HEAD^", "HEAD"])?.unwrap_or_default();
    let mut dirty_diff = git_stdout(repo, &["diff", "--unified=0"])?.unwrap_or_default();
    if let Some(cached) = git_stdout(repo, &["diff", "--cached", "--unified=0"])? {
        if !dirty_diff.is_empty() && !cached.is_empty() {
            dirty_diff.push('\n');
        }
        dirty_diff.push_str(&cached);
    }

    let Some(finding) = agent_doc_diff::partial_staging_companion_finding(
        &committed_paths,
        &dirty_paths,
        &committed_diff,
        &dirty_diff,
    ) else {
        return Ok(None);
    };

    Ok(Some(PartialStagingFinding {
        repo: repo.to_path_buf(),
        committed_paths: finding.committed_paths,
        dirty_paths: finding.dirty_paths,
        literals: finding.literals,
    }))
}

pub(crate) fn git_toplevel(start: &Path) -> Result<Option<std::path::PathBuf>> {
    let Some(stdout) = git_stdout(start, &["rev-parse", "--show-toplevel"])? else {
        return Ok(None);
    };
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(std::path::PathBuf::from(trimmed)))
}

pub(crate) fn git_name_lines(repo: &Path, args: &[&str]) -> Result<Vec<String>> {
    let Some(stdout) = git_stdout(repo, args)? else {
        return Ok(Vec::new());
    };
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn git_stdout(repo: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?} in {}", args, repo.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}
