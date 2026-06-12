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
//!   Refuses untracked paths matched by `.gitignore` before using plumbing that would otherwise
//!   bypass porcelain ignore checks. Falls back to `git add -f` when hash-object fails for
//!   non-ignored paths. If the staged snapshot already matches `HEAD`, `commit` closes the cycle
//!   as an already-committed no-op instead of logging a false `commit_failed`. After a successful
//!   commit, post-commit cleanup collapses boundary churn in the snapshot and fires an IPC
//!   reposition signal (`try_ipc_reposition_boundary`) so the IDE plugin can normalize the working
//!   tree to the same clean shape as the committed blob. Without a live listener, the file is
//!   rewritten locally to that same clean shape. Returns `true` when a git commit was created and
//!   `false` when there was nothing new to commit.
//! - `show_head(file)`: returns the file content from `HEAD` as `Some(String)`, or `None` if not
//!   tracked.
//! - `commit_with_outcome(file)`: same as `commit`, but also reports whether the
//!   VCS refresh signal was available and successfully written after the commit.
//! - `verify_snapshot_committed(file)`: verifies that the current snapshot for `file` is
//!   committed in its owning git root (narrowed to submodule when applicable). Compares the
//!   snapshot content (modulo transient markers) against `git show HEAD:<file>`. Returns
//!   `Committed`, `SnapshotDiffersFromHead`, `NoSnapshot`, `NoHead`, or `NotInGitRepo`.
//! - `is_submodule_pointer_stale(file)`: checks whether the parent repo's committed submodule
//!   pointer still differs from the submodule HEAD for a file in a submodule.
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
//! - commit_skips_ignored_untracked_path: untracked session docs matched by `.gitignore` are not staged via hash-object/update-index or `git add -f`
//! - commit_retries_full_transaction_when_stage_hits_index_lock: `update-index` index.lock contention retries the full stage+commit transaction until the lock clears
//! - commit_serializes_closeout_per_git_root: two different docs in the same repo contend on one repo-scoped lock and both close out cleanly
//! - reposition_boundary_to_end_basic: stale boundary before user prompt → boundary repositioned after prompt
//! - reposition_boundary_no_exchange: doc with no exchange component → content returned unchanged
//! - reposition_boundary_preserves_user_edits: user text between response and boundary → all user text preserved, boundary after it
//! - reposition_boundary_cleans_multiple_stale: document with 2 stale boundaries → all removed, exactly 1 fresh boundary at end after user text
//! - commit_repairs_committed_historical_snapshot_drift: historical direct response already committed in `HEAD` repairs the stale snapshot without creating a duplicate commit
//! - commit_repairs_committed_head_before_user_follow_up_noop: snapshot lags behind a committed response in `HEAD`, working tree adds only a new user follow-up, and commit repairs the snapshot up to `HEAD` instead of staging the stale snapshot and rewinding the doc
//! - commit_closes_cycle_when_staged_snapshot_already_matches_head: stale open cycle + later user edit → close as already committed instead of `commit_failed`
//! - commit_skips_terminal_user_follow_up_noop_closeout: terminal committed cycle + later user follow-up → leave the prompt untouched without re-emitting closeout lifecycle bookkeeping
//! - commit_already_current_repairs_transient_working_tree_churn: already-committed no-op closeout repairs boundary / `(HEAD)`-only file drift back to clean `HEAD`
//! - commit_already_current_repairs_transient_working_tree_churn_refreshes_crdt_and_signal: no-op closeout cleanup also refreshes CRDT/editor-facing sidecars so stale transient churn cannot reappear from cached live state
//! - commit_already_current_repairs_stale_agent_response_collapse_preserving_queue_follow_up: already-committed no-op closeout restores a collapsed committed exchange heading while preserving later queue prompt drift outside `agent:exchange`
//! - verify_snapshot_committed_returns_committed_when_matching: snapshot matches HEAD → `Committed`
//! - verify_snapshot_committed_returns_differs_when_snapshot_ahead: snapshot has content not in HEAD → `SnapshotDiffersFromHead`
//! - verify_snapshot_committed_no_snapshot: no snapshot file → `NoSnapshot`
//! - verify_snapshot_committed_no_head: file not tracked → `NoHead`
//! - submodule_noop_commit_updates_stale_parent_pointer: no-op commit in submodule still updates stale parent pointer

use anyhow::{Context, Result};
use fs2::FileExt;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
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

fn is_git_dir(path: &Path) -> bool {
    path.join("HEAD").is_file() && path.join("config").is_file()
}

fn collect_nested_git_dirs(root: &Path, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if is_git_dir(&path) {
            dirs.push(path);
            continue;
        }
        collect_nested_git_dirs(&path, dirs);
    }
}

