//! # Module: git
//!
//! ## Spec
//! - `commit(file)`: stages and commits a session document with an auto-generated
//!   `agent-doc(<stem>): <timestamp>` message, skipping hooks (`--no-verify`).  Relative paths are
//!   resolved against the git superproject root first, then the toplevel.  When a snapshot exists,
//!   a CLEAN copy of the snapshot (with all ` (HEAD)` heading suffixes stripped via
//!   `strip_head_markers`) is staged via `git hash-object + update-index` so the working tree is
//!   not staged directly. Narrow agent-owned snapshot drift can be absorbed first; plain user
//!   keystrokes typed after the snapshot was taken remain uncommitted (green gutter).
//!   Narrow exception: when the working tree is ahead of the snapshot due to a missed
//!   agent-doc-style status mutation and/or exchange append and/or pending mutation, `commit`
//!   first absorbs that live document state into the snapshot, then stages it. Plain user prompts
//!   are not absorbed.
//!   Falls back to `git add -f` when hash-object fails.  If the staged snapshot already matches
//!   `HEAD`, `commit` closes the cycle as an already-committed no-op instead of logging a false
//!   `commit_failed`. After a successful commit, post-commit cleanup collapses boundary churn in
//!   the snapshot and fires an IPC reposition signal (`try_ipc_reposition_boundary`) so the IDE
//!   plugin can normalize the working tree to the same clean shape as the committed blob. Without
//!   a live listener, the file is rewritten locally to that same clean shape. Returns `true` when
//!   a git commit was created and `false` when there was nothing new to commit.
//! - `show_head(file)`: returns the file content from `HEAD` as `Some(String)`, or `None` if not
//!   tracked.
//! - `commit_with_outcome(file)`: same as `commit`, but also reports whether the
//!   VCS refresh signal was available and successfully written after the commit.
//! - `verify_snapshot_committed(file)`: verifies that the current snapshot for `file` is
//!   committed in its owning git root (narrowed to submodule when applicable). Compares the
//!   snapshot content (modulo transient markers) against `git show HEAD:<file>`. Returns
//!   `Committed`, `SnapshotDiffersFromHead`, `NoSnapshot`, `NoHead`, or `NotInGitRepo`.
//! - `is_submodule_pointer_stale(file)`: checks whether the parent repo's submodule pointer
//!   needs updating for a file in a submodule. Returns `true` when the submodule is dirty
//!   in the parent diff.
//! - `last_commit_mtime(file)`: returns the author timestamp of the most recent commit touching the
//!   file, or `None` if none exists.
//! - `create_branch(file)`: creates and checks out `agent-doc/<stem>`, or switches to it if it
//!   already exists.
//! - `squash_session(file)`: soft-resets to before the first `agent-doc` commit touching the file
//!   and recommits as a single squashed commit.
//! - `strip_head_markers` (private): strips ` (HEAD)` suffix from markdown headings and bold-text
//!   pseudo-headers in the commit-staging path.  `(HEAD)` is treated as a transient artifact and
//!   must never appear in the committed blob.
//! - `strip_guard_markers` (private): strips `<!-- no-pending-capture -->` and
//!   `<!-- no-pending-done-guard -->` from the commit-staging path.  These are ephemeral
//!   per-cycle signals for `session-check`; the check reads from the capture file, not
//!   the committed document.  Post-commit cleanup also strips them from the snapshot and
//!   working-tree file.
//!
//! ## Agentic Contracts
//! - `commit` never modifies the working tree file directly; all staging is done through the git
//!   index.  The only disk write is to the snapshot file.
//! - `commit` captures all git stdout to stderr so callers that reserve stdout for JSON (e.g.,
//!   `preflight`) are not polluted.
//! - All public functions resolve paths relative to the superproject root when running inside a
//!   submodule, so git commands always run in the correct repo.
//! - `show_head` and `last_commit_mtime` return `Ok(None)` (not `Err`) when the file has no git
//!   history.
//! - Safe out-of-band absorb is narrow: only component-local drift that leaves the redacted
//!   document structure unchanged and looks like an agent-owned `status` change and/or a
//!   `### Re:` response-block insertion and/or a pending-ID superset is absorbed. Free-form user
//!   edits remain uncommitted. Historical response-block insertions that are already committed in
//!   `HEAD` may also repair the snapshot even when they are no longer appended at the tail.
//!
//! ## Evals
//! - strip_head_markers_from_headings: heading lines with ` (HEAD)` suffix → suffix removed; non-heading lines unchanged
//! - strip_head_markers_preserves_non_heading_lines: body text containing "(HEAD)" → preserved verbatim
//! - strip_head_markers_bold_text: bold-text pseudo-header `**Re: Something** (HEAD)` → suffix removed
//! - strip_head_markers_ignores_fenced_code_hash: `(HEAD)` inside fenced code block preserved verbatim
//! - strip_guard_markers_removes_standalone_lines: lines containing only `<!-- no-pending-capture -->` or `<!-- no-pending-done-guard -->` → removed; surrounding content preserved
//! - strip_guard_markers_strips_inline_content: guard markers embedded in content lines → marker text removed, trailing whitespace trimmed
//! - strip_guard_markers_strips_trailing_on_content_line: guard marker appended to end of content line → marker removed, content preserved
//! - commit_staged_blob_has_no_head_markers: regression for #dsng — commit staging strips `(HEAD)` from the blob and post-commit cleanup leaves the working tree/snapshot clean
//! - commit_retries_full_transaction_when_stage_hits_index_lock: `update-index` index.lock contention retries the full stage+commit transaction until the lock clears
//! - commit_serializes_closeout_per_git_root: two different docs in the same repo contend on one repo-scoped lock and both close out cleanly
//! - reposition_boundary_to_end_basic: stale boundary before user prompt → boundary repositioned after prompt
//! - reposition_boundary_no_exchange: doc with no exchange component → content returned unchanged
//! - reposition_boundary_preserves_user_edits: user text between response and boundary → all user text preserved, boundary after it
//! - reposition_boundary_cleans_multiple_stale: document with 2 stale boundaries → all removed, exactly 1 fresh boundary at end after user text
//! - commit_repairs_committed_historical_snapshot_drift: historical direct response already committed in `HEAD` repairs the stale snapshot without creating a duplicate commit
//! - commit_repairs_committed_head_before_user_follow_up_noop: snapshot lags behind a committed response in `HEAD`, working tree adds only a new user follow-up, and commit repairs the snapshot up to `HEAD` instead of staging the stale snapshot and rewinding the doc
//! - commit_closes_cycle_when_staged_snapshot_already_matches_head: stale open cycle + later user edit → close as already committed instead of `commit_failed`
//! - commit_already_current_repairs_transient_working_tree_churn: already-committed no-op closeout repairs boundary / `(HEAD)`-only file drift back to clean `HEAD`
//! - verify_snapshot_committed_returns_committed_when_matching: snapshot matches HEAD → `Committed`
//! - verify_snapshot_committed_returns_differs_when_snapshot_ahead: snapshot has content not in HEAD → `SnapshotDiffersFromHead`
//! - verify_snapshot_committed_no_snapshot: no snapshot file → `NoSnapshot`
//! - verify_snapshot_committed_no_head: file not tracked → `NoHead`
//! - submodule_noop_commit_updates_stale_parent_pointer: no-op commit in submodule still updates stale parent pointer

use anyhow::Result;
use fs2::FileExt;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::component::is_backlog_component;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitOutcome {
    pub did_commit: bool,
    pub vcs_refresh_signaled: Option<bool>,
}

/// RAII guard for an exclusive advisory lock serializing `git::commit()` runs
/// per git repo / submodule across concurrent sessions. Protects the short
/// stage+commit critical section so two different docs in the same repo do not
/// race on one shared git index.
///
/// Lock file lives under the resolved git dir (`git rev-parse
/// --absolute-git-dir`) at `agent-doc-locks/commit-repo-<hash>.lock`. That
/// means nested docs in the same repo/submodule all share one lock, while
/// different repos do not block each other, and lock creation does not alter
/// `.agent-doc` project-root discovery.
///
/// Best-effort: if the path cannot be resolved or opened, returns `None` and
/// the caller proceeds unlocked. When the lock is already held, acquisition
/// blocks for the short commit critical section instead of proceeding unlocked.
struct CommitLock {
    _file: File,
}

impl CommitLock {
    fn acquire(git_root: &Path) -> Option<Self> {
        let lock_path = commit_lock_path_for_git_root(git_root)?;
        let scope = commit_lock_scope_path(git_root)?;
        let lock_dir = lock_path.parent()?.to_path_buf();
        if let Err(e) = std::fs::create_dir_all(&lock_dir) {
            eprintln!(
                "[commit] commit-lock dir create failed: {} (proceeding unlocked)",
                e
            );
            return None;
        }
        let file = match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "[commit] commit-lock open failed: {} (proceeding unlocked)",
                    e
                );
                return None;
            }
        };
        if let Err(e) = file.try_lock_exclusive() {
            eprintln!(
                "[commit] repo commit-lock contended for {}: {} (waiting)",
                scope.display(),
                e
            );
            if let Err(e) = file.lock_exclusive() {
                eprintln!(
                    "[commit] commit-lock wait failed for {}: {} (proceeding unlocked)",
                    scope.display(),
                    e
                );
                return None;
            }
        }
        Some(Self { _file: file })
    }
}

impl Drop for CommitLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

fn absolute_git_dir_at(git_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    path.canonicalize().ok().or(Some(path))
}

fn commit_lock_scope_path(git_root: &Path) -> Option<PathBuf> {
    absolute_git_dir_at(git_root)
}

fn commit_lock_path_for_git_root(git_root: &Path) -> Option<PathBuf> {
    let scope_path = commit_lock_scope_path(git_root)
        .or_else(|| git_root.canonicalize().ok())
        .unwrap_or_else(|| git_root.to_path_buf());
    let key = crate::snapshot::doc_hash_from_str(scope_path.to_string_lossy().as_ref());
    Some(
        scope_path
            .join("agent-doc-locks")
            .join(format!("commit-repo-{}.lock", key)),
    )
}

fn push_workspace_access_dir(dirs: &mut Vec<PathBuf>, git_root: &Path, candidate: Option<PathBuf>) {
    let Some(dir) = candidate else {
        return;
    };
    if dir.starts_with(git_root) || dirs.contains(&dir) {
        return;
    }
    dirs.push(dir);
}

/// Return extra directories a workspace-scoped harness must be allowed to
/// write when operating on `file`.
///
/// Ordinary repos return an empty list because the working tree and `.git/`
/// both live under the repo root already. Submodule docs return the
/// superproject working tree (so the harness can patch parent-repo docs such
/// as shared backlog files) plus any external git metadata dirs needed for git
/// lifecycle operations.
pub(crate) fn workspace_access_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Vec::new();
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    let mut dirs = Vec::new();
    if in_submodule {
        push_workspace_access_dir(&mut dirs, &git_root, Some(super_root.clone()));
    }
    for dir in external_git_dirs_for_doc(file) {
        push_workspace_access_dir(&mut dirs, &git_root, Some(dir));
    }
    dirs
}

/// Return external git metadata directories a workspace-scoped harness must be
/// allowed to write when operating on `file`.
///
/// Ordinary repos return an empty list because `.git/` lives under the repo
/// root. Submodules return the external `.git/modules/...` gitdir plus the
/// superproject `.git` used by parent-pointer updates.
pub(crate) fn external_git_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Vec::new();
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    let mut dirs = Vec::new();
    push_workspace_access_dir(&mut dirs, &git_root, absolute_git_dir_at(&git_root));
    if in_submodule {
        push_workspace_access_dir(&mut dirs, &git_root, absolute_git_dir_at(&super_root));
    }
    dirs
}

fn resolve_absolute_to_git_root(
    file: &Path,
    cwd_fallback: &Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let parent = file.parent().unwrap_or(Path::new("/"));
    if let Some(superproject) = git_superproject_at(parent) {
        return (superproject, file.to_path_buf());
    }
    let root = git_toplevel_at(parent).unwrap_or_else(|| cwd_fallback.to_path_buf());
    (root, file.to_path_buf())
}

fn resolve_relative_to_git_root_from(
    cwd: &Path,
    file: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let cwd_candidate = cwd.join(file);
    if cwd_candidate.exists() {
        let resolved = cwd_candidate.canonicalize().unwrap_or(cwd_candidate);
        return Ok(resolve_absolute_to_git_root(&resolved, cwd));
    }

    // Try superproject first (handles submodule CWD case)
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Try git toplevel
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    Ok((cwd.to_path_buf(), file.to_path_buf()))
}

