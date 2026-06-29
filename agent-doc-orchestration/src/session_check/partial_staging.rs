use super::*;

pub(crate) fn check_partial_closeout_state_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() || state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-partial-closeout-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let text = response_text_for_guards(&capture.response_body);
    let lower = text.to_ascii_lowercase();
    if !(text_has_shipped_signal(&lower) && text_has_partial_remaining_signal(&lower)) {
        return Ok(GuardResult::None);
    }

    // Only directed ids that are still open in agent:backlog and not resolved
    // (done/reaped) this cycle are candidates for next-phase narrowing.
    let resolved: std::collections::HashSet<String> = state
        .pending_done_ids
        .iter()
        .chain(state.reaped_pending_ids.iter())
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();

    let mut candidates: Vec<String> = Vec::new();
    for id in state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if resolved.contains(&id) || !open_backlog.contains(&id) {
            continue;
        }
        if !candidates.iter().any(|existing| existing == &id) {
            candidates.push(id);
        }
    }
    if candidates.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = candidates
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let edit_hint = candidates
        .iter()
        .map(|id| format!("--pending-edit \"{}=<remaining next-phase scope>\"", id))
        .collect::<Vec<_>>()
        .join(" ");

    crate::ops_log::log_op(
        file,
        &format!(
            "partial_closeout_state_guard_fired file={} candidates={}",
            file.display(),
            candidates.join(",")
        ),
    );

    Ok(GuardResult::Warn(vec![
        format!(
            "[session-check] warn: partial `do [#id]` closeout — work shipped (committed + pushed) but the response says live deploy/sync/verification work remains, yet tracked target {} still carries its original full-task text in agent:backlog",
            ids
        ),
        format!(
            "[session-check] hint: narrow the backlog item + queue head to the next phase with `{}` (or `--pending-gate <id>` if only review/external validation remains), or add `<!-- no-partial-closeout-guard -->` when it is already narrowed",
            edit_hint
        ),
    ]))
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

    let mut lines = Vec::new();
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
        lines.push(format!(
            "[session-check] warn: possible partial staging closeout in {} — latest commit changed {}, but tracked uncommitted companion changes remain in {} with overlapping changed string literal(s): {}.",
            finding.repo.display(),
            preview_items(&finding.committed_paths, 4),
            preview_items(&finding.dirty_paths, 4),
            preview_items(&finding.literals, 3)
        ));
    }
    if findings.len() > 3 {
        lines.push(format!(
            "[session-check] warn: {} additional partial staging candidate(s) omitted.",
            findings.len() - 3
        ));
    }
    lines.push(
        "[session-check] hint: commit the companion changes, revert them, or rerun verification against the committed tree before reporting CI-ready closeout."
            .to_string(),
    );
    Ok(GuardResult::Warn(lines))
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
    )?
    .into_iter()
    .filter(|path| agent_doc_diff::is_partial_staging_relevant_path(path))
    .collect::<Vec<_>>();
    if committed_paths.is_empty() {
        return Ok(None);
    }

    let mut dirty_paths = git_name_lines(repo, &["diff", "--name-only", "--diff-filter=ACMRT"])?;
    dirty_paths.extend(git_name_lines(
        repo,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMRT"],
    )?);
    dirty_paths = dirty_paths
        .into_iter()
        .filter(|path| agent_doc_diff::is_partial_staging_relevant_path(path))
        .collect::<Vec<_>>();
    dirty_paths.sort();
    dirty_paths.dedup();
    if dirty_paths.is_empty()
        || !agent_doc_diff::partial_staging_paths_look_related(&committed_paths, &dirty_paths)
    {
        return Ok(None);
    }

    let committed_diff =
        git_stdout(repo, &["diff", "--unified=0", "HEAD^", "HEAD"])?.unwrap_or_default();
    let mut dirty_diff = git_stdout(repo, &["diff", "--unified=0"])?.unwrap_or_default();
    if let Some(cached) = git_stdout(repo, &["diff", "--cached", "--unified=0"])? {
        if !dirty_diff.is_empty() && !cached.is_empty() {
            dirty_diff.push('\n');
        }
        dirty_diff.push_str(&cached);
    }

    let committed_literals = agent_doc_diff::extract_changed_string_literals(&committed_diff);
    let dirty_literals = agent_doc_diff::extract_changed_string_literals(&dirty_diff);
    let mut overlap = committed_literals
        .intersection(&dirty_literals)
        .cloned()
        .collect::<Vec<_>>();
    overlap.sort();
    if overlap.is_empty() {
        return Ok(None);
    }

    Ok(Some(PartialStagingFinding {
        repo: repo.to_path_buf(),
        committed_paths,
        dirty_paths,
        literals: overlap,
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

pub(crate) fn parse_porcelain_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let status = &line[..2];
    if status == "??" {
        return None;
    }
    let raw = line[3..].trim();
    if raw.is_empty() {
        return None;
    }
    let path = raw.rsplit(" -> ").next().unwrap_or(raw).trim();
    Some(path.trim_matches('"').to_string())
}

pub(crate) fn preview_items(items: &[String], limit: usize) -> String {
    let mut preview = items
        .iter()
        .take(limit)
        .map(|item| format!("`{}`", item))
        .collect::<Vec<_>>();
    if items.len() > limit {
        preview.push(format!("...(+{})", items.len() - limit));
    }
    preview.join(", ")
}
