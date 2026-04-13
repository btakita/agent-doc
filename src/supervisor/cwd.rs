//! # Module: supervisor::cwd
//!
//! Deterministic CWD resolution for the supervised claude child process.
//!
//! ## Spec
//! Resolution priority (first match wins):
//! 1. `--cwd <path>` CLI flag (resolved relative to the supervisor's invocation CWD)
//! 2. Frontmatter `agent_doc_cwd: <path>` (resolved relative to the document's parent directory)
//! 3. Project root — walk up from the document until a directory containing `.agent-doc/` is found
//! 4. The document's parent directory (ultimate fallback)
//!
//! Every candidate is canonicalized. If a candidate path does not exist or is not a
//! directory, the resolver returns a hard error — it does **not** silently fall through
//! to a lower priority. Misconfiguration should surface immediately, not drift.
//!
//! The resolved CWD is returned alongside its `CwdSource` so the supervisor can log
//! which rule fired (see `specs/supervisor.md` § Logging: `cwd_resolved source=...`).
//!
//! See `src/agent-doc/specs/supervisor.md` § Core Invariants / CWD determinism.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Which rule in the priority chain produced the resolved CWD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CwdSource {
    /// `--cwd <path>` command-line flag.
    CliFlag,
    /// `agent_doc_cwd: <path>` frontmatter field.
    Frontmatter,
    /// Directory containing `.agent-doc/`, found by walking up from the document.
    ProjectRoot,
    /// The document's parent directory (fallback).
    DocumentParent,
}

impl CwdSource {
    /// Stable string tag used in logs and IPC state responses.
    pub fn as_str(&self) -> &'static str {
        match self {
            CwdSource::CliFlag => "cli_flag",
            CwdSource::Frontmatter => "frontmatter",
            CwdSource::ProjectRoot => "project_root",
            CwdSource::DocumentParent => "document_parent",
        }
    }
}

/// Resolved CWD plus the rule that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCwd {
    pub path: PathBuf,
    pub source: CwdSource,
}

/// Resolve the supervisor's working directory for a given document.
///
/// - `cli_cwd` — value of `--cwd` if the user passed it on the command line.
///   Resolved relative to the process's current working directory.
/// - `frontmatter_cwd` — value of `agent_doc_cwd` frontmatter if set.
///   Resolved relative to the document's parent directory, so `..` means
///   "parent of the document's parent."
/// - `document` — path to the session document. Must exist on disk; the
///   function canonicalizes it to derive the parent directory and walk for
///   project root detection.
///
/// Returns `ResolvedCwd` on success. Returns an error if any candidate
/// resolves to a non-existent path or a non-directory.
pub fn resolve(
    cli_cwd: Option<&Path>,
    frontmatter_cwd: Option<&str>,
    document: &Path,
) -> Result<ResolvedCwd> {
    let canonical_doc = document
        .canonicalize()
        .with_context(|| format!("document does not exist: {}", document.display()))?;
    let doc_parent = canonical_doc
        .parent()
        .context("document has no parent directory")?;

    if let Some(cli) = cli_cwd {
        let path = canonicalize_dir(cli, None, "--cwd flag")?;
        return Ok(ResolvedCwd {
            path,
            source: CwdSource::CliFlag,
        });
    }

    if let Some(fm) = frontmatter_cwd {
        let path = canonicalize_dir(Path::new(fm), Some(doc_parent), "agent_doc_cwd frontmatter")?;
        return Ok(ResolvedCwd {
            path,
            source: CwdSource::Frontmatter,
        });
    }

    if let Some(root) = find_project_root(doc_parent) {
        return Ok(ResolvedCwd {
            path: root,
            source: CwdSource::ProjectRoot,
        });
    }

    Ok(ResolvedCwd {
        path: doc_parent.to_path_buf(),
        source: CwdSource::DocumentParent,
    })
}

/// Canonicalize a candidate path and assert it is an existing directory.
///
/// If `base` is `Some`, relative candidates are joined onto `base` before
/// canonicalization. If `base` is `None`, relative candidates are resolved
/// against the process's current working directory (the standard behavior
/// of `Path::canonicalize`).
fn canonicalize_dir(candidate: &Path, base: Option<&Path>, context_label: &str) -> Result<PathBuf> {
    let joined: PathBuf = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Some(base) = base {
        base.join(candidate)
    } else {
        candidate.to_path_buf()
    };
    let canonical = joined
        .canonicalize()
        .with_context(|| format!("{}: path does not exist: {}", context_label, joined.display()))?;
    if !canonical.is_dir() {
        bail!(
            "{}: path is not a directory: {}",
            context_label,
            canonical.display()
        );
    }
    Ok(canonical)
}