/// Resolve a file path to absolute form, preferring the CWD's git root when
/// the same relative path exists in both the main repo and a submodule.
///
/// When route.rs sends trigger commands to tmux panes, relative paths resolve
/// against the pane's CWD — which may be narrowed to a submodule root. This
/// function canonicalizes relative paths against the process CWD so the trigger
/// always targets the correct file.
pub fn resolve_absolute_file_path(file: &Path) -> PathBuf {
    if file.is_absolute() {
        return file.to_path_buf();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidate = cwd.join(file);
    if candidate.exists() {
        candidate.canonicalize().unwrap_or(candidate)
    } else {
        file.to_path_buf()
    }
}

/// Resolve a relative path against the git root (superproject root if in a submodule).
/// Returns (git_root, resolved_file_path) so callers can run git commands in the correct repo.
pub(crate) fn resolve_to_git_root(file: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    if file.is_absolute() {
        return Ok(resolve_absolute_to_git_root(
            file,
            &std::env::current_dir().unwrap_or_default(),
        ));
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    resolve_relative_to_git_root_from(&cwd, file)
}

/// Get git toplevel from a specific directory.
pub(crate) fn git_toplevel_at(dir: &Path) -> Option<std::path::PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(s))
            }
        })
}

/// Get the superproject working tree from a specific directory.
/// Returns `Some(path)` only when `dir` is inside a submodule. Returns `None`
/// for top-level repos or when git is unavailable.
pub(crate) fn git_superproject_at(dir: &Path) -> Option<std::path::PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(s))
            }
        })
}

/// If `file` lives inside a submodule of `super_root`, return the submodule's
/// own git toplevel and `true`. Otherwise return `super_root` unchanged.
///
/// This lets `commit()` run git operations from within the submodule (where
/// they're valid) instead of the parent repo (where the file appears as both
/// a submodule entry and a tracked path, causing `update-index --cacheinfo`
/// and `git add` to refuse the path with "appears as both a file and as a
/// directory" / "Pathspec ... is in submodule" errors).
pub(crate) fn narrow_to_submodule(super_root: &Path, file: &Path) -> (PathBuf, bool) {
    let parent = file.parent().unwrap_or(Path::new("/"));
    if let Some(inner) = git_toplevel_at(parent)
        && inner != super_root
        && inner.starts_with(super_root)
    {
        return (inner, true);
    }
    (super_root.to_path_buf(), false)
}

/// After a successful commit inside a submodule, stage and partial-commit the
/// updated submodule pointer in the superproject. Uses an explicit pathspec on
/// the commit so any other staged files in the parent index are preserved.
fn update_parent_submodule_pointer(super_root: &Path, submodule_root: &Path, msg: &str) {
    let _commit_lock = CommitLock::acquire(super_root);
    let rel = match submodule_root.strip_prefix(super_root) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("[commit] cannot compute submodule relative path; skipping pointer update");
            return;
        }
    };
    let rel_str = rel.to_string_lossy().to_string();

    let add = Command::new("git")
        .current_dir(super_root)
        .args(["add", "--", &rel_str])
        .output();
    match add {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "[commit] parent git add for submodule pointer failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return;
        }
        Err(e) => {
            eprintln!("[commit] parent git add error: {}", e);
            return;
        }
    }

    let parent_msg = format!("{} (submodule pointer)", msg);
    let commit = Command::new("git")
        .current_dir(super_root)
        .args(["commit", "-m", &parent_msg, "--no-verify", "--", &rel_str])
        .output();
    match commit {
        Ok(o) if o.status.success() => {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let t = line.trim();
                if t.starts_with('[') && t.contains(']') {
                    eprintln!("{}", line);
                }
            }
            eprintln!("[commit] parent submodule pointer updated for {}", rel_str);
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            // "nothing to commit" / "no changes added" is fine — pointer was already current.
            if stderr.contains("nothing to commit")
                || stdout.contains("nothing to commit")
                || stderr.contains("no changes added")
            {
                return;
            }
            eprintln!(
                "[commit] parent submodule pointer commit failed: {}",
                stderr.trim()
            );
        }
        Err(e) => eprintln!("[commit] parent submodule pointer commit error: {}", e),
    }
}

/// Resolve the cwd to use when spawning a tmux pane for `file`.
///
/// For documents inside a submodule, returns the submodule's own git toplevel
/// so the spawned Claude session starts inside that submodule. For top-level
/// docs (or when git resolution fails), falls back to the process cwd —
/// matching the pre-existing behavior.
pub fn resolve_pane_cwd(file: &Path) -> std::path::PathBuf {
    if let Ok((super_root, resolved)) = resolve_to_git_root(file) {
        let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
        if in_submodule {
            return git_root;
        }
        return super_root;
    }
    std::env::current_dir().unwrap_or_default()
}

/// Check if `file` is inside a git repository.
/// Returns `true` if the file's directory (or any ancestor) is a git repo.
/// Returns `false` if git is not available or the path is not tracked.
pub(crate) fn is_in_git_repo(file: &Path) -> bool {
    let dir = if file.is_absolute() {
        file.parent().unwrap_or(Path::new("/")).to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    Command::new("git")
        .current_dir(&dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn strip_boundary_markers(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn normalize_transient_agent_doc_markers(content: &str) -> String {
    strip_guard_markers(&strip_head_markers(&strip_boundary_markers(content)))
}

fn strip_re_heading_attribution(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<String> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        if !is_in_code_block(&code_ranges, offset) {
            let trimmed = line.trim_start();
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hash_count) && trimmed.chars().nth(hash_count) == Some(' ') {
                let after_hash = trimmed[hash_count..].trim_start();
                if after_hash.starts_with("Re:")
                    && let Some(pos) = line.rfind(" — ")
                {
                    result_lines.push(line[..pos].to_string());
                    offset += line.len() + 1;
                    continue;
                }
            }
        }
        result_lines.push(line.to_string());
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

fn normalize_post_commit_re_heading_drift(content: &str) -> String {
    strip_re_heading_attribution(&normalize_transient_agent_doc_markers(content))
}

fn normalize_component_content_for_absorb(content: &str) -> String {
    normalize_transient_agent_doc_markers(content)
        .trim()
        .to_string()
}

fn redact_component_contents_for_absorb(body: &str) -> Option<String> {
    let components = crate::component::parse(body).ok()?;
    let mut redacted = String::with_capacity(body.len());
    let mut last = 0usize;
    for comp in components {
        if comp.open_end < last {
            // Nested inside a previously processed component — already redacted
            continue;
        }
        redacted.push_str(&body[last..comp.open_end]);
        redacted.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    redacted.push_str(&body[last..]);
    Some(redacted)
}

fn is_safe_out_of_band_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    if !file_content.starts_with(snapshot_content) {
        return false;
    }
    let suffix = file_content[snapshot_content.len()..].trim();
    !suffix.is_empty() && suffix.starts_with("### Re:")
}

fn flush_exchange_insert_block(block: &mut String) -> bool {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        block.clear();
        return true;
    }
    let ok = is_safe_historical_exchange_insert_block(trimmed);
    block.clear();
    ok
}

fn is_safe_historical_exchange_insert_block(block: &str) -> bool {
    let non_blank: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_blank.is_empty() {
        return true;
    }

    let Some(first_response_idx) = non_blank.iter().position(|line| {
        line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("##### Re:")
    }) else {
        return false;
    };
    if first_response_idx == 0 {
        return true;
    }

    non_blank[..first_response_idx]
        .iter()
        .all(|line| historical_exchange_prelude_looks_like_prompt_target(line))
}

fn historical_exchange_prelude_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !trimmed.starts_with("#### Re:")
        && !trimmed.starts_with("##### Re:")
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || historical_exchange_prelude_looks_like_imperative(trimmed))
}

fn historical_exchange_prelude_looks_like_imperative(line: &str) -> bool {
    let compact = line.trim_start_matches('>').trim().to_ascii_lowercase();
    compact == "go"
        || compact == "continue"
        || compact.starts_with("do #")
        || compact.starts_with("run ")
        || compact.starts_with("rerun ")
        || compact.starts_with("build ")
        || compact.starts_with("test ")
        || compact.starts_with("commit ")
        || compact.starts_with("push ")
        || compact.starts_with("fix ")
        || compact.starts_with("complete ")
}

fn is_safe_historical_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut insert_block = String::new();
    let mut saw_insert = false;

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                if !flush_exchange_insert_block(&mut insert_block) {
                    return false;
                }
            }
            similar::ChangeTag::Delete => return false,
            similar::ChangeTag::Insert => {
                saw_insert = true;
                insert_block.push_str(change.value());
            }
        }
    }

    saw_insert && flush_exchange_insert_block(&mut insert_block)
}

fn is_safe_user_follow_up_exchange_growth(head_content: &str, current_content: &str) -> bool {
    if head_content == current_content || !current_content.starts_with(head_content) {
        return false;
    }

    let suffix = current_content[head_content.len()..].trim();
    !suffix.is_empty()
        && suffix != "## Assistant"
        && !suffix.starts_with("### Re:")
        && !suffix.starts_with("#### Re:")
}

fn is_safe_out_of_band_pending_mutation(snapshot_content: &str, file_content: &str) -> bool {
    let (snap_prelude, snap_items, snap_postlude) = crate::pending::parse_items(snapshot_content);
    let (file_prelude, file_items, file_postlude) = crate::pending::parse_items(file_content);

    if snap_prelude.trim() != file_prelude.trim() || snap_postlude.trim() != file_postlude.trim() {
        return false;
    }
    if file_items.is_empty() {
        return false;
    }

    let file_ids: HashSet<&str> = file_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.as_str())
        .collect();
    if file_ids.is_empty() {
        return false;
    }

    snap_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .all(|item| file_ids.contains(item.id.as_str()))
}

fn strip_promptish_list_prefix(line: &str) -> &str {
    let mut trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }

    trimmed
}

fn starts_with_bare_prompt_preset_reference(line: &str) -> bool {
    let trimmed = strip_promptish_list_prefix(line);
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn status_mutation_introduces_prompt_work(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut added = String::new();

    for change in diff.iter_all_changes() {
        if change.tag() == similar::ChangeTag::Insert {
            added.push_str(change.value());
        }
    }

    if added.trim().is_empty() {
        return false;
    }

    if !crate::diff::extract_prompt_preset_requests_from_text(&added).is_empty() {
        return true;
    }

    added.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && (crate::diff::text_line_looks_like_prompt_target(trimmed)
                || starts_with_bare_prompt_preset_reference(trimmed))
    })
}

fn is_safe_out_of_band_status_mutation(snapshot_content: &str, file_content: &str) -> bool {
    snapshot_content.trim() != file_content.trim()
        && !status_mutation_introduces_prompt_work(snapshot_content, file_content)
}

fn is_empty_template_scaffold_snapshot(snapshot_doc: &str) -> bool {
    let body = crate::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let Ok(components) = crate::component::parse(body) else {
        return false;
    };

    let has_status = components.iter().any(|c| c.name == "status");
    let has_exchange = components.iter().any(|c| c.name == "exchange");
    let has_pending = components.iter().any(|c| is_backlog_component(&c.name));
    if !(has_status && has_exchange && has_pending) {
        return false;
    }

    components.iter().all(|component| {
        (matches!(component.name.as_str(), "status" | "exchange" | "queue")
            || is_backlog_component(&component.name))
            && normalize_component_content_for_absorb(component.content(body)).is_empty()
    })
}

