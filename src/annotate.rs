//! # Module: annotate
//!
//! ## Spec
//! - `generate(doc, force)` produces a content-source annotation sidecar at
//!   `.agent-doc/annotations/<doc_hash>.json`. The sidecar maps each line in
//!   the current file to its authorship source (`agent` or `user`) by diffing
//!   the snapshot (last agent write) against the current file.
//! - `generate_with_history(doc, force)` additionally records `git blame`
//!   attribution for every current line and the latest commit that changed the
//!   document. History mode requires the document to be tracked by Git.
//! - Cache check: if the sidecar exists and both `snapshot_content_hash` and
//!   `file_content_hash` match current state, and the optional Git history
//!   revision matches the requested mode, returns the existing path unless
//!   `force` is true.
//! - When no snapshot exists, all lines are attributed to `user`.
//! - Uses `similar::TextDiff::from_lines()` on raw content (no comment stripping)
//!   so the sidecar maps 1:1 to actual file lines.
//! - `run(file, force, history)` is the CLI entry point for the `annotate`
//!   subcommand. Prints the sidecar path to stdout.
//!
//! ## Agentic Contracts
//! - The sidecar is a **cache, not state** — always reconstructable from the
//!   snapshot, current file, and (when requested) Git history. Deleting it has
//!   no side effects.
//! - `generate` is idempotent: calling it twice with the same inputs yields
//!   the same output and returns the same path.
//! - The sidecar JSON is stable: same inputs produce identical JSON output.
//! - Line numbers in the sidecar are 1-indexed, matching editor conventions.
//!
//! ## Evals
//! - `no_snapshot_all_user`: no snapshot → every line is `user`
//! - `identical_all_agent`: snapshot == file → every line is `agent`
//! - `user_additions`: appended lines → original `agent`, new `user`
//! - `user_modifications`: modified line → `user`, context → `agent`
//! - `cache_invalidation`: file changed after sidecar → regenerates
//! - `cache_valid_skips`: no changes → returns existing path without regen
//! - `annotate_history`: history mode adds blame metadata and invalidates on a
//!   path-history change even when snapshot and file hashes are unchanged

use agent_doc_hash::content_hash;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};
use std::process::Command;

const ANNOTATION_DIR: &str = ".agent-doc/annotations";

/// Source attribution for a single line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineSource {
    /// Line unchanged from the snapshot (agent-written content).
    Agent,
    /// Line added or modified by the user since the last snapshot.
    User,
}

/// Per-line annotation entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct LineAnnotation {
    /// 1-indexed line number in the current file.
    pub line: usize,
    /// Authorship source.
    pub source: LineSource,
    /// Git blame attribution when generated with `--history`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<GitHistoryAttribution>,
}

/// Git history attribution for a current document line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHistoryAttribution {
    /// Commit that last changed the line (all zeroes for uncommitted content).
    pub commit: String,
    /// Author name reported by Git.
    pub author: String,
    /// Author email reported by Git, without angle brackets.
    pub author_email: String,
    /// Author timestamp as Unix seconds.
    pub author_time: i64,
    /// Commit summary reported by Git.
    pub summary: String,
}

/// The full annotation sidecar for a document.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnnotationSidecar {
    /// Document path (relative to project root).
    pub file: String,
    /// SHA256 hash of the document's canonical path (matches snapshot key).
    pub doc_hash: String,
    /// SHA256 of the snapshot content at generation time (cache key).
    pub snapshot_content_hash: String,
    /// SHA256 of the current file content at generation time (cache key).
    pub file_content_hash: String,
    /// Latest commit affecting the document when generated with `--history`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_revision: Option<String>,
    /// Per-line attributions, ordered by line number.
    pub lines: Vec<LineAnnotation>,
}

struct GitHistoryContext {
    root: PathBuf,
    relative: PathBuf,
    revision: String,
}