fn nested_git_dirs_under(git_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_nested_git_dirs(&git_dir.join("modules"), &mut dirs);
    dirs
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
pub fn workspace_access_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
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
/// Plain repos expose any nested submodule gitdirs under `.git/modules/...`.
/// Submodules additionally expose their own external `.git/modules/...` gitdir
/// plus the superproject `.git` used by parent-pointer updates.
pub fn external_git_dirs_for_doc(file: &Path) -> Vec<PathBuf> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Vec::new();
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    let mut dirs = Vec::new();
    if let Some(git_dir) = absolute_git_dir_at(&git_root) {
        push_workspace_access_dir(&mut dirs, &git_root, Some(git_dir.clone()));
        for nested in nested_git_dirs_under(&git_dir) {
            push_workspace_access_dir(&mut dirs, &git_root, Some(nested));
        }
    }
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
pub fn resolve_to_git_root(file: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
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
pub fn git_toplevel_at(dir: &Path) -> Option<std::path::PathBuf> {
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
pub fn git_superproject_at(dir: &Path) -> Option<std::path::PathBuf> {
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
pub fn narrow_to_submodule(super_root: &Path, file: &Path) -> (PathBuf, bool) {
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
pub fn is_in_git_repo(file: &Path) -> bool {
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

pub fn normalize_transient_agent_doc_markers(content: &str) -> String {
    // #22a8: also drop the managed `agent_doc_pipeline:` frontmatter block. It is
    // mirrored onto the document mid-cycle (after response capture, cleared at a
    // terminal phase), so a comparison that kept it would read the managed write
    // as a direct response patchback / closeout drift. Stripping it here keeps
    // every doc-vs-snapshot/HEAD comparison routed through this normalizer
    // invariant to the pipeline mirror.
    crate::frontmatter::strip_pipeline_block_lines(&strip_guard_markers(&strip_head_markers(
        &strip_boundary_markers(content),
    )))
}

/// Replace the `agent:queue` component (opening-tag attributes + body) with a
/// canonical empty placeholder.
///
/// The queue is rewritten by preflight queue-maintenance on essentially every
/// cycle (activation toggles, `auto` strip, head strike, dedup, IPC-buffer
/// merge artifacts) independently of the response body, which always targets
/// `exchange`/`output`. Neutralizing it before hashing keeps response-replay /
/// stale-lock recovery stable across queue churn (#adoc-queue-ipc-buffer-divergence
/// root cause #4: the capture-replay guard must validate the response body, not
/// a whole-document hash that queue-component churn invalidates).
fn neutralize_queue_component(content: &str) -> String {
    let Ok(components) = crate::component::parse(content) else {
        return content.to_string();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return content.to_string();
    };
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..queue.open_start]);
    out.push_str("<!-- agent:queue -->\n<!-- /agent:queue -->");
    out.push_str(&content[queue.close_end..]);
    out
}

/// Drop the transient queue activation frontmatter — the canonical `queue:`
/// control (`#queue-state-unify`) and the deprecated `queue_active:` line — which
/// queue maintenance toggles in lockstep with the `agent:queue` component and is
/// likewise independent of the response body. Both are normalized away together
/// so a legacy `queue_active:` and a migrated `queue: start|stop` compare equal,
/// avoiding the snapshot/HEAD drift loop. Only used for replay-hash
/// normalization, never persisted.
fn strip_queue_active_frontmatter(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("queue_active:") && !t.starts_with("queue:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalization used for response-replay / stale-cycle hash matching.
///
/// Builds on [`normalize_transient_agent_doc_markers`] (boundary/`(HEAD)`/guard
/// markers) and additionally neutralizes the `agent:queue` component **and** the
/// `queue_active:` frontmatter flag so that independent queue-maintenance churn
/// does not invalidate the match. Used by both `cycle_state` (store side) and
/// `repair` (compare side) so the two always normalize identically.
pub fn normalize_for_replay_hash(content: &str) -> String {
    normalize_transient_agent_doc_markers(&strip_queue_active_frontmatter(
        &neutralize_queue_component(content),
    ))
}

fn is_response_heading_line(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
}

fn fence_open(trimmed: &str) -> Option<(char, usize)> {
    let fc = trimmed.chars().next()?;
    if fc != '`' && fc != '~' {
        return None;
    }
    let fl = trimmed.chars().take_while(|&c| c == fc).count();
    if fl >= 3 { Some((fc, fl)) } else { None }
}

fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
    let fc = trimmed.chars().next().unwrap_or('\0');
    if fc != fence_char {
        return false;
    }
    let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
    fl >= fence_len && trimmed[fl..].trim().is_empty()
}

fn prefix_prompt_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with('❯')
        || crate::diff::line_looks_like_markdown_list_item(trimmed)
    {
        return None;
    }
    let indent_len = line.len() - trimmed.len();
    Some(format!("{}❯ {}", &line[..indent_len], trimmed))
}

fn answered_prompt_prelude_start(lines: &[&str]) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if crate::diff::line_looks_like_prompt_prefix_repair_start(trimmed, false) {
            return Some(idx);
        }
    }
    None
}

fn canonicalize_answered_prompt_prefixes(exchange_content: &str) -> String {
    // When a response heading is present, the contiguous prose block
    // immediately above it is the user prelude for that turn. Canonicalize
    // that prelude back to `❯ ...` so answered prompts keep their visual
    // marker after staging/cleanup instead of collapsing to bare lines.

    let lines: Vec<&str> = exchange_content.split_inclusive('\n').collect();
    if lines.is_empty() {
        return exchange_content.to_string();
    }

    let mut line_in_fence = vec![false; lines.len()];
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    for idx in 0..lines.len() {
        let line = lines[idx].trim_end_matches('\n');
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                line_in_fence[idx] = true;
                continue;
            }
        } else {
            line_in_fence[idx] = true;
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }
    }

    let mut prefix_targets = vec![false; lines.len()];
    for idx in 0..lines.len() {
        let trimmed = lines[idx].trim_end_matches('\n').trim();
        if line_in_fence[idx] || !is_response_heading_line(trimmed) {
            continue;
        }

        let mut block_indices = Vec::new();
        let mut cursor = idx;
        let mut stopped_on_response_heading = false;
        while cursor > 0 {
            cursor -= 1;
            let line = lines[cursor].trim_end_matches('\n');
            let trimmed = line.trim();
            if line_in_fence[cursor]
                || trimmed.is_empty()
                || trimmed.starts_with("<!--")
                || is_response_heading_line(trimmed)
            {
                stopped_on_response_heading =
                    !line_in_fence[cursor] && is_response_heading_line(trimmed);
                break;
            }
            block_indices.push(cursor);
        }
        block_indices.reverse();
        if block_indices.is_empty() {
            continue;
        }
        // A prose block that butts directly against a preceding `### Re:`
        // heading with no blank-line / comment separator is the trailing body
        // of that response (e.g. a duplicated response block left by a
        // multi-retry / late-IPC reposition), not a fresh user prelude. Never
        // canonicalize those lines into `❯ ` prompt prefixes — agent response
        // body must never receive the user-prompt marker.
        if stopped_on_response_heading {
            continue;
        }

        let block_lines: Vec<&str> = block_indices
            .iter()
            .map(|&line_idx| lines[line_idx])
            .collect();
        let Some(prefix_start) = answered_prompt_prelude_start(&block_lines) else {
            continue;
        };
        for line_idx in block_indices.into_iter().skip(prefix_start) {
            prefix_targets[line_idx] = true;
        }
    }

    let mut normalized = String::with_capacity(exchange_content.len());
    let mut changed = false;
    for (idx, segment) in lines.iter().enumerate() {
        let line = segment.trim_end_matches('\n');
        if prefix_targets[idx] {
            if let Some(prefixed) = prefix_prompt_line(line) {
                normalized.push_str(&prefixed);
                changed |= prefixed != line;
            } else {
                normalized.push_str(line);
            }
        } else {
            normalized.push_str(line);
        }
        if segment.ends_with('\n') {
            normalized.push('\n');
        }
    }

    if changed {
        normalized
    } else {
        exchange_content.to_string()
    }
}

