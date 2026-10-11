use std::path::Path;
use std::process::Output;

use anyhow::{Error, anyhow};

pub trait CommitResultReportingEffects {
    fn log_op(&self, file: &Path, message: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitCommandOutcome {
    Success,
    Failed,
    Error,
    /// `#splicectlcommit`: the document is git-ignored AND declares itself a
    /// scratch/proof document (`agent_doc_scratch: true`). Nothing was staged
    /// or committed — exactly as for any ignored path — but the skip is the
    /// declared, expected terminal of that document, so it is reported as a
    /// clean skip instead of a `commit_error`.
    SkippedIgnoredScratch,
}

pub fn ignored_untracked_path_error(
    effects: &impl CommitResultReportingEffects,
    file: &Path,
    path: &str,
) -> Error {
    eprintln!(
        "[commit] skipped ignored untracked path {} (matched .gitignore); not staging",
        path
    );
    effects.log_op(
        file,
        &format!(
            "commit_skipped_ignored_path file={} rel_path={}",
            file.display(),
            path
        ),
    );
    anyhow!(
        "refusing to commit ignored untracked path {} (matched .gitignore)",
        path
    )
}

/// Report the declared-scratch variant of the ignored-path refusal
/// (`#splicectlcommit`). The refusal itself is identical — the ignored file is
/// never staged and the cycle closes as `commit_refused_ignored_path` — only the
/// receipt differs: `commit_skipped_ignored_scratch outcome=clean_skip` instead
/// of `commit_skipped_ignored_path` + `commit_error`. A document without the
/// declaration keeps the refusal receipts unchanged.
pub fn report_ignored_scratch_skip(
    effects: &impl CommitResultReportingEffects,
    file: &Path,
    path: &str,
) -> CommitCommandOutcome {
    eprintln!(
        "[commit] {} is a declared scratch document (agent_doc_scratch: true) ignored by .gitignore; not staging, no git commit",
        path
    );
    effects.log_op(
        file,
        &format!(
            "commit_skipped_ignored_scratch file={} rel_path={} outcome=clean_skip",
            file.display(),
            path
        ),
    );
    CommitCommandOutcome::SkippedIgnoredScratch
}

pub fn report_commit_output(
    effects: &impl CommitResultReportingEffects,
    file: &Path,
    commit_output: Result<&Output, &Error>,
) -> CommitCommandOutcome {
    match commit_output {
        Ok(output) => {
            emit_commit_summary_lines(output);
            if output.status.success() {
                CommitCommandOutcome::Success
            } else {
                effects.log_op(
                    file,
                    &format!(
                        "commit_failed file={} exit_code={}",
                        file.display(),
                        output.status.code().unwrap_or(-1)
                    ),
                );
                CommitCommandOutcome::Failed
            }
        }
        Err(err) => {
            effects.log_op(
                file,
                &format!("commit_error file={} err={}", file.display(), err),
            );
            CommitCommandOutcome::Error
        }
    }
}

fn emit_commit_summary_lines(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.contains(']') {
            eprintln!("{}", line);
        }
    }
}