fn git_history_context(doc: &Path) -> Result<GitHistoryContext> {
    let canonical = std::fs::canonicalize(doc)
        .with_context(|| format!("failed to canonicalize {}", doc.display()))?;
    let start = canonical.parent().unwrap_or(Path::new("."));
    let root_output = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse for --history")?;
    if !root_output.status.success() {
        let stderr = String::from_utf8_lossy(&root_output.stderr);
        anyhow::bail!(
            "--history requires a Git worktree containing {}: {}",
            doc.display(),
            stderr.trim()
        );
    }
    let root_text = std::str::from_utf8(&root_output.stdout)
        .context("git rev-parse returned a non-UTF-8 worktree path")?;
    let root = std::fs::canonicalize(root_text.trim())
        .context("failed to canonicalize Git worktree root")?;
    let relative = canonical.strip_prefix(&root).with_context(|| {
        format!(
            "document {} is outside Git worktree {}",
            canonical.display(),
            root.display()
        )
    })?;

    let revision_output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "-1", "--format=%H", "--"])
        .arg(relative)
        .output()
        .context("failed to run git log for --history")?;
    if !revision_output.status.success() {
        let stderr = String::from_utf8_lossy(&revision_output.stderr);
        anyhow::bail!(
            "failed to read Git history for {}: {}",
            doc.display(),
            stderr.trim()
        );
    }
    let revision = std::str::from_utf8(&revision_output.stdout)
        .context("git log returned a non-UTF-8 revision")?
        .trim()
        .to_owned();
    if revision.is_empty() {
        anyhow::bail!(
            "--history requires {} to have committed Git history",
            doc.display()
        );
    }

    Ok(GitHistoryContext {
        root,
        relative: relative.to_path_buf(),
        revision,
    })
}

fn git_blame(context: &GitHistoryContext, doc: &Path) -> Result<Vec<GitHistoryAttribution>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(&context.root)
        .args(["blame", "--line-porcelain", "--"])
        .arg(&context.relative)
        .output()
        .context("failed to run git blame for --history")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to blame Git history for {}: {}",
            doc.display(),
            stderr.trim()
        );
    }
    let porcelain = std::str::from_utf8(&output.stdout)
        .context("git blame returned non-UTF-8 attribution metadata")?;
    parse_blame_porcelain(porcelain)
        .with_context(|| format!("failed to parse git blame for {}", doc.display()))
}

fn parse_blame_porcelain(porcelain: &str) -> Result<Vec<GitHistoryAttribution>> {
    let mut attributions = Vec::new();
    let mut commit = None;
    let mut author = None;
    let mut author_email = None;
    let mut author_time = None;
    let mut summary = None;

    for line in porcelain.lines() {
        if line.starts_with('\t') {
            attributions.push(GitHistoryAttribution {
                commit: commit.take().context("blame entry missing commit")?,
                author: author.take().context("blame entry missing author")?,
                author_email: author_email
                    .take()
                    .context("blame entry missing author email")?,
                author_time: author_time
                    .take()
                    .context("blame entry missing author time")?,
                summary: summary.take().context("blame entry missing summary")?,
            });
            continue;
        }

        if let Some(value) = line.strip_prefix("author ") {
            author = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("author-mail ") {
            author_email = Some(
                value
                    .strip_prefix('<')
                    .and_then(|value| value.strip_suffix('>'))
                    .unwrap_or(value)
                    .to_owned(),
            );
        } else if let Some(value) = line.strip_prefix("author-time ") {
            author_time = Some(value.parse().context("invalid blame author-time")?);
        } else if let Some(value) = line.strip_prefix("summary ") {
            summary = Some(value.to_owned());
        } else {
            let mut fields = line.split_whitespace();
            let candidate = fields.next().unwrap_or_default();
            let normalized = candidate.strip_prefix('^').unwrap_or(candidate);
            let original_line = fields.next();
            let final_line = fields.next();
            if normalized.len() == 40
                && normalized.chars().all(|c| c.is_ascii_hexdigit())
                && original_line.is_some_and(|field| field.parse::<usize>().is_ok())
                && final_line.is_some_and(|field| field.parse::<usize>().is_ok())
            {
                commit = Some(normalized.to_owned());
                author = None;
                author_email = None;
                author_time = None;
                summary = None;
            }
        }
    }

    Ok(attributions)
}

/// Compute the sidecar path for a document.
fn sidecar_path(doc: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(doc)
        .with_context(|| format!("failed to canonicalize {}", doc.display()))?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| doc.parent().unwrap_or(Path::new(".")).to_path_buf());
    let dir = root.join(ANNOTATION_DIR);
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.join(format!("{}.json", hash)))
}