pub fn normalize_committed_exchange_artifacts(content: &str) -> String {
    let transient = normalize_transient_agent_doc_markers(content);
    let body = match crate::frontmatter::parse(&transient) {
        Ok((_, body)) => body,
        Err(_) => return transient,
    };
    let prefix_len = transient.len().saturating_sub(body.len());
    let Ok(components) = crate::component::parse(body) else {
        return transient;
    };

    let mut rebuilt = String::with_capacity(transient.len());
    rebuilt.push_str(&transient[..prefix_len]);
    let mut last = 0usize;
    let mut changed = false;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        rebuilt.push_str(&body[last..comp.open_end]);
        if comp.name == "exchange" {
            let normalized = canonicalize_answered_prompt_prefixes(comp.content(body));
            changed |= normalized != comp.content(body);
            rebuilt.push_str(&normalized);
        } else {
            rebuilt.push_str(comp.content(body));
        }
        rebuilt.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    rebuilt.push_str(&body[last..]);

    if changed { rebuilt } else { transient }
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

pub fn normalize_post_commit_re_heading_drift(content: &str) -> String {
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

fn is_safe_exchange_user_prompt_insert(snapshot_exchange: &str, file_exchange: &str) -> bool {
    let snap_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let file_lines: Vec<&str> = file_exchange.lines().collect();

    if snap_lines.len() >= file_lines.len() {
        return false;
    }

    let prefix_len = snap_lines
        .iter()
        .zip(file_lines.iter())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    let suffix_len = snap_lines
        .iter()
        .rev()
        .zip(file_lines.iter().rev())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    if suffix_len == 0 {
        return false;
    }

    let suffix_start_in_snap = snap_lines.len().saturating_sub(suffix_len);
    let suffix_has_response = snap_lines[suffix_start_in_snap..]
        .iter()
        .any(|line| line.trim().starts_with("### Re:"));

    if !suffix_has_response {
        return false;
    }

    let insert_start = prefix_len;
    let insert_end = file_lines.len().saturating_sub(suffix_len);

    if insert_start >= insert_end {
        return false;
    }

    let inserted_lines = &file_lines[insert_start..insert_end];

    for line in inserted_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
        {
            return false;
        }
    }

    true
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

fn detect_reintroduced_reaped_pending_ids(
    doc: &str,
    reaped_ids: &HashSet<String>,
) -> Result<Vec<String>> {
    if reaped_ids.is_empty() {
        return Ok(Vec::new());
    }

    let components = crate::component::parse(doc)?;
    let mut seen = HashSet::new();
    let mut reintroduced = Vec::new();
    for component in components
        .iter()
        .filter(|component| crate::component::is_tracked_work_component(&component.name))
    {
        let (_, items, _) = crate::pending::parse_items(component.content(doc));
        for item in items {
            if !item.id.is_empty() && reaped_ids.contains(&item.id) && seen.insert(item.id.clone())
            {
                reintroduced.push(item.id);
            }
        }
    }

    reintroduced.sort();
    Ok(reintroduced)
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

fn starts_with_prompt_preset_reference(line: &str) -> bool {
    let trimmed = strip_promptish_list_prefix(line);
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let token_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if token_len == 0 {
        return false;
    }
    let remainder = &rest[token_len..];
    remainder.is_empty()
        || remainder
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
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
                || starts_with_prompt_preset_reference(trimmed))
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
                            && is_safe_historical_exchange_growth(&snap_content, &file_content))
                        || is_safe_exchange_user_prompt_insert(&snap_content, &file_content);
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

pub fn classify_safe_out_of_band_agent_doc_mutation(
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

#[cfg(test)]
fn has_non_exchange_component_drift(snapshot_doc: &str, file_doc: &str) -> bool {
    has_non_exchange_component_drift_scoped(snapshot_doc, file_doc, None)
}

/// `#nm1x` — the finalize-path drift gate, optionally intersected against the
/// current [`TurnScope`].
///
/// Without a scope (`None`) this is the historical coarse gate: any non-exchange
/// component whose content changed between snapshot and file is drift that blocks
/// the narrow agent-owned absorb path. With a scope, a non-exchange content
/// change that is *fully explained by node-level ops outside the turn scope*
/// (a new queue item beside the running one, an icebox/done edit) is
/// `Independent`/`ProvenanceSpoofed` — it integrates and persists without
/// affecting the turn, so it does not block. Structural drift (component count /
/// name / patch-mode mismatch) and any change the node differ cannot fully
/// explain still block conservatively.
fn has_non_exchange_component_drift_scoped(
    snapshot_doc: &str,
    file_doc: &str,
    scope: Option<&agent_doc_core::turn_scope::TurnScope>,
) -> bool {
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
        if snap_comp.name == "exchange" {
            continue;
        }
        // Non-exchange content changed. Coarse default: block. With a turn scope,
        // un-gate only when the change is provably composed of node ops that all
        // fall outside the turn's read/write sets.
        match scope {
            Some(scope)
                if non_exchange_change_is_turn_independent(
                    snap_body,
                    file_body,
                    &snap_comp.name,
                    scope,
                ) =>
            {
                continue;
            }
            _ => return true,
        }
    }

    false
}

/// Whether a non-exchange component's node-level changes between snapshot and
/// file are *all* independent of the current turn scope (`#nm1x`). Returns
/// `false` — meaning the change DOES affect the turn, so keep blocking —
/// whenever the node differ cannot fully explain the content change, so
/// un-gating only ever happens for changes provably composed of out-of-scope
/// node ops.
fn non_exchange_change_is_turn_independent(
    snap_body: &str,
    file_body: &str,
    component_name: &str,
    scope: &agent_doc_core::turn_scope::TurnScope,
) -> bool {
    use agent_doc_core::op_log::OpActor;
    use agent_doc_core::turn_scope::{Address, classify_op};

    let events: Vec<_> = agent_doc_markdown_ast::events::diff_node_events(snap_body, file_body)
        .into_iter()
        .filter(|event| event.component == component_name)
        .collect();
    if events.is_empty() {
        // Content changed but no node-level explanation (e.g. prose / replace-mode
        // component): stay conservative and keep blocking.
        return false;
    }
    // The gate diff is snapshot↔file, so every node op is a `user` edit (the
    // agent's committed output already lives in the snapshot).
    events.iter().all(|event| {
        let address = Address::from_component_node_key(&event.component, &event.node_key);
        let node_index = event.after_index.or(event.before_index);
        !classify_op(
            OpActor::User,
            event.kind.as_str(),
            &address,
            node_index,
            scope,
        )
        .affects_turn()
    })
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

pub fn repair_committed_historical_snapshot_drift(file: &Path) -> Result<Option<&'static str>> {
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
    // #nm1x: intersect the drift against the current turn scope so independent
    // out-of-scope edits (e.g. a queue item added beside the running one) do not
    // block the historical snapshot repair.
    let turn_scope = crate::turn_scope_store::load(file);
    let non_exchange_component_drift =
        has_non_exchange_component_drift_scoped(&snapshot_doc, &head_doc, turn_scope.as_ref());
    let historical_response_marker =
        crate::session_check::detect_bypassed_response_write_between(&snapshot_doc, &head_doc);
    let historical_prompt_prefix_artifact = snapshot_doc != head_doc
        && !non_exchange_component_drift
        && normalize_committed_exchange_artifacts(&snapshot_doc)
            == normalize_committed_exchange_artifacts(&head_doc);
    let Some(reason) = (match historical_mutation {
        Some("exchange") => Some("exchange"),
        None if !non_exchange_component_drift && historical_response_marker.is_some() => {
            Some("exchange")
        }
        None if historical_prompt_prefix_artifact => Some("exchange"),
        _ => None,
    }) else {
        return Ok(None);
    };

    if normalize_committed_exchange_artifacts(&current_doc)
        == normalize_committed_exchange_artifacts(&head_doc)
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
    let mut file_content = std::fs::read_to_string(file).unwrap_or_default();
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
    // #nm1x: intersect the snapshot↔HEAD drift against the current turn scope so
    // an independent out-of-scope edit is not misclassified as blocking
    // `typed_component_drift`.
    let turn_scope = crate::turn_scope_store::load(file);
    let committed_historical_patchback =
        snapshot_content
            .as_deref()
            .zip(head_doc.as_deref())
            .map(|(snapshot, head)| {
                let mutation = classify_committed_historical_agent_doc_mutation(snapshot, head);
                (
                    mutation.or_else(|| {
                        has_non_exchange_component_drift_scoped(snapshot, head, turn_scope.as_ref())
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
    // #exch-intermix: before failing closed on the `live_prompt_drift_after_preflight`
    // wedge (adopted `content_ours` snapshot larger than the fragmented visible
    // file), auto-recover when the snapshot provably owns every on-disk user
    // prompt. Writes the adopted snapshot to disk so the response lands this cycle
    // instead of stranding it for a manual `git checkout HEAD` / `reset
    // --from-current` / `finalize --force-disk` recovery. Fails closed (returns
    // None) on any dropped-prompt record or disk-only user prompt.
    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(recovered) =
            crate::write::try_auto_recover_live_prompt_drift(file, snapshot, &file_content)?
    {
        file_content = recovered;
    }
    crate::write::guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_content.as_deref(),
        &file_content,
        "commit",
    )?;

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

    let cycle_state_for_commit = crate::cycle_state::load(file)?;
    let ipc_snapshot_adoption_blocked = cycle_state_for_commit
        .as_ref()
        .is_some_and(|state| state.ipc_snapshot_adoption_blocked);
    let reintroduced_reaped_ids = cycle_state_for_commit
        .map(|state| state.reaped_pending_ids.into_iter().collect::<HashSet<_>>())
        .map(|ids| detect_reintroduced_reaped_pending_ids(&file_content, &ids))
        .transpose()?
        .unwrap_or_default();
    if !reintroduced_reaped_ids.is_empty() {
        let refs = reintroduced_reaped_ids
            .iter()
            .map(|id| format!("#{}", id))
            .collect::<Vec<_>>()
            .join(", ");
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_blocked_reintroduced_reaped_pending file={} ids={}",
                file.display(),
                refs
            ),
        );
        anyhow::bail!(
            "refusing to close {}: tracked backlog/icebox item(s) reaped earlier in this cycle reappeared in the live file: {}. Re-run preflight after resolving the stale local/editor rewrite",
            file.display(),
            refs
        );
    }

    let snapshot_matches_current_file = snapshot_content
        .as_deref()
        .is_some_and(|snapshot| snapshot == file_content);

    if !repaired_committed_historical
        && !snapshot_matches_current_file
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
        if ipc_snapshot_adoption_blocked {
            eprintln!(
                "[commit] refusing to absorb out-of-band {} mutation after IPC snapshot adoption was blocked for {}",
                reason,
                file.display()
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_absorb_blocked_after_ipc_snapshot_adoption file={} blocked_by={} old_snap_len={} new_snap_len={}",
                    file.display(),
                    reason,
                    snap_len,
                    file_len
                ),
            );
        } else {
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
    }

    dedupe_snapshot_and_worktree_before_commit(file, &mut snapshot_content, &mut file_content)?;

    let mut snapshot_matches_head = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    if snapshot_matches_head
        && let Some(head) = head_doc.as_deref()
        && let Some(cleaned) =
            crate::template::deleted_conversation_tail_cleanup(head, &file_content)?
    {
        eprintln!(
            "[commit] committing manual escaped conversation tail cleanup for {}",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "post_commit_escaped_tail_cleanup file={} basis=head",
                file.display()
            ),
        );
        crate::snapshot::save(file, &cleaned)?;
        snapshot_content = Some(cleaned);
        snapshot_matches_head = false;
    }
    if snapshot_matches_head
        && let Some(head) = head_doc.as_deref()
        && let Some(repaired) =
            repair_stale_agent_response_collapse_worktree(file, head, &file_content)?
    {
        file_content = repaired;
    }
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
    let file_len_after_repair = file_content.len();
    if snap_len > 0 && file_len_after_repair > snap_len && post_commit_local_drift.is_none() {
        let drift = file_len_after_repair - snap_len;
        // Log unclassified positive drift for aggregation/root-cause analysis.
        // Classified post-commit local edits have their own markers below.
        crate::ops_log::log_op(
            file,
            &format!(
                "out_of_band_write file={} drift={} snap_len={} file_len={}",
                file.display(),
                drift,
                snap_len,
                file_len_after_repair
            ),
        );
        if drift > 100 && post_commit_local_drift.is_none() {
            eprintln!(
                "[commit] WARNING: file is {} bytes larger than snapshot for {} — possible out-of-band write (snap={}, file={})",
                drift,
                file.display(),
                snap_len,
                file_len_after_repair
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "drift_warning file={} drift={} snap_len={} file_len={}",
                    file.display(),
                    drift,
                    snap_len,
                    file_len_after_repair
                ),
            );

            // Extreme drift can happen when a newly-bootstrapped document still
            // has the empty scaffold snapshot but the working tree now contains
            // the real file content. Only auto-resync that bootstrap case for
            // files with no HEAD entry yet. Tracked documents stay
            // snapshot-selective here so unanswered user prompts cannot be
            // swallowed into the committed snapshot during preflight.
            if file_len_after_repair > snap_len * 5
                && let Some(ref snapshot) = snapshot_content
            {
                let head_exists = head_doc.is_some();
                let scaffold_snapshot = is_empty_template_scaffold_snapshot(snapshot);
                if !head_exists && scaffold_snapshot {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) — re-syncing bootstrap scaffold snapshot from file content",
                        file_len_after_repair / snap_len.max(1)
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "snapshot_resync file={} old_snap_len={} new_snap_len={}",
                            file.display(),
                            snap_len,
                            file_len_after_repair
                        ),
                    );
                    crate::snapshot::save(file, &file_content)?;
                    snapshot_content = Some(file_content.clone());
                } else {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) — NOT re-syncing tracked/non-scaffold snapshot",
                        file_len_after_repair / snap_len.max(1)
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "snapshot_resync_blocked file={} head_exists={} scaffold_snapshot={} old_snap_len={} file_len={}",
                            file.display(),
                            head_exists,
                            scaffold_snapshot,
                            snap_len,
                            file_len_after_repair
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
        ensure_active_capture_materialized_for_head_current_noop(
            file,
            snapshot_content.as_deref(),
            head_doc.as_deref(),
        )?;
        if let Some(kind) = post_commit_local_drift {
            if kind == PostCommitLocalDriftKind::UserFollowUp {
                eprintln!(
                    "[commit] prior response is already committed in HEAD for {} — leaving later local user follow-up edits uncommitted for the next response cycle. This is not a full closeout for the follow-up prompt; run `agent-doc {}` to answer it or pipe the response through `agent-doc write --commit {}`.",
                    file.display(),
                    file.display(),
                    file.display()
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "post_commit_user_follow_up file={} basis=head",
                        file.display()
                    ),
                );
                if cycle_is_terminal(file) {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "commit_prompt_handoff_noop file={} basis=head",
                            file.display()
                        ),
                    );
                    let elapsed_total = t_total.elapsed().as_millis();
                    if elapsed_total > 0 {
                        eprintln!("[perf] commit total: {}ms", elapsed_total);
                    }
                    return Ok(CommitOutcome {
                        did_commit: false,
                        vcs_refresh_signaled: None,
                    });
                }
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
            post_commit_local_drift,
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
    file_content = std::fs::read_to_string(file).unwrap_or_default();
    dedupe_snapshot_and_worktree_before_commit(file, &mut snapshot_content, &mut file_content)?;
    ensure_active_capture_materialized_for_commit(
        file,
        snapshot_content.as_deref().or(Some(file_content.as_str())),
        "staged",
    )?;
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
            Err(CommitTransactionError::IgnoredPath { path }) => {
                eprintln!(
                    "[commit] skipped ignored untracked path {} (matched .gitignore); not staging",
                    path
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "commit_skipped_ignored_path file={} rel_path={}",
                        file.display(),
                        path
                    ),
                );
                break Err(anyhow::anyhow!(
                    "refusing to commit ignored untracked path {} (matched .gitignore)",
                    path
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
            crate::flow::proof::log_flow_event(
                file,
                crate::flow::types::FlowEvent::new(
                    crate::flow::types::FlowName::Closeout,
                    crate::flow::types::FlowStage::Commit,
                    crate::flow::types::FlowOutcome::Completed,
                )
                .with_reason("commit_success"),
            );
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
            // Reconcile the durable auto-queue continuation marker: write it when
            // a clean closeout still owes an `agent:queue auto` continuation,
            // clear it otherwise. Binary-owned proof that survives missing Codex
            // hook session state. (#codex-auto-queue-stalled-final-gate)
            if let Some(continuation) = crate::queue_continuation::reconcile_marker(file, "commit")
            {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "queue_continuation_required file={} head={}",
                        file.display(),
                        continuation.head_prompt.replace('\n', " ")
                    ),
                );
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
        if let Ok(cleaned) = std::fs::read_to_string(file) {
            match repair_clean_head_if_only_transient_worktree_drift(file, &cleaned) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Err(e) = refresh_live_closeout_sidecars(file, &cleaned, false) {
                        eprintln!(
                            "[commit] warning: failed to refresh CRDT sidecars after post-commit cleanup: {}",
                            e
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[commit] warning: failed to reconcile post-commit transient worktree drift: {}",
                        e
                    );
                }
            }
        }

        // Emit the live worktree==HEAD proof line after post-commit cleanup +
        // transient-drift repair have run, so it reflects the residual visible
        // state. `match=false` is the #postcommit-ipc-worktree-corruption signal.
        emit_postcommit_worktree_check(file);

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

fn strip_exchange_prompt_prefixes_for_compare(content: &str) -> String {
    fn strip_line(line: &str) -> String {
        let trimmed = line.trim_start();
        let indent_len = line.len().saturating_sub(trimmed.len());
        if let Some(rest) = trimmed.strip_prefix("❯ ") {
            format!("{}{}", &line[..indent_len], rest)
        } else {
            line.to_string()
        }
    }

    fn strip_lines(content: &str) -> String {
        let mut stripped = String::with_capacity(content.len());
        for segment in content.split_inclusive('\n') {
            let (line, newline) = segment
                .strip_suffix('\n')
                .map(|line| (line, "\n"))
                .unwrap_or((segment, ""));
            stripped.push_str(&strip_line(line));
            stripped.push_str(newline);
        }
        if !content.ends_with('\n') && content.is_empty() {
            stripped.clear();
        }
        stripped
    }

    let Ok(components) = crate::component::parse(content) else {
        return strip_lines(content);
    };
    let mut rebuilt = String::with_capacity(content.len());
    let mut last = 0usize;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        rebuilt.push_str(&content[last..comp.open_end]);
        if comp.name == "exchange" {
            rebuilt.push_str(&strip_lines(comp.content(content)));
        } else {
            rebuilt.push_str(comp.content(content));
        }
        rebuilt.push_str(&content[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    rebuilt.push_str(&content[last..]);
    rebuilt
}

fn exchange_prompt_prefix_equivalent(left: &str, right: &str) -> bool {
    strip_exchange_prompt_prefixes_for_compare(left)
        == strip_exchange_prompt_prefixes_for_compare(right)
}

fn dedupe_snapshot_and_worktree_before_commit(
    file: &Path,
    snapshot_content: &mut Option<String>,
    file_content: &mut String,
) -> Result<()> {
    let Some(snapshot) = snapshot_content.as_deref() else {
        return Ok(());
    };
    let deduped_snapshot = crate::dedupe::dedupe_responses(snapshot);
    if deduped_snapshot != snapshot {
        eprintln!(
            "[commit] deduped consecutive duplicate response block(s) before staging {}",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe file={} before_commit=true",
                file.display()
            ),
        );
        crate::snapshot::save(file, &deduped_snapshot)?;
        *snapshot_content = Some(deduped_snapshot);
    }

    let deduped_file = crate::dedupe::dedupe_responses(file_content);
    if deduped_file != *file_content {
        crate::write::atomic_write_pub(file, &deduped_file).with_context(|| {
            format!(
                "failed to repair duplicate response blocks in {}",
                file.display()
            )
        })?;
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe_repaired_worktree file={} before_commit=true",
                file.display()
            ),
        );
        *file_content = deduped_file;
    }

    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(repaired_file) = crate::write::repair_commit_prompt_artifacts_against_snapshot(
            file,
            snapshot,
            file_content,
        )
    {
        let mut snapshot_updated = false;
        if exchange_prompt_prefix_equivalent(snapshot, &repaired_file) {
            let clean_snapshot = strip_head_markers(&repaired_file);
            crate::snapshot::save(file, &clean_snapshot)?;
            *snapshot_content = Some(clean_snapshot);
            snapshot_updated = true;
        }
        if repaired_file != *file_content {
            crate::write::atomic_write_pub(file, &repaired_file).with_context(|| {
                format!(
                    "failed to repair duplicate prompt artifacts in {}",
                    file.display()
                )
            })?;
            *file_content = repaired_file;
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "commit_pre_stage_prompt_duplicate_repaired file={} snapshot_updated={} before_commit=true",
                file.display(),
                snapshot_updated
            ),
        );
    }

    Ok(())
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
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&snap_content);
        let new_snap = crate::template::reposition_boundary_to_end_clean(&prompt_canonicalized);
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
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&working);
        let snapshot_after_reposition = crate::snapshot::load(file).ok().flatten();
        let normalize_prefix_lines = snapshot_after_reposition
            .as_deref()
            .map(|snapshot| {
                crate::write::extract_post_commit_normalization_targets(
                    snapshot,
                    &prompt_canonicalized,
                )
            })
            .unwrap_or_default();
        let prefix_repaired = if normalize_prefix_lines.is_empty() {
            prompt_canonicalized
        } else {
            crate::write::normalize_exchange_prefixes_for_targets(
                &prompt_canonicalized,
                &normalize_prefix_lines,
            )
        };
        let repositioned =
            crate::template::reposition_boundary_to_end_preserve_head(&prefix_repaired);
        if repositioned != working {
            let committed_boundary_id = snapshot_after_reposition
                .as_deref()
                .and_then(|snapshot| crate::write::find_boundary_id(snapshot, "exchange"));
            let file_ipc = crate::write::queue_file_ipc_reposition_boundary(
                file,
                committed_boundary_id.as_deref(),
                &normalize_prefix_lines,
            );
            match file_ipc {
                Ok(crate::write::FileIpcRepositionResult::Queued) => {
                    eprintln!("[commit] queued working-tree boundary reposition through file IPC");
                    changed = true;
                }
                Ok(crate::write::FileIpcRepositionResult::DeferredExistingPatch) => {
                    eprintln!(
                        "[commit] deferred working-tree boundary reposition to existing file IPC patch"
                    );
                    changed = true;
                }
                Ok(crate::write::FileIpcRepositionResult::Unavailable) => {
                    match crate::write::atomic_write_pub(file, &repositioned) {
                        Ok(()) => {
                            if normalize_prefix_lines.is_empty() {
                                eprintln!("[commit] repositioned boundary in working tree");
                            } else {
                                eprintln!(
                                    "[commit] repaired {} prefix lines and repositioned boundary in working tree",
                                    normalize_prefix_lines.len()
                                );
                            }
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
                Err(e) => {
                    eprintln!(
                        "[commit] failed to queue file IPC boundary reposition: {}",
                        e
                    );
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
///
/// Also used by the response-materialization probe (`write::response_materialized_*`)
/// so a captured response body carrying these ephemeral markers still matches the
/// committed (marker-stripped) HEAD/archive blob — otherwise `stuck_captured_cycle`
/// false-alarms on a response that *is* committed (#8j86).
pub(crate) fn strip_guard_markers(content: &str) -> String {
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

fn document_uses_crdt(content: &str) -> bool {
    crate::frontmatter::parse(content)
        .map(|(fm, _)| fm.resolve_mode().is_crdt())
        .unwrap_or(false)
}

fn refresh_live_closeout_sidecars(
    file: &Path,
    committed_doc: &str,
    signal_editor_refresh: bool,
) -> Result<Option<bool>> {
    if document_uses_crdt(committed_doc) {
        let crdt = crate::crdt::CrdtDoc::from_text(committed_doc).encode_state();
        crate::snapshot::save_document_crdt(file, &crdt, committed_doc)?;
    }

    if !signal_editor_refresh {
        return Ok(None);
    }

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };

    if crate::ipc_socket::is_listener_active(&root)
        && crate::ipc_socket::send_vcs_refresh(&root).unwrap_or(false)
    {
        return Ok(Some(true));
    }

    let Some(signal_file) = vcs_refresh_signal_path(file) else {
        return Ok(None);
    };
    match std::fs::write(&signal_file, "") {
        Ok(()) => Ok(Some(true)),
        Err(e) => {
            eprintln!(
                "[commit] VCS refresh signal failed during closeout sidecar refresh: {}",
                e
            );
            Ok(Some(false))
        }
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
    IgnoredPath { path: String },
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

fn git_path_is_tracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(rel_path)
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    Ok(output.status.success())
}

fn git_path_is_ignored(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["check-ignore", "--quiet", "--no-index", "--"])
        .arg(rel_path)
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(CommitTransactionError::Fatal(anyhow::anyhow!(
            "git check-ignore failed for {}: {}",
            rel_path.display(),
            render_git_output(&output)
        ))),
    }
}

fn git_path_is_ignored_untracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    if git_path_is_tracked(git_root, rel_path)? {
        return Ok(false);
    }
    git_path_is_ignored(git_root, rel_path)
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
    let rel_path = relative_to(resolved, git_root);
    if git_path_is_ignored_untracked(git_root, &rel_path)? {
        return Err(CommitTransactionError::IgnoredPath {
            path: rel_path.to_string_lossy().into_owned(),
        });
    }

    if let Some(snap) = snapshot_content {
        // Stage a CLEAN copy of the snapshot — `(HEAD)` is transient metadata
        // and must never appear in the committed blob. Post-commit cleanup also
        // collapses the working tree/snapshot back to the same clean shape, so
        // moving the boundary across cycles cannot produce phantom marker-only
        // diffs on previously committed headings.
        let staged_content = strip_guard_markers(&strip_head_markers(
            &canonicalize_answered_prompt_prefixes(snap),
        ));
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

fn prompt_classifier_post_commit_drift_kind(
    head_doc: &str,
    current_doc: &str,
) -> Option<PostCommitLocalDriftKind> {
    let prompt_bearing_body = |content: &str| {
        crate::frontmatter::parse(content)
            .map(|(_, body)| body.to_string())
            .unwrap_or_else(|_| content.to_string())
    };
    let norm = |content: &str| {
        crate::git::normalize_committed_exchange_artifacts(&prompt_bearing_body(content))
    };
    let diff_text = crate::diff::unified_diff_from_contents(&norm(head_doc), &norm(current_doc))?;
    let changes = crate::diff::classify_prompt_bearing_changes(&diff_text);
    if changes.is_empty() {
        return None;
    }
    let has_explicit_prompt_target = changes
        .iter()
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .any(|change| {
            change
                .text
                .lines()
                .any(line_looks_like_explicit_post_commit_prompt_directive)
        });
    let has_content_edit = changes
        .iter()
        .any(|change| change.kind == crate::diff::PromptBearingChangeKind::ContentEdit);
    let has_recovery_artifact = changes
        .iter()
        .any(|change| change.kind == crate::diff::PromptBearingChangeKind::RecoveryArtifact);
    if has_explicit_prompt_target && !has_content_edit && !has_recovery_artifact {
        Some(PostCommitLocalDriftKind::UserFollowUp)
    } else {
        Some(PostCommitLocalDriftKind::WorkingTreeEdits)
    }
}

fn line_looks_like_explicit_post_commit_prompt_directive(line: &str) -> bool {
    let mut trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed
        .strip_prefix("[ ]")
        .or_else(|| trimmed.strip_prefix("[x]"))
        .or_else(|| trimmed.strip_prefix("[X]"))
        .or_else(|| trimmed.strip_prefix("[/]"))
    {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("[#")
        && let Some(close) = rest.find(']')
    {
        trimmed = rest[close + 1..].trim_start();
    }

    let lower = trimmed
        .trim_start_matches('❯')
        .trim_start()
        .to_ascii_lowercase();
    trimmed.starts_with('❯')
        || trimmed.ends_with('?')
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("fix this")
        || lower.starts_with("run tests")
        || lower.starts_with("build + install")
        || lower.starts_with("build and install")
        || lower.starts_with("commit + push")
        || lower.starts_with("commit and push")
        || lower.contains(" spec-test")
        || lower.contains(" spec test")
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
    if let Ok(Some(cleaned_head)) =
        crate::template::strip_conversation_tail_outside_exchange(head_doc)
        && is_safe_user_only_follow_up_after_committed_head(&cleaned_head, current_doc)
    {
        return Some(PostCommitLocalDriftKind::UserFollowUp);
    }
    if let Some(kind) = prompt_classifier_post_commit_drift_kind(head_doc, current_doc) {
        return Some(kind);
    }
    Some(PostCommitLocalDriftKind::WorkingTreeEdits)
}

fn exchange_component(doc: &str) -> Option<crate::component::Component> {
    crate::component::parse(doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")
}

fn first_hash_id(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch != '#' {
            continue;
        }
        let mut id = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || next == '-' || next == '_' {
                id.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if !id.is_empty() {
            return Some(id);
        }
    }
    None
}

fn normalized_response_heading_key(line: &str) -> Option<String> {
    let normalized = normalize_post_commit_re_heading_drift(line);
    let trimmed = normalized.trim();
    if trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn normalized_exchange_inventory_line(line: &str) -> Option<String> {
    let normalized = normalize_post_commit_re_heading_drift(line);
    let trimmed = normalized.trim();
    if trimmed.is_empty() || trimmed.starts_with("<!-- agent:boundary:") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn exchange_line_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange
        .lines()
        .filter_map(normalized_exchange_inventory_line)
    {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn current_exchange_is_committed_line_subset(head_exchange: &str, current_exchange: &str) -> bool {
    let head_counts = exchange_line_counts(head_exchange);
    let current_counts = exchange_line_counts(current_exchange);
    if current_counts.is_empty() {
        return false;
    }
    current_counts
        .into_iter()
        .all(|(line, count)| head_counts.get(&line).copied().unwrap_or(0) >= count)
}

fn exchange_has_blockquoted_prompt_for_id(exchange: &str, id: &str) -> bool {
    let bracketed = format!("[#{id}]");
    let bare = format!("#{id}");
    exchange.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix('>')
            .is_some_and(|rest| rest.contains(&bracketed) || rest.contains(&bare))
    })
}

fn stale_agent_response_collapse_exchange(head_exchange: &str, current_exchange: &str) -> bool {
    if normalize_post_commit_re_heading_drift(current_exchange)
        == normalize_post_commit_re_heading_drift(head_exchange)
    {
        return false;
    }
    if !current_exchange_is_committed_line_subset(head_exchange, current_exchange) {
        return false;
    }

    let head_headings: Vec<String> = head_exchange
        .lines()
        .filter_map(normalized_response_heading_key)
        .collect();
    let current_headings: HashSet<String> = current_exchange
        .lines()
        .filter_map(normalized_response_heading_key)
        .collect();

    head_headings.into_iter().any(|heading| {
        !current_headings.contains(&heading)
            && first_hash_id(&heading)
                .as_deref()
                .is_some_and(|id| exchange_has_blockquoted_prompt_for_id(current_exchange, id))
    })
}

fn repair_stale_agent_response_collapse_doc(head_doc: &str, current_doc: &str) -> Option<String> {
    if head_doc == current_doc {
        return None;
    }
    let head_exchange = exchange_component(head_doc)?;
    let current_exchange = exchange_component(current_doc)?;
    if !stale_agent_response_collapse_exchange(
        head_exchange.content(head_doc),
        current_exchange.content(current_doc),
    ) {
        return None;
    }
    Some(current_exchange.replace_content(current_doc, head_exchange.content(head_doc)))
}

fn repair_stale_agent_response_collapse_worktree(
    file: &Path,
    head_doc: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(repaired) = repair_stale_agent_response_collapse_doc(head_doc, file_content) else {
        return Ok(None);
    };

    crate::write::atomic_write_pub(file, &repaired)?;
    if repaired == head_doc {
        crate::snapshot::save(file, head_doc)?;
    }
    refresh_live_closeout_sidecars(file, &repaired, true)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "stale_agent_response_collapse_cleanup file={} basis=head preserved_local_drift={}",
            file.display(),
            repaired != head_doc
        ),
    );
    Ok(Some(repaired))
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
    if let Some(repaired) =
        repair_stale_agent_response_collapse_worktree(file, &head_doc, file_content)?
    {
        return Ok(Some((Some(head_doc.clone()), repaired)));
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
    refresh_live_closeout_sidecars(file, &head_doc, true)?;
    crate::ops_log::log_op(
        file,
        &format!("transient_cleanup file={} basis=head", file.display()),
    );
    Ok(Some((Some(head_doc.clone()), head_doc)))
}

/// `#postcommit-ipc-worktree-corruption`: emit a live worktree==HEAD proof line
/// after each successful committed closeout so the corruption class (a late IPC
/// reposition / stale-patch replay that splices the latest `### Re:` response
/// into an earlier block and duplicates queue-prompt blockquotes) is provable
/// from `ops.log` without a dedicated live-verify session.
///
/// The comparison is taken modulo the legitimate transient churn the working
/// tree intentionally keeps relative to the clean committed blob — boundary
/// markers, `(HEAD)` annotations, guard markers, the managed pipeline block, and
/// independent `agent:queue` maintenance — via [`normalize_for_replay_hash`], the
/// same normalizer the replay/repair paths use. So `match=true` means the visible
/// document is structurally equal to HEAD, and `match=false` is the corruption
/// signal for the operator to file (the SimWorld `worktree==HEAD` guard already
/// asserts the invariant; this adds the live log marker).
///
/// Best-effort and non-fatal: a missing HEAD blob (untracked doc) or an unreadable
/// working tree skips the check rather than failing the commit.
/// `#pcwc` — discriminate post-commit worktree drift (already proven `match=false`
/// against a correct HEAD) on the replay-normalized content:
///
/// - **corruption** — the working tree LOST committed content (a non-blank line
///   HEAD committed is absent from the tree: a dropped `### Re:` heading, a stale
///   reposition that deleted a response block) AND the tree added no new
///   carry-forward signal (prompt / question / `do #` / `dispatch` / `#tag`). HEAD
///   is authoritative and the tree is safe to reconcile back to it → `true`.
/// - **legitimate carry-forward / ambiguous** — the tree preserves every committed
///   line (a superset: all of HEAD plus new uncommitted user edits), OR it added a
///   carry-forward signal (real next-cycle user work). Either way the tree must be
///   preserved, never clobbered → `false`.
///
/// Conservative by construction: a false `false` only defers to the existing
/// detect-and-warn path; a false `true` would clobber a user edit, so every
/// uncertain shape (any added directive, or no proven content loss) returns
/// `false`. Shares [`crate::write::line_is_carry_forward_signal`] with the
/// `#fintol` finalize-tolerance gate so both paths classify user work the same way.
fn postcommit_worktree_lost_committed_content(head_norm: &str, tree_norm: &str) -> bool {
    use std::collections::HashSet;
    let tree_lines: HashSet<&str> = tree_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let lost_committed = head_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .any(|l| !tree_lines.contains(l));
    if !lost_committed {
        // Worktree preserves all committed content → carry-forward superset → preserve.
        return false;
    }
    let head_lines: HashSet<&str> = head_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let added_user_work = tree_norm
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !head_lines.contains(l))
        .any(crate::write::line_is_carry_forward_signal);
    // Committed content was lost AND no new user work was added → corruption.
    !added_user_work
}

fn send_postcommit_editor_refresh(file: &Path, head_doc: &str, stale_working: &str) {
    let canonical = match file.canonicalize() {
        Ok(canonical) => canonical,
        Err(e) => {
            eprintln!(
                "[commit] postcommit editor refresh skipped for {}: canonicalize failed: {e}",
                file.display()
            );
            return;
        }
    };
    let project_root = crate::write::resolve_ipc_project_root_pub(&canonical);
    if !crate::ipc_socket::is_listener_active(&project_root) {
        crate::ops_log::log_op(
            file,
            &format!(
                "postcommit_editor_refresh_skipped file={} reason=no_listener",
                file.display()
            ),
        );
        return;
    }

    let stale_hash = crate::ops_log::content_hash(stale_working);
    let stale_len = stale_working.len();
    match crate::ipc_socket::send_refresh_content(
        &project_root,
        &canonical.to_string_lossy(),
        head_doc,
        &stale_hash,
        stale_len,
    ) {
        Ok(true) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "postcommit_editor_refresh_sent file={} expected_len={} expected_hash={} target_len={} target_hash={}",
                    file.display(),
                    stale_len,
                    &stale_hash[..stale_hash.len().min(12)],
                    head_doc.len(),
                    &crate::ops_log::content_hash(head_doc)[..12],
                ),
            );
            eprintln!(
                "[commit] postcommit editor buffer refresh sent for {} (#pcwc)",
                file.display()
            );
        }
        Ok(false) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "postcommit_editor_refresh_skipped file={} reason=no_ack",
                    file.display()
                ),
            );
            eprintln!(
                "[commit] postcommit editor buffer refresh had no ack for {} (non-fatal)",
                file.display()
            );
        }
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "postcommit_editor_refresh_failed file={} err={}",
                    file.display(),
                    e
                ),
            );
            eprintln!(
                "[commit] postcommit editor buffer refresh failed for {} (non-fatal): {e}",
                file.display()
            );
        }
    }
}