fn classify_safe_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
    allow_historical_exchange_growth: bool,
) -> Option<&'static str> {
    if snapshot_doc == file_doc {
        return None;
    }

    let snap_body = crate::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let file_body = crate::frontmatter::parse(file_doc)
        .map(|(_, body)| body)
        .unwrap_or(file_doc);

    if redact_component_contents_for_absorb(snap_body)?
        != redact_component_contents_for_absorb(file_body)?
    {
        return None;
    }

    let snap_components = crate::component::parse(snap_body).ok()?;
    let file_components = crate::component::parse(file_body).ok()?;
    if snap_components.len() != file_components.len() {
        return None;
    }

    let mut saw_exchange = false;
    let mut saw_pending = false;
    let mut saw_status = false;

    for (snap_comp, file_comp) in snap_components.iter().zip(file_components.iter()) {
        if snap_comp.name != file_comp.name {
            return None;
        }
        // Backlog/pending components tolerate patch attr differences (deprecated attr being stripped)
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != file_comp.patch_mode()
        {
            return None;
        }

        let snap_content = normalize_component_content_for_absorb(snap_comp.content(snap_body));
        let file_content = normalize_component_content_for_absorb(file_comp.content(file_body));
        if snap_content == file_content {
            continue;
        }

        match snap_comp.name.as_str() {
            "exchange" => {
                let safe_exchange =
                    is_safe_out_of_band_exchange_growth(&snap_content, &file_content)
                        || (allow_historical_exchange_growth
                            && is_safe_historical_exchange_growth(&snap_content, &file_content));
                if !safe_exchange {
                    return None;
                }
                saw_exchange = true;
            }
            name if is_backlog_component(name) => {
                if !is_safe_out_of_band_pending_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_pending = true;
            }
            "status" => {
                if !is_safe_out_of_band_status_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_status = true;
            }
            _ => return None,
        }
    }

    match (saw_status, saw_exchange, saw_pending) {
        (true, true, true) => Some("status+exchange+pending"),
        (true, true, false) => Some("status+exchange"),
        (true, false, true) => Some("status+pending"),
        (true, false, false) => Some("status"),
        (false, true, true) => Some("exchange+pending"),
        (false, true, false) => Some("exchange"),
        (false, false, true) => Some("pending"),
        (false, false, false) => None,
    }
}

fn classify_safe_out_of_band_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, false)
}

fn classify_committed_historical_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, true)
}

fn has_non_exchange_component_drift(snapshot_doc: &str, file_doc: &str) -> bool {
    let snap_body = crate::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let file_body = crate::frontmatter::parse(file_doc)
        .map(|(_, body)| body)
        .unwrap_or(file_doc);

    let Ok(snap_components) = crate::component::parse(snap_body) else {
        return false;
    };
    let Ok(file_components) = crate::component::parse(file_body) else {
        return false;
    };
    if snap_components.is_empty() || file_components.is_empty() {
        return false;
    }
    if snap_components.len() != file_components.len() {
        return true;
    }

    for (snap_comp, file_comp) in snap_components.iter().zip(file_components.iter()) {
        if snap_comp.name != file_comp.name {
            return true;
        }
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != file_comp.patch_mode()
        {
            return true;
        }
        let snap_content = normalize_component_content_for_absorb(snap_comp.content(snap_body));
        let file_content = normalize_component_content_for_absorb(file_comp.content(file_body));
        if snap_content == file_content {
            continue;
        }
        if snap_comp.name != "exchange" {
            return true;
        }
    }

    false
}

#[cfg(test)]
fn classify_safe_committed_historical_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    if has_non_exchange_component_drift(snapshot_doc, file_doc) {
        None
    } else {
        match classify_committed_historical_agent_doc_mutation(snapshot_doc, file_doc) {
            Some("exchange") => Some("exchange"),
            None if crate::session_check::detect_bypassed_response_write_between(
                snapshot_doc,
                file_doc,
            )
            .is_some() =>
            {
                Some("exchange")
            }
            _ => None,
        }
    }
}

fn is_safe_user_only_follow_up_after_committed_head(head_doc: &str, current_doc: &str) -> bool {
    if head_doc == current_doc {
        return false;
    }

    let head_body = crate::frontmatter::parse(head_doc)
        .map(|(_, body)| body)
        .unwrap_or(head_doc);
    let current_body = crate::frontmatter::parse(current_doc)
        .map(|(_, body)| body)
        .unwrap_or(current_doc);

    if redact_component_contents_for_absorb(head_body)
        != redact_component_contents_for_absorb(current_body)
    {
        return false;
    }

    let Ok(head_components) = crate::component::parse(head_body) else {
        return false;
    };
    let Ok(current_components) = crate::component::parse(current_body) else {
        return false;
    };
    if head_components.len() != current_components.len() {
        return false;
    }

    let mut saw_exchange = false;

    for (head_comp, current_comp) in head_components.iter().zip(current_components.iter()) {
        if head_comp.name != current_comp.name {
            return false;
        }
        // Backlog/pending: tolerate patch attr differences (deprecated attr being stripped)
        if !is_backlog_component(&head_comp.name)
            && head_comp.patch_mode() != current_comp.patch_mode()
        {
            return false;
        }

        let head_content = normalize_component_content_for_absorb(head_comp.content(head_body));
        let current_content =
            normalize_component_content_for_absorb(current_comp.content(current_body));
        if head_content == current_content {
            continue;
        }

        match head_comp.name.as_str() {
            "exchange" => {
                if !is_safe_user_follow_up_exchange_growth(&head_content, &current_content) {
                    return false;
                }
                saw_exchange = true;
            }
            _ => return false,
        }
    }

    saw_exchange
}

pub(crate) fn repair_committed_historical_snapshot_drift(
    file: &Path,
) -> Result<Option<&'static str>> {
    let Some(snapshot_doc) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current_doc = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current_doc == snapshot_doc {
        return Ok(None);
    }

    let Some(head_doc) = show_head(file)? else {
        return Ok(None);
    };
    let historical_mutation =
        classify_committed_historical_agent_doc_mutation(&snapshot_doc, &head_doc);
    let non_exchange_component_drift = has_non_exchange_component_drift(&snapshot_doc, &head_doc);
    let historical_response_marker =
        crate::session_check::detect_bypassed_response_write_between(&snapshot_doc, &head_doc);
    let Some(reason) = (match historical_mutation {
        Some("exchange") => Some("exchange"),
        None if !non_exchange_component_drift && historical_response_marker.is_some() => {
            Some("exchange")
        }
        _ => None,
    }) else {
        return Ok(None);
    };

    if normalize_transient_agent_doc_markers(&current_doc)
        == normalize_transient_agent_doc_markers(&head_doc)
    {
        crate::snapshot::save(file, &current_doc)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis=head",
                file.display(),
                reason
            ),
        );
        return Ok(Some(reason));
    }

    if crate::session_check::detect_bypassed_response_write_between(&head_doc, &current_doc)
        .is_none()
    {
        let basis = if is_safe_user_only_follow_up_after_committed_head(&head_doc, &current_doc) {
            "head_follow_up"
        } else {
            "head_local_drift"
        };
        crate::snapshot::save(file, &head_doc)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis={}",
                file.display(),
                reason,
                basis
            ),
        );
        return Ok(Some(reason));
    }

    Ok(None)
}

/// Commit a file with an auto-generated message. Skips hooks.
/// Relative paths are resolved against the git root (superproject if in a submodule).
/// Git commands run from the resolved git root, so this works even when CWD is a submodule.
pub fn commit(file: &Path) -> Result<bool> {
    Ok(commit_with_outcome(file)?.did_commit)
}