/// Generate (or return cached) content-source annotation sidecar.
///
/// Returns the path to the sidecar JSON file.
pub fn generate(doc: &Path, force: bool) -> Result<PathBuf> {
    generate_internal(doc, force, false)
}

/// Generate a content-source sidecar with per-line Git history attribution.
pub fn generate_with_history(doc: &Path, force: bool) -> Result<PathBuf> {
    generate_internal(doc, force, true)
}

fn generate_internal(doc: &Path, force: bool, history: bool) -> Result<PathBuf> {
    let path = sidecar_path(doc)?;
    let canonical = std::fs::canonicalize(doc)?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;

    // Load current file content.
    let file_content = std::fs::read_to_string(doc)
        .with_context(|| format!("failed to read {}", doc.display()))?;
    let file_hash = content_hash(&file_content);

    // Load snapshot (baseline from last agent write).
    let snapshot_content = agent_doc_snapshot_io::resolve(doc)?.unwrap_or_default();
    let snap_hash = content_hash(&snapshot_content);
    let history_context = history.then(|| git_history_context(doc)).transpose()?;
    let history_revision = history_context
        .as_ref()
        .map(|context| context.revision.clone());

    // Cache check: if sidecar exists with matching hashes, skip regeneration.
    if !force
        && path.exists()
        && let Ok(existing_json) = std::fs::read_to_string(&path)
        && let Ok(existing) = serde_json::from_str::<AnnotationSidecar>(&existing_json)
        && existing.snapshot_content_hash == snap_hash
        && existing.file_content_hash == file_hash
        && existing.history_revision == history_revision
    {
        eprintln!("[annotate] cache valid, skipping regeneration");
        return Ok(path);
    }

    // Compute line-level attribution via diff.
    let mut lines = Vec::new();
    let mut line_no = 0usize;

    if snapshot_content.is_empty() {
        // No snapshot → all lines are user-written.
        for _ in file_content.lines() {
            line_no += 1;
            lines.push(LineAnnotation {
                line: line_no,
                source: LineSource::User,
                history: None,
            });
        }
    } else {
        let diff = TextDiff::from_lines(&snapshot_content, &file_content);
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    line_no += 1;
                    lines.push(LineAnnotation {
                        line: line_no,
                        source: LineSource::Agent,
                        history: None,
                    });
                }
                ChangeTag::Insert => {
                    line_no += 1;
                    lines.push(LineAnnotation {
                        line: line_no,
                        source: LineSource::User,
                        history: None,
                    });
                }
                ChangeTag::Delete => {
                    // Line removed from snapshot — not in current file, skip.
                }
            }
        }
    }

    if let Some(context) = &history_context {
        let blame = git_blame(context, doc)?;
        if blame.len() != lines.len() {
            anyhow::bail!(
                "git blame returned {} lines for {}, but annotation has {} lines",
                blame.len(),
                doc.display(),
                lines.len()
            );
        }
        for (line, history) in lines.iter_mut().zip(blame) {
            line.history = Some(history);
        }
    }

    // Build relative file path for the sidecar.
    let root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| doc.parent().unwrap_or(Path::new(".")).to_path_buf());
    let relative = canonical
        .strip_prefix(&root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string();

    let sidecar = AnnotationSidecar {
        file: relative,
        doc_hash: hash,
        snapshot_content_hash: snap_hash,
        file_content_hash: file_hash,
        history_revision,
        lines,
    };

    // Atomic write via tempfile + rename.
    let json =
        serde_json::to_string_pretty(&sidecar).context("failed to serialize annotation sidecar")?;
    let dir = path.parent().unwrap();
    let tmp = tempfile::NamedTempFile::new_in(dir)
        .context("failed to create temp file for annotation")?;
    std::fs::write(tmp.path(), &json)?;
    tmp.persist(&path)
        .with_context(|| format!("failed to persist annotation to {}", path.display()))?;

    eprintln!("[annotate] generated {}", path.display());
    Ok(path)
}