/// Walk up from `start` until a directory containing `.agent-doc/` is found.
///
/// Returns the containing directory on hit. Returns `None` if the walk
/// reaches the filesystem root without finding `.agent-doc/`.
///
/// This mirrors `snapshot::find_project_root` but is duplicated here so the
/// supervisor module stays self-contained — it will be called from a
/// supervisor process that may not have a snapshot path available yet.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current: &Path = start;
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Build a temp project layout:
    /// ```
    /// <tmp>/
    ///   .agent-doc/              (project root marker)
    ///   docs/plan.md             (document)
    ///   subdir/                  (extra dir for --cwd / frontmatter targets)
    /// ```
    fn make_layout() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("subdir")).unwrap();
        let doc = root.join("docs").join("plan.md");
        fs::write(&doc, "# plan").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        (tmp, canonical_root, doc)
    }

    #[test]
    fn cli_flag_wins_over_everything() {
        let (_tmp, root, doc) = make_layout();
        let target = root.join("subdir");
        let resolved = resolve(Some(&target), Some(".."), &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::CliFlag);
        assert_eq!(resolved.path, target.canonicalize().unwrap());
    }

    #[test]
    fn frontmatter_wins_when_no_cli_flag() {
        let (_tmp, root, doc) = make_layout();
        // agent_doc_cwd is resolved relative to the document's parent (root/docs),
        // so ".." lands on the project root.
        let resolved = resolve(None, Some(".."), &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::Frontmatter);
        assert_eq!(resolved.path, root);
    }

    #[test]
    fn frontmatter_absolute_path_is_respected() {
        let (_tmp, root, doc) = make_layout();
        let target = root.join("subdir");
        let resolved = resolve(None, Some(target.to_str().unwrap()), &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::Frontmatter);
        assert_eq!(resolved.path, target.canonicalize().unwrap());
    }

    #[test]
    fn project_root_wins_when_no_cli_or_frontmatter() {
        let (_tmp, root, doc) = make_layout();
        let resolved = resolve(None, None, &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::ProjectRoot);
        assert_eq!(resolved.path, root);
    }

    #[test]
    fn document_parent_fallback_when_no_project_root() {
        // No .agent-doc/ anywhere above the document.
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().join("lonely");
        fs::create_dir_all(&doc_dir).unwrap();
        let doc = doc_dir.join("plan.md");
        fs::write(&doc, "# plan").unwrap();
        let resolved = resolve(None, None, &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::DocumentParent);
        assert_eq!(resolved.path, doc_dir.canonicalize().unwrap());
    }

    #[test]
    fn cli_flag_nonexistent_path_is_hard_error() {
        let (_tmp, _root, doc) = make_layout();
        let bogus = PathBuf::from("/definitely/does/not/exist/zzzqx");
        let err = resolve(Some(&bogus), None, &doc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("--cwd flag"), "error message: {}", msg);
    }

    #[test]
    fn cli_flag_file_not_directory_is_hard_error() {
        let (_tmp, _root, doc) = make_layout();
        // Point --cwd at the document itself (a file, not a directory).
        let err = resolve(Some(&doc), None, &doc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("not a directory"), "error message: {}", msg);
    }

    #[test]
    fn frontmatter_nonexistent_does_not_fall_through_to_project_root() {
        let (_tmp, _root, doc) = make_layout();
        // Frontmatter points at a path that doesn't exist. We must NOT silently
        // fall through to the project-root walk — misconfiguration should surface.
        let err = resolve(None, Some("nowhere-zzzqx"), &doc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("agent_doc_cwd"), "error message: {}", msg);
    }

    #[test]
    fn document_must_exist() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("missing.md");
        let err = resolve(None, None, &doc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("document does not exist"), "error message: {}", msg);
    }

    #[test]
    fn source_tag_strings_are_stable() {
        assert_eq!(CwdSource::CliFlag.as_str(), "cli_flag");
        assert_eq!(CwdSource::Frontmatter.as_str(), "frontmatter");
        assert_eq!(CwdSource::ProjectRoot.as_str(), "project_root");
        assert_eq!(CwdSource::DocumentParent.as_str(), "document_parent");
    }

    #[test]
    fn deeply_nested_document_still_finds_project_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let nested = root.join("a").join("b").join("c").join("d");
        fs::create_dir_all(&nested).unwrap();
        let doc = nested.join("plan.md");
        fs::write(&doc, "# plan").unwrap();
        let resolved = resolve(None, None, &doc).unwrap();
        assert_eq!(resolved.source, CwdSource::ProjectRoot);
        assert_eq!(resolved.path, root.canonicalize().unwrap());
    }
}