/// Commit a file and report whether the VCS refresh signal was written.
///
/// `vcs_refresh_signaled` is:
/// - `Some(true)` when the commit path wrote `.agent-doc/patches/vcs-refresh.signal`
/// - `Some(false)` when a refresh target was available but writing it failed
/// - `None` when no refresh target was available or no new git commit was created
pub fn commit_with_outcome(file: &Path) -> Result<CommitOutcome> {
    let t_total = std::time::Instant::now();

    let (super_root, resolved) = resolve_to_git_root(file)?;
    // If the file lives inside a submodule, run git ops in the submodule itself.
    // The parent repo refuses to stage/commit paths that cross a submodule boundary
    // (`update-index --cacheinfo` and `git add` both fail with "appears as both a
    // file and as a directory" / "Pathspec ... is in submodule"). Routing the commit
    // through the submodule's own repo sidesteps the boundary entirely.
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if in_submodule {
        eprintln!(
            "[commit] file is in submodule {} — running git ops there",
            git_root.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "submodule_route file={} submodule={}",
                file.display(),
                git_root.display()
            ),
        );
    }
    // Serialize the short git index transaction per resolved repo / submodule.
    // Without this, two different docs in the same repo can still interleave on
    // one shared index even if their document hashes differ.
    let _commit_lock = CommitLock::acquire(&git_root);
    let timestamp = chrono_timestamp();
    let doc_name = file
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let msg = format!("agent-doc({}): {}", doc_name, timestamp);

    // Selective commit: stage only the snapshot content (agent response),
    // leaving user edits in the working tree as uncommitted.
    //
    // If a snapshot exists, use git hash-object + update-index to stage the
    // snapshot version without touching the working tree file. This means:
    // - Agent response → committed (no git gutter)
    // - User's subsequent edits → uncommitted (green git gutter)
    let mut snapshot_content = crate::snapshot::load(file)?;
    let file_content = std::fs::read_to_string(file).unwrap_or_default();
    let head_doc = show_head(file)?;
    let snapshot_matched_head_before_absorb = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    let bypassed_response_write = snapshot_matched_head_before_absorb
        .then(|| crate::session_check::detect_bypassed_response_write(file))
        .transpose()?
        .flatten();
    let safe_out_of_band_mutation = snapshot_content
        .as_deref()
        .and_then(|snapshot| classify_safe_out_of_band_agent_doc_mutation(snapshot, &file_content));
    let safe_out_of_band_exchange_only = safe_out_of_band_mutation == Some("exchange");
    let only_heading_attribution_drift = head_doc.as_deref().is_some_and(|head| {
        normalize_post_commit_re_heading_drift(&file_content)
            == normalize_post_commit_re_heading_drift(head)
    });
    if let Some(marker) = bypassed_response_write
        && !safe_out_of_band_exchange_only
        && !only_heading_attribution_drift
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_blocked_bypassed_patchback file={} basis=head marker={}",
                file.display(),
                marker.replace('\n', " ")
            ),
        );
        anyhow::bail!(
            "refusing to treat {} as already committed: found likely direct response patchback without agent-doc cycle: {}",
            file.display(),
            marker
        );
    }
    let committed_historical_patchback =
        snapshot_content
            .as_deref()
            .zip(head_doc.as_deref())
            .map(|(snapshot, head)| {
                let mutation = classify_committed_historical_agent_doc_mutation(snapshot, head);
                (
                    mutation.or_else(|| {
                        has_non_exchange_component_drift(snapshot, head)
                            .then_some("typed_component_drift")
                    }),
                    crate::session_check::detect_bypassed_response_write_between(snapshot, head),
                )
            });
    let file_len = file_content.len();
    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    crate::ops_log::log_op(
        file,
        &format!(
            "commit_staging file={} snap_len={} file_len={}",
            file.display(),
            snap_len,
            file_len
        ),
    );

    let repaired_committed_historical =
        if let Some(reason) = repair_committed_historical_snapshot_drift(file)? {
            eprintln!(
                "[commit] repaired committed historical {} drift into snapshot for {}",
                reason,
                file.display()
            );
            snapshot_content = crate::snapshot::load(file)?;
            true
        } else {
            false
        };

    if !repaired_committed_historical
        && let Some((Some(kind), Some(marker))) = committed_historical_patchback.as_ref()
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_blocked_committed_historical_patchback file={} kind={} marker={}",
                file.display(),
                kind,
                marker.replace('\n', " ")
            ),
        );
        anyhow::bail!(
            "refusing to auto-adopt committed historical response patchback for {}: HEAD contains an out-of-band {} mutation with response marker {}",
            file.display(),
            kind,
            marker
        );
    }

    if let Some(ref snapshot) = snapshot_content
        && snapshot != &file_content
        && !(repaired_committed_historical
            && snapshot
                .as_str()
                .eq(head_doc.as_deref().unwrap_or_default()))
        && let Some(reason) = classify_safe_out_of_band_agent_doc_mutation(snapshot, &file_content)
    {
        eprintln!(
            "[commit] absorbing out-of-band {} mutation into snapshot for {}",
            reason,
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "snapshot_absorb file={} reason={} old_snap_len={} new_snap_len={}",
                file.display(),
                reason,
                snap_len,
                file_len
            ),
        );
        crate::snapshot::save(file, &file_content)?;
        snapshot_content = Some(file_content.clone());
    }

    let snapshot_matches_head = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    let post_commit_local_drift = if snapshot_matches_head {
        head_doc
            .as_deref()
            .and_then(|head| classify_post_commit_local_drift(head, &file_content))
    } else {
        None
    };

    // Warn on significant file/snapshot drift — may indicate an out-of-band write
    // that bypassed the agent-doc write pipeline (snapshot not updated).
    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    if snap_len > 0 && file_len > snap_len {
        let drift = file_len - snap_len;
        // Always log out-of-band drift (any positive delta) for aggregation and
        // root-cause analysis — classifies small/frequent vs large/rare gaps.
        crate::ops_log::log_op(
            file,
            &format!(
                "out_of_band_write file={} drift={} snap_len={} file_len={}",
                file.display(),
                drift,
                snap_len,
                file_len
            ),
        );
        if drift > 100 && post_commit_local_drift.is_none() {
            eprintln!(
                "[commit] WARNING: file is {} bytes larger than snapshot for {} — possible out-of-band write (snap={}, file={})",
                drift,
                file.display(),
                snap_len,
                file_len
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "drift_warning file={} drift={} snap_len={} file_len={}",
                    file.display(),
                    drift,
                    snap_len,
                    file_len
                ),
            );

            // Extreme drift can happen when a newly-bootstrapped document still
            // has the empty scaffold snapshot but the working tree now contains
            // the real file content. Only auto-resync that bootstrap case for
            // files with no HEAD entry yet. Tracked documents stay
            // snapshot-selective here so unanswered user prompts cannot be
            // swallowed into the committed snapshot during preflight.
            if file_len > snap_len * 5
                && let Some(ref snapshot) = snapshot_content
            {
                let head_exists = head_doc.is_some();
                let scaffold_snapshot = is_empty_template_scaffold_snapshot(snapshot);
                if !head_exists && scaffold_snapshot {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) — re-syncing bootstrap scaffold snapshot from file content",
                        file_len / snap_len.max(1)
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "snapshot_resync file={} old_snap_len={} new_snap_len={}",
                            file.display(),
                            snap_len,
                            file_len
                        ),
                    );
                    crate::snapshot::save(file, &file_content)?;
                    snapshot_content = Some(file_content.clone());
                } else {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) — NOT re-syncing tracked/non-scaffold snapshot",
                        file_len / snap_len.max(1)
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "snapshot_resync_blocked file={} head_exists={} scaffold_snapshot={} old_snap_len={} file_len={}",
                            file.display(),
                            head_exists,
                            scaffold_snapshot,
                            snap_len,
                            file_len
                        ),
                    );
                }
            }
        }
    }

    // Handle missing snapshot: if no snapshot exists but file has content, create one.
    // This bootstraps the commit flow for files that were never written by agent-doc.
    //
    // NOTE: Snapshot/file divergence detection (Bug 2B) was removed here because it
    // cannot distinguish "file has user edits" from "file has a missed agent response".
    // Both cases look identical (file has content snapshot doesn't). The IPC snapshot
    // save failure case is handled by Bug 2A (non-fatal save with warning) and the
    // recover step in preflight (detects orphaned responses).
    if snapshot_content.is_none() && !file_content.is_empty() {
        eprintln!(
            "[commit] WARNING: no snapshot exists for {}. Creating from file content.",
            file.display()
        );
        crate::snapshot::save(file, &file_content)?;
        snapshot_content = Some(file_content.clone());
    }

    if snapshot_matches_head {
        if let Some(kind) = post_commit_local_drift {
            if kind == PostCommitLocalDriftKind::UserFollowUp {
                eprintln!(
                    "[commit] prior response is already committed in HEAD for {} — no new assistant response body was supplied, so commit will not synthesize a second assistant patchback; leaving later local user follow-up edits uncommitted",
                    file.display()
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "prior_patchback_without_response_body file={} basis=head",
                        file.display()
                    ),
                );
            } else {
                eprintln!(
                    "[commit] detected post-commit local drift for {} — HEAD already contains the committed response; leaving {} uncommitted",
                    file.display(),
                    kind.describe()
                );
            }
            crate::ops_log::log_op(
                file,
                &format!(
                    "post_commit_local_drift file={} kind={} basis=head",
                    file.display(),
                    kind.as_str()
                ),
            );
        }
        eprintln!(
            "[commit] staged snapshot already matches HEAD for {} — closing cycle as already committed",
            file.display()
        );
        let (snapshot_after_noop, file_after_noop) =
            repair_clean_head_if_only_transient_worktree_drift(file, &file_content)?
                .unwrap_or((snapshot_content.clone(), file_content.clone()));
        finalize_already_committed_noop(
            file,
            "commit_already_current",
            snapshot_after_noop.as_deref(),
            Some(&file_after_noop),
        );

        // Even for no-op submodule commits, the parent pointer may be stale
        // (e.g., submodule committed in a previous cycle but parent never updated).
        if in_submodule && is_submodule_pointer_stale(file) {
            eprintln!("[commit] submodule pointer stale in parent after no-op commit — updating");
            update_parent_submodule_pointer(&super_root, &git_root, &msg);
        }

        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] commit total: {}ms", elapsed_total);
        }
        return Ok(CommitOutcome {
            did_commit: false,
            vcs_refresh_signaled: None,
        });
    }

    // Reposition boundary BEFORE staging so the commit captures the new
    // boundary id atomically. Previously this ran post-commit, which left
    // the boundary-id delta to be picked up by the next turn's preflight
    // commit — producing two commits per turn (one for the prior turn's
    // stale reposition, one for the current turn's content). Running it
    // here folds both into a single commit.
    //
    // The active-run guard inside `reposition_boundary_in_snapshot` still
    // applies: if a concurrent `agent-doc write` is in flight, reposition
    // is skipped and the IPC path owns the transition, matching prior
    // behavior for that case.
    let t_reposition = std::time::Instant::now();
    let _snap_changed = reposition_boundary_in_snapshot(file);
    // Reload snapshot_content from disk — the reposition may have rewritten
    // it with a fresh boundary id. Staging must use the repositioned blob.
    if let Ok(Some(reloaded)) = crate::snapshot::load(file) {
        snapshot_content = Some(reloaded);
    }
    let elapsed_reposition = t_reposition.elapsed().as_millis();
    if elapsed_reposition > 0 {
        eprintln!("[perf] commit.reposition: {}ms", elapsed_reposition);
    }

    let t_commit = std::time::Instant::now();
    let mut commit_attempts = 0u32;
    let commit_output = loop {
        let t_staging = std::time::Instant::now();
        match stage_and_commit_once(&git_root, &resolved, snapshot_content.as_deref(), &msg) {
            Ok(out) => {
                let elapsed_staging = t_staging.elapsed().as_millis();
                if elapsed_staging > 0 {
                    eprintln!(
                        "[perf] commit.staging (hash_object+update-index): {}ms",
                        elapsed_staging
                    );
                }
                break Ok(out);
            }
            Err(CommitTransactionError::RetryableIndexLock { phase, detail })
                if commit_attempts < 3 =>
            {
                commit_attempts += 1;
                let elapsed_staging = t_staging.elapsed().as_millis();
                if elapsed_staging > 0 {
                    eprintln!(
                        "[perf] commit.staging (hash_object+update-index): {}ms",
                        elapsed_staging
                    );
                }
                eprintln!(
                    "[commit] index.lock contention during {} (retry {}/3): {}",
                    phase, commit_attempts, detail
                );
                std::thread::sleep(commit_retry_backoff(commit_attempts));
                continue;
            }
            Err(CommitTransactionError::RetryableIndexLock { phase, detail }) => {
                break Err(anyhow::anyhow!(
                    "git {} failed after index.lock retries: {}",
                    phase,
                    detail
                ));
            }
            Err(CommitTransactionError::Fatal(err)) => break Err(err),
        }
    };
    let commit_status = commit_output.as_ref().map(|o| o.status);
    let elapsed_commit = t_commit.elapsed().as_millis();
    if elapsed_commit > 0 {
        eprintln!("[perf] commit.git_commit: {}ms", elapsed_commit);
    }

    // Log commit result line to stderr (suppress verbose git status output)
    if let Ok(ref o) = commit_output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Only print the commit result line (e.g. "[main abc123] message")
            // and skip git status output (branch info, file listings, etc.)
            if trimmed.starts_with('[') && trimmed.contains(']') {
                eprintln!("{}", line);
            }
        }
    }

    // Log commit result
    let mut did_commit = false;
    match &commit_status {
        Ok(s) if s.success() => {
            did_commit = true;
            crate::ops_log::log_cycle(file, "commit", None, None);
            crate::ops_log::log_op(file, &format!("commit_success file={}", file.display()));
            let snap = crate::snapshot::load(file).ok().flatten();
            let file_content = std::fs::read_to_string(file).ok();
            if let Err(e) = crate::cycle_state::mark_committed(
                file,
                "commit_success",
                snap.as_deref(),
                file_content.as_deref(),
            ) {
                eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
            }
            if let Err(e) = crate::capture::mark_committed(file) {
                eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
            }
            // Fire post_commit hook for cross-session coordination
            let session_id = crate::frontmatter::read_session_id(file).unwrap_or_default();
            crate::hooks::fire_post_commit(file, &session_id);
            crate::hooks::fire_doc_event(file, "post_commit");
        }
        Ok(s) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "commit_failed file={} exit_code={}",
                    file.display(),
                    s.code().unwrap_or(-1)
                ),
            );
        }
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!("commit_error file={} err={}", file.display(), e),
            );
        }
    }

    // Post-commit housekeeping. The staged blob is already clean (commit
    // staging strips `(HEAD)` and guard markers from the snapshot before
    // `git hash-object`), and post-commit cleanup keeps the snapshot /
    // visible document in that same clean shape.
    let mut vcs_refresh_signaled = None;
    if let Ok(ref s) = commit_status
        && s.success()
    {
        // Boundary reposition happens pre-commit now (see above) so the
        // new boundary id lands in the same commit as the response.
        // IPC reposition signal is still sent here so the plugin's
        // Document buffer picks up the new boundary without a disk reload.
        crate::write::try_ipc_reposition_boundary(file);

        // Signal plugin to refresh VCS state so the gutter reflects the commit.
        // Without this, the IDE shows the entire response as uncommitted until
        // the user manually refreshes the file.
        // Uses file-based signal (vcs-refresh.signal) since the socket listener
        // may not be active — the plugin watches .agent-doc/patches/ for both
        // patch files and signal files.
        if let Some(signal_file) = vcs_refresh_signal_path(file) {
            match std::fs::write(&signal_file, "") {
                Ok(()) => {
                    eprintln!("[commit] VCS refresh signal written");
                    vcs_refresh_signaled = Some(true);
                }
                Err(e) => {
                    eprintln!("[commit] VCS refresh signal failed: {} (non-fatal)", e);
                    vcs_refresh_signaled = Some(false);
                }
            }
        }

        // Strip ephemeral guard markers from snapshot and working tree so they
        // match the committed blob (which was already stripped during staging).
        strip_guard_markers_from_disk(file);

        // Submodule pointer update: if we just committed inside a submodule,
        // stage the new submodule HEAD in the parent and partial-commit it.
        if in_submodule {
            update_parent_submodule_pointer(&super_root, &git_root, &msg);
        }
    }

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] commit total: {}ms", elapsed_total);
    }

    Ok(CommitOutcome {
        did_commit,
        vcs_refresh_signaled,
    })
}

fn vcs_refresh_signal_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let signal_file = project_root.join(".agent-doc/patches/vcs-refresh.signal");
    signal_file.parent().filter(|p| p.exists())?;
    Some(signal_file)
}

/// Strip ephemeral guard markers from the snapshot and working-tree file on disk.
/// Best-effort: logs warnings on failure but does not propagate errors.
fn strip_guard_markers_from_disk(file: &Path) {
    if let Ok(Some(ref content)) = crate::snapshot::load(file) {
        let cleaned = strip_guard_markers(content);
        if cleaned != *content
            && let Err(e) = crate::snapshot::save(file, &cleaned)
        {
            eprintln!("[commit] warning: failed to strip guard markers from snapshot: {e}");
        }
    }
    if let Ok(content) = std::fs::read_to_string(file) {
        let cleaned = strip_guard_markers(&content);
        if cleaned != content
            && let Err(e) = std::fs::write(file, &cleaned)
        {
            eprintln!("[commit] warning: failed to strip guard markers from file: {e}");
        }
    }
}