/// CLI entry point for `agent-doc annotate`.
pub fn run(file: &Path, force: bool, history: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let path = if history {
        generate_with_history(file, force)?
    } else {
        generate(file, force)?
    };
    println!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc/snapshots");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        std::fs::create_dir_all(dir.path().join(ANNOTATION_DIR)).unwrap();
        let doc = dir.path().join("test.md");
        (dir, doc)
    }

    fn save_snapshot(doc: &Path, content: &str) {
        agent_doc_snapshot_io::checkpoint_document_baseline(
            doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
    }

    #[test]
    fn no_snapshot_all_user() {
        let (_dir, doc) = setup_test_dir();
        std::fs::write(&doc, "line 1\nline 2\nline 3\n").unwrap();

        let path = generate(&doc, true).unwrap();
        let json: AnnotationSidecar =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json.lines.len(), 3);
        assert!(json.lines.iter().all(|l| l.source == LineSource::User));
    }

    #[test]
    fn identical_all_agent() {
        let (_dir, doc) = setup_test_dir();
        let content = "line 1\nline 2\nline 3\n";
        std::fs::write(&doc, content).unwrap();
        save_snapshot(&doc, content);

        let path = generate(&doc, true).unwrap();
        let json: AnnotationSidecar =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json.lines.len(), 3);
        assert!(json.lines.iter().all(|l| l.source == LineSource::Agent));
    }

    #[test]
    fn user_additions() {
        let (_dir, doc) = setup_test_dir();
        let snapshot = "line 1\nline 2\n";
        let current = "line 1\nline 2\nuser added\n";
        std::fs::write(&doc, current).unwrap();
        save_snapshot(&doc, snapshot);

        let path = generate(&doc, true).unwrap();
        let json: AnnotationSidecar =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json.lines.len(), 3);
        assert_eq!(json.lines[0].source, LineSource::Agent);
        assert_eq!(json.lines[1].source, LineSource::Agent);
        assert_eq!(json.lines[2].source, LineSource::User);
    }

    #[test]
    fn user_modifications() {
        let (_dir, doc) = setup_test_dir();
        let snapshot = "line 1\noriginal line\nline 3\n";
        let current = "line 1\nmodified line\nline 3\n";
        std::fs::write(&doc, current).unwrap();
        save_snapshot(&doc, snapshot);

        let path = generate(&doc, true).unwrap();
        let json: AnnotationSidecar =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(json.lines.len(), 3);
        assert_eq!(json.lines[0].source, LineSource::Agent); // unchanged
        assert_eq!(json.lines[1].source, LineSource::User); // modified
        assert_eq!(json.lines[2].source, LineSource::Agent); // unchanged
    }

    #[test]
    fn cache_invalidation() {
        let (_dir, doc) = setup_test_dir();
        let content = "line 1\n";
        std::fs::write(&doc, content).unwrap();
        save_snapshot(&doc, content);

        // Generate initial sidecar.
        let path1 = generate(&doc, false).unwrap();
        let mtime1 = std::fs::metadata(&path1).unwrap().modified().unwrap();

        // Modify the file.
        std::fs::write(&doc, "line 1\nnew line\n").unwrap();

        // Small sleep to ensure mtime differs.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Regenerate — cache should be invalidated.
        let path2 = generate(&doc, false).unwrap();
        let mtime2 = std::fs::metadata(&path2).unwrap().modified().unwrap();

        assert_eq!(path1, path2);
        assert!(mtime2 > mtime1, "sidecar should have been regenerated");
    }

    #[test]
    fn cache_valid_skips() {
        let (_dir, doc) = setup_test_dir();
        let content = "line 1\n";
        std::fs::write(&doc, content).unwrap();
        save_snapshot(&doc, content);

        // Generate initial sidecar.
        let path1 = generate(&doc, false).unwrap();
        let mtime1 = std::fs::metadata(&path1).unwrap().modified().unwrap();

        // Call again without changes — should skip regeneration.
        let path2 = generate(&doc, false).unwrap();
        let mtime2 = std::fs::metadata(&path2).unwrap().modified().unwrap();

        assert_eq!(path1, path2);
        assert_eq!(mtime1, mtime2, "sidecar should not have been regenerated");
    }
}