fn emit_postcommit_worktree_check(file: &Path) {
    let head_doc = match show_head(file) {
        Ok(Some(head)) => head,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[commit] postcommit worktree check: HEAD read failed (non-fatal): {e}");
            return;
        }
    };
    let working = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[commit] postcommit worktree check: working-tree read failed (non-fatal): {e}"
            );
            return;
        }
    };
    let head_norm = normalize_for_replay_hash(&head_doc);
    let tree_norm = normalize_for_replay_hash(&working);
    let head_sha = crate::ops_log::content_hash(&head_norm);
    let tree_sha = crate::ops_log::content_hash(&tree_norm);
    let matches = head_norm == tree_norm;
    crate::ops_log::log_op(
        file,
        &format!(
            "postcommit_worktree_check file={} head={} tree={} match={}",
            file.display(),
            &head_sha[..head_sha.len().min(12)],
            &tree_sha[..tree_sha.len().min(12)],
            matches
        ),
    );
    if !matches {
        // #pcwc — auto-reconcile ONLY the corruption class: the working tree LOST
        // committed content (HEAD content dropped from the tree) with no new user
        // work. HEAD is authoritative, so restore the tree to the committed blob,
        // turning the old manual `git checkout HEAD -- FILE` recovery into an
        // automatic repair. A legitimate carry-forward superset (a concurrent user
        // edit carried forward UNCOMMITTED — the deliberate carry-forward invariant,
        // see `finalize_preserves_late_comment_tail_edit_outside_exchange_uncommitted`)
        // preserves every committed line, so the discriminator returns `false` and
        // the tree is left untouched on the detect-and-warn path below. After
        // repairing disk, push the committed blob back through editor IPC with a
        // stale-buffer hash guard so the IDE stops writing the stale content back.
        if postcommit_worktree_lost_committed_content(&head_norm, &tree_norm) {
            match std::fs::write(file, &head_doc) {
                Ok(()) => {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "postcommit_worktree_auto_reconciled file={} head={} tree_before={} reason=committed_content_lost",
                            file.display(),
                            &head_sha[..head_sha.len().min(12)],
                            &tree_sha[..tree_sha.len().min(12)],
                        ),
                    );
                    eprintln!(
                        "[commit] postcommit_worktree_check match=false for {} — working tree lost committed content; auto-reconciled to HEAD (#pcwc)",
                        file.display()
                    );
                    send_postcommit_editor_refresh(file, &head_doc, &working);
                }
                Err(e) => eprintln!(
                    "[commit] postcommit worktree auto-reconcile write failed (non-fatal): {e}"
                ),
            }
            return;
        }
        eprintln!(
            "[commit] WARNING postcommit_worktree_check match=false for {} — working tree drifted from HEAD (#postcommit-ipc-worktree-corruption)",
            file.display()
        );
    }
}