/// Reposition boundary in snapshot AND working tree deterministically.
///
/// After commit, moves the boundary to the end of exchange in both the
/// snapshot and the working-tree file. The active-run guard
/// (`pending_path_for`) prevents racing a concurrent `agent-doc write`:
/// in-flight runs are skipped so the plugin's IPC write path owns the
/// transition. Outside of an active run — including sweep-committed
/// foreign docs that never touch `agent-doc write` — this function is
/// the canonical place the on-disk state becomes consistent.
///
/// The cleanup uses the clean boundary-only reposition helper so the on-disk
/// snapshot/file match the committed blob shape instead of introducing
/// transient `(HEAD)` / boundary-only churn.
///
/// Returns true if the snapshot OR working tree content changed.
fn reposition_boundary_in_snapshot(file: &Path) -> bool {
    // Check for active run — don't reposition if a run is in progress.
    // The in-flight `agent-doc write` owns the transition via IPC.
    if let Ok(canonical) = file.canonicalize()
        && let Ok(pending_path) = crate::snapshot::pending_path_for(&canonical)
        && pending_path.exists()
    {
        eprintln!("[commit] skipping boundary reposition — active run detected");
        return false;
    }

    let mut changed = false;

    // Reposition the snapshot to the same clean shape we stage into git.
    if let Ok(Some(snap_content)) = crate::snapshot::load(file) {
        let new_snap = crate::template::reposition_boundary_to_end_clean(&snap_content);
        if new_snap != snap_content {
            match crate::snapshot::save(file, &new_snap) {
                Ok(()) => {
                    eprintln!("[commit] repositioned boundary in snapshot");
                    changed = true;
                }
                Err(e) => {
                    eprintln!(
                        "[commit] failed to update snapshot after boundary reposition: {}",
                        e
                    );
                }
            }
        }
    }

    // Reposition in the working tree unless a live IDE listener is available.
    // A stale `.agent-doc/patches/` directory by itself is not enough — the
    // reposition signal is socket-based, so skipping the disk rewrite without
    // a listener would leave boundary-only dirtiness behind.
    //
    // When the listener is present, the IPC reposition signal (sent
    // post-commit) lets the plugin handle the working-tree boundary via
    // Document API, coordinated with user edits.
    // Doing a disk-level read-modify-write here races with the user typing
    // in the IDE, producing duplicate structural tails (bug #xbs3).
    let ipc_listener_active = file
        .canonicalize()
        .map(|c| crate::write::resolve_ipc_project_root_pub(&c))
        .map(|root| crate::ipc_socket::is_listener_active(&root))
        .unwrap_or(false);
    if ipc_listener_active {
        eprintln!("[commit] skipping working-tree boundary reposition — IPC listener active");
    } else if let Ok(working) = std::fs::read_to_string(file) {
        let repositioned = crate::template::reposition_boundary_to_end_preserve_head(&working);
        if repositioned != working {
            match crate::write::atomic_write_pub(file, &repositioned) {
                Ok(()) => {
                    eprintln!("[commit] repositioned boundary in working tree");
                    changed = true;
                }
                Err(e) => {
                    eprintln!(
                        "[commit] failed to reposition boundary in working tree: {}",
                        e
                    );
                }
            }
        }
    }

    changed
}

/// Returns the byte ranges of all fenced code blocks in `content` using a
/// CommonMark-compliant parser (pulldown-cmark).
///
/// A closing fence MUST consist of plain backticks/tildes with no info string,
/// so `` ```bash `` inside a `` ``` `` block is treated as literal content —
/// not as a fence closer.  This is the root fix for the `(HEAD)` marker being
/// incorrectly applied to bash-comment lines inside code fences.
fn code_block_byte_ranges(content: &str) -> Vec<std::ops::Range<usize>> {
    let parser = Parser::new_ext(content, Options::empty()).into_offset_iter();
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;
    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = start.take() {
                    ranges.push(s..range.end);
                }
            }
            _ => {}
        }
    }
    ranges
}

/// Returns `true` if the byte at `offset` falls within any code block range.
#[inline]
fn is_in_code_block(ranges: &[std::ops::Range<usize>], offset: usize) -> bool {
    ranges.iter().any(|r| r.contains(&offset))
}

/// Strip ` (HEAD)` suffix from markdown heading lines and bold-text pseudo-headers.
/// Uses a CommonMark-compliant parser to detect code blocks so that `# comment (HEAD)`
/// inside a fenced block is preserved unchanged.
fn strip_head_markers(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<&str> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !is_in_code_block(&code_ranges, offset)
            && let Some(stripped) = line.strip_suffix(" (HEAD)")
        {
            // Strip from markdown headings
            if trimmed.starts_with('#') {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
            // Strip from bold-text pseudo-headers (e.g., "**Re: Foo** (HEAD)")
            let without_suffix = stripped.trim_end();
            if trimmed.starts_with("**") && without_suffix.trim_start().ends_with("**") {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
        }
        result_lines.push(line);
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{}\n", result)
    } else {
        result
    }
}

/// Strip guard suppression markers from content before committing.
/// These markers (`<!-- no-pending-capture -->`, `<!-- no-pending-done-guard -->`)
/// are ephemeral per-cycle signals for `session-check` and should not persist
/// in committed blobs. The check reads from the capture file, not the document.
fn strip_guard_markers(content: &str) -> String {
    const MARKERS: &[&str] = &[
        "<!-- no-pending-capture -->",
        "<!-- no-pending-done-guard -->",
    ];
    let mut result_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if MARKERS.contains(&trimmed) {
            continue;
        }
        if MARKERS.iter().any(|m| line.contains(m)) {
            let mut cleaned = line.to_string();
            for marker in MARKERS {
                cleaned = cleaned.replace(marker, "");
            }
            result_lines.push(cleaned.trim_end().to_string());
        } else {
            result_lines.push(line.to_string());
        }
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{}\n", result)
    } else {
        result
    }
}

/// Write content to git's object database and return the blob hash.
fn hash_object(git_root: &Path, content: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait_with_output()
        })?;
    if !output.status.success() {
        anyhow::bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

enum CommitTransactionError {
    RetryableIndexLock { phase: &'static str, detail: String },
    Fatal(anyhow::Error),
}

fn commit_retry_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(50 * (1u64 << attempt))
}

fn is_index_lock_contention_text(text: &str) -> bool {
    text.contains("index.lock") || text.contains("Unable to create")
}

fn render_git_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, true) => stderr,
        (true, false) => stdout,
        (false, false) => format!("{} | {}", stderr, stdout),
        (true, true) => "no git output".to_string(),
    }
}

fn output_has_index_lock_contention(output: &std::process::Output) -> bool {
    is_index_lock_contention_text(&String::from_utf8_lossy(&output.stderr))
        || is_index_lock_contention_text(&String::from_utf8_lossy(&output.stdout))
}

fn git_add_force(
    git_root: &Path,
    resolved: &Path,
) -> std::result::Result<(), CommitTransactionError> {
    let rel_path = relative_to(resolved, git_root);
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["add", "-f", &rel_path.to_string_lossy()])
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    if !output.status.success() {
        let detail = render_git_output(&output);
        if output_has_index_lock_contention(&output) {
            return Err(CommitTransactionError::RetryableIndexLock {
                phase: "git add",
                detail,
            });
        }
        return Err(CommitTransactionError::Fatal(anyhow::anyhow!(
            "git add failed: {}",
            detail
        )));
    }
    Ok(())
}

fn stage_snapshot_for_commit(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
) -> std::result::Result<(), CommitTransactionError> {
    if let Some(snap) = snapshot_content {
        // Stage a CLEAN copy of the snapshot — `(HEAD)` is transient metadata
        // and must never appear in the committed blob. Post-commit cleanup also
        // collapses the working tree/snapshot back to the same clean shape, so
        // moving the boundary across cycles cannot produce phantom marker-only
        // diffs on previously committed headings.
        let staged_content = strip_guard_markers(&strip_head_markers(snap));
        let rel_path = relative_to(resolved, git_root);

        if let Ok(hash) = hash_object(git_root, &staged_content) {
            let cacheinfo = format!("100644,{},{}", hash, rel_path.to_string_lossy());
            let output = Command::new("git")
                .current_dir(git_root)
                .args(["update-index", "--add", "--cacheinfo", &cacheinfo])
                .output()
                .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
            if !output.status.success() {
                if output_has_index_lock_contention(&output) {
                    return Err(CommitTransactionError::RetryableIndexLock {
                        phase: "update-index",
                        detail: render_git_output(&output),
                    });
                }
                eprintln!("[commit] update-index failed, falling back to git add");
                return git_add_force(git_root, resolved);
            }
            return Ok(());
        }
    }

    // No snapshot or hash-object fallback — stage the live file path.
    git_add_force(git_root, resolved)
}

fn stage_and_commit_once(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    msg: &str,
) -> std::result::Result<std::process::Output, CommitTransactionError> {
    stage_snapshot_for_commit(git_root, resolved, snapshot_content)?;

    // Commit — ignore failure (nothing to commit is fine).
    // Use .output() to capture stdout (prevents git status leaking to stdout
    // when called from `preflight` which reserves stdout for JSON).
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["commit", "-m", msg, "--no-verify"])
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    if !output.status.success() && output_has_index_lock_contention(&output) {
        return Err(CommitTransactionError::RetryableIndexLock {
            phase: "git commit",
            detail: render_git_output(&output),
        });
    }
    Ok(output)
}

/// Compute `path` relative to `root`, canonicalizing both sides so symlinks
/// don't cause `strip_prefix` mismatches. Falls back gracefully through
/// non-canonical strip → original path.
fn relative_to(path: &Path, root: &Path) -> PathBuf {
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(rel) = canon_path.strip_prefix(&canon_root) {
        return rel.to_path_buf();
    }
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.to_path_buf();
    }
    path.to_path_buf()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostCommitLocalDriftKind {
    UserFollowUp,
    WorkingTreeEdits,
}

impl PostCommitLocalDriftKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserFollowUp => "user_follow_up",
            Self::WorkingTreeEdits => "working_tree_edits",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::UserFollowUp => "later local user follow-up edits",
            Self::WorkingTreeEdits => "later local working-tree edits",
        }
    }
}

fn classify_post_commit_local_drift(
    head_doc: &str,
    current_doc: &str,
) -> Option<PostCommitLocalDriftKind> {
    if head_doc == current_doc {
        return None;
    }
    if normalize_transient_agent_doc_markers(current_doc)
        == normalize_transient_agent_doc_markers(head_doc)
    {
        return None;
    }
    if normalize_post_commit_re_heading_drift(current_doc)
        == normalize_post_commit_re_heading_drift(head_doc)
    {
        return None;
    }
    if is_safe_user_only_follow_up_after_committed_head(head_doc, current_doc) {
        return Some(PostCommitLocalDriftKind::UserFollowUp);
    }
    Some(PostCommitLocalDriftKind::WorkingTreeEdits)
}

fn repair_clean_head_if_only_transient_worktree_drift(
    file: &Path,
    file_content: &str,
) -> Result<Option<(Option<String>, String)>> {
    let Some(head_doc) = show_head(file)? else {
        return Ok(None);
    };
    if file_content == head_doc {
        return Ok(None);
    }
    if normalize_transient_agent_doc_markers(file_content)
        != normalize_transient_agent_doc_markers(&head_doc)
        && normalize_post_commit_re_heading_drift(file_content)
            != normalize_post_commit_re_heading_drift(&head_doc)
    {
        return Ok(None);
    }

    crate::write::atomic_write_pub(file, &head_doc)?;
    crate::snapshot::save(file, &head_doc)?;
    crate::ops_log::log_op(
        file,
        &format!("transient_cleanup file={} basis=head", file.display()),
    );
    Ok(Some((Some(head_doc.clone()), head_doc)))
}

fn finalize_already_committed_noop(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) {
    crate::ops_log::log_cycle(file, "commit_noop", snapshot_content, file_content);
    crate::ops_log::log_op(
        file,
        &format!("commit_already_current file={} basis=head", file.display()),
    );
    if let Err(e) = crate::cycle_state::mark_committed(file, event, snapshot_content, file_content)
    {
        eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
    }
    if let Err(e) = crate::capture::mark_committed(file) {
        eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
    }
}

/// Create and checkout a branch for the session.
pub fn create_branch(file: &Path) -> Result<()> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "session".to_string());
    let branch_name = format!("agent-doc/{}", stem);

    let status = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .status()?;
    if !status.success() {
        // Branch may already exist — try switching to it
        let status = Command::new("git")
            .args(["checkout", &branch_name])
            .status()?;
        if !status.success() {
            anyhow::bail!("failed to create or switch to branch {}", branch_name);
        }
    }
    Ok(())
}