fn ensure_active_capture_materialized_for_head_current_noop(
    file: &Path,
    snapshot_content: Option<&str>,
    head_doc: Option<&str>,
) -> Result<()> {
    ensure_active_capture_materialized_for_commit(
        file,
        snapshot_content.or(head_doc),
        "head_current",
    )
}

fn ensure_active_capture_materialized_for_commit(
    file: &Path,
    staged_content: Option<&str>,
    basis: &str,
) -> Result<()> {
    let Some(capture) = crate::capture::load_active(file)? else {
        return Ok(());
    };
    if matches!(
        capture.state,
        crate::capture::CaptureState::Committed | crate::capture::CaptureState::Discarded
    ) {
        return Ok(());
    }
    let Some(materialized) = staged_content else {
        return Ok(());
    };
    if crate::write::response_materialized_in_content(&capture.response_body, materialized) {
        return Ok(());
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "commit_blocked_missing_captured_response file={} capture_id={} response_sha256={} basis={}",
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            basis
        ),
    );
    crate::flow::closeout::log_closeout_guard_event(
        file,
        crate::flow::types::FlowStage::TerminalGuard,
        crate::flow::types::FlowOutcome::FailedClosed,
        crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
    );
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout. Replay the captured response with `agent-doc write --commit {}` before marking the cycle committed.",
        file.display(),
        file.display()
    );
}