/// Squash all agent-doc commits touching a file into one.
pub fn squash_session(file: &Path) -> Result<()> {
    let file_str = file.to_string_lossy();

    // Find the first agent-doc commit for this file
    let output = Command::new("git")
        .args([
            "log",
            "--oneline",
            "--reverse",
            "--grep=^agent-doc",
            "--",
            &file_str,
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next();
    let first_hash = match first_line {
        Some(line) => line.split_whitespace().next().unwrap_or(""),
        None => {
            eprintln!("No agent-doc commits found for {}", file.display());
            return Ok(());
        }
    };

    // Soft reset to the commit before the first agent-doc commit
    let status = Command::new("git")
        .args(["reset", "--soft", &format!("{}~1", first_hash)])
        .status()?;
    if !status.success() {
        anyhow::bail!("git reset failed");
    }

    // Recommit as a single squashed commit
    let status = Command::new("git")
        .args([
            "commit",
            "-m",
            &format!("agent-doc: squashed session for {}", file.display()),
            "--no-verify",
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed during squash");
    }

    eprintln!("Squashed agent-doc commits for {}", file.display());
    Ok(())
}

/// Get the content of a file from the last agent-doc commit (or HEAD).
/// Returns None if the file is not tracked or no commits exist.
pub fn show_head(file: &Path) -> Result<Option<String>> {
    let (super_root, resolved) = resolve_to_git_root(file)?;
    // Narrow to the submodule's own repo when the file lives inside a submodule.
    // `resolve_to_git_root` prefers the superproject, but `git show HEAD:<path>`
    // from a superproject cannot traverse a submodule gitlink — the lookup
    // fails and callers fall back to no-HEAD branches that drop `(HEAD)`
    // markers on submodule-hosted documents.
    let (git_root, _in_submodule) = narrow_to_submodule(&super_root, &resolved);

    // Get the file path relative to the git root (submodule root when narrowed)
    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["show", &format!("HEAD:{}", rel_path.to_string_lossy())])
        .output()?;

    if !output.status.success() {
        // File not tracked or no commits — not an error
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCommitStatus {
    Committed,
    SnapshotDiffersFromHead {
        snapshot_len: usize,
        head_len: usize,
    },
    NoSnapshot,
    NoHead,
    NotInGitRepo,
}

/// Verify that the current snapshot for `file` is committed in its owning git root.
///
/// Compares the snapshot content (modulo transient markers) against `git show HEAD:<file>`
/// in the narrowed git root (submodule when applicable). Returns `Committed` when they
/// match, or a specific variant explaining the mismatch.
pub fn verify_snapshot_committed(file: &Path) -> Result<SnapshotCommitStatus> {
    if !is_in_git_repo(file) {
        return Ok(SnapshotCommitStatus::NotInGitRepo);
    }
    let snapshot = match crate::snapshot::load(file)? {
        Some(s) => s,
        None => return Ok(SnapshotCommitStatus::NoSnapshot),
    };
    let head_doc = match show_head(file)? {
        Some(h) => h,
        None => return Ok(SnapshotCommitStatus::NoHead),
    };
    let normalized_snapshot = normalize_transient_agent_doc_markers(&snapshot);
    let normalized_head = normalize_transient_agent_doc_markers(&head_doc);
    if normalized_snapshot == normalized_head {
        Ok(SnapshotCommitStatus::Committed)
    } else {
        Ok(SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len: normalized_snapshot.len(),
            head_len: normalized_head.len(),
        })
    }
}

/// Check whether the parent repo's submodule pointer is current for a file in a submodule.
/// Returns `true` if the pointer needs updating (submodule is dirty in parent), `false` otherwise.
pub(crate) fn is_submodule_pointer_stale(file: &Path) -> bool {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return false;
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if !in_submodule {
        return false;
    }
    let rel = match git_root.strip_prefix(&super_root) {
        Ok(r) => r.to_string_lossy().to_string(),
        Err(_) => return false,
    };
    let output = Command::new("git")
        .current_dir(&super_root)
        .args(["diff", "--name-only", "--", &rel])
        .output();
    match output {
        Ok(o) => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

/// Get the author timestamp of the last commit touching a file.
/// Returns None if the file has no commits.
pub fn last_commit_mtime(file: &Path) -> Result<Option<std::time::SystemTime>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;

    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            "-1",
            "--format=%ct",
            "--",
            &rel_path.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let ts_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ts_str.is_empty() {
        return Ok(None);
    }

    let epoch: u64 = ts_str.parse().unwrap_or(0);
    if epoch == 0 {
        return Ok(None);
    }

    Ok(Some(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch),
    ))
}

fn chrono_timestamp() -> String {
    // Use date command for simplicity — no extra dependency
    let output = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_head_markers_from_headings() {
        let input =
            "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result,
            "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n"
        );
    }

    #[test]
    fn strip_head_markers_preserves_non_heading_lines() {
        let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
    }

    #[test]
    fn strip_head_markers_bold_text() {
        let input = "**Re: Something** (HEAD)\nSome text.\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "**Re: Something**\nSome text.\n");
    }

    #[test]
    fn strip_head_markers_ignores_fenced_code_hash() {
        // strip_head_markers should not remove content inside fenced code blocks.
        // If somehow `# comment (HEAD)` ended up in a fence, it should be left alone.
        let input = "### Re: Answer (HEAD)\nResponse.\n```bash\n# comment (HEAD)\n```\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result, "### Re: Answer\nResponse.\n```bash\n# comment (HEAD)\n```\n",
            "fenced (HEAD) must be preserved, got:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_removes_standalone_lines() {
        let input = "### Re: topic\nResponse text.\n<!-- no-pending-capture -->\nMore text.\n<!-- no-pending-done-guard -->\nEnd.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "### Re: topic\nResponse text.\nMore text.\nEnd.\n",
            "standalone guard markers should be removed:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_inline_content() {
        let input = "Text with <!-- no-pending-capture --> inline.\nNormal line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "Text with  inline.\nNormal line.\n",
            "inline guard markers should be stripped:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_trailing_on_content_line() {
        let input = "**All 39 variable products now have defaults set.** <!-- no-pending-capture -->\nNext line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "**All 39 variable products now have defaults set.**\nNext line.\n",
            "trailing guard marker should be stripped with trailing whitespace trimmed:\n{result}"
        );
    }

    #[test]
    fn reposition_boundary_to_end_basic() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // Boundary should be after user prompt, before close tag
        assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
        assert!(result.contains("-->\n<!-- /agent:exchange -->"));
        // Old boundary consumed
        assert!(!result.contains("abc123"));
    }

    #[test]
    fn reposition_boundary_no_exchange() {
        let content = "# No exchange component\nJust text.\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // Should return unchanged if no exchange
        assert_eq!(result.trim(), content.trim());
    }

    #[test]
    fn reposition_boundary_preserves_user_edits() {
        let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        assert!(
            result.contains("User's new prompt here."),
            "user edit must be preserved"
        );
        assert!(
            result.contains("More user text."),
            "user edit must be preserved"
        );
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User's new prompt here.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }

    #[test]
    fn reposition_boundary_cleans_multiple_stale() {
        // Simulate a document with multiple stale boundary markers
        let content = "<!-- agent:exchange patch=append -->\n\
            First response.\n\
            <!-- agent:boundary:aaa111 -->\n\
            Second response.\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // All old boundaries should be removed
        assert!(
            !result.contains("aaa111"),
            "first stale boundary must be removed"
        );
        assert!(
            !result.contains("bbb222"),
            "second stale boundary must be removed"
        );
        // Exactly one fresh boundary should exist
        let boundary_count = result.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "exactly one boundary marker should remain"
        );
        // The single boundary should be after user prompt
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User prompt.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }

    // --- Bug 2B regression tests ---
    // Verify that commit does NOT overwrite the snapshot with user edits.
    // The divergence detection was removed from commit because is_stale_baseline
    // cannot distinguish "file has user edits" from "file has a missed agent response" —
    // both look like "file has content snapshot doesn't have".

    #[test]
    fn is_stale_baseline_write_path_user_edits_in_baseline_not_stale() {
        // Write path: baseline has user edits appended, snapshot is the committed state.
        // is_stale_baseline(baseline_with_edits, snapshot) should be FALSE
        // because the baseline's exchange CONTAINS the snapshot's exchange content.
        let snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            <!-- /agent:exchange -->\n";
        let baseline_with_user_edits = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            Implement agent-kit changes.\n\
            Implement updates to agent-doc.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            !crate::write::is_stale_baseline(baseline_with_user_edits, snapshot),
            "baseline with user edits should NOT be stale (it contains snapshot content)"
        );
    }

    #[test]
    fn is_stale_baseline_write_path_stale_baseline_detected() {
        // Write path: baseline is from before the last agent response.
        // is_stale_baseline(old_baseline, current_snapshot) should be TRUE.
        let current_snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            ### Re: Response 2\n\
            Second response.\n\
            <!-- /agent:exchange -->\n";
        let old_baseline = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            crate::write::is_stale_baseline(old_baseline, current_snapshot),
            "baseline missing committed response should be stale"
        );
    }

    #[test]
    fn is_in_git_repo_true_inside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            is_in_git_repo(&doc),
            "file inside git repo should return true"
        );
    }

    #[test]
    fn is_in_git_repo_false_outside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            !is_in_git_repo(&doc),
            "file outside git repo should return false"
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_exchange_and_pending() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("exchange+pending")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_user_prompt_append() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_status_and_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("status+exchange")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn is_safe_user_only_follow_up_after_committed_head_exchange_only() {
        let head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        let current = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";

        assert!(is_safe_user_only_follow_up_after_committed_head(
            head, current
        ));
    }

    #[test]
    fn is_safe_historical_exchange_growth_allows_prompt_target_before_response() {
        let snapshot = "### Re: older\nold body\n";
        let head = "### Re: older\nold body\n\ndo #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted.\n";

        assert!(is_safe_historical_exchange_insert_block(
            "do #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted."
        ));
        assert!(is_safe_historical_exchange_growth(snapshot, head));
    }

    #[test]
    fn classify_safe_committed_historical_agent_doc_mutation_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_committed_historical_agent_doc_mutation(snapshot, file),
            Some("exchange")
        );
        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn write_commit_lifecycle() {
        // Full lifecycle: git repo + snapshot + commit → verify commit in log.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Set up git repo
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create and commit an initial file so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Create a document at its pre-response state and commit it.
        let doc = root.join("session.md");
        let initial_content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial_content).unwrap();

        // Stage + initial commit so the file is tracked
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate a write cycle landing a new response: update both the
        // working tree and the snapshot with the post-response content so
        // commit staging has something to commit.
        let post_response = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, post_response).unwrap();

        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, post_response).unwrap();

        // Now call commit (simulating what --commit does after write)
        commit(&doc).expect("commit should succeed");

        // Verify a new commit exists with the agent-doc message
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-3"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }

    // --- #73tv: repo-scoped commit serialization + full transaction retry ---

    #[test]
    fn commit_retries_full_transaction_when_stage_hits_index_lock() {
        use std::fs;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let updated =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nFixed.\n\n";
        fs::write(&doc, updated).unwrap();
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        crate::snapshot::save(&doc, updated).unwrap();

        let index_lock = root.join(".git/index.lock");
        fs::write(&index_lock, "held").unwrap();

        let remover = thread::spawn({
            let index_lock = index_lock.clone();
            move || {
                thread::sleep(Duration::from_millis(200));
                fs::remove_file(index_lock).unwrap();
            }
        });

        let did_commit = commit(&doc).expect("commit should retry until index.lock clears");
        remover.join().unwrap();

        assert!(
            did_commit,
            "commit should create a git commit after retrying"
        );
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-2"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain the retried agent-doc commit, got:\n{log_str}"
        );
    }

    #[test]
    fn commit_succeeds_when_no_lock_contention() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // No lock present — commit should succeed on first try
        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "commit without lock should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn commit_staged_blob_has_no_head_markers() {
        // Regression for bug #dsng: (HEAD) is a working-tree-only marker and
        // must never appear in the committed blob. If it does, the next
        // cycle's reposition produces a phantom "strip (HEAD)" diff on
        // prior-cycle headings the user is editing.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Initial doc + snapshot, tracked cleanly (no HEAD markers yet).
        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate a write cycle: snapshot has a new response whose heading
        // still carries a transient `(HEAD)` marker.
        let cycle1 = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n\n### Re: newer (HEAD)\nnew body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle1).unwrap();
        fs::write(&snap_abs, cycle1).unwrap();

        commit(&doc).expect("commit should succeed");

        // Assert the committed blob has ZERO `(HEAD)` occurrences.
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            !blob.contains("(HEAD)"),
            "committed blob must not contain (HEAD); got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: newer\n"),
            "committed blob should contain the clean new heading; got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: older\n"),
            "committed blob should still contain the older heading; got:\n{blob}"
        );

        // Post-commit cleanup preserves (HEAD) in the working tree so the user
        // sees which headings are new, while committed blob + snapshot stay clean.
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("### Re: newer (HEAD)"),
            "working tree must preserve (HEAD) on newest heading; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "working tree should retain exactly one (HEAD) marker; got:\n{working}"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should keep the clean heading; got:\n{snap}"
        );
        assert!(
            snap.matches("(HEAD)").count() == 0,
            "snapshot should not retain transient head markers; got:\n{snap}"
        );
    }

    #[test]
    fn commit_blocks_out_of_band_exchange_and_pending_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed pending mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }

    #[test]
    fn commit_does_not_absorb_out_of_band_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        commit(&doc).expect("commit should succeed even when there's nothing new to stage");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("follow-up question"),
            "user prompt should remain uncommitted:\n{committed}"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("follow-up question"),
            "snapshot should stay at the older committed state:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ follow-up question"),
            "working tree should retain the user prompt:\n{working}"
        );
    }

    #[test]
    fn commit_blocks_extreme_drift_resync_for_tracked_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        crate::snapshot::save(&doc, scaffold).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add scaffold", "--no-verify"])
            .output()
            .unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ user question that still needs an answer\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should succeed without absorbing the prompt");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("user question that still needs an answer"),
            "tracked extreme drift must not absorb unanswered prompt:\n{committed}"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("user question that still needs an answer"),
            "snapshot should remain selective for tracked docs:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ user question that still needs an answer"),
            "working tree should retain the unanswered prompt:\n{working}"
        );
    }

    #[test]
    fn commit_resyncs_extreme_drift_for_untracked_scaffold_doc() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        crate::snapshot::save(&doc, scaffold).unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            Ready\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: imported\n\
           body from moved file\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] imported\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should resync bootstrap scaffold snapshot");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed.contains("### Re: imported\n"),
            "bootstrap resync should stage the real file content:\n{committed}"
        );
        assert!(
            committed.contains("[#a1b2] imported"),
            "bootstrap resync should carry pending content too:\n{committed}"
        );
    }

    #[test]
    fn commit_blocks_out_of_band_status_and_exchange_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed status mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }

    #[test]
    fn commit_repairs_committed_historical_snapshot_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let tracked = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, tracked).unwrap();
        crate::snapshot::save(&doc, tracked).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let historical = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, historical).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, tracked).unwrap();

        commit(&doc).expect("commit should repair the stale snapshot");

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: historical\n"),
            "snapshot should repair to the committed historical response:\n{snap}"
        );

        let committed = show_head(&doc).unwrap().unwrap();
        assert!(
            committed.contains("### Re: historical\n"),
            "committed blob should keep the historical response after repair:\n{committed}"
        );
    }

    #[test]
    fn commit_closes_cycle_when_staged_snapshot_already_matches_head() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let visible_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        crate::snapshot::save(&doc, visible_snapshot).unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        fs::write(&doc, &with_user_edit).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(visible_snapshot), Some(&with_user_edit))
            .unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("commit should treat HEAD-current snapshot as no-op");
        assert!(
            !did_commit,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let capture = crate::capture::load_active(&doc).unwrap();
        assert!(
            capture.is_none(),
            "already-committed no-op closeout should clear active capture state"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "ops log should record the dedicated no-op closeout:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "already-committed no-op must not be logged as commit_failed:\n{log}"
        );
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "out-of-component local edits should be classified as working-tree drift:\n{log}"
        );
    }

    #[test]
    fn commit_repairs_committed_head_before_user_follow_up_noop() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let stale_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:old -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, stale_snapshot).unwrap();
        crate::snapshot::save(&doc, stale_snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let committed_head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed_head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, stale_snapshot).unwrap();

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, working).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(working)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let did_commit = commit(&doc).expect("commit should not rewind a stale snapshot");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            !did_commit,
            "repairing the snapshot up to committed HEAD should close as a no-op"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let committed = show_head(&doc).unwrap().unwrap();
        assert!(
            committed.contains("### Re: newer\n"),
            "HEAD should keep the newer committed response:\n{committed}"
        );
        assert!(
            !committed.contains("❯ follow-up question"),
            "HEAD should not absorb the user's follow-up prompt:\n{committed}"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should repair up to the already-committed response:\n{snap}"
        );
        assert!(
            !snap.contains("❯ follow-up question"),
            "snapshot repair must stop at HEAD, not absorb the follow-up prompt:\n{snap}"
        );

        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("❯ follow-up question"),
            "working tree should keep the user's follow-up prompt uncommitted:\n{working_after}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=") && log.contains("kind=user_follow_up"),
            "follow-up noop closeout should classify post-commit local drift:\n{log}"
        );
        assert!(
            log.contains("prior_patchback_without_response_body file="),
            "follow-up noop closeout should record the missing-second-patchback diagnostic:\n{log}"
        );
    }

    #[test]
    fn commit_already_current_repairs_transient_working_tree_churn() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer\n\
            body\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let transient = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer (HEAD)\n\
            body\n\
            <!-- agent:boundary:fresh-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, transient).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();

        let did_commit = commit(&doc).expect("HEAD-current closeout should succeed");
        assert!(
            !did_commit,
            "transient-only churn should close as already committed"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to clean HEAD when only transient churn differed"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also be restored to clean HEAD after transient cleanup"
        );
    }

    #[test]
    fn commit_fails_closed_when_committed_historical_response_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, stale_snapshot).unwrap();
        crate::snapshot::save(&doc, stale_snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        let working = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, working).unwrap();
        crate::snapshot::save(&doc, stale_snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(working)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("committed historical response patchback"),
            "error should explain the blocked historical patchback:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, stale_snapshot,
            "snapshot must stay on the pre-repair baseline when the historical patchback is rejected"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::ResponseCaptured
        );
        assert_eq!(state.last_event, "response_captured");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "blocked historical patchback should be recorded in ops.log:\n{log}"
        );
        assert!(
            !log.contains("snapshot_repair file="),
            "rejected historical patchback must not rewrite the snapshot:\n{log}"
        );
    }

    #[test]
    fn commit_already_current_repairs_response_heading_attribution_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — gpt-5\n\
            body\n\
            <!-- agent:boundary:committed-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — codex (HEAD)\n\
            body\n\
            <!-- agent:boundary:stale-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, drifted).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();

        let did_commit = commit(&doc).expect("heading attribution drift should self-heal");
        assert!(!did_commit, "repair should close as already committed");

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to the committed response heading and boundary"
        );

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also return to committed HEAD"
        );
    }

    #[test]
    fn commit_identifies_post_commit_local_working_tree_edits() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response plus later local edit\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- later local note -->\n";
        fs::write(&doc, working).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();

        let did_commit = commit(&doc).expect("HEAD-current local edits should close as no-op");
        assert!(
            !did_commit,
            "later local edits on top of HEAD must stay uncommitted"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let working_after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working_after, working,
            "commit should not overwrite later local edits when HEAD is already current"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "working-tree edits should be classified as post-commit local drift:\n{log}"
        );
        assert!(
            !log.contains("drift_warning file="),
            "post-commit local drift should not be mislabeled as a generic out-of-band write:\n{log}"
        );
    }

    #[test]
    fn commit_blocks_bypassed_response_patchback_on_head_current() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let bypassed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            \n\
            do #later. spec-test-build-install-commit-push\n\
            \n\
            ### Re: bypassed\n\
            landed outside agent-doc\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, bypassed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(bypassed)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "response_captured",
            Some(committed),
            Some(bypassed),
            "sha256",
            None,
        )
        .unwrap();

        let err = commit(&doc).expect_err("bypassed response patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the bypassed patchback:\n{message}"
        );
        assert!(
            message.contains("### Re: bypassed"),
            "error should surface the offending heading:\n{message}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::ResponseCaptured
        );
        assert_eq!(state.last_event, "response_captured");

        let head_doc = show_head(&doc).unwrap().unwrap();
        assert!(
            !head_doc.contains("### Re: bypassed"),
            "HEAD must stay on the last binary-owned patchback:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_bypassed_patchback file="),
            "ops log should record the blocked bypassed patchback:\n{log}"
        );
    }

    #[test]
    fn commit_blocks_committed_historical_patchback_that_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();

        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("committed historical response patchback"),
            "error should explain the committed historical patchback:\n{message}"
        );
        assert!(
            message.contains("typed_component_drift")
                || message.contains("status+exchange")
                || message.contains("status"),
            "error should surface the out-of-band mutation kind:\n{message}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "ops log should record the blocked historical patchback:\n{log}"
        );
    }

    // --- Fix 1: snapshot saved before process::exit(75) (structural test) ---
    // The actual exit path in write::run_stream calls snapshot::save before process::exit(75).
    // We verify this by checking that snapshot::save is callable at that point.
    // Full integration testing requires IPC infrastructure; unit coverage is in write.rs.

    // --- Submodule-aware commit routing ---

    #[test]
    fn commit_in_submodule_routes_through_submodule_repo() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        // Initialize a "submodule" repo inside a temp dir
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        // Allow file:// transport inside this test invocation
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        // Initialize the outer repo
        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        // Add the submodule
        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_path = outer.join("src/sub");
        // Configure the checked-out submodule for committing
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Sanity: narrow_to_submodule returns the submodule path, not the outer
        let doc = submodule_path.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let (narrowed, in_sub) = narrow_to_submodule(outer, &doc);
        assert!(in_sub, "doc inside src/sub should be detected as submodule");
        assert_eq!(
            narrowed, submodule_path,
            "narrowed root should be the submodule toplevel"
        );

        // Stage + commit the file inside the submodule so it's tracked
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Modify the file (simulate an agent response landing) and create snapshot
        let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
        fs::write(&doc, new_content).unwrap();
        let snap_rel = crate::snapshot::path_for(&doc).unwrap();
        // The snapshot path is computed against the project root (walks for .agent-doc).
        // For this test, ensure the .agent-doc dir exists at the outer root and write the snapshot there.
        let project_root = crate::snapshot::find_project_root(&doc.canonicalize().unwrap())
            .unwrap_or_else(|| outer.to_path_buf());
        let snap_abs = project_root.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, new_content).unwrap();

        // Run commit() — should route through the submodule, succeed, and update parent pointer
        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "commit should succeed for submodule file: {:?}",
            result.err()
        );

        // Verify the submodule has a new agent-doc commit
        let sub_log = Command::new("git")
            .current_dir(&submodule_path)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let sub_log_str = String::from_utf8_lossy(&sub_log.stdout);
        assert!(
            sub_log_str.contains("agent-doc(session)"),
            "submodule git log should contain agent-doc commit, got:\n{sub_log_str}"
        );

        // Verify the parent has a submodule-pointer commit
        let outer_log = Command::new("git")
            .current_dir(outer)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let outer_log_str = String::from_utf8_lossy(&outer_log.stdout);
        assert!(
            outer_log_str.contains("(submodule pointer)"),
            "parent git log should contain pointer-update commit, got:\n{outer_log_str}"
        );
    }

    #[test]
    fn external_git_dirs_for_submodule_include_submodule_and_parent_gitdirs() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let doc = outer.join("src/sub/session.md");
        fs::write(&doc, "test\n").unwrap();

        let dirs = external_git_dirs_for_doc(&doc);
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub")),
            "submodule gitdir should be exposed to workspace-write harnesses: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git")),
            "superproject gitdir should be exposed for pointer updates: {dirs:?}"
        );
    }

    #[test]
    fn workspace_access_dirs_for_submodule_include_superproject_root_and_gitdirs() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let doc = outer.join("src/sub/session.md");
        fs::write(&doc, "test\n").unwrap();

        let dirs = workspace_access_dirs_for_doc(&doc);
        assert!(
            dirs.contains(&outer.to_path_buf()),
            "superproject working tree should be writable for parent-repo patchback targets: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git/modules/src/sub")),
            "submodule gitdir should still be exposed: {dirs:?}"
        );
        assert!(
            dirs.contains(&outer.join(".git")),
            "superproject gitdir should still be exposed: {dirs:?}"
        );
    }

    #[test]
    fn narrow_to_submodule_returns_super_root_for_non_submodule_file() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        let doc = root.join("session.md");
        fs::write(&doc, "x").unwrap();
        let (narrowed, in_sub) = narrow_to_submodule(root, &doc);
        assert!(
            !in_sub,
            "non-submodule file should not be detected as in-submodule"
        );
        assert_eq!(narrowed, root);
    }

    // --- relative_to path normalization ---

    #[test]
    fn relative_to_strips_prefix_for_normal_paths() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/project/src/main.rs");
        let rel = relative_to(file, root);
        assert_eq!(rel, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn relative_to_returns_original_when_no_common_prefix() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/other/path/file.rs");
        let rel = relative_to(file, root);
        assert_eq!(rel, PathBuf::from("/other/path/file.rs"));
    }

    #[test]
    fn relative_to_handles_symlinked_path() {
        use std::fs;
        let real_dir = tempfile::TempDir::new().unwrap();
        let link_dir = tempfile::TempDir::new().unwrap();
        let real_root = real_dir.path();
        let link_path = link_dir.path().join("link");

        // Create a real file
        let subdir = real_root.join("tasks");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("doc.md"), "content").unwrap();

        // Create symlink: link -> real_root
        std::os::unix::fs::symlink(real_root, &link_path).unwrap();

        // Access the file through the symlink
        let file_via_symlink = link_path.join("tasks/doc.md");
        assert!(file_via_symlink.exists());

        // relative_to should resolve symlinks and produce the correct relative path
        let rel = relative_to(&file_via_symlink, real_root);
        assert_eq!(
            rel,
            PathBuf::from("tasks/doc.md"),
            "should produce submodule-relative path even when accessed via symlink"
        );
    }

    #[test]
    fn commit_in_submodule_with_symlinked_absolute_path() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        let link_dir = tempfile::TempDir::new().unwrap();
        let link_path = link_dir.path().join("workspace");

        // Create symlink: workspace -> outer
        std::os::unix::fs::symlink(outer, &link_path).unwrap();

        // Initialize a "submodule" origin repo
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        // Initialize the outer repo (via real path, as git would)
        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        // Add submodule
        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_path = outer.join("src/sub");
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create and track the document inside the submodule
        let doc_real = submodule_path.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
        fs::write(&doc_real, content).unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Modify the file and create snapshot
        let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
        fs::write(&doc_real, new_content).unwrap();
        let project_root = crate::snapshot::find_project_root(&doc_real.canonicalize().unwrap())
            .unwrap_or_else(|| outer.to_path_buf());
        let snap_rel = crate::snapshot::path_for(&doc_real).unwrap();
        let snap_abs = project_root.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, new_content).unwrap();

        // Access the file via the SYMLINK path — this is the bug scenario
        let doc_via_symlink = link_path.join("src/sub/session.md");
        assert!(doc_via_symlink.exists(), "symlinked path should exist");

        // commit() should succeed even with the symlinked absolute path
        let result = commit(&doc_via_symlink);
        assert!(
            result.is_ok(),
            "commit should succeed for submodule file accessed via symlink: {:?}",
            result.err()
        );

        // Verify the submodule has the agent-doc commit
        let sub_log = Command::new("git")
            .current_dir(&submodule_path)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let sub_log_str = String::from_utf8_lossy(&sub_log.stdout);
        assert!(
            sub_log_str.contains("agent-doc(session)"),
            "submodule git log should contain agent-doc commit, got:\n{sub_log_str}"
        );
    }

    // --- #8jzg: resolve_pane_cwd tests ---

    #[test]
    fn resolve_pane_cwd_returns_git_root_for_file_in_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        let doc = root.join("plan.md");
        fs::write(&doc, "# Plan\n").unwrap();

        // resolve_pane_cwd should return the git root (not the file's parent)
        let cwd = resolve_pane_cwd(&doc);
        assert_eq!(
            cwd, root,
            "cwd should be the git root for a file inside a plain repo"
        );
    }

    #[test]
    fn resolve_pane_cwd_falls_back_to_process_cwd_for_non_git_path() {
        // A file in a temp dir with no git repo — should fall back to process cwd
        let dir = tempfile::TempDir::new().unwrap();
        let non_git_file = dir.path().join("notes.md");
        std::fs::write(&non_git_file, "notes\n").unwrap();

        // resolve_pane_cwd should not panic and should return a valid path
        let cwd = resolve_pane_cwd(&non_git_file);
        assert!(
            cwd.exists() || cwd == std::env::current_dir().unwrap_or_default(),
            "fallback cwd should be the process cwd or an existing path"
        );
    }

    #[test]
    fn resolve_relative_path_prefers_existing_submodule_file_over_superproject_shadow() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();

        let shadow_dir = outer.join("tasks");
        fs::create_dir_all(&shadow_dir).unwrap();
        fs::write(shadow_dir.join("monsterrodholders.md"), "outer shadow\n").unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(sub_origin.join("tasks")).unwrap();
        fs::write(
            sub_origin.join("tasks/monsterrodholders.md"),
            "submodule doc\n",
        )
        .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_add = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/boost-client",
            ])
            .output()
            .unwrap();
        assert!(
            sub_add.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_add.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_root = outer.join("src/boost-client");
        let (super_root, resolved) = resolve_relative_to_git_root_from(
            &submodule_root,
            Path::new("tasks/monsterrodholders.md"),
        )
        .unwrap();

        assert_eq!(
            super_root, outer,
            "superproject root should still be returned for IPC/project-root coordination"
        );
        assert_eq!(
            resolved,
            submodule_root
                .join("tasks/monsterrodholders.md")
                .canonicalize()
                .unwrap(),
            "relative path should resolve to the existing submodule file, not the outer shadow file"
        );
    }

    #[test]
    fn resolve_absolute_file_path_returns_absolute_for_existing_relative() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let tasks = root.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("plan.md");
        std::fs::write(&doc, "# Plan\n").unwrap();

        // Temporarily set CWD to root
        let prev_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();

        let resolved = resolve_absolute_file_path(Path::new("tasks/plan.md"));
        assert!(resolved.is_absolute(), "resolved path must be absolute");
        assert_eq!(resolved, doc, "must resolve to the CWD-relative file");

        std::env::set_current_dir(prev_cwd).unwrap();
    }

    #[test]
    fn resolve_absolute_file_path_preserves_absolute_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let doc = root.join("test.md");
        std::fs::write(&doc, "test\n").unwrap();

        let resolved = resolve_absolute_file_path(&doc);
        assert_eq!(resolved, doc, "absolute paths must be returned as-is");
    }

    #[test]
    fn resolve_absolute_file_path_returns_relative_when_not_found() {
        let rel = Path::new("nonexistent/path.md");
        let resolved = resolve_absolute_file_path(rel);
        assert_eq!(
            resolved, rel,
            "missing files should return the original path"
        );
    }

    #[test]
    fn is_stale_baseline_write_path_replace_edits_ignored() {
        // Write path: user edited a replace-mode component in the baseline.
        // Only append-mode components are checked. Replace edits are fine.
        let snapshot = "<!-- agent:status patch=replace -->\nOriginal\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nUser changed\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(
            !crate::write::is_stale_baseline(baseline, snapshot),
            "user edits in replace + append components should NOT be stale"
        );
    }

    #[test]
    fn reposition_skips_working_tree_when_ipc_listener_active() {
        use std::fs;
        use std::thread;
        use std::time::Duration;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid123 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        crate::snapshot::save(&doc, doc_content).unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Start a live IPC listener to simulate an active editor plugin.
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let root_clone = root.to_path_buf();
        let server = thread::spawn(move || {
            crate::ipc_socket::start_listener(&root_clone, |_msg| {
                Some(serde_json::json!({"type": "ack"}).to_string())
            })
            .ok();
        });
        thread::sleep(Duration::from_millis(100));

        // Run reposition — should skip working tree because the listener is active.
        let changed = reposition_boundary_in_snapshot(&doc);

        // Snapshot should be repositioned
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("oldid123"),
            "snapshot boundary should be repositioned"
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        // Working tree should NOT be modified (listener owns the update)
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("oldid123"),
            "working tree should keep old boundary when listener is active"
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)\n"),
            "working tree should stay untouched before plugin reposition"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "working tree should retain exactly one visible head marker"
        );

        assert!(changed, "snapshot change should report changed=true");

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(root));
        drop(server);
    }

    #[test]
    fn reposition_updates_working_tree_when_only_patches_dir_exists() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid456 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        crate::snapshot::save(&doc, doc_content).unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Stale plugin install marker without a live listener should not block
        // the clean disk rewrite that removes transient marker churn.
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        // Run reposition
        reposition_boundary_in_snapshot(&doc);

        // Both snapshot AND working tree should be repositioned
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("oldid456"),
            "snapshot boundary should be repositioned"
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            !working.contains("oldid456"),
            "working tree should be repositioned when only patches dir exists"
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)"),
            "working tree must preserve (HEAD) annotations; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "working tree should retain exactly one (HEAD) marker; got:\n{working}"
        );
    }

    #[test]
    fn commit_serializes_closeout_per_git_root() {
        use std::fs;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_a = root.join("plan-a.md");
        let doc_b = root.join("plan-b.md");
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc_a, initial).unwrap();
        fs::write(&doc_b, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "plan-a.md", "plan-b.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let updated_a =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nA\n\n";
        let updated_b =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nB\n\n";
        fs::write(&doc_a, updated_a).unwrap();
        fs::write(&doc_b, updated_b).unwrap();
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        crate::snapshot::save(&doc_a, updated_a).unwrap();
        crate::snapshot::save(&doc_b, updated_b).unwrap();

        let lock_path = commit_lock_path_for_git_root(root).unwrap();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let held = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        held.lock_exclusive().unwrap();

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for doc in [doc_a.clone(), doc_b.clone()] {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                let result = commit(&doc);
                tx.send((doc, result)).unwrap();
            }));
        }
        drop(tx);

        assert!(
            rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "both commit threads should be waiting on the shared repo lock"
        );

        held.unlock().unwrap();

        let mut results = Vec::new();
        results.push(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        results.push(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        for handle in handles {
            handle.join().unwrap();
        }

        for (doc, result) in results {
            let did_commit = result
                .unwrap_or_else(|e| panic!("commit should succeed for {}: {e}", doc.display()));
            assert!(did_commit, "{} should create a git commit", doc.display());
        }

        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-4"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(plan-a):"),
            "git log should contain the plan-a closeout, got:\n{log_str}"
        );
        assert!(
            log_str.contains("agent-doc(plan-b):"),
            "git log should contain the plan-b closeout, got:\n{log_str}"
        );
    }

    #[test]
    fn redact_component_contents_handles_nested_components() {
        let body = r#"## Status

<!-- agent:status patch=replace -->
Status content here.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Some exchange content.
Add <!-- agent:queue -->...<!-- /agent:queue --> to the template.
More content.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] task
<!-- /agent:pending -->
"#;
        let result = redact_component_contents_for_absorb(body);
        assert!(result.is_some(), "should not panic on nested components");
        let redacted = result.unwrap();
        assert!(
            redacted.contains("<!-- agent:status patch=replace -->"),
            "should contain status open marker"
        );
        assert!(
            redacted.contains("<!-- /agent:status -->"),
            "should contain status close marker"
        );
        assert!(
            !redacted.contains("Status content here."),
            "should redact status content"
        );
        assert!(
            !redacted.contains("Some exchange content."),
            "should redact exchange content (including nested markers)"
        );
    }

    #[test]
    fn verify_snapshot_committed_returns_committed_when_matching() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let content = "# Hello\n\nbody\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::Committed,
        );
    }

    #[test]
    fn verify_snapshot_committed_returns_differs_when_snapshot_ahead() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let old_content = "# Hello\n\nold body\n";
        fs::write(&doc, old_content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let new_content = "# Hello\n\nnew response body\n";
        crate::snapshot::save(&doc, new_content).unwrap();

        match verify_snapshot_committed(&doc).unwrap() {
            SnapshotCommitStatus::SnapshotDiffersFromHead { .. } => {}
            other => panic!("expected SnapshotDiffersFromHead, got {:?}", other),
        }
    }

    #[test]
    fn verify_snapshot_committed_no_snapshot() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        fs::write(&doc, "body\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NoSnapshot,
        );
    }

    #[test]
    fn verify_snapshot_committed_no_head() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        fs::write(&doc, "body\n").unwrap();
        crate::snapshot::save(&doc, "body\n").unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NoHead,
        );
    }
}