fn finalize_already_committed_noop(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    drift_kind: Option<PostCommitLocalDriftKind>,
) {
    crate::ops_log::log_cycle(file, "commit_noop", snapshot_content, file_content);
    let drift_kind = drift_kind
        .map(PostCommitLocalDriftKind::as_str)
        .unwrap_or("none");
    crate::ops_log::log_op(
        file,
        &format!(
            "commit_noop file={} reason=already_current drift_kind={} basis=head",
            file.display(),
            drift_kind
        ),
    );
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::Closeout,
            crate::flow::types::FlowStage::Commit,
            crate::flow::types::FlowOutcome::Completed,
        )
        .with_reason(format!("already_current_{drift_kind}")),
    );
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
    // Reconcile the durable auto-queue continuation marker on the
    // already-committed closeout path too. (#codex-auto-queue-stalled-final-gate)
    crate::queue_continuation::reconcile_marker(file, "commit_already_current");
}

fn cycle_is_terminal(file: &Path) -> bool {
    crate::cycle_state::load(file)
        .ok()
        .flatten()
        .is_some_and(|state| !state.is_open())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmodulePointerDrift {
    pub relative_path: String,
    pub parent_head: Option<String>,
    pub submodule_head: String,
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

/// List tracked modified paths in the owning git repo for `file`.
///
/// Paths are returned relative to the narrowed repo root (submodule when
/// applicable). Untracked files are excluded.
pub fn tracked_modified_paths(file: &Path) -> Result<Vec<String>> {
    if !is_in_git_repo(file) {
        return Ok(Vec::new());
    }
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
            "--ignored=no",
        ])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.len() < 4 {
            continue;
        }
        if line.starts_with("??") {
            continue;
        }
        let mut path = line[3..].trim().to_string();
        if let Some((_, renamed_to)) = path.rsplit_once(" -> ") {
            path = renamed_to.trim().to_string();
        }
        if !path.is_empty() {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Check whether the parent repo's committed submodule pointer is current for a file in a submodule.
/// Returns `true` if the parent gitlink still differs from the submodule HEAD, `false` otherwise.
pub fn is_submodule_pointer_stale(file: &Path) -> bool {
    submodule_pointer_drift(file)
        .map(|drift| drift.is_some())
        .unwrap_or(false)
}

/// Return the exact parent gitlink drift for a document inside a submodule.
///
/// This compares the superproject's committed gitlink (`HEAD:<submodule>`)
/// against the submodule's current `HEAD`. Working-tree dirt inside the
/// submodule is intentionally ignored; closeout only owns the parent pointer
/// needed to make an already-created submodule commit reachable from the
/// parent repository.
pub fn submodule_pointer_drift(file: &Path) -> Result<Option<SubmodulePointerDrift>> {
    let Ok((super_root, resolved)) = resolve_to_git_root(file) else {
        return Ok(None);
    };
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if !in_submodule {
        return Ok(None);
    }
    let rel = match git_root.strip_prefix(&super_root) {
        Ok(r) => r.to_string_lossy().to_string(),
        Err(_) => return Ok(None),
    };
    let Some(submodule_head) = git_rev_parse(&git_root, "HEAD")? else {
        return Ok(None);
    };
    let parent_spec = format!("HEAD:{rel}");
    let parent_head = git_rev_parse(&super_root, &parent_spec)?;
    if parent_head.as_deref() == Some(submodule_head.as_str()) {
        Ok(None)
    } else {
        Ok(Some(SubmodulePointerDrift {
            relative_path: rel,
            parent_head,
            submodule_head,
        }))
    }
}

fn git_rev_parse(repo: &Path, rev: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", rev])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
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
mod tests;
